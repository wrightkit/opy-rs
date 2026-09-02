//! First-party OPY Language Provider Protocol process.
//!
//! The process is deliberately a thin owner-side adapter: OPY project loading,
//! preprocessing, diagnostics, and compilation remain in `opy-rs`. Only the
//! LPP envelope and source-oriented projections live here.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};

use opy_rs::tooling::{CheckOutcome, Diagnostic as OpyDiagnostic, SourceLocation};
use opy_rs::{CompileDiagnostic, Compiler};
use serde::Deserialize;
use serde_json::{Value, json};

const PROTOCOL_VERSION: &str = "1.0";
const SERVER_NAME: &str = "opy-provider";
const LANGUAGE_ID: &str = "opy";
const LANGUAGE_EXTENSIONS: [&str; 1] = ["opy"];
// The payload is canonical Workshop text; the envelope remains opaque to LPP.
const WORKSHOP_ARTIFACT_FORMAT: &str = "workshop-rs/text-v1";

#[derive(Debug, Clone, Copy)]
struct Capabilities {
    check: bool,
    compile: bool,
}

impl Capabilities {
    const fn first_party() -> Self {
        Self {
            check: true,
            compile: true,
        }
    }

    fn enabled(self, capability: &str) -> bool {
        match capability {
            "check" => self.check,
            "compile" => self.compile,
            _ => false,
        }
    }

    fn as_json(self) -> Value {
        json!({
            "check": self.check,
            "compile": self.compile,
            "reconstruct": false,
            "symbols": false,
            "definition": false,
            "references": false,
            "rename": false,
            "editValidation": false,
        })
    }
}

fn capability_for(method: &str) -> Option<&'static str> {
    Some(match method {
        "lpp/check" => "check",
        "lpp/compile" => "compile",
        "lpp/reconstruct" => "reconstruct",
        "lpp/symbols" => "symbols",
        "lpp/definition" => "definition",
        "lpp/references" => "references",
        "lpp/rename" => "rename",
        "lpp/validateEdits" => "editValidation",
        _ => return None,
    })
}

#[derive(Debug)]
enum HandlerError {
    Lpp {
        kind: &'static str,
        details: Value,
        message: String,
    },
    Standard {
        code: i64,
        message: &'static str,
    },
}

impl HandlerError {
    fn refusal(code: &'static str, details: Value, message: impl Into<String>) -> Self {
        let mut details = details;
        details["refusalCode"] = json!(code);
        Self::Lpp {
            kind: "refusal",
            details,
            message: message.into(),
        }
    }

    fn invalid_document(uri: Option<&str>, reason: &'static str) -> Self {
        let mut details = json!({ "reason": reason });
        if let Some(uri) = uri {
            details["uri"] = json!(uri);
        }
        Self::Lpp {
            kind: "invalidDocument",
            details,
            message: format!("invalid document: {reason}"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    protocol_version: String,
    #[allow(dead_code)]
    client_info: Option<ClientInfo>,
}

#[derive(Debug, Deserialize)]
struct ClientInfo {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    version: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    uri: String,
    language_id: String,
    version: i64,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectParams {
    #[serde(default)]
    documents: Option<BTreeMap<String, Document>>,
    #[serde(default)]
    project_root: Option<String>,
    // `entryUri` is the additive entry-based field owned by LPP. The `entry`
    // spelling is accepted while the protocol repository rolls out the same
    // contract to clients; it is not advertised as a second capability.
    #[serde(default)]
    entry_uri: Option<String>,
    #[serde(default)]
    entry: Option<EntryTarget>,
    #[serde(default)]
    locale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EntryTarget {
    Uri(String),
    Object {
        uri: Option<String>,
        path: Option<String>,
    },
}

impl EntryTarget {
    fn value(self) -> Option<String> {
        match self {
            Self::Uri(value) => Some(value),
            Self::Object { uri, path } => uri.or(path),
        }
    }
}

#[derive(Debug)]
struct LoadedProject {
    source: String,
    main_path: PathBuf,
    root: PathBuf,
    documents: BTreeMap<String, Document>,
    locale: String,
}

struct Server {
    initialized: bool,
    exiting: bool,
    capabilities: Capabilities,
    compiler: Option<Compiler>,
}

fn main() {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    let mut server = Server {
        initialized: false,
        exiting: false,
        capabilities: Capabilities::first_party(),
        compiler: None,
    };
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .unwrap_or_else(|error| panic!("opy-provider: failed to read stdin: {error}"));
        if read == 0 {
            break;
        }
        let message = line.trim_end_matches(['\r', '\n']);
        if message.is_empty() {
            continue;
        }
        if let Some(response) = server.handle_message(message) {
            let serialized = serde_json::to_string(&response).expect("LPP response serializes");
            writeln!(writer, "{serialized}").expect("write LPP response");
            writer.flush().expect("flush LPP response");
        }
        if server.exiting {
            break;
        }
    }
}

impl Server {
    fn handle_message(&mut self, line: &str) -> Option<Value> {
        let parsed: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => return Some(standard_error(Value::Null, -32700, "Parse error")),
        };
        if parsed.is_array() || !parsed.is_object() {
            return Some(standard_error(Value::Null, -32600, "Invalid Request"));
        }
        let object = parsed.as_object().expect("object checked");
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(standard_error(Value::Null, -32600, "Invalid Request"));
        }
        let id = match object.get("id") {
            Some(id @ (Value::Number(_) | Value::String(_))) => id.clone(),
            _ => {
                return Some(lpp_error(
                    Value::Null,
                    "invalidRequest",
                    json!({ "reason": "notificationNotSupported" }),
                    "invalid request: LPP v1 defines no notifications",
                ));
            }
        };
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            return Some(standard_error(id, -32600, "Invalid Request"));
        };
        let Some(params) = object.get("params") else {
            return Some(standard_error(id, -32602, "Invalid params"));
        };

        match method {
            "lpp/initialize" => Some(self.initialize(id, params.clone())),
            "lpp/shutdown" => Some(self.shutdown(id)),
            _ => Some(self.dispatch(id, method, params.clone())),
        }
    }

    fn initialize(&mut self, id: Value, value: Value) -> Value {
        if self.initialized {
            return lpp_error(
                id,
                "invalidRequest",
                json!({ "reason": "alreadyInitialized" }),
                "invalid request: already initialized",
            );
        }
        let params: InitializeParams = match serde_json::from_value(value) {
            Ok(params) => params,
            Err(_) => return standard_error(id, -32602, "Invalid params"),
        };
        if params.protocol_version != PROTOCOL_VERSION {
            return lpp_error(
                id,
                "protocolVersionMismatch",
                json!({ "supportedProtocolVersions": [PROTOCOL_VERSION] }),
                format!("unsupported protocol version {}", params.protocol_version),
            );
        }
        self.initialized = true;
        ok(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "languages": [{
                    "id": LANGUAGE_ID,
                    "extensions": LANGUAGE_EXTENSIONS,
                }],
                "capabilities": self.capabilities.as_json(),
            }),
        )
    }

    fn shutdown(&mut self, id: Value) -> Value {
        if !self.initialized {
            return lpp_error(
                id,
                "invalidRequest",
                json!({ "reason": "notInitialized" }),
                "invalid request: session not initialized",
            );
        }
        self.exiting = true;
        ok(id, Value::Null)
    }

    fn dispatch(&mut self, id: Value, method: &str, params: Value) -> Value {
        if !self.initialized {
            return lpp_error(
                id,
                "invalidRequest",
                json!({ "reason": "notInitialized" }),
                "invalid request: session not initialized",
            );
        }
        let Some(capability) = capability_for(method) else {
            return standard_error(id, -32601, "Method not found");
        };
        if !self.capabilities.enabled(capability) {
            return lpp_error(
                id,
                "capabilityUnavailable",
                json!({ "capability": capability, "method": method }),
                format!("capability unavailable: {method}"),
            );
        }
        let result = match method {
            "lpp/check" => self.check(params),
            "lpp/compile" => self.compile(params),
            _ => Err(HandlerError::Standard {
                code: -32601,
                message: "Method not found",
            }),
        };
        match result {
            Ok(result) => ok(id, result),
            Err(error) => error_response(id, error),
        }
    }

    fn check(&mut self, value: Value) -> Result<Value, HandlerError> {
        let params: ProjectParams =
            serde_json::from_value(value).map_err(|_| HandlerError::Standard {
                code: -32602,
                message: "Invalid params",
            })?;
        let project = load_project(params)?;
        let outcome = opy_rs::tooling::check(
            &project.source,
            &path_string(&project.main_path),
            &project.root,
        );
        Ok(check_result(&project, &outcome))
    }

    fn compile(&mut self, value: Value) -> Result<Value, HandlerError> {
        let params: ProjectParams =
            serde_json::from_value(value).map_err(|_| HandlerError::Standard {
                code: -32602,
                message: "Invalid params",
            })?;
        let project = load_project(params)?;
        if self.compiler.is_none() {
            self.compiler = Some(Compiler::new().map_err(|error| HandlerError::Lpp {
                kind: "providerFailure",
                details: json!({ "code": "compiler-init" }),
                message: format!("cannot initialize compiler: {error}"),
            })?);
        }
        let report = self
            .compiler
            .as_ref()
            .expect("compiler initialized")
            .compile_source_report_with_language(
                &project.source,
                &path_string(&project.main_path),
                &project.root,
                &project.locale,
            );
        let diagnostics = compile_diagnostics(&project, &report.compile.diagnostics);
        let artifact = (report.compile.status == opy_rs::CompileStatus::Success).then(|| {
            json!({
                "format": WORKSHOP_ARTIFACT_FORMAT,
                "content": report.compile.workshop_exact,
            })
        });
        Ok(json!({ "diagnostics": diagnostics, "artifact": artifact }))
    }
}

fn load_project(params: ProjectParams) -> Result<LoadedProject, HandlerError> {
    let ProjectParams {
        documents,
        project_root: _project_root,
        entry_uri,
        entry,
        locale,
    } = params;
    let mut documents = documents.unwrap_or_default();
    validate_documents(&documents)?;
    let selected = entry_uri.or_else(|| entry.and_then(EntryTarget::value));

    let selected_for_loading = selected.as_deref();
    let (main_path, source) = match selected_for_loading {
        Some(uri) => {
            let path = filesystem_path(uri)
                .ok_or_else(|| HandlerError::invalid_document(Some(uri), "entryMustBeFileUri"))?;
            let canonical = path
                .canonicalize()
                .map_err(|_| HandlerError::invalid_document(Some(uri), "entryNotFound"))?;
            let source = documents
                .get(uri)
                .filter(|document| document.uri == uri)
                .map(|document| document.text.clone())
                .map_or_else(
                    || {
                        std::fs::read_to_string(&canonical).map_err(|_| {
                            HandlerError::invalid_document(Some(uri), "entryUnreadable")
                        })
                    },
                    Ok,
                )?;
            (canonical, source)
        }
        None if documents.len() == 1 => {
            let (_, document) = documents.iter().next().expect("one document");
            let path = filesystem_path(&document.uri).ok_or_else(|| {
                HandlerError::invalid_document(Some(&document.uri), "documentMustBeFileUri")
            })?;
            (path, document.text.clone())
        }
        None => {
            return Err(HandlerError::refusal(
                "project.requiresEntry",
                json!({}),
                "an OPY project request requires one entry URI",
            ));
        }
    };
    let root = main_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    // URI and resolved path are both useful overlay keys to the frontend.
    if let Some(uri) = selected {
        if let Some(document) = documents.remove(&uri) {
            documents.insert(path_to_file_uri(&main_path), document);
        }
    }
    Ok(LoadedProject {
        source,
        main_path,
        root,
        documents,
        locale: locale.unwrap_or_else(|| "en-US".to_string()),
    })
}

fn validate_documents(documents: &BTreeMap<String, Document>) -> Result<(), HandlerError> {
    for (key, document) in documents {
        if key != &document.uri {
            return Err(HandlerError::invalid_document(
                Some(&document.uri),
                "uriKeyMismatch",
            ));
        }
        if document.language_id != LANGUAGE_ID {
            return Err(HandlerError::Lpp {
                kind: "invalidLanguage",
                details: json!({ "languageId": document.language_id }),
                message: format!("language not served by provider: {}", document.language_id),
            });
        }
        if document.version < 0 {
            return Err(HandlerError::invalid_document(
                Some(&document.uri),
                "negativeVersion",
            ));
        }
    }
    Ok(())
}

fn check_result(project: &LoadedProject, outcome: &CheckOutcome) -> Value {
    let mut entries = file_entries(
        project,
        &outcome
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>(),
    );
    for diagnostic in &outcome.diagnostics {
        let path = diagnostic.span.as_ref().map(|span| span.path.as_str());
        let index = path
            .and_then(|path| entries.iter().position(|entry| entry.path == path))
            .unwrap_or(0);
        entries[index]
            .diagnostics
            .push(diagnostic_json(project, diagnostic));
    }
    json!({
        "documents": entries
            .into_iter()
            .map(|entry| entry.json(project))
            .collect::<Vec<_>>()
    })
}

fn compile_diagnostics(project: &LoadedProject, diagnostics: &[CompileDiagnostic]) -> Vec<Value> {
    let paths = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.span.as_ref().map(|span| span.path.clone()))
        .collect::<Vec<_>>();
    let mut entries = file_entries(project, &paths);
    for diagnostic in diagnostics {
        let path = diagnostic.span.as_ref().map(|span| span.path.as_str());
        let index = path
            .and_then(|path| entries.iter().position(|entry| entry.path == path))
            .unwrap_or(0);
        entries[index]
            .diagnostics
            .push(compile_diagnostic_json(project, diagnostic));
    }
    entries
        .into_iter()
        .map(|entry| entry.json(project))
        .collect()
}

#[derive(Debug)]
struct FileEntry {
    path: String,
    version: i64,
    diagnostics: Vec<Value>,
}

impl FileEntry {
    fn json(self, project: &LoadedProject) -> Value {
        json!({
            "uri": path_to_file_uri(&resolved_path(project, &self.path)),
            "version": self.version,
            "diagnostics": self.diagnostics,
        })
    }
}

fn file_entries(project: &LoadedProject, paths: &[String]) -> Vec<FileEntry> {
    let mut paths = paths.to_vec();
    if paths.is_empty() {
        paths.push(path_string(&project.main_path));
    }
    paths.dedup();
    paths
        .into_iter()
        .map(|path| FileEntry {
            version: version_for_path(project, &path),
            path,
            diagnostics: Vec::new(),
        })
        .collect()
}

fn version_for_path(project: &LoadedProject, path: &str) -> i64 {
    let path = resolved_path(project, path);
    project
        .documents
        .values()
        .find(|document| {
            filesystem_path(&document.uri)
                .map(|document_path| resolved_path(project, &path_string(&document_path)) == path)
                .unwrap_or(false)
        })
        .map_or(0, |document| document.version)
}

fn diagnostic_json(project: &LoadedProject, diagnostic: &OpyDiagnostic) -> Value {
    json!({
        "range": diagnostic_range(project, diagnostic.span.as_ref()),
        "severity": "error",
        "code": diagnostic.code,
        "message": diagnostic.message,
        "source": LANGUAGE_ID,
    })
}

fn compile_diagnostic_json(project: &LoadedProject, diagnostic: &CompileDiagnostic) -> Value {
    json!({
        "range": diagnostic_range(project, diagnostic.span.as_ref()),
        "severity": "error",
        "code": diagnostic.code,
        "message": diagnostic.message,
        "source": LANGUAGE_ID,
    })
}

fn diagnostic_range(project: &LoadedProject, location: Option<&SourceLocation>) -> Value {
    let Some(location) = location else {
        return json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 0 },
        });
    };
    json!({
        "start": lsp_position(project, &location.path, location.start.line, location.start.col),
        "end": lsp_position(project, &location.path, location.end.line, location.end.col),
    })
}

fn lsp_position(project: &LoadedProject, path: &str, line: u32, col: u32) -> Value {
    let source = if path == path_string(&project.main_path) {
        project.source.clone()
    } else {
        std::fs::read_to_string(resolved_path(project, path)).unwrap_or_default()
    };
    let character = source
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .map(|text| {
            text.chars()
                .take(col.saturating_sub(1) as usize)
                .map(char::len_utf16)
                .sum::<usize>() as u32
        })
        .unwrap_or_else(|| col.saturating_sub(1));
    json!({
        "line": line.saturating_sub(1),
        "character": character,
    })
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn resolved_path(project: &LoadedProject, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project.root.join(path)
    }
}

fn filesystem_path(value: &str) -> Option<PathBuf> {
    if let Some(path) = value.strip_prefix("file://") {
        let path = if path.starts_with('/') {
            path
        } else {
            return None;
        };
        return Some(PathBuf::from(percent_decode(path)?));
    }
    Some(PathBuf::from(value))
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes
                .get(index + 1)
                .and_then(|byte| (*byte as char).to_digit(16))?;
            let low = bytes
                .get(index + 2)
                .and_then(|byte| (*byte as char).to_digit(16))?;
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn path_to_file_uri(path: &Path) -> String {
    let raw = path_string(path);
    let mut uri = String::from("file://");
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push('%');
            uri.push(
                char::from_digit(u32::from(byte >> 4), 16)
                    .expect("hex digit")
                    .to_ascii_uppercase(),
            );
            uri.push(
                char::from_digit(u32::from(byte & 0x0f), 16)
                    .expect("hex digit")
                    .to_ascii_uppercase(),
            );
        }
    }
    uri
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn standard_error(id: Value, code: i64, message: &'static str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn lpp_error(id: Value, kind: &'static str, details: Value, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32000,
            "message": message.into(),
            "data": { "lpp": { "kind": kind, "details": details } },
        },
    })
}

fn error_response(id: Value, error: HandlerError) -> Value {
    match error {
        HandlerError::Lpp {
            kind,
            details,
            message,
        } => lpp_error(id, kind, details, message),
        HandlerError::Standard { code, message } => standard_error(id, code, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn file_uri_round_trips_spaces_and_unicode() {
        let path = Path::new("/project/中文 file.opy");
        assert_eq!(
            filesystem_path(&path_to_file_uri(path)),
            Some(path.to_path_buf())
        );
    }

    #[test]
    fn initialize_advertises_only_implemented_capabilities() {
        let mut server = Server {
            initialized: false,
            exiting: false,
            capabilities: Capabilities::first_party(),
            compiler: None,
        };
        let response = server.handle_message(
            r#"{"jsonrpc":"2.0","id":1,"method":"lpp/initialize","params":{"protocolVersion":"1.0"}}"#,
        ).expect("response");
        assert_eq!(response["result"]["languages"][0]["id"], LANGUAGE_ID);
        assert_eq!(response["result"]["capabilities"]["check"], true);
        assert_eq!(response["result"]["capabilities"]["compile"], true);
        assert_eq!(response["result"]["capabilities"]["rename"], false);
    }

    #[test]
    fn entry_loads_filesystem_project_without_documents() {
        let dir = std::env::temp_dir().join(format!("opy-provider-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("test directory");
        let entry = dir.join("main.opy");
        fs::write(&entry, "rule \"r\":\n    @Event global\n").expect("entry");
        let params = ProjectParams {
            documents: None,
            project_root: None,
            entry_uri: Some(path_to_file_uri(&entry)),
            entry: None,
            locale: None,
        };
        let project = load_project(params).expect("project loads");
        assert_eq!(project.source, "rule \"r\":\n    @Event global\n");
        assert!(project.documents.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
