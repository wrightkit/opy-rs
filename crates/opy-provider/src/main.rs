//! First-party OPY Language Provider Protocol process.
//!
//! The process is deliberately a thin owner-side adapter: OPY project loading,
//! preprocessing, diagnostics, and compilation remain in `opy-rs`. Only the
//! LPP envelope and source-oriented projections live here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};

use opy_rs::lookup::{
    LookupHit, LookupOutcome, LookupParam, LookupQuery, LookupScope, LookupWithin,
    SettingValueForm, lookup as opy_lookup,
};
use opy_rs::tooling::{CheckOutcome, Diagnostic as OpyDiagnostic, SourceLocation};
use opy_rs::{CompileDiagnostic, Compiler};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use workshop_rs::program::MappedText;

mod edits;

const PROTOCOL_VERSIONS: [&str; 7] = ["1.0", "1.1", "1.2", "1.3", "1.4", "1.5", "1.6"];
const PROJECT_LOADING_VERSION: &str = "1.1";
const DIRECTORY_TARGET_VERSION: &str = "1.2";
const SOURCE_IDENTITY_VERSION: &str = "1.3";
const ARTIFACT_NEGOTIATION_VERSION: &str = "1.4";
const LOOKUP_VERSION: &str = "1.5";
const RESOLVE_IDS_VERSION: &str = "1.6";
const SERVER_NAME: &str = "opy-provider";
const LANGUAGE_ID: &str = "opy";
const LANGUAGE_EXTENSIONS: [&str; 1] = ["opy"];
// The payload is canonical Workshop text; the envelope remains opaque to LPP.
const WORKSHOP_ARTIFACT_FORMAT: &str = "workshop-rs/text-v1";
const MAPPED_ARTIFACT_FORMAT: &str = "workshop-rs/mapped-text-v1";

/// Whether the negotiated `protocol_version` is `minimum` or later.
fn version_at_least(protocol_version: &str, minimum: &str) -> bool {
    let rank = |version: &str| PROTOCOL_VERSIONS.iter().position(|known| *known == version);
    rank(protocol_version) >= rank(minimum)
}

#[derive(Debug, Clone, Copy)]
struct Capabilities {
    check: bool,
    compile: bool,
    project_loading: bool,
    source_identity: bool,
    rename: bool,
    edit_validation: bool,
    lookup: bool,
    resolve_ids: bool,
}

impl Capabilities {
    const fn first_party() -> Self {
        Self {
            check: true,
            compile: true,
            project_loading: true,
            source_identity: true,
            rename: true,
            edit_validation: true,
            lookup: true,
            resolve_ids: true,
        }
    }

    fn enabled(self, capability: &str) -> bool {
        match capability {
            "check" => self.check,
            "compile" => self.compile,
            "projectLoading" => self.project_loading,
            "rename" => self.rename,
            "editValidation" => self.edit_validation,
            "lookup" => self.lookup,
            "resolveIds" => self.resolve_ids,
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
            "rename": self.rename,
            "editValidation": self.edit_validation,
        });
        if version_at_least(protocol_version, PROJECT_LOADING_VERSION) {
            capabilities["projectLoading"] = json!(self.project_loading);
        }
        if version_at_least(protocol_version, SOURCE_IDENTITY_VERSION) {
            capabilities["sourceIdentity"] = json!(self.source_identity);
        }
        if version_at_least(protocol_version, LOOKUP_VERSION) {
            capabilities["lookup"] = json!(self.lookup);
        }
        if version_at_least(protocol_version, RESOLVE_IDS_VERSION) {
            capabilities["resolveIds"] = json!(self.resolve_ids);
        }
        capabilities
    }
}

/// The minimum protocol version a method capability can be negotiated at,
/// when the method does not exist in earlier sessions.
fn capability_min_version(capability: &str) -> Option<&'static str> {
    Some(match capability {
        "lookup" => LOOKUP_VERSION,
        "resolveIds" => RESOLVE_IDS_VERSION,
        _ => return None,
    })
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
        "lpp/lookup" => "lookup",
        "lpp/resolveIds" => "resolveIds",
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
    fn invalid_params() -> Self {
        Self::Standard {
            code: -32602,
            message: "Invalid params",
        }
    }

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
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum ProjectTargetKind {
    File,
    Directory,
}

/// `lpp/lookup` params (LPP 1.5 `name-lookup.md` §21.1).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LookupParams {
    language_id: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    within: Option<LookupWithinParam>,
    #[allow(dead_code)]
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
}

/// One `within` selector: `{ "kind": "...", "value": "..." }`; `kind` is
/// validated against the closed set in `lookup_within`.
#[derive(Debug, Deserialize)]
struct LookupWithinParam {
    kind: String,
    value: String,
}

/// `lpp/resolveIds` params (LPP 1.6 `resolve-ids.md` §22.1).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveIdsParams {
    language_id: String,
    #[serde(default)]
    calls: Vec<String>,
    #[serde(default)]
    enums: BTreeMap<String, Vec<String>>,
}

/// The bound applied when an `lpp/lookup` request omits `limit`.
const DEFAULT_LOOKUP_LIMIT: u64 = 20;

/// The most ids (calls plus enum members, duplicates counted) one
/// `lpp/resolveIds` request may carry (`resolve-ids.md` §22.1).
const RESOLVE_IDS_MAX_IDS: usize = 1024;

#[derive(Debug)]
struct LoadedProject {
    filesystem: opy_rs::FilesystemProject,
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
            && !version_at_least(
                self.protocol_version.as_deref().expect("initialized"),
                PROJECT_LOADING_VERSION,
            )
        {
            return lpp_error(
                id,
                "capabilityUnavailable",
                json!({ "capability": "projectLoading", "method": method }),
                "capability 'projectLoading' is not available",
            );
        }
        if !self.capabilities.enabled(capability)
            || capability_min_version(capability).is_some_and(|minimum| {
                !version_at_least(
                    self.protocol_version.as_deref().expect("initialized"),
                    minimum,
                )
            })
        {
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
            "lpp/rename" => edits::rename(params),
            "lpp/validateEdits" => edits::validate_edits(params),
            "lpp/lookup" => self.lookup(params),
            "lpp/resolveIds" => self.resolve_ids(params),
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
        match load_project(
            params,
            self.protocol_version.as_deref().expect("initialized"),
        )? {
            LoadedRequest::Entry(project) => {
                let outcome = opy_rs::tooling::check(
                    project.filesystem.source(),
                    &path_string(project.filesystem.main_path()),
                    project.filesystem.root(),
                );
                ensure_entry_sources_loaded(&project, &outcome.diagnostics)?;
                Ok(check_result(&project, &outcome))
            }
            LoadedRequest::Documents(request) => check_documents(&request.documents),
        }
    }

    fn compile(&mut self, value: Value) -> Result<Value, HandlerError> {
        let accepted = accepted_artifact_formats(
            &value,
            self.protocol_version.as_deref().expect("initialized"),
        )?;
        let format = select_artifact_format(accepted.as_deref());
        let params: ProjectParams =
            serde_json::from_value(value).map_err(|_| HandlerError::Standard {
                code: -32602,
                message: "Invalid params",
            })?;
        let loaded = load_project(
            params,
            self.protocol_version.as_deref().expect("initialized"),
        )?;
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
                return compile_document(self, &request, format);
            }
        };
        if self.compiler.is_none() {
            self.compiler = Some(Compiler::new().map_err(|error| HandlerError::Lpp {
                kind: "providerFailure",
                details: json!({ "code": "compiler-init" }),
                message: format!("cannot initialize compiler: {error}"),
            })?);
        }
        let compiler = self.compiler.as_ref().expect("compiler initialized");
        let main_path = path_string(project.filesystem.main_path());
        let outcome = opy_rs::compile_with_overlay_outcome(
            project.filesystem.source(),
            &main_path,
            project.filesystem.root(),
            &BTreeMap::new(),
        );
        ensure_entry_sources_loaded(&project, &outcome.diagnostics)?;
        let display_root = outcome.display_root.clone();
        let paths = outcome
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let (report, mapped) = if format == Some(ArtifactFormat::Mapped) {
            compiler.compile_outcome_report_mapped_with_language(
                outcome,
                project.filesystem.root(),
                &project.locale,
            )
        } else {
            let report = compiler.compile_outcome_report_with_language(
                outcome,
                project.filesystem.root(),
                &project.locale,
            );
            (report, None)
        };
        let display_root = &display_root;
        let diagnostics =
            compile_diagnostics(&project, display_root, &paths, &report.compile.diagnostics);
        let artifact = (report.compile.status == opy_rs::CompileStatus::Success)
            .then(|| {
                artifact_json(
                    format,
                    &report.compile.workshop_exact,
                    mapped.as_ref(),
                    &|path| path_to_file_uri(&resolved_path(display_root, path)),
                )
            })
            .transpose()?;
        let result = json!({
            "diagnostics": diagnostics,
            "artifact": artifact,
            "sourceIdentity": source_identity(&project)?,
        });
        Ok(result)
    }

    /// `lpp/lookup` — served from the language vocabulary alone; a loaded
    /// project is never required.
    fn lookup(&self, value: Value) -> Result<Value, HandlerError> {
        let within_value = value.get("within").cloned();
        let params: LookupParams =
            serde_json::from_value(value).map_err(|_| HandlerError::invalid_params())?;
        if params.language_id != LANGUAGE_ID {
            return Err(invalid_language(&params.language_id));
        }
        if matches!(params.limit, Some(0)) {
            return Err(HandlerError::invalid_params());
        }
        let within = params.within.as_ref().map(lookup_within).transpose()?;
        // The OPY catalog serves `en-US` display names; that is also the
        // deterministic fallback the spec permits for any requested locale.
        let query = LookupQuery {
            text: params.query.unwrap_or_default(),
            scope: LookupScope::ALL,
            within,
            locale: None,
            limit: usize::MAX,
        };
        let scoped = query.within.is_some();
        let mut outcome = opy_lookup(&query);
        // A settings enum domain answers under `within: "enum"` too.
        if let (LookupOutcome::UnknownWithin { .. }, Some(LookupWithin::Enum(domain))) =
            (&outcome, &query.within)
        {
            outcome = opy_lookup(&LookupQuery {
                within: Some(LookupWithin::SettingEnum(domain.clone())),
                ..query
            });
        }
        match outcome {
            LookupOutcome::Matched { results, .. } => {
                let mut entries: Vec<Value> = results
                    .iter()
                    .map(|hit| lookup_entry(hit, scoped))
                    .collect();
                if let Some(kind) = &params.kind {
                    entries.retain(|entry| entry["kind"] == *kind);
                }
                entries.truncate(
                    usize::try_from(params.limit.unwrap_or(DEFAULT_LOOKUP_LIMIT))
                        .unwrap_or(usize::MAX),
                );
                Ok(json!({ "entries": entries }))
            }
            LookupOutcome::UnknownWithin { .. } => Err(HandlerError::refusal(
                "lookup.unknownWithin",
                json!({ "within": within_value }),
                "within selector names no known scope",
            )),
            LookupOutcome::Unsupported { reason, .. } => Err(HandlerError::Lpp {
                kind: "providerFailure",
                details: json!({ "code": "lookup-unsupported" }),
                message: reason,
            }),
        }
    }

    /// `lpp/resolveIds` — answered from the language vocabulary alone
    /// (`resolve-ids.md` §22.1): the request carries no target and a loaded
    /// project is never required. Ids with no dedicated OPY spelling are
    /// omitted rather than guessed.
    fn resolve_ids(&self, value: Value) -> Result<Value, HandlerError> {
        let params: ResolveIdsParams =
            serde_json::from_value(value).map_err(|_| HandlerError::invalid_params())?;
        if params.language_id != LANGUAGE_ID {
            return Err(invalid_language(&params.language_id));
        }
        let total_ids = params.calls.len() + params.enums.values().map(Vec::len).sum::<usize>();
        if total_ids > RESOLVE_IDS_MAX_IDS {
            return Err(HandlerError::invalid_params());
        }
        let calls: BTreeMap<&String, &str> = params
            .calls
            .iter()
            .filter_map(|id| opy_rs::lookup::call_spelling(id).map(|spelling| (id, spelling)))
            .collect();
        let enums: BTreeMap<&String, BTreeMap<&String, &str>> = params
            .enums
            .iter()
            .filter_map(|(domain, members)| {
                let resolved: BTreeMap<&String, &str> = members
                    .iter()
                    .filter_map(|member| {
                        opy_rs::lookup::enum_member_spelling(domain, member)
                            .map(|spelling| (member, spelling))
                    })
                    .collect();
                (!resolved.is_empty()).then_some((domain, resolved))
            })
            .collect();
        Ok(json!({ "calls": calls, "enums": enums }))
    }
}

// -- lpp/lookup -------------------------------------------------------------

/// The identity prefixes the provider issues and accepts back as `within`
/// values (`name-lookup.md` §21.2: clients must not parse identities).
const CALLABLE_IDENTITY: &str = "opy:callable/";
const ENUM_IDENTITY: &str = "opy:enum/";
const ENUM_MEMBER_IDENTITY: &str = "opy:enum-member/";
const SETTING_IDENTITY: &str = "opy:setting/";
const SETTING_PATH_IDENTITY: &str = "opy:setting-path/";
const PARAM_IDENTITY: &str = "opy:param/";
const EVENT_IDENTITY: &str = "opy:event/";

/// Map a wire `within` selector to the vocabulary scope. `callable`/`enum`
/// values are provider-issued identities (a bare spelling is accepted for
/// convenience); `settings` takes a raw path prefix. A kind outside the
/// closed set is `Invalid params`.
fn lookup_within(within: &LookupWithinParam) -> Result<LookupWithin, HandlerError> {
    let value = within.value.as_str();
    Ok(match within.kind.as_str() {
        "callable" => LookupWithin::Callable(identity_value(value, CALLABLE_IDENTITY).to_string()),
        "enum" => LookupWithin::Enum(identity_value(value, ENUM_IDENTITY).to_string()),
        "settings" => LookupWithin::Settings(value.to_string()),
        _ => return Err(HandlerError::invalid_params()),
    })
}

/// The scope name behind an identity, or the raw value when it is not an
/// `opy:` identity of that kind (an unknown name resolves to an
/// `unknownWithin` refusal downstream, not a param error).
fn identity_value<'a>(value: &'a str, prefix: &str) -> &'a str {
    value.strip_prefix(prefix).unwrap_or(value)
}

/// The last segment of a settings path; a scoped settings listing spells
/// each child by its own segment (`name-lookup.md` §21.3).
fn path_segment(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

/// The LPP wire entry for one `opy_rs::lookup` hit. `scoped` is set inside
/// a `within` listing, where settings children spell their own segment.
fn lookup_entry(hit: &LookupHit, scoped: bool) -> Value {
    match hit {
        LookupHit::Function {
            spelling,
            function_kind,
            receiver,
            params,
            display_name,
            ..
        } => {
            let mut callable = json!({
                "parameters": params.iter().map(callable_param_wire).collect::<Vec<_>>(),
            });
            if let Some(receiver) = receiver {
                callable["receiver"] = serde_json::to_value(receiver).expect("category string");
            }
            json!({
                "identity": format!("{CALLABLE_IDENTITY}{spelling}"),
                "kind": function_kind.as_str(),
                "spelling": spelling,
                "displayName": display_name.as_deref().unwrap_or(spelling),
                "callable": callable,
            })
        }
        LookupHit::EnumDomain {
            domain,
            members,
            display_name,
            ..
        } => {
            let mut entry = json!({
                "identity": format!("{ENUM_IDENTITY}{domain}"),
                "kind": "enum",
                "spelling": domain,
                "displayName": display_name.as_deref().unwrap_or(domain),
            });
            if let Some(members) = members {
                entry["enum"] = json!({
                    "domain": format!("{ENUM_IDENTITY}{domain}"),
                    "members": members
                        .iter()
                        .map(|member| member.spelling.clone())
                        .collect::<Vec<_>>(),
                });
            }
            entry
        }
        LookupHit::EnumMember {
            spelling,
            member,
            display_name,
            ..
        } => json!({
            "identity": format!("{ENUM_MEMBER_IDENTITY}{spelling}"),
            "kind": "enumMember",
            "spelling": spelling,
            "displayName": display_name.as_deref().unwrap_or(member),
        }),
        LookupHit::Setting {
            path,
            display_name,
            value,
            ..
        } => json!({
            "identity": format!("{SETTING_IDENTITY}{path}"),
            "kind": "setting",
            "spelling": if scoped { path_segment(path) } else { path },
            "displayName": display_name.as_deref().unwrap_or(path),
            "setting": setting_wire(value),
        }),
        LookupHit::Parameter {
            callable, param, ..
        } => {
            let mut fact = callable_param_wire(param);
            // The parameter's `name` is the entry spelling (§21.2).
            fact.as_object_mut()
                .expect("parameter object")
                .remove("name");
            json!({
                "identity": format!("{PARAM_IDENTITY}{callable}/{}", param.name),
                "kind": "parameter",
                "spelling": param.name,
                "displayName": param.name,
                "parameter": fact,
            })
        }
        LookupHit::SettingPath { path, .. } => json!({
            "identity": format!("{SETTING_PATH_IDENTITY}{path}"),
            "kind": "settingPath",
            "spelling": path_segment(path),
            "displayName": path,
        }),
        LookupHit::Event {
            spelling,
            accepts_filters,
            display_name,
            ..
        } => json!({
            "identity": format!("{EVENT_IDENTITY}{spelling}"),
            "kind": "event",
            "spelling": spelling,
            "displayName": display_name.as_deref().unwrap_or(spelling),
            "event": { "acceptsFilters": accepts_filters },
        }),
    }
}

/// The wire form of one callable parameter (§21.4): name, type, required,
/// optional default and the enum domain it accepts.
fn callable_param_wire(param: &LookupParam) -> Value {
    let mut wire = json!({
        "name": param.name,
        "type": param.param_type.as_deref().unwrap_or("any"),
        "required": param.required,
    });
    if let Some(default) = &param.default {
        wire["default"] = json!(default);
    }
    if let Some(domain) = &param.domain {
        wire["enum"] = json!({
            "domain": format!("{ENUM_IDENTITY}{domain}"),
            "members": param
                .members
                .as_ref()
                .map(|members| {
                    members
                        .iter()
                        .map(|member| member.spelling.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        });
    }
    wire
}

/// The wire form of a settings value form (§21.2 `setting`).
fn setting_wire(value: &SettingValueForm) -> Value {
    let mut wire = json!({ "type": value.kind.as_str() });
    if let Some(min) = value.min {
        wire["minimum"] = json!(min);
    }
    if let Some(max) = value.max {
        wire["maximum"] = json!(max);
    }
    if let Some(domain) = &value.domain {
        wire["enum"] = json!({
            "domain": format!("{ENUM_IDENTITY}{domain}"),
            "members": value.members,
        });
    }
    wire
}

fn invalid_language(language_id: &str) -> HandlerError {
    HandlerError::Lpp {
        kind: "invalidLanguage",
        details: json!({ "languageId": language_id }),
        message: format!("language not served by provider: {language_id}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArtifactFormat {
    Text,
    Mapped,
}

/// The `acceptedArtifactFormats` of an `lpp/compile` request; valid only in LPP 1.4 sessions.
fn accepted_artifact_formats(
    params: &Value,
    protocol_version: &str,
) -> Result<Option<Vec<String>>, HandlerError> {
    let Some(value) = params.get("acceptedArtifactFormats") else {
        return Ok(None);
    };
    let invalid = || HandlerError::Standard {
        code: -32602,
        message: "Invalid params",
    };
    if !version_at_least(protocol_version, ARTIFACT_NEGOTIATION_VERSION) {
        return Err(invalid());
    }
    let formats = value
        .as_array()
        .filter(|formats| !formats.is_empty())
        .and_then(|formats| {
            formats
                .iter()
                .map(|format| format.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
        })
        .ok_or_else(invalid)?;
    Ok(Some(formats))
}

/// The first accepted format this provider produces; `None` when none is producible.
fn select_artifact_format(accepted: Option<&[String]>) -> Option<ArtifactFormat> {
    let Some(accepted) = accepted else {
        return Some(ArtifactFormat::Text);
    };
    accepted.iter().find_map(|id| match id.as_str() {
        WORKSHOP_ARTIFACT_FORMAT => Some(ArtifactFormat::Text),
        MAPPED_ARTIFACT_FORMAT => Some(ArtifactFormat::Mapped),
        _ => None,
    })
}

/// Build the artifact envelope. `uri_for` turns a mapped file path into its document URI.
fn artifact_json(
    format: Option<ArtifactFormat>,
    text: &str,
    mapped: Option<&MappedText>,
    uri_for: &dyn Fn(&str) -> String,
) -> Result<Value, HandlerError> {
    match (format, mapped) {
        (Some(ArtifactFormat::Text), _) => Ok(json!({
            "format": WORKSHOP_ARTIFACT_FORMAT,
            "content": text,
        })),
        (Some(ArtifactFormat::Mapped), Some(mapped)) => {
            let mut content: Value =
                serde_json::from_str(&mapped.to_json()).expect("mapped text is JSON");
            for file in content["files"].as_array_mut().expect("file table") {
                let uri = uri_for(file["path"].as_str().expect("file path"));
                file["path"] = json!(uri);
            }
            Ok(json!({
                "format": MAPPED_ARTIFACT_FORMAT,
                "content": content.to_string(),
            }))
        }
        _ => Err(HandlerError::refusal(
            "compile.artifactFormatUnsupported",
            json!({}),
            "none of the accepted artifact formats can be produced",
        )),
    }
}

fn source_identity(project: &LoadedProject) -> Result<String, HandlerError> {
    let source = if let Some(path) = effective_primary_source_path(project)? {
        fs::read_to_string(&path).map_err(|error| HandlerError::Lpp {
            kind: "providerFailure",
            details: json!({ "code": "source-identity-read" }),
            message: format!("cannot read effective primary source: {error}"),
        })?
    } else {
        project.filesystem.source().to_owned()
    };
    Ok(hash_source(&source))
}

fn effective_primary_source_path(project: &LoadedProject) -> Result<Option<PathBuf>, HandlerError> {
    let Some(first_line) = project.filesystem.source().lines().next() else {
        return Ok(None);
    };
    let first_line = first_line.trim_end_matches('\r');
    let Some(rest) = first_line.strip_prefix("#!mainFile").map(str::trim) else {
        return Ok(None);
    };
    let main_file = rest
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            rest.strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        });
    let Some(main_file) = main_file else {
        return Ok(None);
    };
    if main_file.is_empty() {
        return Ok(None);
    }
    let path = project
        .filesystem
        .root()
        .join(main_file)
        .canonicalize()
        .map_err(|error| HandlerError::Lpp {
            kind: "providerFailure",
            details: json!({ "code": "source-identity-read" }),
            message: format!("cannot resolve effective primary source: {error}"),
        })?;
    Ok(Some(path))
}

fn hash_source(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn load_project(
    params: ProjectParams,
    protocol_version: &str,
) -> Result<LoadedRequest, HandlerError> {
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
        (None, Some(entry)) => load_entry(entry, locale, protocol_version),
        _ => Err(HandlerError::Standard {
            code: -32602,
            message: "Invalid params",
        }),
    }
}

fn load_entry(
    entry: ProjectEntry,
    locale: Option<String>,
    protocol_version: &str,
) -> Result<LoadedRequest, HandlerError> {
    let target_kind = project_target_kind(&entry, protocol_version)?;
    let target_name = match target_kind {
        ProjectTargetKind::File => "entry",
        ProjectTargetKind::Directory => "target",
    };
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
            "project target must be an absolute file URI",
        )
    })?;
    validate_target_kind(&path, &entry.uri, target_kind)?;
    let filesystem = opy_rs::FilesystemProject::load(&path).map_err(|error| {
        let reason = if error.is_entry_not_found() {
            match target_kind {
                ProjectTargetKind::File => "entryNotFound",
                ProjectTargetKind::Directory => "targetNotFound",
            }
        } else {
            match target_kind {
                ProjectTargetKind::File => "entryUnreadable",
                ProjectTargetKind::Directory => "targetUnreadable",
            }
        };
        HandlerError::project_load_failed(
            &entry.uri,
            reason,
            Some(&entry.uri),
            format!("project {target_name} could not be loaded"),
        )
    })?;
    Ok(LoadedRequest::Entry(LoadedProject {
        filesystem,
        locale: locale.unwrap_or_else(|| "en-US".to_string()),
        entry_uri: entry.uri,
        entry_version: entry.version,
    }))
}

fn project_target_kind(
    entry: &ProjectEntry,
    protocol_version: &str,
) -> Result<ProjectTargetKind, HandlerError> {
    match entry.kind.as_deref() {
        None | Some("file") => Ok(ProjectTargetKind::File),
        Some("directory") if version_at_least(protocol_version, DIRECTORY_TARGET_VERSION) => {
            Ok(ProjectTargetKind::Directory)
        }
        Some("directory") => Err(HandlerError::invalid_entry(
            &entry.uri,
            "unsupportedKind",
            "directory project targets require protocol version 1.2",
        )),
        Some(_) => Err(HandlerError::invalid_entry(
            &entry.uri,
            "unsupportedKind",
            "project target kind is not supported",
        )),
    }
}

fn validate_target_kind(
    path: &Path,
    entry_uri: &str,
    target_kind: ProjectTargetKind,
) -> Result<(), HandlerError> {
    let metadata = fs::metadata(path).map_err(|error| {
        // A path through a regular file names a target that does not exist
        // (`NotADirectory` on Unix, a not-found error on Windows), so it
        // shares the not-found reason with a genuinely absent path (#497).
        let absent = matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        );
        let reason = match (absent, target_kind) {
            (true, ProjectTargetKind::File) => "entryNotFound",
            (true, ProjectTargetKind::Directory) => "targetNotFound",
            (false, ProjectTargetKind::File) => "entryUnreadable",
            (false, ProjectTargetKind::Directory) => "targetUnreadable",
        };
        let target_name = match target_kind {
            ProjectTargetKind::File => "entry",
            ProjectTargetKind::Directory => "target",
        };
        HandlerError::project_load_failed(
            entry_uri,
            reason,
            Some(entry_uri),
            format!("project {target_name} could not be loaded"),
        )
    })?;
    let matches_kind = match target_kind {
        ProjectTargetKind::File => metadata.is_file(),
        ProjectTargetKind::Directory => metadata.is_dir(),
    };
    if matches_kind {
        return Ok(());
    }
    let (reason, message) = match target_kind {
        ProjectTargetKind::File => ("entryNotFile", "project entry is not a file"),
        ProjectTargetKind::Directory => ("targetNotDirectory", "project target is not a directory"),
    };
    Err(HandlerError::project_load_failed(
        entry_uri,
        reason,
        Some(entry_uri),
        message,
    ))
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

/// The effective check entry for `document`: a first-line `#!mainFile`
/// directive resolves to its target when the target exists on disk —
/// documents redirecting to the same entry run the same project parse, so
/// checking them individually would repeat that parse once per document.
/// An unreadable or malformed redirect keeps the document as its own entry,
/// preserving its own `main-file-*` refusal.
pub(crate) fn effective_entry(document: &Document, path: &Path) -> PathBuf {
    let Some(target) = opy_rs::preprocess::main_file_directive(&document.text) else {
        return path.to_path_buf();
    };
    let candidate = path
        .parent()
        .map(|dir| dir.join(&target))
        .unwrap_or_else(|| PathBuf::from(&target));
    match std::fs::canonicalize(&candidate) {
        Ok(canonical) if canonical.is_file() => canonical,
        _ => path.to_path_buf(),
    }
}

/// The lexically normalized tail of an `#!include` target: `.` segments are
/// dropped and `name/..` pairs cancelled. A target that escapes its includer's
/// directory can still name a supplied document under a different include base;
/// matching the remainder by suffix is the safe approximation for deferral.
fn include_remainder(target: &str) -> PathBuf {
    let mut parts: Vec<&str> = Vec::new();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.into_iter().collect()
}

fn check_documents(documents: &BTreeMap<String, Document>) -> Result<Value, HandlerError> {
    let overlays = document_overlays(documents)?;
    let mut diagnostics_by_uri = documents
        .keys()
        .map(|uri| (uri.clone(), Vec::new()))
        .collect::<BTreeMap<String, Vec<Value>>>();

    let mut groups: BTreeMap<PathBuf, Vec<&Document>> = BTreeMap::new();
    for document in documents.values() {
        let path = document_path(document)?;
        groups
            .entry(effective_entry(document, &path))
            .or_default()
            .push(document);
    }

    // One project parse per effective entry. Two disciplines keep the shared
    // parses faithful:
    //
    // - Coverage counts only a *completed* analysis: `CheckOutcome.files` on
    //   a failed check is the registry reached before the failure, not a set
    //   of analyzed documents, so only a clean outcome covers them.
    // - Standalone fallbacks run after the includers, not in path order: a
    //   member that sorts before its root would otherwise report
    //   out-of-context errors the entry parse never produces.
    let canonical =
        |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let supplied: BTreeSet<PathBuf> = documents
        .values()
        .filter_map(|document| document_path(document).ok())
        .map(|path| canonical(&path))
        .collect();
    let mut included: BTreeSet<PathBuf> = BTreeSet::new();
    for document in documents.values() {
        let path = document_path(document)?;
        let Some(base) = path.parent() else {
            continue;
        };
        for target in opy_rs::preprocess::include_directives(&document.text) {
            // `Preprocessor::include` normalizes `\` to `/` before joining —
            // match that spelling so `.\\member.opy` marks its member too.
            let target = target.replace('\\', "/");
            let candidate = canonical(&base.join(&target));
            if supplied.contains(&candidate) {
                included.insert(candidate);
            } else if candidate.is_dir() {
                // `#!include dir` splices every `.opy` file in the directory.
                for member in &supplied {
                    if member.parent() == Some(candidate.as_path()) {
                        included.insert(member.clone());
                    }
                }
            } else {
                // `#!include "../x"` spellings can escape the includer's
                // directory: the preprocessor resolves them against the last
                // macro file's directory (`Preprocessor::include_base`), which
                // this static scan cannot reproduce. Mark every supplied
                // document the normalized remainder could name instead.
                // Over-marking is safe — deferral only delays a document's own
                // check, and an uncovered member still runs afterwards.
                let remainder = include_remainder(&target);
                if remainder.as_os_str().is_empty() {
                    continue;
                }
                for member in &supplied {
                    if member
                        .ancestors()
                        .any(|ancestor| ancestor.ends_with(&remainder))
                    {
                        included.insert(member.clone());
                    }
                }
            }
        }
    }

    let mut deferred: Vec<&Document> = Vec::new();
    let mut covered: BTreeSet<PathBuf> = BTreeSet::new();
    let mut run_check = |representative: &Document,
                         covered: &mut BTreeSet<PathBuf>|
     -> Result<(), HandlerError> {
        let path = document_path(representative)?;
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let outcome = opy_rs::tooling::check_with_overlay(
            &representative.text,
            &path_string(&path),
            &root,
            &overlays,
        );
        if outcome.is_clean() {
            covered.extend(
                outcome
                    .files
                    .iter()
                    .map(|file| canonical(&resolved_path(&outcome.display_root, &file.path))),
            );
        }
        for diagnostic in &outcome.diagnostics {
            let target_uri = diagnostic
                .span
                .as_ref()
                .and_then(|span| {
                    supplied_uri_for_path(documents, &outcome.display_root, &span.path)
                })
                .unwrap_or_else(|| representative.uri.clone());
            let value = diagnostic_json_for_documents(documents, &outcome.display_root, diagnostic);
            let target = diagnostics_by_uri
                .get_mut(&target_uri)
                .expect("diagnostic target is a supplied document");
            if !target.contains(&value) {
                target.push(value);
            }
        }
        Ok(())
    };

    for (entry, members) in groups {
        // The entry document itself is the faithful representative when it is
        // in the set; otherwise any member's redirect reaches the same parse.
        let representative = members
            .iter()
            .copied()
            .find(|document| document_path(document).is_ok_and(|path| same_path(&path, &entry)))
            .unwrap_or_else(|| members[0]);
        // A single document that is its own entry may still be another
        // supplied document's include member; defer those until the roots —
        // redirect groups and non-included self entries — have run.
        let self_entry = members.len() == 1
            && document_path(representative).is_ok_and(|path| same_path(&path, &entry));
        if self_entry && included.contains(&canonical(&entry)) {
            deferred.push(representative);
        } else {
            run_check(representative, &mut covered)?;
        }
    }
    for document in deferred {
        let path = document_path(document)?;
        if !covered.contains(&canonical(&path)) {
            run_check(document, &mut covered)?;
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

fn compile_document(
    server: &mut Server,
    request: &LoadedDocuments,
    format: Option<ArtifactFormat>,
) -> Result<Value, HandlerError> {
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
            .and_then(|span| {
                supplied_uri_for_path(&request.documents, &outcome.display_root, &span.path)
            })
            .unwrap_or_else(|| uri.clone());
        diagnostics_by_uri
            .get_mut(&target_uri)
            .expect("diagnostic target is a supplied document")
            .push(diagnostic_json_for_documents(
                &request.documents,
                &outcome.display_root,
                diagnostic,
            ));
    }
    let compiler = server.compiler.as_ref().expect("compiler initialized");
    let artifact = match outcome.hir.as_ref() {
        Some(hir) => {
            let compiled = if format == Some(ArtifactFormat::Mapped) {
                compiler
                    .compile_hir_mapped(hir)
                    .map(|(artifact, mapped)| (artifact, Some(mapped)))
            } else {
                compiler.compile_hir(hir).map(|artifact| (artifact, None))
            };
            match compiled {
                Ok((artifact, mapped)) => Some(artifact_json(
                    format,
                    &artifact.final_output,
                    mapped.as_ref(),
                    &|path| {
                        supplied_uri_for_path(&request.documents, &outcome.display_root, path)
                            .unwrap_or_else(|| {
                                path_to_file_uri(&resolved_path(&outcome.display_root, path))
                            })
                    },
                )?),
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
            }
        }
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
    diagnostics: &[OpyDiagnostic],
) -> Result<(), HandlerError> {
    if diagnostics.iter().any(|diagnostic| {
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
    display_root: &Path,
    path: &str,
) -> Option<String> {
    let resolved = resolved_path(display_root, path);
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
    display_root: &Path,
    diagnostic: &OpyDiagnostic,
) -> Value {
    json!({
        "range": diagnostic_range_for_documents(documents, display_root, diagnostic.span.as_ref()),
        "severity": diagnostic.severity.as_str(),
        "code": diagnostic.code,
        "message": diagnostic.message,
        "source": LANGUAGE_ID,
    })
}

fn diagnostic_range_for_documents(
    documents: &BTreeMap<String, Document>,
    display_root: &Path,
    location: Option<&SourceLocation>,
) -> Value {
    let Some(location) = location else {
        return json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 0 },
        });
    };
    json!({
        "start": document_lsp_position(documents, display_root, &location.path, location.start.line, location.start.col),
        "end": document_lsp_position(documents, display_root, &location.path, location.end.line, location.end.col),
    })
}

fn document_lsp_position(
    documents: &BTreeMap<String, Document>,
    display_root: &Path,
    path: &str,
    line: u32,
    col: u32,
) -> Value {
    let resolved = resolved_path(display_root, path);
    let source = documents
        .values()
        .find(|document| {
            filesystem_path(&document.uri)
                .is_some_and(|document_path| same_path(&document_path, &resolved))
        })
        .map(|document| document.text.clone())
        .or_else(|| std::fs::read_to_string(&resolved).ok())
        .unwrap_or_default();
    lsp_position(&source, line, col)
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
        entries[index].diagnostics.push(diagnostic_json(
            project,
            &outcome.display_root,
            diagnostic,
        ));
    }
    json!({
        "documents": entries
            .into_iter()
            .map(|entry| entry.json(&outcome.display_root))
            .collect::<Vec<_>>()
    })
}

fn compile_diagnostics(
    project: &LoadedProject,
    display_root: &Path,
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
            .push(compile_diagnostic_json(project, display_root, diagnostic));
    }
    entries
        .into_iter()
        .map(|entry| entry.json(display_root))
        .collect()
}

#[derive(Debug)]
struct FileEntry {
    path: String,
    version: i64,
    diagnostics: Vec<Value>,
}

impl FileEntry {
    fn json(self, display_root: &Path) -> Value {
        json!({
            "uri": path_to_file_uri(&resolved_path(display_root, &self.path)),
            "version": self.version,
            "diagnostics": self.diagnostics,
        })
    }
}

fn file_entries(project: &LoadedProject, paths: &[String]) -> Vec<FileEntry> {
    let mut paths = paths.to_vec();
    if paths.is_empty() {
        paths.push(path_string(project.filesystem.main_path()));
    }
    paths.dedup();
    paths
        .into_iter()
        .map(|path| FileEntry {
            // Filesystem-loaded documents all echo the entry version.
            version: project.entry_version,
            path,
            diagnostics: Vec::new(),
        })
        .collect()
}

fn diagnostic_json(
    project: &LoadedProject,
    display_root: &Path,
    diagnostic: &OpyDiagnostic,
) -> Value {
    json!({
        "range": diagnostic_range(project, display_root, diagnostic.span.as_ref()),
        "severity": diagnostic.severity.as_str(),
        "code": diagnostic.code,
        "message": diagnostic.message,
        "source": LANGUAGE_ID,
    })
}

fn compile_diagnostic_json(
    project: &LoadedProject,
    display_root: &Path,
    diagnostic: &CompileDiagnostic,
) -> Value {
    json!({
        "range": diagnostic_range(project, display_root, diagnostic.span.as_ref()),
        "severity": diagnostic.severity.as_str(),
        "code": diagnostic.code,
        "message": diagnostic.message,
        "source": LANGUAGE_ID,
    })
}

fn diagnostic_range(
    project: &LoadedProject,
    display_root: &Path,
    location: Option<&SourceLocation>,
) -> Value {
    let Some(location) = location else {
        return json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 0 },
        });
    };
    json!({
        "start": project_lsp_position(project, display_root, &location.path, location.start.line, location.start.col),
        "end": project_lsp_position(project, display_root, &location.path, location.end.line, location.end.col),
    })
}

fn project_lsp_position(
    project: &LoadedProject,
    display_root: &Path,
    path: &str,
    line: u32,
    col: u32,
) -> Value {
    let source = if path == path_string(project.filesystem.main_path()) {
        project.filesystem.source().to_owned()
    } else {
        std::fs::read_to_string(resolved_path(display_root, path)).unwrap_or_default()
    };
    lsp_position(&source, line, col)
}

/// The LPP position of a 1-based frontend `(line, col)`: frontend columns
/// expand tabs to four columns, so the column first maps to a character
/// index within the line and only then counts UTF-16 units.
fn lsp_position(source: &str, line: u32, col: u32) -> Value {
    let character = edits::SourceText::new(source)
        .line_text(line.saturating_sub(1) as usize)
        .map(|text| edits::utf16_character(text, col))
        .unwrap_or_else(|| col.saturating_sub(1));
    json!({
        "line": line.saturating_sub(1),
        "character": character,
    })
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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
    // A `scheme:` prefix makes `value` a URI, not a filesystem path — a
    // document URI like `untitled:Untitled-1` must not silently resolve
    // against the provider's working directory. A single letter before ':'
    // is a Windows drive (`C:`), not a scheme.
    if let Some(end) = value.find(':') {
        let scheme = &value[..end];
        if scheme.len() > 1
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            return None;
        }
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
        assert_eq!(response["result"]["capabilities"]["rename"], true);
        assert_eq!(response["result"]["capabilities"]["editValidation"], true);
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
                kind: None,
            }),
            locale: None,
        };
        let LoadedRequest::Entry(project) =
            load_project(params, PROJECT_LOADING_VERSION).expect("project loads")
        else {
            panic!("expected entry project");
        };
        assert_eq!(
            project.filesystem.source(),
            "rule \"r\":\n    @Event global\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
