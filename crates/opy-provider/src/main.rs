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

const PROTOCOL_VERSIONS: [&str; 2] = ["1.0", "1.1"];
const PROJECT_LOADING_VERSION: &str = "1.1";
const SERVER_NAME: &str = "opy-provider";
const LANGUAGE_ID: &str = "opy";
const LANGUAGE_EXTENSIONS: [&str; 1] = ["opy"];
// The payload is canonical Workshop text; the envelope remains opaque to LPP.
const WORKSHOP_ARTIFACT_FORMAT: &str = "workshop-rs/text-v1";

#[derive(Debug, Clone, Copy)]
struct Capabilities {
    check: bool,
    compile: bool,
    project_loading: bool,
}

impl Capabilities {
    const fn first_party() -> Self {
        Self {
            check: true,
            compile: true,
            project_loading: true,
        }
    }

    fn enabled(self, capability: &str) -> bool {
        match capability {
            "check" => self.check,
            "compile" => self.compile,
            "projectLoading" => self.project_loading,
            _ => false,
        }
    }

    fn as_json(self, protocol_version: &str) -> Value {
        let mut capabilities = json!({
            "check": self.check,
            "compile": self.compile,
            "reconstruct": false,
            "symbols": false,
            "definition": false,
            "references": false,
            "rename": false,
            "editValidation": false,
        });
        if protocol_version == PROJECT_LOADING_VERSION {
            capabilities["projectLoading"] = json!(self.project_loading);
        }
        capabilities
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

    fn invalid_entry(uri: &str, reason: &'static str, message: impl Into<String>) -> Self {
        Self::Lpp {
            kind: "invalidEntry",
            details: json!({ "entryUri": uri, "reason": reason }),
            message: message.into(),
        }
    }

    fn project_load_failed(
        entry_uri: &str,
        reason: &'static str,
        uri: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        let mut details = json!({ "entryUri": entry_uri, "reason": reason });
        if let Some(uri) = uri {
            details["uri"] = json!(uri);
        }
        Self::Lpp {
            kind: "projectLoadFailed",
            details,
            message: message.into(),
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
    #[serde(default)]
    entry: Option<ProjectEntry>,
    #[serde(default)]
    locale: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectEntry {
    uri: String,
    language_id: String,
    version: i64,
}

#[derive(Debug)]
struct LoadedProject {
    source: String,
    main_path: PathBuf,
    root: PathBuf,
    documents: BTreeMap<String, Document>,
    locale: String,
    entry_uri: String,
    entry_version: i64,
}

#[derive(Debug)]
struct LoadedDocuments {
    documents: BTreeMap<String, Document>,
}

#[derive(Debug)]
enum LoadedRequest {
    Entry(LoadedProject),
    Documents(LoadedDocuments),
}

struct Server {
    initialized: bool,
    exiting: bool,
    capabilities: Capabilities,
    protocol_version: Option<String>,
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
        protocol_version: None,
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
        if !PROTOCOL_VERSIONS.contains(&params.protocol_version.as_str()) {
            return lpp_error(
                id,
                "protocolVersionMismatch",
                json!({ "supportedProtocolVersions": PROTOCOL_VERSIONS }),
                format!("unsupported protocol version {}", params.protocol_version),
            );
        }
        let protocol_version = params.protocol_version;
        self.initialized = true;
        self.protocol_version = Some(protocol_version.clone());
        ok(
            id,
            json!({
                "protocolVersion": protocol_version,
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "languages": [{
                    "id": LANGUAGE_ID,
                    "extensions": LANGUAGE_EXTENSIONS,
                }],
                "capabilities": self.capabilities.as_json(
                    self.protocol_version.as_deref().expect("protocol version")
                ),
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
        if matches!(method, "lpp/check" | "lpp/compile")
            && params.get("entry").is_some()
            && self.protocol_version.as_deref() != Some(PROJECT_LOADING_VERSION)
        {
            return lpp_error(
                id,
                "capabilityUnavailable",
                json!({ "capability": "projectLoading", "method": method }),
                "capability 'projectLoading' is not available",
            );
        }
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
        match load_project(params)? {
            LoadedRequest::Entry(project) => {
                let outcome = opy_rs::tooling::check(
                    &project.source,
                    &path_string(&project.main_path),
                    &project.root,
                );
                ensure_entry_sources_loaded(&project, &outcome)?;
                Ok(check_result(&project, &outcome))
            }
            LoadedRequest::Documents(request) => check_documents(&request.documents),
        }
    }

    fn compile(&mut self, value: Value) -> Result<Value, HandlerError> {
        let params: ProjectParams =
            serde_json::from_value(value).map_err(|_| HandlerError::Standard {
                code: -32602,
                message: "Invalid params",
            })?;
        let loaded = load_project(params)?;
        let project = match loaded {
            LoadedRequest::Entry(project) => project,
            LoadedRequest::Documents(request) => {
                if request.documents.len() != 1 {
                    return Err(HandlerError::refusal(
                        "compile.requiresSingleDocument",
                        json!({}),
                        "the OPY compiler requires one document",
                    ));
                }
                return compile_document(self, &request);
            }
        };
        if self.compiler.is_none() {
            self.compiler = Some(Compiler::new().map_err(|error| HandlerError::Lpp {
                kind: "providerFailure",
                details: json!({ "code": "compiler-init" }),
                message: format!("cannot initialize compiler: {error}"),
            })?);
        }
        let check_outcome = opy_rs::tooling::check(
            &project.source,
            &path_string(&project.main_path),
            &project.root,
        );
        ensure_entry_sources_loaded(&project, &check_outcome)?;
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
        let paths = check_outcome
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let diagnostics = compile_diagnostics(&project, &paths, &report.compile.diagnostics);
        let artifact = (report.compile.status == opy_rs::CompileStatus::Success).then(|| {
            json!({
                "format": WORKSHOP_ARTIFACT_FORMAT,
                "content": report.compile.workshop_exact,
            })
        });
        Ok(json!({ "diagnostics": diagnostics, "artifact": artifact }))
    }
}

fn load_project(params: ProjectParams) -> Result<LoadedRequest, HandlerError> {
    let ProjectParams {
        documents,
        project_root: _project_root,
        entry,
        locale,
    } = params;
    match (documents, entry) {
        (Some(documents), None) => {
            validate_documents(&documents)?;
            Ok(LoadedRequest::Documents(LoadedDocuments { documents }))
        }
        (None, Some(entry)) => load_entry(entry, locale),
        _ => Err(HandlerError::Standard {
            code: -32602,
            message: "Invalid params",
        }),
    }
}

fn load_entry(entry: ProjectEntry, locale: Option<String>) -> Result<LoadedRequest, HandlerError> {
    if entry.language_id != LANGUAGE_ID {
        return Err(HandlerError::invalid_entry(
            &entry.uri,
            "unsupportedLanguage",
            format!(
                "project entry language is not served: {}",
                entry.language_id
            ),
        ));
    }
    if entry.version < 0 {
        return Err(HandlerError::invalid_entry(
            &entry.uri,
            "invalidVersion",
            "project entry version must be a non-negative integer",
        ));
    }
    let path = file_uri_path(&entry.uri).ok_or_else(|| {
        HandlerError::invalid_entry(
            &entry.uri,
            "unsupportedUri",
            "project entry must be an absolute file URI",
        )
    })?;
    let canonical = path.canonicalize().map_err(|_| {
        HandlerError::project_load_failed(
            &entry.uri,
            "entryNotFound",
            Some(&entry.uri),
            "project entry could not be loaded",
        )
    })?;
    let source = std::fs::read_to_string(&canonical).map_err(|_| {
        HandlerError::project_load_failed(
            &entry.uri,
            "entryUnreadable",
            Some(&entry.uri),
            "project entry could not be loaded",
        )
    })?;
    let root = canonical
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(LoadedRequest::Entry(LoadedProject {
        source,
        main_path: canonical,
        root,
        documents: BTreeMap::new(),
        locale: locale.unwrap_or_else(|| "en-US".to_string()),
        entry_uri: entry.uri,
        entry_version: entry.version,
    }))
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

fn document_path(document: &Document) -> Result<PathBuf, HandlerError> {
    filesystem_path(&document.uri)
        .ok_or_else(|| HandlerError::invalid_document(Some(&document.uri), "documentMustBeFileUri"))
}

fn document_overlays(
    documents: &BTreeMap<String, Document>,
) -> Result<BTreeMap<String, String>, HandlerError> {
    documents
        .values()
        .map(|document| {
            Ok((
                path_string(&document_path(document)?),
                document.text.clone(),
            ))
        })
        .collect()
}

fn check_documents(documents: &BTreeMap<String, Document>) -> Result<Value, HandlerError> {
    let overlays = document_overlays(documents)?;
    let mut diagnostics_by_uri = documents
        .keys()
        .map(|uri| (uri.clone(), Vec::new()))
        .collect::<BTreeMap<String, Vec<Value>>>();

    for (uri, document) in documents {
        let path = document_path(document)?;
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let outcome = opy_rs::tooling::check_with_overlay(
            &document.text,
            &path_string(&path),
            &root,
            &overlays,
        );
        for diagnostic in &outcome.diagnostics {
            let target_uri = diagnostic
                .span
                .as_ref()
                .and_then(|span| supplied_uri_for_path(documents, &root, &span.path))
                .unwrap_or_else(|| uri.clone());
            let value = diagnostic_json_for_documents(documents, &root, diagnostic);
            let target = diagnostics_by_uri
                .get_mut(&target_uri)
                .expect("diagnostic target is a supplied document");
            if !target.contains(&value) {
                target.push(value);
            }
        }
    }

    Ok(json!({
        "documents": documents
            .iter()
            .map(|(uri, document)| json!({
                "uri": uri,
                "version": document.version,
                "diagnostics": diagnostics_by_uri
                    .remove(uri)
                    .expect("diagnostics initialized"),
            }))
            .collect::<Vec<_>>(),
    }))
}

fn compile_document(server: &mut Server, request: &LoadedDocuments) -> Result<Value, HandlerError> {
    let (uri, document) = request.documents.iter().next().expect("one document");
    let path = document_path(document)?;
    let root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let overlays = document_overlays(&request.documents)?;
    if server.compiler.is_none() {
        server.compiler = Some(Compiler::new().map_err(|error| HandlerError::Lpp {
            kind: "providerFailure",
            details: json!({ "code": "compiler-init" }),
            message: format!("cannot initialize compiler: {error}"),
        })?);
    }
    let outcome =
        opy_rs::compile_with_overlay_outcome(&document.text, &path_string(&path), &root, &overlays);
    let mut diagnostics_by_uri = request
        .documents
        .keys()
        .map(|uri| (uri.clone(), Vec::new()))
        .collect::<BTreeMap<String, Vec<Value>>>();
    for diagnostic in &outcome.diagnostics {
        let target_uri = diagnostic
            .span
            .as_ref()
            .and_then(|span| supplied_uri_for_path(&request.documents, &root, &span.path))
            .unwrap_or_else(|| uri.clone());
        diagnostics_by_uri
            .get_mut(&target_uri)
            .expect("diagnostic target is a supplied document")
            .push(diagnostic_json_for_documents(
                &request.documents,
                &root,
                diagnostic,
            ));
    }
    let artifact = match outcome.hir.as_ref() {
        Some(hir) => match server
            .compiler
            .as_ref()
            .expect("compiler initialized")
            .compile_hir(hir)
        {
            Ok(artifact) => Some(json!({
                "format": WORKSHOP_ARTIFACT_FORMAT,
                "content": artifact.final_output,
            })),
            Err(error) => {
                let diagnostic = json!({
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 0 },
                    },
                    "severity": "error",
                    "code": error.diagnostic.code,
                    "message": error.diagnostic.message,
                    "source": LANGUAGE_ID,
                });
                diagnostics_by_uri
                    .get_mut(uri)
                    .expect("diagnostics initialized")
                    .push(diagnostic);
                None
            }
        },
        None => None,
    };
    let diagnostics = request
        .documents
        .iter()
        .map(|(uri, document)| {
            json!({
                "uri": uri,
                "version": document.version,
                "diagnostics": diagnostics_by_uri
                    .remove(uri)
                    .expect("diagnostics initialized"),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "diagnostics": diagnostics, "artifact": artifact }))
}

fn ensure_entry_sources_loaded(
    project: &LoadedProject,
    outcome: &CheckOutcome,
) -> Result<(), HandlerError> {
    if outcome.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code.as_str(),
            "include-not-found" | "main-file-not-found" | "script-not-found"
        )
    }) {
        return Err(HandlerError::project_load_failed(
            &project.entry_uri,
            "requiredSourceUnavailable",
            None,
            "a required OPY project source could not be loaded",
        ));
    }
    Ok(())
}

fn supplied_uri_for_path(
    documents: &BTreeMap<String, Document>,
    root: &Path,
    path: &str,
) -> Option<String> {
    let resolved = resolved_path(root, path);
    documents.iter().find_map(|(uri, document)| {
        let document_path = filesystem_path(&document.uri)?;
        (same_path(&document_path, &resolved)).then(|| uri.clone())
    })
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.canonicalize().unwrap_or_else(|_| left.to_path_buf())
        == right.canonicalize().unwrap_or_else(|_| right.to_path_buf())
}

fn diagnostic_json_for_documents(
    documents: &BTreeMap<String, Document>,
    root: &Path,
    diagnostic: &OpyDiagnostic,
) -> Value {
    json!({
        "range": diagnostic_range_for_documents(documents, root, diagnostic.span.as_ref()),
        "severity": "error",
        "code": diagnostic.code,
        "message": diagnostic.message,
        "source": LANGUAGE_ID,
    })
}

fn diagnostic_range_for_documents(
    documents: &BTreeMap<String, Document>,
    root: &Path,
    location: Option<&SourceLocation>,
) -> Value {
    let Some(location) = location else {
        return json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 0 },
        });
    };
    json!({
        "start": document_lsp_position(documents, root, &location.path, location.start.line, location.start.col),
        "end": document_lsp_position(documents, root, &location.path, location.end.line, location.end.col),
    })
}

fn document_lsp_position(
    documents: &BTreeMap<String, Document>,
    root: &Path,
    path: &str,
    line: u32,
    col: u32,
) -> Value {
    let resolved = resolved_path(root, path);
    let source = documents
        .values()
        .find(|document| {
            filesystem_path(&document.uri)
                .is_some_and(|document_path| same_path(&document_path, &resolved))
        })
        .map(|document| document.text.clone())
        .or_else(|| std::fs::read_to_string(&resolved).ok())
        .unwrap_or_default();
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
    json!({ "line": line.saturating_sub(1), "character": character })
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

fn compile_diagnostics(
    project: &LoadedProject,
    paths: &[String],
    diagnostics: &[CompileDiagnostic],
) -> Vec<Value> {
    let mut entries = file_entries(project, paths);
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
            "uri": path_to_file_uri(&resolved_project_path(project, &self.path)),
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
    if project.entry_version >= 0 {
        return project.entry_version;
    }
    let path = resolved_project_path(project, path);
    project
        .documents
        .values()
        .find(|document| {
            filesystem_path(&document.uri)
                .map(|document_path| {
                    resolved_project_path(project, &path_string(&document_path)) == path
                })
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
        std::fs::read_to_string(resolved_project_path(project, path)).unwrap_or_default()
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

fn resolved_project_path(project: &LoadedProject, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project.root.join(path)
    }
}

fn resolved_path(root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
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

fn file_uri_path(value: &str) -> Option<PathBuf> {
    let path = value.strip_prefix("file://")?;
    if !path.starts_with('/') {
        return None;
    }
    let path = percent_decode(path)?;
    #[cfg(windows)]
    let path = path
        .strip_prefix('/')
        .filter(|path| {
            let bytes = path.as_bytes();
            bytes.first().is_some_and(u8::is_ascii_alphabetic)
                && bytes.get(1) == Some(&b':')
                && bytes.get(2) == Some(&b'/')
        })
        .unwrap_or(&path);
    Some(PathBuf::from(path))
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
    fn file_uri_path_handles_windows_drive_letter_uris() {
        let path = file_uri_path("file:///D:/path/to/project.opy").expect("file URI path");
        let expected = if cfg!(windows) {
            PathBuf::from("D:/path/to/project.opy")
        } else {
            PathBuf::from("/D:/path/to/project.opy")
        };
        assert_eq!(path, expected);
    }

    #[test]
    fn initialize_advertises_only_implemented_capabilities() {
        let mut server = Server {
            initialized: false,
            exiting: false,
            capabilities: Capabilities::first_party(),
            protocol_version: None,
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
            entry: Some(ProjectEntry {
                uri: path_to_file_uri(&entry),
                language_id: LANGUAGE_ID.to_string(),
                version: 7,
            }),
            locale: None,
        };
        let LoadedRequest::Entry(project) = load_project(params).expect("project loads") else {
            panic!("expected entry project");
        };
        assert_eq!(project.source, "rule \"r\":\n    @Event global\n");
        assert!(project.documents.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
