//! Process-level contract tests for the first-party OPY provider.

use std::collections::BTreeMap;
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
const MAIN_FILE_SUBDIR_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/project-main-file-subdir/source.opy"
);
const DIAGNOSTIC_POSITIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/diagnostic-positions/main.opy"
);
const DUPLICATE_INCLUDE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/duplicate-include/main.opy"
);
const UNSUPPORTED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/corpus/synthetic/directives/source.opy"
);
const DIAGNOSTICS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/corpus/synthetic/diagnostics/source.opy"
);
const RENAME_MAIN_FILE_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/rename-main-file"
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
    assert_eq!(initialized["result"]["capabilities"]["rename"], true);
    assert_eq!(
        initialized["result"]["capabilities"]["editValidation"],
        true
    );

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
        "e534d55d330626abcc547e9d70c09af23275de37f0e5e1cdbe125a13a4853526"
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
fn check_project_main_file_error_reports_effective_document_uris() {
    let mut session = Session::spawn();
    session.initialize();
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(MAIN_FILE_ERROR_ENTRY),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    let documents = checked["result"]["documents"]
        .as_array()
        .expect("documents");
    // The file registry resolves against the `#!mainFile` effective directory
    // (`sub/`), not the directory that held the entry source.
    let fixture = Path::new(MAIN_FILE_ERROR_ENTRY)
        .parent()
        .expect("fixture directory");
    assert_eq!(
        documents
            .iter()
            .map(|document| document["uri"].as_str().expect("uri").to_owned())
            .collect::<Vec<_>>(),
        vec![
            file_uri(MAIN_FILE_ERROR_ENTRY),
            file_uri(fixture.join("sub/error-entry.opy").to_str().expect("utf8")),
            file_uri(
                fixture
                    .join("sub/error-include.opy")
                    .to_str()
                    .expect("utf8")
            ),
        ]
    );
    assert!(documents.iter().all(|document| document["version"] == 7));
    assert_eq!(documents[0]["diagnostics"], json!([]));
    let diagnostics = documents[1]["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(diagnostics.len(), 2);
    // `rule "broken"` misses its colon at line 3 column 14; `@Event` is then
    // an unexpected token at line 4 column 5.
    assert_eq!(
        diagnostics[0],
        json!({
            "range": {
                "start": { "line": 2, "character": 13 },
                "end": { "line": 2, "character": 13 },
            },
            "severity": "error",
            "code": "parse-error",
            "message": "expected ':' after the rule name",
            "source": "opy",
        })
    );
    assert_eq!(
        diagnostics[1],
        json!({
            "range": {
                "start": { "line": 3, "character": 4 },
                "end": { "line": 3, "character": 5 },
            },
            "severity": "error",
            "code": "parse-error",
            "message": "expected a top-level declaration (rule/def/globalvar/playervar/subroutine/enum/macro) but found '@'",
            "source": "opy",
        })
    );
    assert_eq!(documents[2]["diagnostics"], json!([]));
    session.shutdown();
}

#[test]
fn check_project_main_file_subdirectory_reports_effective_document_uris() {
    let mut session = Session::spawn();
    session.initialize();
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(MAIN_FILE_SUBDIR_ENTRY),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    let documents = checked["result"]["documents"]
        .as_array()
        .expect("documents");
    let fixture = Path::new(MAIN_FILE_SUBDIR_ENTRY)
        .parent()
        .expect("fixture directory");
    assert_eq!(
        documents
            .iter()
            .map(|document| document["uri"].as_str().expect("uri").to_owned())
            .collect::<Vec<_>>(),
        vec![
            file_uri(MAIN_FILE_SUBDIR_ENTRY),
            file_uri(fixture.join("sub/entry.opy").to_str().expect("utf8")),
            file_uri(fixture.join("sub/defs.opy").to_str().expect("utf8")),
        ]
    );
    assert!(
        documents
            .iter()
            .all(|document| document["version"] == 7 && document["diagnostics"] == json!([]))
    );
    session.shutdown();
}

#[test]
fn document_check_deduplicates_documents_sharing_one_effective_entry() {
    // `rename-main-file`: member.opy and stray.opy redirect to main.opy via
    // first-line `#!mainFile`; caller.opy is included by main.opy and carries
    // no directive of its own. Documents sharing one effective entry must not
    // re-run its project parse per document, diagnostics must stay on the
    // document that produced them, and a document already analyzed inside an
    // entry's include closure must not get a second, out-of-context parse.
    let root = Path::new(RENAME_MAIN_FILE_ROOT);
    let uris: BTreeMap<&str, String> = ["main.opy", "caller.opy", "stray.opy", "env/member.opy"]
        .into_iter()
        .map(|member| (member, file_uri(root.join(member).to_str().expect("utf8"))))
        .collect();
    let document_map = |inject: bool| {
        let mut documents = serde_json::Map::new();
        for (member, uri) in &uris {
            let mut text = std::fs::read_to_string(root.join(member)).expect("fixture text");
            if inject && *member == "env/member.opy" {
                text.push_str("\nrule \"broken\n");
            }
            if inject && *member == "stray.opy" {
                // A redirect-only document's own text is not part of the
                // entry closure: a syntax break here is invisible.
                text.push_str("\nrule \"stray-broken\n");
            }
            documents.insert(
                uri.clone(),
                json!({
                    "uri": uri,
                    "languageId": "opy",
                    "version": 3,
                    "text": text,
                }),
            );
        }
        documents
    };
    let check = |session: &mut Session, id: u64, documents: serde_json::Map<String, Value>| {
        session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/check",
            "params": { "documents": Value::Object(documents) },
        }))["result"]["documents"]
            .as_array()
            .expect("documents")
            .clone()
    };
    let find = |documents: &[Value], member: &str| -> Value {
        documents
            .iter()
            .find(|document| document["uri"] == uris[member])
            .cloned()
            .expect("document present")
    };

    let mut session = Session::spawn();
    session.initialize();

    // Clean project: one entry parse covers main, member, and caller through
    // the include closure. caller.opy's `worker()` resolves against
    // member.opy in context — a standalone parse would have reported
    // `unknown action 'worker'` — so an empty caller result proves it was not
    // re-checked out of context.
    let clean = check(&mut session, 2, document_map(false));
    assert_eq!(clean.len(), 4);
    assert!(clean.iter().all(|document| document["version"] == 3));
    for document in &clean {
        assert_eq!(
            document["diagnostics"],
            json!([]),
            "{} reports no diagnostics",
            document["uri"]
        );
    }

    // member's injected error aborts preprocessing before caller.opy is
    // registered, so caller falls back to its standalone check and reports
    // `unknown-action`. stray.opy redirects to the entry but is never parsed.
    let broken = check(&mut session, 3, document_map(true));
    assert_eq!(broken.len(), 4);
    let member = find(&broken, "env/member.opy");
    assert!(
        member["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["severity"] == "error"),
        "the included document's injected syntax error reports on its URI: {}",
        member["diagnostics"]
    );
    assert_eq!(
        find(&broken, "caller.opy")["diagnostics"][0]["code"],
        "unknown-action"
    );
    assert_eq!(find(&broken, "stray.opy")["diagnostics"], json!([]));
    session.shutdown();
}

#[test]
fn check_duplicate_include_warning_keeps_its_severity() {
    let mut session = Session::spawn();
    session.initialize();
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(DUPLICATE_INCLUDE),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    let documents = checked["result"]["documents"]
        .as_array()
        .expect("documents");
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0]["uri"], file_uri(DUPLICATE_INCLUDE));
    assert_eq!(
        documents[0]["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .len(),
        1
    );
    assert_eq!(
        documents[0]["diagnostics"][0],
        json!({
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 22 },
            },
            "severity": "warning",
            "code": "w_already_imported",
            "message": format!(
                "The file '{}' was already imported and will not be imported again.",
                Path::new(DUPLICATE_INCLUDE)
                    .parent()
                    .expect("fixture directory")
                    .join("shared.opy")
                    .canonicalize()
                    .expect("include path")
                    .display(),
            ),
            "source": "opy",
        })
    );
    assert_eq!(
        documents[1]["uri"],
        file_uri(
            Path::new(DUPLICATE_INCLUDE)
                .parent()
                .expect("fixture directory")
                .join("shared.opy")
                .to_str()
                .expect("utf8")
        )
    );
    assert_eq!(documents[1]["diagnostics"], json!([]));
    session.shutdown();
}

#[test]
fn check_maps_tab_expanded_columns_to_utf16_positions() {
    let mut session = Session::spawn();
    session.initialize();
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(DIAGNOSTIC_POSITIONS),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    let diagnostics = checked["result"]["documents"][0]["diagnostics"]
        .as_array()
        .expect("diagnostics");
    assert_eq!(diagnostics.len(), 1);
    // Line 3 is `\tbad ║`: the frontend column is 9 because the tab expands to
    // four columns, but the UTF-16 character is 5. A raw character-index
    // projection would report 8.
    assert_eq!(
        diagnostics[0],
        json!({
            "range": {
                "start": { "line": 2, "character": 5 },
                "end": { "line": 2, "character": 6 },
            },
            "severity": "error",
            "code": "lex-error",
            "message": "unexpected character '║'",
            "source": "opy",
        })
    );
    session.shutdown();
}

#[test]
fn document_check_maps_bmp_and_supplementary_columns_to_utf16() {
    let mut session = Session::spawn();
    session.initialize();
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "documents": {
                "file:///pos/bmp.opy": {
                    "uri": "file:///pos/bmp.opy",
                    "languageId": "opy",
                    "version": 1,
                    "text": "rule \"r\":\n\t@Event global\n\tbad ║\n"
                },
                "file:///pos/supplementary.opy": {
                    "uri": "file:///pos/supplementary.opy",
                    "languageId": "opy",
                    "version": 1,
                    "text": "rule \"r\":\n\t@Event global\n\tbad \u{1D4E7}\n"
                }
            }
        }
    }));
    let documents = checked["result"]["documents"]
        .as_array()
        .expect("documents");
    let diagnostics_of = |uri: &str| {
        documents
            .iter()
            .find(|document| document["uri"] == uri)
            .unwrap_or_else(|| panic!("document {uri}"))["diagnostics"]
            .clone()
    };
    // `\tbad ║`: the BMP character is one UTF-16 unit.
    assert_eq!(
        diagnostics_of("file:///pos/bmp.opy")[0]["range"],
        json!({
            "start": { "line": 2, "character": 5 },
            "end": { "line": 2, "character": 6 },
        })
    );
    // `\tbad 𝓧`: the supplementary character is a surrogate pair.
    assert_eq!(
        diagnostics_of("file:///pos/supplementary.opy")[0]["range"],
        json!({
            "start": { "line": 2, "character": 5 },
            "end": { "line": 2, "character": 7 },
        })
    );
    session.shutdown();
}

#[test]
fn document_check_reports_effective_main_file_and_include_uris() {
    let mut session = Session::spawn();
    session.initialize();
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "documents": {
                "file:///proj/source.opy": {
                    "uri": "file:///proj/source.opy",
                    "languageId": "opy",
                    "version": 3,
                    "text": "#!mainFile \"sub/entry.opy\"\n"
                },
                "file:///proj/sub/entry.opy": {
                    "uri": "file:///proj/sub/entry.opy",
                    "languageId": "opy",
                    "version": 4,
                    "text": "#!include \"defs.opy\"\nrule \"broken\"\n    @Event global\n"
                },
                "file:///proj/sub/defs.opy": {
                    "uri": "file:///proj/sub/defs.opy",
                    "languageId": "opy",
                    "version": 5,
                    "text": "rule \"also broken\"\n    @Event global\n"
                }
            }
        }
    }));
    let documents = checked["result"]["documents"]
        .as_array()
        .expect("documents");
    let diagnostics_of = |uri: &str| {
        documents
            .iter()
            .find(|document| document["uri"] == uri)
            .unwrap_or_else(|| panic!("document {uri}"))["diagnostics"]
            .clone()
    };
    // The diagnostics found through the redirected effective main file and its
    // include resolve against `sub/` and land on the supplied documents.
    assert_eq!(diagnostics_of("file:///proj/source.opy"), json!([]));
    let entry_diagnostics = diagnostics_of("file:///proj/sub/entry.opy");
    assert_eq!(
        entry_diagnostics
            .as_array()
            .expect("diagnostics")
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic["range"]["start"].clone(),
                    diagnostic["range"]["end"].clone(),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (
                json!({ "line": 1, "character": 13 }),
                json!({ "line": 1, "character": 13 })
            ),
            (
                json!({ "line": 2, "character": 4 }),
                json!({ "line": 2, "character": 5 })
            ),
        ]
    );
    let defs_diagnostics = diagnostics_of("file:///proj/sub/defs.opy");
    assert_eq!(
        defs_diagnostics
            .as_array()
            .expect("diagnostics")
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic["range"]["start"].clone(),
                    diagnostic["range"]["end"].clone(),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (
                json!({ "line": 0, "character": 18 }),
                json!({ "line": 0, "character": 18 })
            ),
            (
                json!({ "line": 1, "character": 4 }),
                json!({ "line": 1, "character": 5 })
            ),
        ]
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
fn mapped_artifact_file_uris_resolve_against_the_effective_main_directory() {
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let response = compile_entry(
        &mut session,
        MAIN_FILE_SUBDIR_ENTRY,
        Some(json!([MAPPED_V1])),
    );
    let document = mapped_document(&response);
    let fixture = Path::new(MAIN_FILE_SUBDIR_ENTRY)
        .parent()
        .expect("fixture directory");
    assert_eq!(
        document["files"]
            .as_array()
            .expect("files")
            .iter()
            .map(|file| file["path"].as_str().expect("path").to_owned())
            .collect::<Vec<_>>(),
        vec![
            file_uri(MAIN_FILE_SUBDIR_ENTRY),
            file_uri(fixture.join("sub/entry.opy").to_str().expect("utf8")),
            file_uri(fixture.join("sub/defs.opy").to_str().expect("utf8")),
        ]
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

// ---------- lpp/rename + lpp/validateEdits (issue #403) ----------

const RENAME_MAIN_URI: &str = "file:///project/main.opy";
const RENAME_DEFS_URI: &str = "file:///project/shared/defs.opy";

/// Line 6 of `main` is tab-indented: `\t` occupies one UTF-16 unit in the
/// returned ranges but four frontend columns in the semantic model.
const RENAME_MAIN: &str = concat!(
    "#!include \"shared/defs.opy\"\n",
    "\n",
    "playervar hp\n",
    "\n",
    "rule \"tick\":\n",
    "    @Event eachPlayer\n",
    "\tscoreBank += 1\n",
    "    eventPlayer.hp = scoreBank\n",
    "    reset()\n",
);
const RENAME_DEFS: &str = concat!(
    "globalvar scoreBank = 0\n",
    "\n",
    "subroutine reset\n",
    "\n",
    "def reset():\n",
    "    scoreBank = 0\n",
    "\n",
    "globalvar other = 1\n",
);

fn rename_documents(main_version: i64, defs_version: i64) -> Value {
    json!({
        RENAME_MAIN_URI: {
            "uri": RENAME_MAIN_URI,
            "languageId": "opy",
            "version": main_version,
            "text": RENAME_MAIN,
        },
        RENAME_DEFS_URI: {
            "uri": RENAME_DEFS_URI,
            "languageId": "opy",
            "version": defs_version,
            "text": RENAME_DEFS,
        },
    })
}

fn rename_request(
    id: i64,
    documents: Value,
    position_uri: &str,
    line: u32,
    character: u32,
    new_name: &str,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "lpp/rename",
        "params": {
            "documents": documents,
            "positionDocumentUri": position_uri,
            "position": { "line": line, "character": character },
            "newName": new_name,
        },
    })
}

fn refusal_code(response: &Value) -> &str {
    assert_eq!(response["error"]["data"]["lpp"]["kind"], "refusal");
    response["error"]["data"]["lpp"]["details"]["refusalCode"]
        .as_str()
        .expect("refusal code")
}

fn sorted_ranges(edits: &Value) -> Vec<(u32, u32, u32)> {
    edits["textEdits"]
        .as_array()
        .expect("textEdits")
        .iter()
        .map(|edit| {
            (
                edit["range"]["start"]["line"].as_u64().expect("line") as u32,
                edit["range"]["start"]["character"].as_u64().expect("start") as u32,
                edit["range"]["end"]["character"].as_u64().expect("end") as u32,
            )
        })
        .collect()
}

/// The byte offset of `units` UTF-16 code units into `line`.
fn utf16_byte_offset(line: &str, units: usize) -> usize {
    let mut used = 0usize;
    for (byte, ch) in line.char_indices() {
        if used == units {
            return byte;
        }
        used += ch.len_utf16();
    }
    assert_eq!(used, units, "UTF-16 offset past the line end");
    line.len()
}

/// Apply `edits` (original-source UTF-16 coordinates) to `text` and return
/// the edited source. Edits on one line apply right-to-left so earlier
/// replacements don't shift later offsets.
fn apply_text_edits(text: &str, edits: &Value) -> String {
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    let mut per_line: BTreeMap<usize, Vec<(usize, usize, String)>> = BTreeMap::new();
    for edit in edits["textEdits"].as_array().expect("textEdits") {
        let start_line = edit["range"]["start"]["line"].as_u64().expect("line") as usize;
        let end_line = edit["range"]["end"]["line"].as_u64().expect("line") as usize;
        assert_eq!(
            start_line, end_line,
            "test helper only applies inline edits"
        );
        per_line.entry(start_line).or_default().push((
            edit["range"]["start"]["character"].as_u64().expect("s") as usize,
            edit["range"]["end"]["character"].as_u64().expect("e") as usize,
            edit["newText"].as_str().expect("newText").to_string(),
        ));
    }
    for (line, mut line_edits) in per_line {
        line_edits.sort_by_key(|edit| usize::MAX - edit.0);
        for (start, end, new_text) in line_edits {
            let content = &mut lines[line];
            let range = utf16_byte_offset(content, start)..utf16_byte_offset(content, end);
            content.replace_range(range, &new_text);
        }
    }
    lines.concat()
}

#[test]
fn rename_global_covers_declaration_and_references_across_documents() {
    let mut session = Session::spawn();
    session.initialize();
    let renamed = session.request(rename_request(
        2,
        rename_documents(3, 5),
        RENAME_MAIN_URI,
        7,
        25,
        "vault",
    ));
    let edits = renamed["result"]["edits"].as_array().expect("edits");
    assert_eq!(edits.len(), 2);

    let main = edits
        .iter()
        .find(|edit| edit["documentUri"] == RENAME_MAIN_URI)
        .expect("main edits");
    assert_eq!(main["version"], 3);
    // `scoreBank` sites in main: the tab-indented `+=` target and the
    // assignment value — UTF-16 characters, sorted by range start.
    assert_eq!(sorted_ranges(main), vec![(6, 1, 10), (7, 21, 30)]);

    let defs = edits
        .iter()
        .find(|edit| edit["documentUri"] == RENAME_DEFS_URI)
        .expect("defs edits");
    assert_eq!(defs["version"], 5);
    assert_eq!(sorted_ranges(defs), vec![(0, 10, 19), (5, 4, 13)]);

    // The applied edits keep the project clean and only move identifiers.
    let edited_main = apply_text_edits(RENAME_MAIN, main);
    let edited_defs = apply_text_edits(RENAME_DEFS, defs);
    assert_eq!(edited_main, RENAME_MAIN.replace("scoreBank", "vault"));
    assert_eq!(edited_defs, RENAME_DEFS.replace("scoreBank", "vault"));
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/check",
        "params": {
            "documents": {
                RENAME_MAIN_URI: {
                    "uri": RENAME_MAIN_URI,
                    "languageId": "opy",
                    "version": 3,
                    "text": edited_main,
                },
                RENAME_DEFS_URI: {
                    "uri": RENAME_DEFS_URI,
                    "languageId": "opy",
                    "version": 5,
                    "text": edited_defs,
                },
            }
        }
    }));
    assert!(
        checked["result"]["documents"]
            .as_array()
            .expect("documents")
            .iter()
            .all(|document| document["diagnostics"] == json!([]))
    );
    session.shutdown();
}

#[test]
fn rename_player_and_subroutine_cover_their_binding_groups() {
    let mut session = Session::spawn();
    session.initialize();

    // Player rename: declaration plus the `eventPlayer.hp` member token only.
    let renamed = session.request(rename_request(
        2,
        rename_documents(3, 5),
        RENAME_MAIN_URI,
        7,
        16,
        "health",
    ));
    let edits = renamed["result"]["edits"].as_array().expect("edits");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0]["documentUri"], RENAME_MAIN_URI);
    assert_eq!(sorted_ranges(&edits[0]), vec![(2, 10, 12), (7, 16, 18)]);

    // Subroutine rename rewrites the `subroutine` declaration, the `def`
    // implementation name, and every call site — one binding group.
    let renamed = session.request(rename_request(
        3,
        rename_documents(3, 5),
        RENAME_DEFS_URI,
        4,
        5,
        "wipe",
    ));
    let edits = renamed["result"]["edits"].as_array().expect("edits");
    assert_eq!(edits.len(), 2);
    let defs = edits
        .iter()
        .find(|edit| edit["documentUri"] == RENAME_DEFS_URI)
        .expect("defs edits");
    assert_eq!(sorted_ranges(defs), vec![(2, 11, 16), (4, 4, 9)]);
    let main = edits
        .iter()
        .find(|edit| edit["documentUri"] == RENAME_MAIN_URI)
        .expect("main edits");
    assert_eq!(sorted_ranges(main), vec![(8, 4, 9)]);
    session.shutdown();
}

#[test]
fn rename_refusals_are_structured() {
    let mut session = Session::spawn();
    session.initialize();
    let documents = || rename_documents(3, 5);

    // Invalid identifiers and reserved words refuse before any analysis.
    let invalid = session.request(rename_request(2, documents(), RENAME_MAIN_URI, 7, 25, "9x"));
    assert_eq!(refusal_code(&invalid), "rename.invalidName");
    let reserved = session.request(rename_request(
        3,
        documents(),
        RENAME_MAIN_URI,
        7,
        25,
        "rule",
    ));
    assert_eq!(refusal_code(&reserved), "rename.invalidName");

    // Positions with no symbol: a blank line, and an unknown name.
    let blank = session.request(rename_request(4, documents(), RENAME_MAIN_URI, 1, 0, "x"));
    assert_eq!(refusal_code(&blank), "rename.noSymbolAtPosition");
    let whitespace = session.request(rename_request(5, documents(), RENAME_MAIN_URI, 5, 1, "x"));
    assert_eq!(refusal_code(&whitespace), "rename.noSymbolAtPosition");

    // `eventPlayer` is upstream-reserved for globals (a context name): the
    // name itself is invalid, not merely a binding collision.
    let captured = session.request(rename_request(
        6,
        documents(),
        RENAME_MAIN_URI,
        7,
        25,
        "eventPlayer",
    ));
    assert_eq!(refusal_code(&captured), "rename.invalidName");

    // Renaming onto an existing global duplicates the declaration.
    let colliding = session.request(rename_request(
        7,
        documents(),
        RENAME_MAIN_URI,
        7,
        25,
        "other",
    ));
    assert_eq!(refusal_code(&colliding), "rename.nameCollision");

    // A reference inside an unsent project file refuses instead of producing
    // a partial edit set.
    let partial = session.request(rename_request(
        8,
        json!({
            RENAME_MAIN_URI: {
                "uri": RENAME_MAIN_URI,
                "languageId": "opy",
                "version": 3,
                "text": RENAME_MAIN,
            }
        }),
        RENAME_MAIN_URI,
        7,
        25,
        "vault",
    ));
    assert_eq!(refusal_code(&partial), "rename.requiresDocument");
    let details = &partial["error"]["data"]["lpp"]["details"];
    let mentions_defs = |value: &Value| {
        value.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str().is_some_and(|item| item.contains("defs.opy")))
        })
    };
    assert!(
        mentions_defs(&details["uris"]) || mentions_defs(&details["missing"]),
        "the refusal names the missing include: {details}"
    );

    // When the missing file exists on disk the model still resolves the
    // include, so the refusal names the exact document holding a rename site.
    let main_uri = file_uri(MULTI_FILE_MAIN);
    let on_disk = session.request(rename_request(
        10,
        json!({
            main_uri.clone(): {
                "uri": main_uri.clone(),
                "languageId": "opy",
                "version": 3,
                "text": std::fs::read_to_string(
                    Path::new(MULTI_FILE_MAIN).canonicalize().expect("fixture"),
                )
                .expect("fixture source"),
            }
        }),
        &main_uri,
        8,
        9,
        "grand_total",
    ));
    assert_eq!(refusal_code(&on_disk), "rename.requiresDocument");
    assert!(
        on_disk["error"]["data"]["lpp"]["details"]["uris"]
            .as_array()
            .expect("missing uris")
            .iter()
            .any(|uri| uri.as_str().is_some_and(|uri| uri.ends_with("defs.opy")))
    );

    // The position document must be part of the request's document set.
    let outside = session.request(rename_request(
        9,
        documents(),
        "file:///project/elsewhere.opy",
        0,
        0,
        "x",
    ));
    assert_eq!(outside["error"]["data"]["lpp"]["kind"], "invalidDocument");
    session.shutdown();
}

#[test]
fn rename_through_main_file_member_names_the_documents_to_supply() {
    let mut session = Session::spawn();
    session.initialize();
    let document = |name: &str| {
        let path = Path::new(RENAME_MAIN_FILE_ROOT).join(name);
        let uri = file_uri(&path.to_string_lossy());
        json!({
            "uri": uri,
            "languageId": "opy",
            "version": 1,
            "text": std::fs::read_to_string(&path).expect("fixture source"),
        })
    };

    // A member file whose `#!mainFile` resolves on disk is analyzed under its
    // include record; the registry's entry record for it is a redirect stub.
    // Rename must find the symbol anyway and report the unsupplied site
    // document, not `noSymbolAtPosition`.
    let member = document("env/member.opy");
    let member_uri = member["uri"].as_str().expect("member uri").to_string();
    let partial = session.request(rename_request(
        2,
        json!({ member_uri.clone(): member.clone() }),
        &member_uri,
        4,
        5,
        "helper",
    ));
    assert_eq!(refusal_code(&partial), "rename.requiresDocument");
    assert!(
        partial["error"]["data"]["lpp"]["details"]["uris"]
            .as_array()
            .expect("uris")
            .iter()
            .any(|uri| uri.as_str().is_some_and(|uri| uri.ends_with("caller.opy"))),
        "the refusal names the unsupplied site document: {partial}"
    );

    // Supplying the site documents — the project entry is not required —
    // completes the rename across the member and the call site.
    let caller = document("caller.opy");
    let caller_uri = caller["uri"].as_str().expect("caller uri").to_string();
    let renamed = session.request(rename_request(
        3,
        json!({
            member_uri.clone(): member.clone(),
            caller_uri.clone(): caller,
        }),
        &member_uri,
        4,
        5,
        "helper",
    ));
    let edits = renamed["result"]["edits"].as_array().expect("edits");
    assert_eq!(edits.len(), 2);
    let member_edits = edits
        .iter()
        .find(|edit| edit["documentUri"] == member_uri)
        .expect("member edits");
    assert_eq!(sorted_ranges(member_edits), vec![(2, 11, 17), (4, 4, 10)]);
    let caller_edits = edits
        .iter()
        .find(|edit| edit["documentUri"] == caller_uri)
        .expect("caller edits");
    assert_eq!(sorted_ranges(caller_edits), vec![(2, 4, 10)]);

    // A `#!mainFile` file that the resolved project does not include is still
    // a document-set problem: the refusal names the entry it resolves
    // through.
    let stray = document("stray.opy");
    let stray_uri = stray["uri"].as_str().expect("stray uri").to_string();
    let uncovered = session.request(rename_request(
        4,
        json!({ stray_uri.clone(): stray }),
        &stray_uri,
        2,
        5,
        "renamed",
    ));
    assert_eq!(refusal_code(&uncovered), "rename.requiresDocument");
    assert!(
        uncovered["error"]["data"]["lpp"]["details"]["missing"]
            .as_array()
            .expect("missing")
            .iter()
            .any(|uri| uri.as_str().is_some_and(|uri| uri.ends_with("main.opy"))),
        "the refusal names the entry the file resolves through: {uncovered}"
    );

    // A position on no symbol inside a covered member still reports
    // `noSymbolAtPosition`.
    let blank = session.request(rename_request(
        5,
        json!({ member_uri.clone(): member }),
        &member_uri,
        1,
        0,
        "x",
    ));
    assert_eq!(refusal_code(&blank), "rename.noSymbolAtPosition");
    session.shutdown();
}

#[test]
fn rename_refuses_unsupported_kinds_and_expanded_sites() {
    let mut session = Session::spawn();
    session.initialize();

    // `macro NAME = value` indexes a Constant; `macro NAME(args):` a Macro.
    // Both stay outside the renameable scope.
    let source = concat!(
        "macro LIMIT = 4\n",
        "\n",
        "macro double(value):\n",
        "    value + value\n",
        "\n",
        "rule \"r\":\n",
        "    @Event global\n",
        "    LIMIT\n",
        "    double(2)\n",
    );
    let uri = "file:///project/macros.opy";
    let documents = || {
        json!({
            uri: { "uri": uri, "languageId": "opy", "version": 2, "text": source }
        })
    };
    let constant = session.request(rename_request(2, documents(), uri, 0, 7, "CAP"));
    assert_eq!(refusal_code(&constant), "rename.unsupportedSymbolKind");
    let constant_use = session.request(rename_request(3, documents(), uri, 7, 4, "CAP"));
    assert_eq!(refusal_code(&constant_use), "rename.unsupportedSymbolKind");
    let macro_call = session.request(rename_request(4, documents(), uri, 8, 5, "triple"));
    assert_eq!(refusal_code(&macro_call), "rename.unsupportedSymbolKind");

    // A `#!define` alias expands to a generated `scoreBank` token at the use
    // site; that reference does not spell authored text, so the rename must
    // refuse rather than emit a partial set.
    let aliased = concat!(
        "#!define ALIAS scoreBank\n",
        "\n",
        "globalvar scoreBank = 0\n",
        "\n",
        "rule \"r\":\n",
        "    @Event global\n",
        "    scoreBank = ALIAS\n",
    );
    let alias_uri = "file:///project/alias.opy";
    let refused = session.request(rename_request(
        5,
        json!({
            alias_uri: { "uri": alias_uri, "languageId": "opy", "version": 2, "text": aliased }
        }),
        alias_uri,
        6,
        4,
        "vault",
    ));
    assert_eq!(
        refusal_code(&refused),
        "rename.unsupportedReference",
        "generated reference sites refuse the rename"
    );
    session.shutdown();
}

#[test]
fn rename_keeps_compile_output_identical_modulo_the_identifier() {
    let mut session = Session::spawn();
    let source = concat!(
        "globalvar scoreBank = 0\n",
        "\n",
        "rule \"r\":\n",
        "    @Event global\n",
        "    scoreBank += 1\n",
        "    wait(0.1)\n",
    );
    let uri = "file:///project/main.opy";
    let document =
        |text: &str| json!({ "uri": uri, "languageId": "opy", "version": 1, "text": text });
    session.initialize();

    let before = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/compile",
        "params": { "documents": { uri: document(source) } },
    }));
    let before_content = before["result"]["artifact"]["content"]
        .as_str()
        .expect("compiled workshop text")
        .to_string();

    let renamed = session.request(rename_request(
        3,
        json!({ uri: document(source) }),
        uri,
        4,
        5,
        "vault",
    ));
    let edits = &renamed["result"]["edits"][0];
    let edited = apply_text_edits(source, edits);

    let after = session.request(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "lpp/compile",
        "params": { "documents": { uri: document(&edited) } },
    }));
    let after_content = after["result"]["artifact"]["content"]
        .as_str()
        .expect("compiled workshop text");
    assert_eq!(after_content, before_content.replace("scoreBank", "vault"));
    session.shutdown();
}

#[test]
fn validate_edits_applies_normative_rules() {
    let mut session = Session::spawn();
    session.initialize();
    let document = || json!({ "uri": RENAME_MAIN_URI, "languageId": "opy", "version": 7, "text": RENAME_MAIN });
    let validate = |session: &mut Session, id: i64, edits: Value| {
        session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/validateEdits",
            "params": { "document": document(), "edits": edits },
        }))
    };
    let edit = |line: u32, start: u32, end: u32, new_text: &str| {
        json!({
            "range": {
                "start": { "line": line, "character": start },
                "end": { "line": line, "character": end },
            },
            "newText": new_text,
        })
    };

    // A clean identifier swap validates and echoes the document version.
    let valid = validate(
        &mut session,
        2,
        json!([edit(7, 21, 30, "vault"), edit(6, 1, 10, "vault")]),
    );
    assert_eq!(valid["result"], json!({ "valid": true, "version": 7 }));

    // Overlaps report the later edit's index in the original request order:
    // request edit 1 sorts first and overlaps request edit 0.
    let overlap = validate(
        &mut session,
        3,
        json!([edit(7, 25, 30, "x"), edit(7, 21, 26, "y")]),
    );
    assert_eq!(
        overlap["result"],
        json!({
            "valid": false,
            "version": 7,
            "reason": "overlappingEdits",
            "failingEditIndex": 0,
        })
    );

    // Out-of-bounds and reversed ranges report the first offending index.
    let bounds = validate(&mut session, 4, json!([edit(99, 0, 1, "x")]));
    assert_eq!(
        bounds["result"],
        json!({
            "valid": false,
            "version": 7,
            "reason": "rangeOutOfBounds",
            "failingEditIndex": 0,
        })
    );
    let reversed = validate(&mut session, 5, json!([edit(6, 10, 1, "x")]));
    assert_eq!(reversed["result"]["reason"], "rangeOutOfBounds");

    // A result that no longer parses reports `syntaxError` without an index.
    // (A standalone document: an unresolvable `#!include` would short-circuit
    // checking before the parser runs.)
    let standalone = session.request(json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "lpp/validateEdits",
        "params": {
            "document": {
                "uri": "file:///project/standalone.opy",
                "languageId": "opy",
                "version": 4,
                "text": "rule \"r\":\n    @Event global\n    wait(0.1)\n",
            },
            "edits": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 4 },
                },
                "newText": "zzz",
            }],
        },
    }));
    assert_eq!(
        standalone["result"],
        json!({ "valid": false, "version": 4, "reason": "syntaxError" })
    );

    // A fragment referencing cross-file names is well-formed for this
    // single-document validation; cross-file checking is the client's
    // follow-up `lpp/check`.
    let fragment = session.request(json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "lpp/validateEdits",
        "params": {
            "document": {
                "uri": "file:///project/fragment.opy",
                "languageId": "opy",
                "version": 11,
                "text": "rule \"part\":\n    @Event global\n    foreignCall()\n    foreignVar += 1\n",
            },
            "edits": [{
                "range": {
                    "start": { "line": 2, "character": 4 },
                    "end": { "line": 2, "character": 15 },
                },
                "newText": "otherCall",
            }],
        },
    }));
    assert_eq!(fragment["result"], json!({ "valid": true, "version": 11 }));

    // A cross-file code the edits *introduce* is not fragment leniency:
    // `wiat` is not a catalog action, so the result must report syntaxError.
    let introduced = session.request(json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "lpp/validateEdits",
        "params": {
            "document": {
                "uri": "file:///project/typo.opy",
                "languageId": "opy",
                "version": 2,
                "text": "rule \"r\":\n    @Event global\n    wait(0.1)\n",
            },
            "edits": [{
                "range": {
                    "start": { "line": 2, "character": 4 },
                    "end": { "line": 2, "character": 8 },
                },
                "newText": "wiat",
            }],
        },
    }));
    assert_eq!(
        introduced["result"],
        json!({ "valid": false, "version": 2, "reason": "syntaxError" }),
        "an introduced unknown-action is an error, not a cross-file artifact"
    );

    // The first offending index is the earliest bad edit in request order.
    let multi = validate(
        &mut session,
        9,
        json!([
            edit(7, 21, 30, "vault"),
            edit(42, 0, 1, "x"),
            edit(50, 0, 1, "y")
        ]),
    );
    assert_eq!(
        multi["result"],
        json!({
            "valid": false,
            "version": 7,
            "reason": "rangeOutOfBounds",
            "failingEditIndex": 1,
        })
    );
    session.shutdown();
}

#[test]
fn end_of_document_positions_do_not_panic() {
    // A document ending in a newline has an implicit empty last line; a
    // position or range endpoint on it must produce a structured result, not
    // a provider crash.
    let mut session = Session::spawn();
    session.initialize();
    let uri = "file:///project/eof.opy";
    let text = "globalvar g = 1\n";
    let document = || json!({ "uri": uri, "languageId": "opy", "version": 1, "text": text });

    let position = session.request(rename_request(
        2,
        json!({ uri: document() }),
        uri,
        1,
        0,
        "h",
    ));
    assert_eq!(refusal_code(&position), "rename.noSymbolAtPosition");

    let beyond = session.request(rename_request(
        3,
        json!({ uri: document() }),
        uri,
        5,
        0,
        "h",
    ));
    assert_eq!(beyond["error"]["data"]["lpp"]["kind"], "invalidPosition");

    let insert = session.request(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "lpp/validateEdits",
        "params": {
            "document": document(),
            "edits": [{
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 1, "character": 0 },
                },
                "newText": "globalvar h = 2\n",
            }],
        },
    }));
    assert_eq!(insert["result"], json!({ "valid": true, "version": 1 }));

    let out_of_bounds = session.request(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "lpp/validateEdits",
        "params": {
            "document": document(),
            "edits": [{
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 1, "character": 1 },
                },
                "newText": "x",
            }],
        },
    }));
    assert_eq!(out_of_bounds["result"]["reason"], "rangeOutOfBounds");
    session.shutdown();
}

#[test]
fn rename_refuses_when_a_received_document_escapes_the_project_view() {
    // Two project roots share one include: the analysis covers one root, so
    // the other received document's references would be left dangling. The
    // provider must refuse instead of shipping a partial edit set.
    let mut session = Session::spawn();
    session.initialize();
    let shared_uri = "file:///project/shared.opy";
    let m1_uri = "file:///project/m1.opy";
    let m2_uri = "file:///project/m2.opy";
    let shared = "globalvar sharedBank = 0\n";
    let main = |amount: i32| {
        format!(
            "#!include \"shared.opy\"\n\nrule \"r\":\n    @Event global\n    sharedBank += {amount}\n"
        )
    };
    let documents = |m2_text: &str| {
        json!({
            m1_uri: { "uri": m1_uri, "languageId": "opy", "version": 1, "text": main(1) },
            shared_uri: { "uri": shared_uri, "languageId": "opy", "version": 1, "text": shared },
            m2_uri: { "uri": m2_uri, "languageId": "opy", "version": 1, "text": m2_text },
        })
    };

    let refused = session.request(rename_request(
        2,
        documents(&main(2)),
        m1_uri,
        4,
        6,
        "vault",
    ));
    assert_eq!(refusal_code(&refused), "rename.requiresDocument");
    assert!(
        refused["error"]["data"]["lpp"]["details"]["uris"]
            .as_array()
            .expect("uris")
            .iter()
            .any(|uri| uri.as_str() == Some(m2_uri)),
        "the refusal names the document outside the analyzed view"
    );

    // A received document outside the view that never mentions the symbol is
    // an unrelated buffer, not a rename target — the rename proceeds.
    let unrelated = session.request(rename_request(
        3,
        documents("globalvar other = 1\n"),
        m1_uri,
        4,
        6,
        "vault",
    ));
    let edits = unrelated["result"]["edits"].as_array().expect("edits");
    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit["documentUri"] != m2_uri));
    session.shutdown();
}

#[test]
fn rename_refuses_upstream_reserved_names_per_namespace() {
    let mut session = Session::spawn();
    session.initialize();
    let documents = || rename_documents(3, 5);

    // `elif` and `Map` are upstream-reserved for globals even though the
    // local parser would rebind them.
    for new_name in ["elif", "Map", "Array"] {
        let refused = session.request(rename_request(
            2,
            documents(),
            RENAME_MAIN_URI,
            7,
            25,
            new_name,
        ));
        assert_eq!(refusal_code(&refused), "rename.invalidName", "{new_name}");
    }

    // Context names are global-reserved upstream — `invalidName`, not a
    // binding collision.
    let context = session.request(rename_request(
        3,
        documents(),
        RENAME_MAIN_URI,
        7,
        25,
        "hostPlayer",
    ));
    assert_eq!(refusal_code(&context), "rename.invalidName");

    // Player variables reserve only the member axis names.
    let axis = session.request(rename_request(4, documents(), RENAME_MAIN_URI, 7, 16, "x"));
    assert_eq!(refusal_code(&axis), "rename.invalidName");

    // A keyword reserved in every namespace still refuses for subroutines;
    // so do builtins callable with an empty argument list.
    let keyword = session.request(rename_request(
        5,
        documents(),
        RENAME_DEFS_URI,
        4,
        5,
        "elif",
    ));
    assert_eq!(refusal_code(&keyword), "rename.invalidName");
    let builtin = session.request(rename_request(
        6,
        documents(),
        RENAME_DEFS_URI,
        4,
        5,
        "getTotalTimeElapsed",
    ));
    assert_eq!(refusal_code(&builtin), "rename.invalidName");

    // But a name that is only global-reserved is still a legal player
    // variable name upstream — `playervar Map` compiles there.
    let renamed = session.request(rename_request(
        7,
        documents(),
        RENAME_MAIN_URI,
        7,
        16,
        "elif",
    ));
    assert!(
        renamed["result"]["edits"].is_array(),
        "player rename onto a global-only reserved name is upstream-legal: {renamed}"
    );
    session.shutdown();
}

// -- lpp/lookup (LPP 1.5) ---------------------------------------------------

fn lookup_request(id: i64, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "lpp/lookup",
        "params": params,
    })
}

/// `lookup` is negotiated only at LPP 1.5: the capability is absent at 1.4
/// and an `lpp/lookup` request there is `capabilityUnavailable`.
#[test]
fn lookup_capability_is_negotiated_at_1_5() {
    let mut session = Session::spawn();
    let initialized = session.initialize_version("1.4");
    assert_eq!(initialized["result"]["protocolVersion"], "1.4");
    assert_eq!(
        initialized["result"]["capabilities"]["lookup"],
        Value::Null,
        "lookup is not advertised before 1.5"
    );
    let response = session.request(lookup_request(
        2,
        json!({ "languageId": "opy", "query": "wait" }),
    ));
    assert_eq!(response["error"]["code"], -32000);
    assert_eq!(
        response["error"]["data"]["lpp"]["kind"],
        "capabilityUnavailable"
    );
    assert_eq!(
        response["error"]["data"]["lpp"]["details"],
        json!({ "capability": "lookup", "method": "lpp/lookup" })
    );
    session.shutdown();

    let mut session = Session::spawn();
    let initialized = session.initialize_version("1.5");
    assert_eq!(initialized["result"]["protocolVersion"], "1.5");
    assert_eq!(initialized["result"]["capabilities"]["lookup"], true);
    session.shutdown();
}

/// An `lpp/lookup` answer comes from the language vocabulary: the session
/// never loads a project or opens a document.
#[test]
fn lookup_resolves_names_without_a_project() {
    let mut session = Session::spawn();
    session.initialize_version("1.5");

    let response = session.request(lookup_request(
        2,
        json!({ "languageId": "opy", "query": "wait", "limit": 3 }),
    ));
    let entries = response["result"]["entries"].as_array().expect("entries");
    assert_eq!(entries[0]["identity"], "opy:callable/wait");
    assert_eq!(entries[0]["kind"], "action");
    assert_eq!(entries[0]["spelling"], "wait");
    assert_eq!(entries[0]["displayName"], "Wait");
    assert!(
        entries[0]["callable"]["parameters"]
            .as_array()
            .is_some_and(|params| !params.is_empty())
    );

    // A display-name guess resolves to the OPY spelling through the same
    // vocabulary ("Wait Until" is the catalog name of `waitUntil`).
    let display = session.request(lookup_request(
        3,
        json!({ "languageId": "opy", "query": "Wait Until", "limit": 3 }),
    ));
    let spellings: Vec<&str> = display["result"]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|entry| entry["spelling"].as_str())
        .collect();
    assert!(spellings.contains(&"waitUntil"), "{spellings:?}");
    session.shutdown();
}

/// `within` selects the scope the listing draws from: a callable identity
/// lists parameters in call order, an enum domain identity lists members in
/// domain order, and a settings prefix lists immediate children spelled by
/// their own segment.
#[test]
fn lookup_within_scopes_list_children() {
    let mut session = Session::spawn();
    session.initialize_version("1.5");

    let params = session.request(lookup_request(
        2,
        json!({
            "languageId": "opy",
            "within": { "kind": "callable", "value": "opy:callable/wait" },
        }),
    ));
    let entries = params["result"]["entries"].as_array().expect("entries");
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["spelling"].as_str().expect("spelling"))
            .collect::<Vec<_>>(),
        ["time", "waitBehavior"],
        "parameter entries keep call order"
    );
    assert!(entries.iter().all(|entry| entry["kind"] == "parameter"));
    assert_eq!(
        entries[1]["parameter"]["enum"]["domain"], "opy:enum/Wait",
        "the enum domain a parameter accepts is a valid within value"
    );

    let members = session.request(lookup_request(
        3,
        json!({
            "languageId": "opy",
            "within": { "kind": "enum", "value": "opy:enum/Button" },
        }),
    ));
    let entries = members["result"]["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 10);
    assert!(entries.iter().all(|entry| entry["kind"] == "enumMember"));
    assert_eq!(entries[0]["spelling"], "Button.PRIMARY_FIRE");
    assert_eq!(
        entries[0]["identity"],
        "opy:enum-member/Button.PRIMARY_FIRE"
    );

    let root = session.request(lookup_request(
        4,
        json!({
            "languageId": "opy",
            "within": { "kind": "settings", "value": "" },
        }),
    ));
    let entries = root["result"]["entries"].as_array().expect("entries");
    assert!(
        entries
            .iter()
            .all(|entry| entry["kind"] == "settingPath" && entry["spelling"].is_string())
    );
    assert!(entries.iter().any(|entry| entry["spelling"] == "gamemodes"));

    let children = session.request(lookup_request(
        5,
        json!({
            "languageId": "opy",
            "within": { "kind": "settings", "value": "gamemodes.ffa" },
        }),
    ));
    let entries = children["result"]["entries"].as_array().expect("entries");
    let score_to_win = entries
        .iter()
        .find(|entry| entry["spelling"] == "scoreToWin")
        .expect("leaf child");
    assert_eq!(score_to_win["kind"], "setting");
    assert_eq!(
        score_to_win["identity"],
        "opy:setting/gamemodes.ffa.scoreToWin"
    );
    assert_eq!(score_to_win["setting"]["type"], "number");

    let leaf = session.request(lookup_request(
        6,
        json!({
            "languageId": "opy",
            "within": { "kind": "settings", "value": "gamemodes.ffa.scoreToWin" },
        }),
    ));
    assert_eq!(
        leaf["error"]["data"]["lpp"]["details"]["refusalCode"], "lookup.unknownWithin",
        "a settings leaf is not a scope"
    );

    // A settings enum domain answers under `within: "enum"` with the member
    // spellings the settings table declares.
    let settings_enum = session.request(lookup_request(
        7,
        json!({
            "languageId": "opy",
            "within": { "kind": "enum", "value": "opy:enum/mapRotation" },
        }),
    ));
    let spellings: Vec<&str> = settings_enum["result"]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|entry| entry["spelling"].as_str())
        .collect();
    assert_eq!(spellings, ["afterAGame", "afterMirrorMatch", "paused"]);
    session.shutdown();
}

/// Repeated requests return identical entries, `limit` bounds the result,
/// and `kind` filters by the entry's provider-defined kind.
#[test]
fn lookup_results_are_deterministic_bounded_and_kind_filtered() {
    let mut session = Session::spawn();
    session.initialize_version("1.5");

    let first = session.request(lookup_request(
        2,
        json!({ "languageId": "opy", "query": "hudText", "limit": 5 }),
    ));
    let second = session.request(lookup_request(
        3,
        json!({ "languageId": "opy", "query": "hudText", "limit": 5 }),
    ));
    assert_eq!(
        first["result"], second["result"],
        "the same request returns the same entries in the same order"
    );
    assert_eq!(
        first["result"]["entries"]
            .as_array()
            .expect("entries")
            .len(),
        5
    );

    let bounded = session.request(lookup_request(
        4,
        json!({ "languageId": "opy", "limit": 1 }),
    ));
    assert_eq!(
        bounded["result"]["entries"]
            .as_array()
            .expect("entries")
            .len(),
        1
    );

    let enums = session.request(lookup_request(
        5,
        json!({ "languageId": "opy", "kind": "enum", "limit": 8 }),
    ));
    let entries = enums["result"]["entries"].as_array().expect("entries");
    assert!(!entries.is_empty());
    assert!(entries.iter().all(|entry| entry["kind"] == "enum"));

    let no_such_kind = session.request(lookup_request(
        6,
        json!({ "languageId": "opy", "kind": "noSuchKind" }),
    ));
    assert_eq!(no_such_kind["result"]["entries"], json!([]));
    session.shutdown();
}

/// Params that do not match the schema are `-32602`; a language the
/// provider does not serve is `invalidLanguage`.
#[test]
fn lookup_validates_params() {
    let mut session = Session::spawn();
    session.initialize_version("1.5");

    let mut id = 2;
    let mut invalid_params = |session: &mut Session, params: Value| {
        id += 1;
        session.request(lookup_request(id, params))["error"]["code"]
            .as_i64()
            .expect("error code")
    };
    assert_eq!(
        invalid_params(&mut session, json!({ "query": "x" })),
        -32602
    );
    assert_eq!(
        invalid_params(&mut session, json!({ "languageId": "opy", "limit": 0 })),
        -32602
    );
    assert_eq!(
        invalid_params(&mut session, json!({ "languageId": "opy", "limit": -1 })),
        -32602
    );
    assert_eq!(
        invalid_params(&mut session, json!({ "languageId": "opy", "limit": "3" })),
        -32602
    );
    assert_eq!(
        invalid_params(
            &mut session,
            json!({ "languageId": "opy", "within": { "kind": "document", "value": "x" } })
        ),
        -32602
    );
    assert_eq!(
        invalid_params(
            &mut session,
            json!({ "languageId": "opy", "within": { "kind": "enum" } })
        ),
        -32602
    );

    let wrong_language =
        session.request(lookup_request(20, json!({ "languageId": "x-demo-lang" })));
    assert_eq!(
        wrong_language["error"]["data"]["lpp"]["kind"],
        "invalidLanguage"
    );
    assert_eq!(
        wrong_language["error"]["data"]["lpp"]["details"]["languageId"],
        "x-demo-lang"
    );
    session.shutdown();
}
