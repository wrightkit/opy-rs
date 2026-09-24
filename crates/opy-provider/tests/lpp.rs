//! Process-level contract tests for the first-party OPY provider.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

const MULTI_FILE_MAIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/multi-file/main.opy"
);
const CLEAN_MULTI_FILE_MAIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/project-preprocessing/main.opy"
);
const PROJECT_PREPROCESSING_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/project-preprocessing"
);
const BASIC_RULE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/corpus/synthetic/basic-rule/source.opy"
);
const MAIN_FILE_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/corpus/synthetic/project-main-file/source.opy"
);
const MAIN_FILE_ERROR_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/project-main-file-error/error-source.opy"
);
const UNSUPPORTED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/corpus/synthetic/directives/source.opy"
);
const DIAGNOSTICS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/corpus/synthetic/diagnostics/source.opy"
);

struct Session {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Session {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_opy-provider"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("provider spawns");
        Self {
            input: child.stdin.take().expect("provider stdin"),
            output: BufReader::new(child.stdout.take().expect("provider stdout")),
            child,
        }
    }

    fn request(&mut self, request: Value) -> Value {
        writeln!(self.input, "{request}").expect("write request");
        self.input.flush().expect("flush request");
        let mut line = String::new();
        self.output.read_line(&mut line).expect("read response");
        serde_json::from_str(&line).expect("response is JSON")
    }

    fn initialize(&mut self) -> Value {
        self.initialize_version("1.1")
    }

    fn initialize_version(&mut self, version: &str) -> Value {
        self.request(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "lpp/initialize",
            "params": { "protocolVersion": version },
        }))
    }

    fn shutdown(mut self) {
        let response = self.request(json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "lpp/shutdown",
            "params": {},
        }));
        assert_eq!(response["result"], Value::Null);
        assert_eq!(self.child.wait().expect("provider exits").code(), Some(0));
    }
}

fn file_uri(path: &str) -> String {
    let path = Path::new(path).canonicalize().expect("fixture path");
    format!("file://{}", path.to_string_lossy())
}

#[test]
fn entry_check_loads_the_owner_project_closure_without_documents() {
    let mut session = Session::spawn();
    let initialized = session.initialize();
    assert_eq!(initialized["result"]["languages"][0]["id"], "opy");
    assert_eq!(
        initialized["result"]["languages"][0]["extensions"],
        json!(["opy"])
    );
    assert_eq!(initialized["result"]["protocolVersion"], "1.1");
    assert_eq!(initialized["result"]["capabilities"]["check"], true);
    assert_eq!(initialized["result"]["capabilities"]["compile"], true);
    assert_eq!(
        initialized["result"]["capabilities"]["projectLoading"],
        true
    );
    assert_eq!(initialized["result"]["capabilities"]["symbols"], false);

    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(MULTI_FILE_MAIN),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    let documents = checked["result"]["documents"]
        .as_array()
        .expect("documents");
    assert_eq!(documents.len(), 2, "main plus reachable include");
    assert!(documents.iter().all(|document| document["version"] == 7));
    assert!(
        documents
            .iter()
            .all(|document| document["diagnostics"] == json!([]))
    );
    assert!(documents.iter().any(|document| {
        document["uri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("/shared/defs.opy"))
    }));

    let compiled = session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/compile",
        "params": {
            "entry": {
                "uri": file_uri(CLEAN_MULTI_FILE_MAIN),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    let compiled_documents = compiled["result"]["diagnostics"]
        .as_array()
        .expect("compile diagnostics documents");
    assert_eq!(compiled_documents.len(), 5, "all reachable source files");
    assert!(
        compiled_documents
            .iter()
            .all(|document| { document["version"] == 7 && document["diagnostics"] == json!([]) })
    );
    session.shutdown();
}

#[test]
fn directory_target_uses_the_owner_default_entry_without_client_discovery() {
    let mut session = Session::spawn();
    let initialized = session.request(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "lpp/initialize",
        "params": { "protocolVersion": "1.2" },
    }));
    assert_eq!(initialized["result"]["protocolVersion"], "1.2");
    assert_eq!(
        initialized["result"]["capabilities"]["projectLoading"],
        true
    );

    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(PROJECT_PREPROCESSING_ROOT),
                "languageId": "opy",
                "version": 9,
                "kind": "directory"
            }
        }
    }));
    let documents = checked["result"]["documents"]
        .as_array()
        .expect("documents");
    assert!(documents.iter().any(|document| {
        document["uri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("/project-preprocessing/main.opy"))
    }));
    assert!(documents.iter().all(|document| document["version"] == 9));
    session.shutdown();
}

#[test]
fn project_target_kind_obeys_protocol_and_filesystem_boundaries() {
    let mut session = Session::spawn();
    session.initialize();
    let rejected = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(PROJECT_PREPROCESSING_ROOT),
                "languageId": "opy",
                "version": 9,
                "kind": "directory"
            }
        }
    }));
    assert_eq!(rejected["error"]["data"]["lpp"]["kind"], "invalidEntry");
    assert_eq!(
        rejected["error"]["data"]["lpp"]["details"]["reason"],
        "unsupportedKind"
    );
    session.shutdown();

    let mut session = Session::spawn();
    let initialized = session.request(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "lpp/initialize",
        "params": { "protocolVersion": "1.2" },
    }));
    assert_eq!(initialized["result"]["protocolVersion"], "1.2");

    let file_as_directory = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(CLEAN_MULTI_FILE_MAIN),
                "languageId": "opy",
                "version": 9,
                "kind": "directory"
            }
        }
    }));
    assert_eq!(
        file_as_directory["error"]["data"]["lpp"]["details"]["reason"],
        "targetNotDirectory"
    );

    let directory_as_file = session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(PROJECT_PREPROCESSING_ROOT),
                "languageId": "opy",
                "version": 9,
                "kind": "file"
            }
        }
    }));
    assert_eq!(
        directory_as_file["error"]["data"]["lpp"]["details"]["reason"],
        "entryNotFile"
    );

    let unknown_kind = session.request(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(CLEAN_MULTI_FILE_MAIN),
                "languageId": "opy",
                "version": 9,
                "kind": "workspace"
            }
        }
    }));
    assert_eq!(unknown_kind["error"]["data"]["lpp"]["kind"], "invalidEntry");

    let omitted_kind = session.request(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(CLEAN_MULTI_FILE_MAIN),
                "languageId": "opy",
                "version": 9
            }
        }
    }));
    assert!(omitted_kind["result"]["documents"].is_array());
    session.shutdown();
}

#[test]
fn compile_returns_canonical_workshop_text_and_no_artifact_on_error() {
    let mut session = Session::spawn();
    session.initialize();

    let compiled = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/compile",
        "params": {
            "entry": {
                "uri": file_uri(BASIC_RULE),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    assert_eq!(
        compiled["result"]["artifact"]["format"],
        "workshop-rs/text-v1"
    );
    assert_eq!(
        compiled["result"]["sourceIdentity"]
            .as_str()
            .expect("source identity"),
        "3f39a0286864f70ce5b5369cbaf663037b1def9984c4e4a0da3da2bb5553dcc4"
    );
    assert!(
        compiled["result"]["artifact"]["content"]
            .as_str()
            .expect("workshop text")
            .starts_with("rule (\"setup\")")
    );

    let failed = session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/compile",
        "params": {
            "entry": {
                "uri": file_uri(UNSUPPORTED),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    assert_eq!(failed["result"]["artifact"], Value::Null);
    assert!(
        !failed["result"]["diagnostics"][0]["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .is_empty()
    );
    session.shutdown();
}

#[test]
fn compile_source_identity_uses_effective_main_file() {
    let mut session = Session::spawn();
    session.initialize();

    let compiled = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/compile",
        "params": {
            "entry": {
                "uri": file_uri(MAIN_FILE_ENTRY),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    assert_eq!(
        compiled["result"]["sourceIdentity"],
        "339950d8191b54ed0aea8cad0e01d544db92c2113be7af0019b15414325b513c"
    );
    session.shutdown();
}

#[test]
fn compile_error_source_identity_uses_effective_main_file() {
    let mut session = Session::spawn();
    session.initialize();

    let compiled = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/compile",
        "params": {
            "entry": {
                "uri": file_uri(MAIN_FILE_ERROR_ENTRY),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    assert_eq!(
        compiled["result"]["sourceIdentity"],
        "643cff51fa14f88a2f18e711820c020a24a859e7f01f3730c95a0709f76f1f33"
    );
    assert!(compiled["result"]["artifact"].is_null());
    assert!(
        compiled["result"]["diagnostics"]
            .as_array()
            .expect("diagnostic documents")
            .iter()
            .any(|document| {
                !document["diagnostics"]
                    .as_array()
                    .expect("diagnostics")
                    .is_empty()
            })
    );
    session.shutdown();
}

#[test]
fn single_document_v1_requests_keep_the_client_version() {
    let mut session = Session::spawn();
    let initialized = session.request(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "lpp/initialize",
        "params": { "protocolVersion": "1.0" },
    }));
    assert_eq!(initialized["result"]["protocolVersion"], "1.0");
    let path = Path::new(BASIC_RULE).canonicalize().expect("fixture path");
    let uri = file_uri(BASIC_RULE);
    let source = std::fs::read_to_string(&path).expect("fixture source");
    let compiled = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/compile",
        "params": {
            "documents": {
                &uri: {
                    "uri": &uri,
                    "languageId": "opy",
                    "version": 7,
                    "text": source,
                },
            },
        },
    }));
    assert_eq!(compiled["result"]["diagnostics"][0]["version"], 7);
    assert!(compiled["result"]["artifact"].is_object());
    session.shutdown();
}

#[test]
fn check_preserves_owner_diagnostic_identity_and_range() {
    let mut session = Session::spawn();
    session.initialize();
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(DIAGNOSTICS),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    let document = &checked["result"]["documents"][0];
    assert!(
        document["uri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("/diagnostics/source.opy"))
    );
    assert_eq!(document["version"], 7);
    let diagnostic = &document["diagnostics"][0];
    assert_eq!(diagnostic["severity"], "error");
    assert_eq!(diagnostic["source"], "opy");
    assert_eq!(diagnostic["range"]["start"]["line"], 0);
    session.shutdown();
}

#[test]
fn document_check_analyzes_every_supplied_document() {
    let mut session = Session::spawn();
    session.initialize();
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "documents": {
                "file:///project/clean.opy": {
                    "uri": "file:///project/clean.opy",
                    "languageId": "opy",
                    "version": 3,
                    "text": "rule \"clean\":\n    @Event global\n"
                },
                "file:///project/broken.opy": {
                    "uri": "file:///project/broken.opy",
                    "languageId": "opy",
                    "version": 4,
                    "text": "rule \"broken\":\n    @Event global\n    ???\n"
                }
            }
        }
    }));
    let documents = checked["result"]["documents"]
        .as_array()
        .expect("documents");
    assert_eq!(documents.len(), 2);
    let clean = documents
        .iter()
        .find(|document| document["uri"] == "file:///project/clean.opy")
        .expect("clean document");
    assert_eq!(clean["version"], 3);
    assert!(
        clean["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .is_empty()
    );
    let broken = documents
        .iter()
        .find(|document| document["uri"] == "file:///project/broken.opy")
        .expect("broken document");
    assert_eq!(broken["version"], 4);
    assert!(
        !broken["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .is_empty()
    );
    let refused = session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/compile",
        "params": {
            "documents": {
                "file:///project/clean.opy": {
                    "uri": "file:///project/clean.opy",
                    "languageId": "opy",
                    "version": 3,
                    "text": "rule \"clean\":\n    @Event global\n"
                },
                "file:///project/broken.opy": {
                    "uri": "file:///project/broken.opy",
                    "languageId": "opy",
                    "version": 4,
                    "text": "rule \"broken\":\n    @Event global\n    ???\n"
                }
            }
        }
    }));
    assert_eq!(refused["error"]["data"]["lpp"]["kind"], "refusal");
    assert_eq!(
        refused["error"]["data"]["lpp"]["details"]["refusalCode"],
        "compile.requiresSingleDocument"
    );
    session.shutdown();
}

#[test]
fn entry_errors_follow_lpp_11_project_loading_contract() {
    let mut session = Session::spawn();
    session.initialize();
    let missing = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": "file:///project/missing.opy",
                "languageId": "opy",
                "version": 7
            }
        }
    }));
    assert_eq!(missing["error"]["data"]["lpp"]["kind"], "projectLoadFailed");
    assert_eq!(
        missing["error"]["data"]["lpp"]["details"]["entryUri"],
        "file:///project/missing.opy"
    );
    let unsupported = session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": "untitled:entry.opy",
                "languageId": "opy",
                "version": 7
            }
        }
    }));
    assert_eq!(unsupported["error"]["data"]["lpp"]["kind"], "invalidEntry");
    session.shutdown();
}

#[test]
fn lifecycle_and_capability_failures_are_structured() {
    let mut session = Session::spawn();
    let before_initialize = session.request(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "lpp/check",
        "params": {},
    }));
    assert_eq!(before_initialize["error"]["code"], -32000);
    assert_eq!(
        before_initialize["error"]["data"]["lpp"]["kind"],
        "invalidRequest"
    );

    session.initialize();
    let unavailable = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/symbols",
        "params": {},
    }));
    assert_eq!(
        unavailable["error"]["data"]["lpp"]["kind"],
        "capabilityUnavailable"
    );
    assert_eq!(
        unavailable["error"]["data"]["lpp"]["details"]["capability"],
        "symbols"
    );
    session.shutdown();
}

const MAPPED_FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mapped-text");
const REAL_PROJECT_MAIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/corpus/real-world/overpy-cake/source.opy"
);
const TEXT_V1: &str = "workshop-rs/text-v1";
const MAPPED_V1: &str = "workshop-rs/mapped-text-v1";

fn compile_entry(session: &mut Session, path: &str, accepted: Option<Value>) -> Value {
    let mut params = json!({
        "entry": { "uri": file_uri(path), "languageId": "opy", "version": 1 }
    });
    if let Some(accepted) = accepted {
        params["acceptedArtifactFormats"] = accepted;
    }
    session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/compile",
        "params": params,
    }))
}

fn mapped_document(response: &Value) -> Value {
    assert_eq!(response["result"]["artifact"]["format"], MAPPED_V1);
    serde_json::from_str(
        response["result"]["artifact"]["content"]
            .as_str()
            .expect("mapped content is a string"),
    )
    .expect("mapped content is JSON")
}

fn mapped_span<'a>(document: &'a Value, node: &str, keys: &[(&str, usize)]) -> Option<&'a Value> {
    document["spans"]
        .as_array()
        .expect("spans")
        .iter()
        .find(|entry| {
            entry["node"] == node && keys.iter().all(|(key, value)| entry[*key] == json!(value))
        })
        .map(|entry| &entry["span"])
}

fn span_text(span: &Value) -> (u64, u64, u64, u64, u64) {
    let number = |value: &Value| value.as_u64().expect("number");
    (
        number(&span["file"]),
        number(&span["start"]["line"]),
        number(&span["start"]["column"]),
        number(&span["end"]["line"]),
        number(&span["end"]["column"]),
    )
}

#[test]
fn compile_negotiation_is_valid_only_in_lpp_14() {
    let main = format!("{MAPPED_FIXTURES}/unicode.opy");
    let mut session = Session::spawn();
    let initialized = session.initialize_version("1.4");
    assert_eq!(initialized["result"]["protocolVersion"], "1.4");
    assert_eq!(
        initialized["result"]["capabilities"]["sourceIdentity"],
        true
    );

    let absent = compile_entry(&mut session, &main, None);
    assert_eq!(absent["result"]["artifact"]["format"], TEXT_V1);
    let text_only = compile_entry(&mut session, &main, Some(json!([TEXT_V1])));
    assert_eq!(
        text_only["result"]["artifact"],
        absent["result"]["artifact"]
    );
    let unknown_first = compile_entry(&mut session, &main, Some(json!(["x/unknown", TEXT_V1])));
    assert_eq!(
        unknown_first["result"]["artifact"],
        absent["result"]["artifact"]
    );
    let mapped_first = compile_entry(&mut session, &main, Some(json!([MAPPED_V1, TEXT_V1])));
    assert_eq!(
        mapped_document(&mapped_first)["text"],
        absent["result"]["artifact"]["content"]
    );

    let unsupported = compile_entry(&mut session, &main, Some(json!(["x/unknown"])));
    assert_eq!(
        unsupported["error"]["data"]["lpp"]["details"]["refusalCode"],
        "compile.artifactFormatUnsupported"
    );
    for invalid in [
        json!([]),
        json!("workshop-rs/text-v1"),
        json!([1]),
        Value::Null,
    ] {
        let rejected = compile_entry(&mut session, &main, Some(invalid));
        assert_eq!(rejected["error"]["code"], -32602);
    }
    session.shutdown();

    let mut session = Session::spawn();
    session.initialize_version("1.3");
    let rejected = compile_entry(&mut session, &main, Some(json!([MAPPED_V1])));
    assert_eq!(rejected["error"]["code"], -32602);
    let absent = compile_entry(&mut session, &main, None);
    assert_eq!(absent["result"]["artifact"]["format"], TEXT_V1);
    session.shutdown();
}

#[test]
fn mapped_columns_are_unicode_scalar_values() {
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let response = compile_entry(
        &mut session,
        &format!("{MAPPED_FIXTURES}/unicode.opy"),
        Some(json!([MAPPED_V1])),
    );
    let document = mapped_document(&response);
    // `    total = "héllo ✓" == "界" and 1` is 30 scalar values from column 5; bytes or
    // UTF-16 units would give a different end column.
    let action = mapped_span(&document, "action", &[("rule", 0), ("action", 0)]).expect("action");
    assert_eq!(span_text(action), (0, 5, 5, 5, 35));
    session.shutdown();
}

#[test]
fn mapped_includes_carry_their_own_document_uris_and_macros_map_to_the_invocation() {
    let main = format!("{MAPPED_FIXTURES}/main.opy");
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let response = compile_entry(&mut session, &main, Some(json!([MAPPED_V1])));
    let document = mapped_document(&response);
    let files = document["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|file| file["path"].as_str().expect("path").to_owned())
        .collect::<Vec<_>>();
    let main_index = files
        .iter()
        .position(|uri| *uri == file_uri(&main))
        .expect("entry document URI");
    let lib_index = files
        .iter()
        .position(|uri| *uri == file_uri(&format!("{MAPPED_FIXTURES}/lib.opy")))
        .expect("included document URI");

    // The included file's rule is emitted first and maps into `lib.opy`.
    let lib_rule = mapped_span(&document, "rule", &[("rule", 0)]).expect("lib rule");
    assert_eq!(span_text(lib_rule).0, lib_index as u64);
    let main_rule = mapped_span(&document, "rule", &[("rule", 1)]).expect("main rule");
    assert_eq!(span_text(main_rule).0, main_index as u64);

    // `bump(total)` is line 6 of `main.opy`: the macro-expanded modification maps to it,
    // not to the `#!define` in `lib.opy`.
    let expanded = mapped_span(&document, "action", &[("rule", 1), ("action", 1)]).expect("bump");
    let (file, start_line, start_column, end_line, _) = span_text(expanded);
    assert_eq!(
        (file, start_line, start_column, end_line),
        (main_index as u64, 6, 5, 6)
    );
    session.shutdown();
}

#[test]
fn hir_macro_expansions_map_to_the_invocation_site() {
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let response = compile_entry(
        &mut session,
        &format!("{MAPPED_FIXTURES}/macro.opy"),
        Some(json!([MAPPED_V1])),
    );
    let document = mapped_document(&response);
    // `twice(3)` is line 9; its two expanded actions map there, the plain `wait(2)` to line 10.
    for action in 0..2 {
        let span = mapped_span(&document, "action", &[("rule", 0), ("action", action)])
            .expect("expanded action");
        assert_eq!(span_text(span), (0, 9, 5, 9, 13));
    }
    let plain = mapped_span(&document, "action", &[("rule", 0), ("action", 2)]).expect("wait");
    assert_eq!(span_text(plain), (0, 10, 5, 10, 12));
    session.shutdown();
}

#[test]
fn generated_helper_nodes_are_unmapped() {
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let response = compile_entry(
        &mut session,
        &format!("{MAPPED_FIXTURES}/helper.opy"),
        Some(json!([MAPPED_V1])),
    );
    let document = mapped_document(&response);
    let rules = document["shape"]["rules"].as_array().expect("rules");
    // The translation/initializer rule is generated: it precedes the authored rule and has no entry.
    assert!(rules.len() >= 2, "expected a generated helper rule");
    let authored = rules.len() - 1;
    assert!(mapped_span(&document, "rule", &[("rule", authored)]).is_some());
    for helper in 0..authored {
        assert!(mapped_span(&document, "rule", &[("rule", helper)]).is_none());
    }
    session.shutdown();
}

#[test]
fn document_requests_map_to_supplied_document_uris() {
    let path = format!("{MAPPED_FIXTURES}/unicode.opy");
    let uri = file_uri(&path);
    let text = std::fs::read_to_string(&path).expect("fixture");
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let response = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/compile",
        "params": {
            "documents": { uri.clone(): {
                "uri": uri, "languageId": "opy", "version": 3, "text": text,
            } },
            "acceptedArtifactFormats": [MAPPED_V1],
        },
    }));
    let document = mapped_document(&response);
    assert_eq!(document["files"][0]["path"], json!(uri));
    session.shutdown();
}

/// Compile `main` to `mapped-text-v1` and prove the mapping applies to the re-parsed text.
fn assert_mapping_applies_to_reparsed_text(main: &str) {
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let response = compile_entry(&mut session, main, Some(json!([MAPPED_V1])));
    let content = response["result"]["artifact"]["content"]
        .as_str()
        .unwrap_or_else(|| panic!("mapped artifact expected: {response}"));
    let mapped = workshop_rs::program::MappedText::from_json(content).expect("mapped-text-v1");
    let catalog = workshop_rs::catalog::Catalog::builtin().expect("catalog");
    let mut program = workshop_rs::parser::parse(
        &mapped.text,
        &catalog,
        &workshop_rs::catalog::Locale::new("en-US"),
    )
    .expect("emitted text parses");
    mapped
        .map
        .apply(&mut program)
        .expect("mapping applies without shape mismatch");
    assert!(
        (0..program.rules.len()).any(|rule| program.rule_span(rule).is_some()),
        "authored rules are mapped"
    );
    session.shutdown();
}

#[test]
fn real_project_mapping_applies_to_the_reparsed_workshop_text() {
    assert_mapping_applies_to_reparsed_text(REAL_PROJECT_MAIN);
}

/// Set `OPY_BASTION_MAIN` to `src/main.opy` of OWBastion/Bastion at revision
/// c010e1a2d468ec7140f474e334067e5ab8d02d89 to run the pinned real-project check.
#[test]
fn pinned_bastion_mapping_applies_to_the_reparsed_workshop_text() {
    let Ok(main) = std::env::var("OPY_BASTION_MAIN") else {
        return;
    };
    assert_mapping_applies_to_reparsed_text(&main);
}

#[test]
fn unsupported_accepted_formats_refuse_only_when_an_artifact_would_be_returned() {
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let failing = compile_entry(
        &mut session,
        &format!("{MAPPED_FIXTURES}/broken.opy"),
        Some(json!(["x/unknown"])),
    );
    assert_eq!(failing["result"]["artifact"], Value::Null);
    assert!(
        failing["result"]["diagnostics"][0]["diagnostics"]
            .as_array()
            .is_some_and(|diagnostics| !diagnostics.is_empty())
    );
    session.shutdown();
}

#[test]
fn macro_definition_spans_stay_in_the_shared_lowering() {
    let path = format!("{MAPPED_FIXTURES}/macro.opy");
    let source = std::fs::read_to_string(&path).expect("fixture");
    let compiler = opy_rs::Compiler::new().expect("compiler");
    let artifact = compiler
        .compile_source_artifact(&source, &path, Path::new(MAPPED_FIXTURES))
        .expect("compiles");
    // Only the mapped artifact relocates expansions; the shared lowering (which diagnostics
    // read) keeps the macro body span on line 4.
    let span = artifact
        .wir
        .action_span(0, 0)
        .expect("expanded action span");
    assert_eq!(span.start.line, 4);
}
