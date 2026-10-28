use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

mod common;
use common::{Session, file_uri};

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
const INCLUDE_PARENT_ESCAPE_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/include-parent-escape/src"
);

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
    assert_eq!(initialized["result"]["capabilities"]["reconstruct"], true);
    assert_eq!(
        initialized["result"]["capabilities"]["projectLoading"],
        true
    );
    assert_eq!(initialized["result"]["capabilities"]["symbols"], true);
    assert_eq!(initialized["result"]["capabilities"]["definition"], true);
    assert_eq!(initialized["result"]["capabilities"]["references"], true);
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
    assert_eq!(compiled_documents.len(), 7, "all reachable source files");
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
    let root = Path::new(RENAME_MAIN_FILE_ROOT);
    let uris: BTreeMap<&str, String> = ["main.opy", "caller.opy", "stray.opy", "env/member.opy"]
        .into_iter()
        .map(|member| (member, file_uri(root.join(member).to_str().expect("utf8"))))
        .collect();
    let document_map = |members: &[&str], mutate: &dyn Fn(&str, &mut String)| {
        let mut documents = serde_json::Map::new();
        for member in members {
            let uri = &uris[*member];
            let mut text = std::fs::read_to_string(root.join(member)).expect("fixture text");
            mutate(member, &mut text);
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

    let all = ["main.opy", "caller.opy", "stray.opy", "env/member.opy"];
    let clean = check(&mut session, 2, document_map(&all, &|_, _| {}));
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

    let unordered = check(
        &mut session,
        3,
        document_map(
            &["main.opy", "caller.opy", "env/member.opy"],
            &|member, text| match member {
                "env/member.opy" => *text = text.lines().skip(1).collect::<Vec<_>>().join("\n"),
                "main.opy" => {
                    *text =
                        text.replacen("#!include \"caller.opy\"", "#!include \".\\caller.opy\"", 1)
                }
                _ => {}
            },
        ),
    );
    assert_eq!(unordered.len(), 3);
    for document in &unordered {
        assert_eq!(
            document["diagnostics"],
            json!([]),
            "{} reports no diagnostics",
            document["uri"]
        );
    }

    let broken = check(
        &mut session,
        4,
        document_map(&all, &|member, text| match member {
            "main.opy" => {
                *text = text.replacen(
                    "#!include \"env/member.opy\"\n#!include \"caller.opy\"",
                    "#!include \"caller.opy\"\n#!include \"env/member.opy\"",
                    1,
                );
            }
            "caller.opy" => {
                text.push_str("\nrule \"bad caller\":\n    definitelyMissingAction()\n")
            }
            "env/member.opy" => text.push_str("\nrule \"broken\n"),
            "stray.opy" => text.push_str("\nrule \"stray-broken\n"),
            _ => {}
        }),
    );
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
fn document_check_defers_members_reached_through_escaping_includes() {
    let root = Path::new(INCLUDE_PARENT_ESCAPE_ROOT);
    let members = [
        "main.opy",
        "env/defs.opy",
        "env/vars.opy",
        "composition/boot.opy",
        "core.opy",
        "member/m.opy",
    ];
    let uris: BTreeMap<&str, String> = members
        .into_iter()
        .map(|member| (member, file_uri(root.join(member).to_str().expect("utf8"))))
        .collect();
    let documents = |subset: &[&str]| {
        let mut documents = serde_json::Map::new();
        for member in subset {
            let uri = &uris[*member];
            documents.insert(
                uri.clone(),
                json!({
                    "uri": uri,
                    "languageId": "opy",
                    "version": 1,
                    "text": std::fs::read_to_string(root.join(member)).expect("fixture text"),
                }),
            );
        }
        Value::Object(documents)
    };
    let check = |session: &mut Session, id: u64, documents: Value| {
        session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/check",
            "params": { "documents": documents },
        }))["result"]["documents"]
            .as_array()
            .expect("documents")
            .clone()
    };

    let mut session = Session::spawn();
    session.initialize();

    let clean = check(&mut session, 2, documents(&members));
    assert_eq!(clean.len(), members.len());
    for document in &clean {
        assert_eq!(
            document["diagnostics"],
            json!([]),
            "{} reports no diagnostics",
            document["uri"]
        );
    }

    let orphaned: Vec<&str> = members
        .iter()
        .copied()
        .filter(|member| *member != "main.opy")
        .collect();
    let orphaned = check(&mut session, 3, documents(&orphaned));
    let member = orphaned
        .iter()
        .find(|document| document["uri"] == uris["member/m.opy"])
        .expect("member result");
    assert!(
        member["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["code"] == "unknown-identifier"),
        "the member's missing declaration still reports: {}",
        member["diagnostics"]
    );
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
    assert_eq!(
        diagnostics_of("file:///pos/bmp.opy")[0]["range"],
        json!({
            "start": { "line": 2, "character": 5 },
            "end": { "line": 2, "character": 6 },
        })
    );
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
    assert_eq!(
        missing["error"]["data"]["lpp"]["details"]["reason"],
        "entryNotFound"
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

#[cfg(unix)]
#[test]
fn unresolvable_entry_reports_unreadable_reason() {
    let dir = std::env::temp_dir().join(format!("opy-lpp-io-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A self-referential symlink fails canonicalization with ELOOP.
    let entry = dir.join("main.opy");
    std::os::unix::fs::symlink(std::path::Path::new("main.opy"), &entry).unwrap();
    let uri = format!("file://{}", entry.display());

    let mut session = Session::spawn();
    session.initialize();
    let response = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": uri,
                "languageId": "opy",
                "version": 7
            }
        }
    }));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        response["error"]["data"]["lpp"]["kind"],
        "projectLoadFailed"
    );
    assert_eq!(
        response["error"]["data"]["lpp"]["details"]["reason"],
        "entryUnreadable"
    );
    session.shutdown();
}

#[test]
fn directory_target_with_file_named_src_reports_not_found() {
    let dir = std::env::temp_dir().join(format!("opy-lpp-file-src-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("src"), "not a directory\n").unwrap();
    let uri = format!("file://{}", dir.canonicalize().unwrap().display());

    let mut session = Session::spawn();
    session.request(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "lpp/initialize",
        "params": { "protocolVersion": "1.2" },
    }));
    let response = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": uri,
                "languageId": "opy",
                "version": 7,
                "kind": "directory"
            }
        }
    }));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        response["error"]["data"]["lpp"]["kind"],
        "projectLoadFailed"
    );
    assert_eq!(
        response["error"]["data"]["lpp"]["details"]["reason"],
        "targetNotFound"
    );
    session.shutdown();
}

/// A file entry addressed through a regular file names an entry that cannot
/// exist: `file.opy/child.opy` fails metadata with `ENOTDIR` on Unix
/// (`NotFound` on Windows) and reports `entryNotFound` — the same reason as
/// an absent file — instead of `entryUnreadable` (opy-rs#497).
#[test]
fn file_entry_through_a_regular_file_reports_not_found() {
    let dir = std::env::temp_dir().join(format!("opy-lpp-notdir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("file.opy");
    std::fs::write(&file, "rule \"x\": @Event global\n").unwrap();
    let uri = format!("file://{}", file.join("child.opy").display());

    let mut session = Session::spawn();
    session.request(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "lpp/initialize",
        "params": { "protocolVersion": "1.2" },
    }));
    let response = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": uri,
                "languageId": "opy",
                "version": 7,
                "kind": "file"
            }
        }
    }));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        response["error"]["data"]["lpp"]["kind"],
        "projectLoadFailed"
    );
    assert_eq!(
        response["error"]["data"]["lpp"]["details"]["reason"],
        "entryNotFound"
    );
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
    // `lpp/lookup` is negotiated at LPP 1.5; the 1.1 session does not offer it.
    let unavailable = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/lookup",
        "params": { "languageId": "opy" },
    }));
    assert_eq!(
        unavailable["error"]["data"]["lpp"]["kind"],
        "capabilityUnavailable"
    );
    assert_eq!(
        unavailable["error"]["data"]["lpp"]["details"]["capability"],
        "lookup"
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

    let lib_rule = mapped_span(&document, "rule", &[("rule", 0)]).expect("lib rule");
    assert_eq!(span_text(lib_rule).0, lib_index as u64);
    let main_rule = mapped_span(&document, "rule", &[("rule", 1)]).expect("main rule");
    assert_eq!(span_text(main_rule).0, main_index as u64);

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
    let span = artifact
        .wir
        .action_span(0, 0)
        .expect("expanded action span");
    assert_eq!(span.start.line, 4);
}

const REFUSED_ARTIFACT: &str = concat!(
    "variables {\n",
    "    global:\n",
    "        0: g\n",
    "}\n",
    "\n",
    "rule (\"main\") {\n",
    "    event {\n",
    "        Ongoing - Global;\n",
    "    }\n",
    "    actions {\n",
    "        Chase Global Variable At Rate(g, 100, 1, Destination And Duration);\n",
    "        Set Player Variable At Index(All Players(All), x, 0, 0);\n",
    "    }\n",
    "}\n",
);

fn reconstruct_artifact(session: &mut Session, artifact: Value) -> Value {
    session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/reconstruct",
        "params": { "artifact": artifact },
    }))
}

#[test]
fn reconstruct_is_advertised_and_returns_opy_for_each_emitted_artifact_format() {
    let mut session = Session::spawn();
    let initialize = session.initialize_version("1.4");
    assert_eq!(
        initialize["result"]["capabilities"]["reconstruct"],
        json!(true)
    );

    let text = compile_entry(&mut session, BASIC_RULE, Some(json!([TEXT_V1, MAPPED_V1])))["result"]
        ["artifact"]
        .clone();
    assert_eq!(text["format"], json!(TEXT_V1));
    let mapped =
        compile_entry(&mut session, BASIC_RULE, Some(json!([MAPPED_V1])))["result"]["artifact"]
            .clone();
    assert_eq!(mapped["format"], json!(MAPPED_V1));

    let from_text = reconstruct_artifact(&mut session, text);
    let source = from_text["result"]["source"]
        .as_str()
        .unwrap_or_else(|| panic!("reconstruct returns OPY source: {from_text}"));
    assert!(source.contains("rule \"setup\""), "{source}");
    assert!(source.contains("@Event global"), "{source}");
    assert!(source.contains("disableInspector()"), "{source}");

    let from_mapped = reconstruct_artifact(&mut session, mapped);
    assert_eq!(from_mapped["result"]["source"], json!(source));
    session.shutdown();
}

#[test]
fn reconstruct_refusal_carries_every_issue_code_and_span() {
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let response = reconstruct_artifact(
        &mut session,
        json!({ "format": TEXT_V1, "content": REFUSED_ARTIFACT }),
    );
    assert_eq!(response["error"]["code"], json!(-32000));
    let lpp = &response["error"]["data"]["lpp"];
    assert_eq!(lpp["kind"], json!("refusal"));
    // Refusal `details` is confined to `refusalCode` plus spec-owned echo
    // keys (LPP §18.3), so it names the first issue's stable code; `message`
    // carries every issue as `<code>: <message> at <line>:<column>` so
    // clients can show each code with its span.
    assert_eq!(
        lpp["details"]["refusalCode"],
        json!("unsupported-enum-domain-mismatch")
    );
    let message = response["error"]["message"].as_str().expect("message");
    for (code, span) in [
        ("unsupported-enum-domain-mismatch", "at 11:50"),
        ("unsupported-value-call", "at 11:50"),
        ("unsupported-action-call", "at 12:9"),
    ] {
        assert!(
            message
                .lines()
                .any(|line| line.contains(&format!("{code}: ")) && line.ends_with(span)),
            "'{code}' issue at {span} missing from refusal message: {message}"
        );
    }
    session.shutdown();
}

#[test]
fn reconstruct_rejects_unknown_formats_and_malformed_artifacts() {
    let mut session = Session::spawn();
    session.initialize_version("1.4");

    // A well-formed envelope in a format this provider does not emit is the
    // §11.3 refusal, echoing `format` as the only spec-owned detail key.
    let unknown = reconstruct_artifact(
        &mut session,
        json!({ "format": "x-other/artifact-v1", "content": "payload" }),
    );
    assert_eq!(unknown["error"]["code"], json!(-32000));
    assert_eq!(
        unknown["error"]["data"]["lpp"],
        json!({
            "kind": "refusal",
            "details": {
                "refusalCode": "reconstruct.artifactFormatUnsupported",
                "format": "x-other/artifact-v1",
            },
        })
    );

    let malformed_text = reconstruct_artifact(
        &mut session,
        json!({ "format": TEXT_V1, "content": "rule (broken" }),
    );
    assert_eq!(
        malformed_text["error"]["data"]["lpp"]["kind"],
        json!("invalidArtifact")
    );
    assert!(
        malformed_text["error"]["data"]["lpp"]["details"]["reason"].is_string(),
        "invalidArtifact carries a reason: {malformed_text}"
    );
    let malformed_mapped = reconstruct_artifact(
        &mut session,
        json!({ "format": MAPPED_V1, "content": "not json" }),
    );
    assert_eq!(
        malformed_mapped["error"]["data"]["lpp"]["kind"],
        json!("invalidArtifact")
    );

    // An envelope that does not match the §11.1 schema is Invalid params.
    let missing_artifact = session.request(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "lpp/reconstruct",
        "params": {},
    }));
    assert_eq!(missing_artifact["error"]["code"], json!(-32602));
    session.shutdown();
}

const RENAME_MAIN_URI: &str = "file:///project/main.opy";
const RENAME_DEFS_URI: &str = "file:///project/shared/defs.opy";

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
    assert_eq!(sorted_ranges(main), vec![(6, 1, 10), (7, 21, 30)]);

    let defs = edits
        .iter()
        .find(|edit| edit["documentUri"] == RENAME_DEFS_URI)
        .expect("defs edits");
    assert_eq!(defs["version"], 5);
    assert_eq!(sorted_ranges(defs), vec![(0, 10, 19), (5, 4, 13)]);

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
fn validate_edits_accepts_fragments_of_a_cross_file_rename() {
    let dir = std::env::temp_dir().join(format!("opy-lpp-fragval-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("shared")).unwrap();
    let defs = "subroutine doWork\n\ndef doWork():\n    pass\n";
    let main = "#!include \"shared/defs.opy\"\n\nrule \"work\":\n    @Event global\n    doWork()\n";
    std::fs::write(dir.join("shared").join("defs.opy"), defs).unwrap();
    std::fs::write(dir.join("main.opy"), main).unwrap();
    let base = dir.canonicalize().unwrap();
    let main_uri = format!("file://{}", base.join("main.opy").display());
    let defs_uri = format!("file://{}", base.join("shared").join("defs.opy").display());
    let documents = || {
        json!({
            main_uri.clone(): {
                "uri": main_uri, "languageId": "opy", "version": 1, "text": main,
            },
            defs_uri.clone(): {
                "uri": defs_uri, "languageId": "opy", "version": 1, "text": defs,
            },
        })
    };

    let mut session = Session::spawn();
    session.initialize();
    let renamed = session.request(rename_request(
        2,
        documents(),
        &defs_uri,
        2,
        6,
        "performTask",
    ));
    let edits = renamed["result"]["edits"].as_array().expect("edits");
    assert_eq!(edits.len(), 2);
    let main_edits = edits
        .iter()
        .find(|edit| edit["documentUri"] == main_uri)
        .expect("main edits");
    let defs_edits = edits
        .iter()
        .find(|edit| edit["documentUri"] == defs_uri)
        .expect("defs edits");

    for (id, uri, text, doc_edits) in [
        (3, &main_uri, main, main_edits),
        (4, &defs_uri, defs, defs_edits),
    ] {
        let response = session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/validateEdits",
            "params": {
                "document": {
                    "uri": uri,
                    "languageId": "opy",
                    "version": doc_edits["version"],
                    "text": text,
                },
                "edits": doc_edits["textEdits"],
            },
        }));
        assert_eq!(
            response["result"],
            json!({ "valid": true, "version": doc_edits["version"] }),
            "{uri} edits are well-formed fragments of the transaction"
        );
    }

    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "lpp/check",
        "params": {
            "documents": {
                main_uri.clone(): {
                    "uri": main_uri, "languageId": "opy", "version": 2,
                    "text": apply_text_edits(main, main_edits),
                },
                defs_uri.clone(): {
                    "uri": defs_uri, "languageId": "opy", "version": 2,
                    "text": apply_text_edits(defs, defs_edits),
                },
            }
        }
    }));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        checked["result"]["documents"]
            .as_array()
            .expect("documents")
            .iter()
            .all(|document| document["diagnostics"] == json!([])),
        "the composed rename checks clean: {checked}"
    );
    session.shutdown();
}

#[test]
fn rename_player_and_subroutine_cover_their_binding_groups() {
    let mut session = Session::spawn();
    session.initialize();

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

    let colliding = session.request(rename_request(
        7,
        documents(),
        RENAME_MAIN_URI,
        7,
        25,
        "other",
    ));
    assert_eq!(refusal_code(&colliding), "rename.nameCollision");

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

    let valid = validate(
        &mut session,
        2,
        json!([edit(7, 21, 30, "vault"), edit(6, 1, 10, "vault")]),
    );
    assert_eq!(valid["result"], json!({ "valid": true, "version": 7 }));

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
        json!({ "valid": true, "version": 2 }),
        "an introduced unknown-action is a semantic diagnostic, not malformed syntax"
    );

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

    let entity = |session: &mut Session, id: i64, new_text: &str| {
        session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/validateEdits",
            "params": {
                "document": {
                    "uri": "file:///project/entity.opy",
                    "languageId": "opy",
                    "version": 3,
                    "text": "rule \"r\":\n    @Event global\n    wait(0.1)\n",
                },
                "edits": [{
                    "range": {
                        "start": { "line": 0, "character": 6 },
                        "end": { "line": 0, "character": 7 },
                    },
                    "newText": new_text,
                }],
            },
        }))
    };
    assert_eq!(
        entity(&mut session, 10, "r \\&bogus;")["result"],
        json!({ "valid": false, "version": 3, "reason": "syntaxError" }),
        "an unknown entity in the edited document is a syntaxError"
    );
    assert_eq!(
        entity(&mut session, 11, "r \\&bogus")["result"]["reason"],
        "syntaxError",
        "an unterminated entity in the edited document is a syntaxError"
    );

    let malformed_doc = |session: &mut Session, id: i64, text: &str| {
        session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/validateEdits",
            "params": {
                "document": {
                    "uri": "file:///project/malformed.opy",
                    "languageId": "opy",
                    "version": 1,
                    "text": text,
                },
                "edits": [],
            },
        }))
    };
    for (id, text) in [
        (
            12,
            "#!postCompileHook junk\nrule \"r\":\n    @Event global\n    pass\n",
        ),
        (
            13,
            "#!define s(x) __script__(x\nrule \"r\":\n    @Event global\n    pass\n",
        ),
        (
            14,
            "enum E:\n    X = 1 + 2\n    Y\nrule \"r\":\n    @Event global\n    pass\n",
        ),
    ] {
        assert_eq!(
            malformed_doc(&mut session, id, text)["result"],
            json!({ "valid": false, "version": 1, "reason": "syntaxError" }),
            "malformation in the document's own text: {text:?}"
        );
    }
    session.shutdown();
}

#[test]
fn end_of_document_positions_do_not_panic() {
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

    let axis = session.request(rename_request(4, documents(), RENAME_MAIN_URI, 7, 16, "x"));
    assert_eq!(refusal_code(&axis), "rename.invalidName");

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

const NAV_DEFS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/multi-file/shared/defs.opy"
);

fn fixture_document(path: &str, version: i64) -> Value {
    json!({
        "uri": file_uri(path),
        "languageId": "opy",
        "version": version,
        "text": std::fs::read_to_string(path).expect("fixture text"),
    })
}

fn position_request(
    method: &str,
    id: i64,
    document: &Value,
    line: u32,
    character: u32,
    include_declaration: Option<bool>,
) -> Value {
    let mut params = json!({
        "document": document,
        "position": { "line": line, "character": character },
    });
    if let Some(include) = include_declaration {
        params["includeDeclaration"] = json!(include);
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
}

#[test]
fn symbols_list_each_documents_declarations_in_map_key_order() {
    let mut session = Session::spawn();
    session.initialize();
    let main_uri = file_uri(MULTI_FILE_MAIN);
    let defs_uri = file_uri(NAV_DEFS);
    let listed = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/symbols",
        "params": {
            "documents": {
                defs_uri.clone(): fixture_document(NAV_DEFS, 7),
                main_uri.clone(): fixture_document(MULTI_FILE_MAIN, 3),
            },
        },
    }));
    let documents = listed["result"]["documents"].as_array().expect("documents");
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0]["uri"], main_uri);
    assert_eq!(documents[0]["version"], 3);
    assert_eq!(
        documents[0]["symbols"],
        json!([
            {
                "name": "P",
                "kind": "player",
                "range": {
                    "start": { "line": 3, "character": 10 },
                    "end": { "line": 3, "character": 11 },
                },
            },
        ])
    );
    assert_eq!(documents[1]["uri"], defs_uri);
    assert_eq!(documents[1]["version"], 7);
    assert_eq!(
        documents[1]["symbols"],
        json!([
            {
                "name": "total",
                "kind": "global",
                "range": {
                    "start": { "line": 0, "character": 10 },
                    "end": { "line": 0, "character": 15 },
                },
            },
            {
                "name": "resetScore",
                "kind": "subroutine",
                "range": {
                    "start": { "line": 2, "character": 11 },
                    "end": { "line": 2, "character": 21 },
                },
            },
            {
                "name": "doubleIt",
                "kind": "macro",
                "range": {
                    "start": { "line": 8, "character": 6 },
                    "end": { "line": 8, "character": 14 },
                },
            },
            {
                "name": "finish",
                "kind": "subroutine",
                "range": {
                    "start": { "line": 11, "character": 4 },
                    "end": { "line": 11, "character": 10 },
                },
            },
            {
                "name": "finish",
                "kind": "def",
                "range": {
                    "start": { "line": 11, "character": 4 },
                    "end": { "line": 11, "character": 10 },
                },
            },
        ])
    );
    session.shutdown();
}

#[test]
fn symbols_attribute_included_declarations_through_the_project_view() {
    let root = Path::new(RENAME_MAIN_FILE_ROOT);
    let uris: BTreeMap<&str, String> = ["caller.opy", "env/member.opy", "stray.opy"]
        .into_iter()
        .map(|member| (member, file_uri(root.join(member).to_str().expect("utf8"))))
        .collect();
    let document = |member: &str| {
        json!({
            "uri": uris[member],
            "languageId": "opy",
            "version": 4,
            "text": std::fs::read_to_string(root.join(member)).expect("fixture text"),
        })
    };
    let mut session = Session::spawn();
    session.initialize();
    let listed = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/symbols",
        "params": {
            "documents": {
                uris["caller.opy"].clone(): document("caller.opy"),
                uris["env/member.opy"].clone(): document("env/member.opy"),
                uris["stray.opy"].clone(): document("stray.opy"),
            },
        },
    }));
    let documents = listed["result"]["documents"].as_array().expect("documents");
    assert_eq!(documents.len(), 3);
    assert_eq!(documents[0]["uri"], uris["caller.opy"]);
    assert_eq!(documents[0]["symbols"], json!([]));
    assert_eq!(documents[1]["uri"], uris["env/member.opy"]);
    assert_eq!(
        documents[1]["symbols"],
        json!([
            {
                "name": "worker",
                "kind": "subroutine",
                "range": {
                    "start": { "line": 2, "character": 11 },
                    "end": { "line": 2, "character": 17 },
                },
            },
            {
                "name": "worker",
                "kind": "def",
                "range": {
                    "start": { "line": 4, "character": 4 },
                    "end": { "line": 4, "character": 10 },
                },
            },
        ])
    );
    assert_eq!(documents[2]["uri"], uris["stray.opy"]);
    assert_eq!(documents[2]["symbols"], json!([]));
    session.shutdown();
}

#[test]
fn symbols_rejects_project_fields_outside_its_schema() {
    // Spec §19.1: a field the spec defines but that `lpp/symbols`' params
    // schema does not include is a schema violation, not an ignored field.
    let mut session = Session::spawn();
    session.initialize();
    let main_uri = file_uri(MULTI_FILE_MAIN);
    let with_entry = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/symbols",
        "params": {
            "documents": {
                main_uri.clone(): fixture_document(MULTI_FILE_MAIN, 3),
            },
            "entry": {
                "uri": main_uri,
                "languageId": "opy",
                "version": 3,
            },
        },
    }));
    assert_eq!(with_entry["error"]["code"], -32602);
    let without_documents = session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/symbols",
        "params": {},
    }));
    assert_eq!(without_documents["error"]["code"], -32602);
    session.shutdown();
}

#[test]
fn definition_returns_authored_include_declarations() {
    let mut session = Session::spawn();
    session.initialize();
    let main_uri = file_uri(MULTI_FILE_MAIN);
    let defs_uri = file_uri(NAV_DEFS);
    let document = fixture_document(MULTI_FILE_MAIN, 5);

    let resolved = session.request(position_request(
        "lpp/definition",
        2,
        &document,
        10,
        6,
        None,
    ));
    assert_eq!(
        resolved["result"]["locations"],
        json!([
            {
                "uri": defs_uri,
                "range": {
                    "start": { "line": 2, "character": 11 },
                    "end": { "line": 2, "character": 21 },
                },
            },
        ])
    );

    let resolved = session.request(position_request(
        "lpp/definition",
        3,
        &document,
        8,
        10,
        None,
    ));
    assert_eq!(
        resolved["result"]["locations"],
        json!([
            {
                "uri": defs_uri,
                "range": {
                    "start": { "line": 0, "character": 10 },
                    "end": { "line": 0, "character": 15 },
                },
            },
        ])
    );

    let resolved = session.request(position_request(
        "lpp/definition",
        4,
        &document,
        9,
        20,
        None,
    ));
    assert_eq!(
        resolved["result"]["locations"],
        json!([
            {
                "uri": main_uri,
                "range": {
                    "start": { "line": 3, "character": 10 },
                    "end": { "line": 3, "character": 11 },
                },
            },
        ])
    );

    let resolved = session.request(position_request(
        "lpp/definition",
        5,
        &document,
        11,
        5,
        None,
    ));
    assert_eq!(
        resolved["result"]["locations"],
        json!([
            {
                "uri": defs_uri,
                "range": {
                    "start": { "line": 8, "character": 6 },
                    "end": { "line": 8, "character": 14 },
                },
            },
        ])
    );
    session.shutdown();
}

#[test]
fn definition_on_a_declaration_returns_the_declaration_itself() {
    let mut session = Session::spawn();
    session.initialize();
    let defs_uri = file_uri(NAV_DEFS);
    let document = fixture_document(NAV_DEFS, 7);

    let resolved = session.request(position_request(
        "lpp/definition",
        2,
        &document,
        0,
        12,
        None,
    ));
    assert_eq!(
        resolved["result"]["locations"],
        json!([
            {
                "uri": defs_uri,
                "range": {
                    "start": { "line": 0, "character": 10 },
                    "end": { "line": 0, "character": 15 },
                },
            },
        ])
    );
    session.shutdown();
}

#[test]
fn references_list_declaration_first_then_sites_sorted_by_start() {
    let mut session = Session::spawn();
    session.initialize();
    let defs_uri = file_uri(NAV_DEFS);
    let document = fixture_document(MULTI_FILE_MAIN, 5);

    let resolved = session.request(position_request(
        "lpp/references",
        2,
        &document,
        8,
        10,
        Some(true),
    ));
    assert_eq!(
        resolved["result"]["locations"],
        json!([
            {
                "uri": defs_uri,
                "range": {
                    "start": { "line": 0, "character": 10 },
                    "end": { "line": 0, "character": 15 },
                },
            },
            {
                "uri": document["uri"],
                "range": {
                    "start": { "line": 8, "character": 8 },
                    "end": { "line": 8, "character": 13 },
                },
            },
            {
                "uri": document["uri"],
                "range": {
                    "start": { "line": 9, "character": 24 },
                    "end": { "line": 9, "character": 29 },
                },
            },
            {
                "uri": defs_uri,
                "range": {
                    "start": { "line": 12, "character": 4 },
                    "end": { "line": 12, "character": 9 },
                },
            },
        ])
    );

    let resolved = session.request(position_request(
        "lpp/references",
        3,
        &document,
        8,
        10,
        Some(false),
    ));
    assert_eq!(
        resolved["result"]["locations"],
        json!([
            {
                "uri": document["uri"],
                "range": {
                    "start": { "line": 8, "character": 8 },
                    "end": { "line": 8, "character": 13 },
                },
            },
            {
                "uri": document["uri"],
                "range": {
                    "start": { "line": 9, "character": 24 },
                    "end": { "line": 9, "character": 29 },
                },
            },
            {
                "uri": defs_uri,
                "range": {
                    "start": { "line": 12, "character": 4 },
                    "end": { "line": 12, "character": 9 },
                },
            },
        ])
    );
    session.shutdown();
}

#[test]
fn navigation_refusals_and_invalid_positions_are_structured() {
    let mut session = Session::spawn();
    session.initialize();
    let main_uri = file_uri(MULTI_FILE_MAIN);
    let document = fixture_document(MULTI_FILE_MAIN, 5);

    let refused = session.request(position_request("lpp/definition", 2, &document, 6, 5, None));
    assert_eq!(refused["error"]["code"], -32000);
    assert_eq!(
        refused["error"]["data"]["lpp"]["details"],
        json!({
            "refusalCode": "definition.noSymbolAtPosition",
            "uri": main_uri,
        })
    );

    let refused = session.request(position_request(
        "lpp/references",
        3,
        &document,
        6,
        5,
        Some(true),
    ));
    assert_eq!(refused["error"]["code"], -32000);
    assert_eq!(
        refused["error"]["data"]["lpp"]["details"],
        json!({
            "refusalCode": "references.noSymbolAtPosition",
            "uri": main_uri,
        })
    );

    for (method, id, line, character, include_declaration) in [
        ("lpp/definition", 4, 99, 0, None),
        ("lpp/references", 5, 4, 999, Some(false)),
    ] {
        let failed = session.request(position_request(
            method,
            id,
            &document,
            line,
            character,
            include_declaration,
        ));
        assert_eq!(failed["error"]["code"], -32000);
        assert_eq!(failed["error"]["data"]["lpp"]["kind"], "invalidPosition");
        assert_eq!(
            failed["error"]["data"]["lpp"]["details"],
            json!({
                "uri": main_uri,
                "position": { "line": line, "character": character },
            })
        );
    }
    session.shutdown();
}

fn lookup_request(id: i64, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "lpp/lookup",
        "params": params,
    })
}

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

#[test]
fn lookup_events_return_the_event_vocabulary() {
    let mut session = Session::spawn();
    session.initialize_version("1.5");

    let events = session.request(lookup_request(
        2,
        json!({ "languageId": "opy", "kind": "event", "limit": 20 }),
    ));
    let entries = events["result"]["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 13, "the accepted @Event set: {entries:?}");
    assert!(entries.iter().all(|entry| entry["kind"] == "event"));
    let died = entries
        .iter()
        .find(|entry| entry["spelling"] == "playerDied")
        .expect("playerDied is an event");
    assert_eq!(died["displayName"], "Player Died");
    assert_eq!(died["identity"], "opy:event/playerDied");
    assert_eq!(died["event"]["acceptsFilters"], true);
    let global = entries
        .iter()
        .find(|entry| entry["spelling"] == "global")
        .expect("global is an event");
    assert_eq!(
        global["event"]["acceptsFilters"], false,
        "global events reject player filters"
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry["spelling"] != "subroutine"),
        "subroutine is not an @Event name: {entries:?}"
    );

    let queried = session.request(lookup_request(
        3,
        json!({ "languageId": "opy", "query": "Player Earned Elimination", "kind": "event" }),
    ));
    let queried_entries = queried["result"]["entries"].as_array().expect("entries");
    assert_eq!(queried_entries[0]["spelling"], "playerEarnedElimination");
    session.shutdown();
}

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

fn resolve_ids_request(id: i64, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "lpp/resolveIds",
        "params": params,
    })
}

#[test]
fn resolve_ids_capability_is_negotiated_at_1_6() {
    let mut session = Session::spawn();
    let initialized = session.initialize_version("1.5");
    assert_eq!(initialized["result"]["protocolVersion"], "1.5");
    assert_eq!(
        initialized["result"]["capabilities"]["resolveIds"],
        Value::Null,
        "resolveIds is not advertised before 1.6"
    );
    let response = session.request(resolve_ids_request(
        2,
        json!({ "languageId": "opy", "calls": ["applyImpulse"] }),
    ));
    assert_eq!(response["error"]["code"], -32000);
    assert_eq!(
        response["error"]["data"]["lpp"]["kind"],
        "capabilityUnavailable"
    );
    assert_eq!(
        response["error"]["data"]["lpp"]["details"],
        json!({ "capability": "resolveIds", "method": "lpp/resolveIds" })
    );
    session.shutdown();

    let mut session = Session::spawn();
    let initialized = session.initialize_version("1.6");
    assert_eq!(initialized["result"]["protocolVersion"], "1.6");
    assert_eq!(initialized["result"]["capabilities"]["resolveIds"], true);
    session.shutdown();
}

#[test]
fn resolve_ids_answers_from_vocabulary_without_a_project() {
    let mut session = Session::spawn();
    session.initialize_version("1.6");

    let response = session.request(resolve_ids_request(
        2,
        json!({
            "languageId": "opy",
            "calls": [
                "applyImpulse",
                "setStatusEffect",
                "createHudText",
                "lastCreatedEntity",
                "isButtonHeld",
            ],
            "enums": {
                "Status": ["STUNNED"],
                "Map": ["ROUTE_66"],
                "Clipping": ["DO_NOT_CLIP"],
            },
        }),
    ));
    let result = &response["result"];
    for (id, spelling) in result["calls"].as_object().expect("calls map") {
        let lookup = session.request(lookup_request(
            100,
            json!({ "languageId": "opy", "query": id, "limit": 10 }),
        ));
        let spellings: Vec<&str> = lookup["result"]["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .filter_map(|entry| entry["spelling"].as_str())
            .collect();
        assert!(
            spellings.contains(&spelling.as_str().expect("spelling")),
            "{id} resolved to {spelling}, absent from lookup entries {spellings:?}"
        );
    }
    assert_eq!(result["calls"]["applyImpulse"], "applyImpulse");
    assert_eq!(result["calls"]["setStatusEffect"], "setStatusEffect");
    assert_eq!(result["calls"]["createHudText"], "hudText");
    assert_eq!(result["calls"]["lastCreatedEntity"], "getLastCreatedEntity");
    assert_eq!(result["calls"]["isButtonHeld"], "isHoldingButton");

    let enums = result["enums"].as_object().expect("enums map");
    let mut request_id = 200;
    for (domain, members) in enums {
        let opy_domain = match domain.as_str() {
            "Clipping" => "Clip",
            other => other,
        };
        for (member, spelling) in members.as_object().expect("member map") {
            let full = format!("{opy_domain}.{}", spelling.as_str().expect("s"));
            let lookup = session.request(lookup_request(
                request_id,
                json!({ "languageId": "opy", "query": full, "limit": 10 }),
            ));
            request_id += 1;
            let spellings: Vec<&str> = lookup["result"]["entries"]
                .as_array()
                .expect("entries")
                .iter()
                .filter_map(|entry| entry["spelling"].as_str())
                .collect();
            assert!(
                spellings.contains(&full.as_str()),
                "{domain}.{member} resolved to {spelling}; lookup has {spellings:?}"
            );
        }
    }
    assert_eq!(enums["Status"]["STUNNED"], "STUNNED");
    assert_eq!(enums["Map"]["ROUTE_66"], "ROUTE66");
    assert_eq!(enums["Clipping"]["DO_NOT_CLIP"], "NONE");
    session.shutdown();
}

#[test]
fn resolve_ids_spells_operators_and_source_forms() {
    let mut session = Session::spawn();
    session.initialize_version("1.6");

    let response = session.request(resolve_ids_request(
        2,
        json!({
            "languageId": "opy",
            "calls": [
                "add", "subtract", "multiply", "divide", "modulo",
                "raiseToPower", "==", "!=", "<", "<=", ">", ">=",
                "and", "or", "not", "arrayContains",
                "appendToArray", "removeFromArray", "removeFromArrayByValue",
                "removeFromArrayByIndex", "mappedArray", "sortedArray",
                "isTrueForAll", "isTrueForAny",
                "abort", "skip", "if", "elseIf", "else", "while",
                "forGlobalVariable", "forPlayerVariable",
            ],
        }),
    ));
    let calls = &response["result"]["calls"];
    let expected = json!({
        "add": "+", "subtract": "-", "multiply": "*", "divide": "/",
        "modulo": "%", "raiseToPower": "**",
        "==": "==", "!=": "!=", "<": "<", "<=": "<=", ">": ">", ">=": ">=",
        "and": "and", "or": "or", "not": "not", "arrayContains": "in",
        "appendToArray": "append", "removeFromArray": "exclude",
        "removeFromArrayByValue": "remove", "removeFromArrayByIndex": "del",
        "mappedArray": "map", "sortedArray": "sorted",
        "isTrueForAll": "all", "isTrueForAny": "any",
        "abort": "return", "skip": "goto",
        "if": "if", "elseIf": "elif", "else": "else", "while": "while",
        "forGlobalVariable": "for", "forPlayerVariable": "for",
    });
    assert_eq!(*calls, expected);
    session.shutdown();
}

#[test]
fn resolve_ids_omits_unspelled_and_unknown_ids() {
    let mut session = Session::spawn();
    session.initialize_version("1.6");

    let response = session.request(resolve_ids_request(
        2,
        json!({
            "languageId": "opy",
            "calls": [
                "notAnId",
                "evalOnce",
                "all", "log", "getPlayers",
                "end", "setGlobalVariable", "globalVariable",
                "callSubroutine", "valueInArray", "firstOf",
                "ifThenElse", "roundToInteger", "loopIf",
            ],
            "enums": {
                "Status": ["STUNNED", "NO_SUCH_MEMBER"],
                "Rounding": ["DOWN"],
                "NoSuchDomain": ["ANY"],
            },
        }),
    ));
    let result = &response["result"];
    assert_eq!(result["calls"], json!({}));
    assert_eq!(
        result["enums"],
        json!({ "Status": { "STUNNED": "STUNNED" } }),
        "unspelled members and unspelled domains are omitted"
    );
    session.shutdown();
}

#[test]
fn resolve_ids_empty_and_deterministic() {
    let mut session = Session::spawn();
    session.initialize_version("1.6");

    let empty = session.request(resolve_ids_request(2, json!({ "languageId": "opy" })));
    assert_eq!(empty["result"], json!({ "calls": {}, "enums": {} }));

    let params = json!({
        "languageId": "opy",
        "calls": ["applyImpulse", "appendToArray"],
        "enums": { "Status": ["STUNNED"] },
    });
    let first = session.request(resolve_ids_request(3, params.clone()));
    let second = session.request(resolve_ids_request(4, params));
    assert_eq!(first["result"], second["result"]);
    session.shutdown();
}

#[test]
fn resolve_ids_validates_params() {
    let mut session = Session::spawn();
    session.initialize_version("1.6");

    let mut id = 2;
    let mut invalid_params = |session: &mut Session, params: Value| {
        id += 1;
        session.request(resolve_ids_request(id, params))["error"]["code"]
            .as_i64()
            .expect("error code")
    };
    assert_eq!(
        invalid_params(&mut session, json!({ "calls": ["applyImpulse"] })),
        -32602
    );
    assert_eq!(
        invalid_params(&mut session, json!({ "languageId": "opy", "calls": [1] })),
        -32602
    );
    assert_eq!(
        invalid_params(
            &mut session,
            json!({ "languageId": "opy", "calls": "applyImpulse" })
        ),
        -32602
    );
    assert_eq!(
        invalid_params(
            &mut session,
            json!({ "languageId": "opy", "enums": ["Status"] })
        ),
        -32602
    );
    assert_eq!(
        invalid_params(
            &mut session,
            json!({ "languageId": "opy", "enums": { "Status": "STUNNED" } })
        ),
        -32602
    );

    let wrong_language = session.request(resolve_ids_request(
        20,
        json!({ "languageId": "x-demo-lang" }),
    ));
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

#[test]
fn resolve_ids_bounds_the_total_ids() {
    let mut session = Session::spawn();
    session.initialize_version("1.6");

    let at_bound = session.request(resolve_ids_request(
        2,
        json!({
            "languageId": "opy",
            "calls": vec!["applyImpulse"; 1024],
        }),
    ));
    assert_eq!(
        at_bound["result"]["calls"],
        json!({ "applyImpulse": "applyImpulse" }),
        "exactly 1024 ids are served"
    );

    let over_bound = session.request(resolve_ids_request(
        3,
        json!({
            "languageId": "opy",
            "calls": vec!["applyImpulse"; 1025],
        }),
    ));
    assert_eq!(over_bound["error"]["code"], -32602);

    let split_over_bound = session.request(resolve_ids_request(
        4,
        json!({
            "languageId": "opy",
            "calls": vec!["applyImpulse"; 1000],
            "enums": { "Status": vec!["STUNNED"; 25] },
        }),
    ));
    assert_eq!(split_over_bound["error"]["code"], -32602);
    session.shutdown();
}

trait CoreDiagnostic {
    fn severity(&self) -> &'static str;
    fn code(&self) -> &str;
    fn message(&self) -> &str;
    fn span(&self) -> Option<&opy_rs::tooling::SourceLocation>;
}

impl CoreDiagnostic for opy_rs::tooling::Diagnostic {
    fn severity(&self) -> &'static str {
        self.severity.as_str()
    }
    fn code(&self) -> &str {
        &self.code
    }
    fn message(&self) -> &str {
        &self.message
    }
    fn span(&self) -> Option<&opy_rs::tooling::SourceLocation> {
        self.span.as_ref()
    }
}

impl CoreDiagnostic for opy_rs::CompileDiagnostic {
    fn severity(&self) -> &'static str {
        self.severity.as_str()
    }
    fn code(&self) -> &str {
        &self.code
    }
    fn message(&self) -> &str {
        &self.message
    }
    fn span(&self) -> Option<&opy_rs::tooling::SourceLocation> {
        self.span.as_ref()
    }
}

fn core_path(display_root: &Path, display_path: &str) -> PathBuf {
    let path = Path::new(display_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        display_root.join(path)
    }
}

fn core_uri(display_root: &Path, display_path: &str) -> String {
    file_uri(
        core_path(display_root, display_path)
            .to_str()
            .expect("utf8 path"),
    )
}

fn utf16_character(line: &str, column: u32) -> u32 {
    let mut units = 0u32;
    let mut current = 1u32;
    for ch in line.chars() {
        if current >= column {
            break;
        }
        units += ch.len_utf16() as u32;
        current += if ch == '\t' { 4 } else { 1 };
    }
    units
}

fn expected_range(
    display_root: &Path,
    location: Option<&opy_rs::tooling::SourceLocation>,
) -> Value {
    let Some(location) = location else {
        return json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 0 },
        });
    };
    let text = std::fs::read_to_string(core_path(display_root, &location.path))
        .expect("registry file reads");
    let position = |line: u32, col: u32| {
        let character = text
            .lines()
            .nth(line.saturating_sub(1) as usize)
            .map(|line| utf16_character(line, col))
            .unwrap_or_else(|| col.saturating_sub(1));
        json!({ "line": line.saturating_sub(1), "character": character })
    };
    json!({
        "start": position(location.start.line, location.start.col),
        "end": position(location.end.line, location.end.col),
    })
}

fn expected_entry_documents<D: CoreDiagnostic>(
    entry_version: i64,
    files: &[opy_rs::preprocess::FileRecord],
    display_root: &Path,
    diagnostics: &[D],
) -> Vec<Value> {
    let mut paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    assert!(
        !paths.is_empty(),
        "the core file registry always keeps file id 0"
    );
    paths.dedup();
    let mut entries: Vec<Vec<Value>> = paths.iter().map(|_| Vec::new()).collect();
    for diagnostic in diagnostics {
        let index = diagnostic
            .span()
            .and_then(|span| paths.iter().position(|path| path == &span.path))
            .unwrap_or(0);
        entries[index].push(json!({
            "range": expected_range(display_root, diagnostic.span()),
            "severity": diagnostic.severity(),
            "code": diagnostic.code(),
            "message": diagnostic.message(),
            "source": "opy",
        }));
    }
    paths
        .iter()
        .zip(entries)
        .map(|(path, diagnostics)| {
            json!({
                "uri": core_uri(display_root, path),
                "version": entry_version,
                "diagnostics": diagnostics,
            })
        })
        .collect()
}

fn expected_source_identity(project: &opy_rs::FilesystemProject) -> String {
    let effective = opy_rs::preprocess::main_file_directive(project.source())
        .and_then(|target| {
            let path = project.root().join(target).canonicalize().ok()?;
            std::fs::read_to_string(path).ok()
        })
        .unwrap_or_else(|| project.source().to_owned());
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(effective.as_bytes());
    let mut hex = String::new();
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[test]
fn entry_check_projects_the_core_check_outcome() {
    let mut session = Session::spawn();
    session.initialize();
    let mut id = 1;
    for (name, entry) in [
        ("clean single file", BASIC_RULE),
        ("failure before a model exists", DIAGNOSTICS),
        ("tab and supplementary-plane columns", DIAGNOSTIC_POSITIONS),
        ("warning severity survives", DUPLICATE_INCLUDE),
        ("multi-file include closure", MULTI_FILE_MAIN),
        ("directory-include processing order", CLEAN_MULTI_FILE_MAIN),
        ("#!mainFile redirect", MAIN_FILE_ENTRY),
        (
            "#!mainFile redirect into a subdirectory",
            MAIN_FILE_SUBDIR_ENTRY,
        ),
        (
            "#!mainFile redirect with a failing effective entry",
            MAIN_FILE_ERROR_ENTRY,
        ),
    ] {
        id += 1;
        let path = Path::new(entry).canonicalize().expect("fixture entry");
        let project = opy_rs::FilesystemProject::load(&path).expect("project loads");
        let outcome = opy_rs::tooling::check(
            project.source(),
            &project.main_path().to_string_lossy(),
            project.root(),
        );

        let checked = session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/check",
            "params": {
                "entry": {
                    "uri": file_uri(entry),
                    "languageId": "opy",
                    "version": 7,
                }
            },
        }));
        assert_eq!(
            checked["result"]["documents"],
            json!(expected_entry_documents(
                7,
                &outcome.files,
                &outcome.display_root,
                &outcome.diagnostics,
            )),
            "{name}: lpp/check must match the core check outcome"
        );
    }
    session.shutdown();
}

#[test]
fn entry_check_projects_include_failures_and_load_errors() {
    let dir = std::env::temp_dir().join(format!("opy-lpp-parity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");

    let mut session = Session::spawn();
    session.initialize();

    let broken = dir.join("broken-include");
    std::fs::create_dir_all(&broken).expect("fixture dir");
    std::fs::write(
        broken.join("main.opy"),
        "#!include \"lib.opy\"\nrule \"main\":\n    @Event global\n    pass\n",
    )
    .expect("main fixture");
    std::fs::write(broken.join("lib.opy"), "rule \"broken\"\n    pass\n").expect("include fixture");
    let main = broken.join("main.opy");
    let project = opy_rs::FilesystemProject::load(&main).expect("project loads");
    let outcome = opy_rs::tooling::check(
        project.source(),
        &project.main_path().to_string_lossy(),
        project.root(),
    );
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(main.to_str().expect("utf8")),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    assert_eq!(
        checked["result"]["documents"],
        json!(expected_entry_documents(
            7,
            &outcome.files,
            &outcome.display_root,
            &outcome.diagnostics,
        )),
        "an include member's diagnostics must land on the member document"
    );

    let missing = dir.join("missing-include");
    std::fs::create_dir_all(&missing).expect("fixture dir");
    std::fs::write(
        missing.join("main.opy"),
        "#!include \"gone.opy\"\nrule \"main\":\n    @Event global\n    pass\n",
    )
    .expect("main fixture");
    let main = missing.join("main.opy");
    let project = opy_rs::FilesystemProject::load(&main).expect("project loads");
    let outcome = opy_rs::tooling::check(
        project.source(),
        &project.main_path().to_string_lossy(),
        project.root(),
    );
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "include-not-found"),
        "fixture premise: the core check reports include-not-found"
    );
    let refused = session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/check",
        "params": {
            "entry": {
                "uri": file_uri(main.to_str().expect("utf8")),
                "languageId": "opy",
                "version": 7,
            }
        },
    }));
    assert_eq!(refused["error"]["data"]["lpp"]["kind"], "projectLoadFailed");
    assert_eq!(
        refused["error"]["data"]["lpp"]["details"]["reason"],
        "requiredSourceUnavailable"
    );

    let path = main.canonicalize().expect("main path");
    let uri = format!("file://{}", path.to_string_lossy());
    let text = std::fs::read_to_string(&main).expect("main text");
    let overlays = BTreeMap::from([(path.to_string_lossy().into_owned(), text.clone())]);
    let outcome = opy_rs::tooling::check_with_overlay(
        &text,
        &path.to_string_lossy(),
        missing.as_path(),
        &overlays,
    );
    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "lpp/check",
        "params": {
            "documents": {
                &uri: {
                    "uri": &uri,
                    "languageId": "opy",
                    "version": 4,
                    "text": &text,
                },
            },
        },
    }));
    let expected: Vec<Value> = outcome
        .diagnostics
        .iter()
        .map(|diagnostic| {
            json!({
                "range": expected_range(&outcome.display_root, diagnostic.span.as_ref()),
                "severity": diagnostic.severity.as_str(),
                "code": diagnostic.code,
                "message": diagnostic.message,
                "source": "opy",
            })
        })
        .collect();
    assert_eq!(
        checked["result"]["documents"],
        json!([{ "uri": uri, "version": 4, "diagnostics": expected }]),
        "documents-mode check keeps the core failure diagnostic"
    );

    std::fs::remove_dir_all(&dir).expect("scratch cleanup");
    session.shutdown();
}

#[test]
fn document_set_check_projects_the_core_check_outcome() {
    let root = Path::new(RENAME_MAIN_FILE_ROOT);
    let members = ["main.opy", "caller.opy", "stray.opy", "env/member.opy"];
    let paths: BTreeMap<&str, PathBuf> = members
        .iter()
        .map(|member| (*member, root.join(member).canonicalize().expect("fixture")))
        .collect();
    let uris: BTreeMap<&str, String> = paths
        .iter()
        .map(|(member, path)| (*member, file_uri(path.to_str().expect("utf8"))))
        .collect();
    let texts = |mutate: &dyn Fn(&str, &mut String)| -> BTreeMap<&str, String> {
        members
            .iter()
            .map(|member| {
                let mut text = std::fs::read_to_string(&paths[member]).expect("fixture text");
                mutate(member, &mut text);
                (*member, text)
            })
            .collect()
    };
    let overlay_map = |texts: &BTreeMap<&str, String>| -> BTreeMap<String, String> {
        texts
            .iter()
            .map(|(member, text)| (paths[member].to_string_lossy().into_owned(), text.clone()))
            .collect()
    };
    let request_documents = |texts: &BTreeMap<&str, String>| -> Value {
        texts
            .iter()
            .map(|(member, text)| {
                let uri = uris[*member].clone();
                (
                    uri.clone(),
                    json!({ "uri": uri, "languageId": "opy", "version": 3, "text": text }),
                )
            })
            .collect::<serde_json::Map<String, Value>>()
            .into()
    };
    let supplied_uri = |display_root: &Path, span_path: &str| -> Option<String> {
        let resolved = core_path(display_root, span_path);
        let resolved = resolved.canonicalize().unwrap_or(resolved);
        paths
            .iter()
            .find_map(|(member, path)| (path == &resolved).then(|| uris[*member].clone()))
    };
    let attribute =
        |outcome: &opy_rs::tooling::CheckOutcome, fallback: &str| -> BTreeMap<String, Vec<Value>> {
            let mut by_uri: BTreeMap<String, Vec<Value>> = BTreeMap::new();
            for diagnostic in &outcome.diagnostics {
                let uri = diagnostic
                    .span
                    .as_ref()
                    .and_then(|span| supplied_uri(&outcome.display_root, &span.path))
                    .unwrap_or_else(|| fallback.to_owned());
                by_uri.entry(uri).or_default().push(json!({
                    "range": expected_range(&outcome.display_root, diagnostic.span.as_ref()),
                    "severity": diagnostic.severity.as_str(),
                    "code": diagnostic.code,
                    "message": diagnostic.message,
                    "source": "opy",
                }));
            }
            by_uri
        };
    let expected_documents =
        |texts: &BTreeMap<&str, String>, by_uri: &BTreeMap<String, Vec<Value>>| -> Vec<Value> {
            let mut uris_sorted: Vec<&String> = texts.keys().map(|member| &uris[*member]).collect();
            uris_sorted.sort();
            uris_sorted
                .iter()
                .map(|uri| {
                    json!({
                        "uri": uri,
                        "version": 3,
                        "diagnostics": by_uri.get(*uri).cloned().unwrap_or_default(),
                    })
                })
                .collect()
        };
    let check = |session: &mut Session, id: u64, texts: &BTreeMap<&str, String>| {
        session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/check",
            "params": { "documents": request_documents(texts) },
        }))["result"]["documents"]
            .as_array()
            .expect("documents")
            .clone()
    };
    let check_on = |texts: &BTreeMap<&str, String>, member: &str| {
        opy_rs::tooling::check_with_overlay(
            &texts[member],
            paths[member].to_str().expect("utf8"),
            paths[member].parent().expect("document directory"),
            &overlay_map(texts),
        )
    };

    let mut session = Session::spawn();
    session.initialize();

    let clean = texts(&|_, _| {});
    let group = check_on(&clean, "main.opy");
    assert!(group.is_clean(), "clean fixture premise");
    assert!(
        !check_on(&clean, "caller.opy").is_clean(),
        "caller cannot resolve worker() outside the project"
    );
    assert_eq!(
        check(&mut session, 2, &clean),
        expected_documents(&clean, &attribute(&group, &uris["main.opy"])),
        "covered members must report the shared analysis, not a standalone parse"
    );

    let broken = texts(&|member, text| match member {
        "main.opy" => {
            *text = text.replacen(
                "#!include \"env/member.opy\"\n#!include \"caller.opy\"",
                "#!include \"caller.opy\"\n#!include \"env/member.opy\"",
                1,
            )
        }
        "caller.opy" => text.push_str("\nrule \"bad caller\":\n    definitelyMissingAction()\n"),
        "env/member.opy" => text.push_str("\nrule \"broken\n"),
        _ => {}
    });
    let group = check_on(&broken, "main.opy");
    assert!(!group.is_clean(), "broken member fails the group check");
    assert!(
        group.files.iter().any(|file| {
            let resolved = core_path(&group.display_root, &file.path);
            resolved.canonicalize().unwrap_or(resolved) == paths["caller.opy"]
        }),
        "caller is registered before the failure — the registry is not coverage"
    );
    let mut by_uri = attribute(&group, &uris["main.opy"]);
    for (uri, diagnostics) in attribute(&check_on(&broken, "caller.opy"), &uris["caller.opy"]) {
        by_uri.entry(uri).or_default().extend(diagnostics);
    }
    assert_eq!(
        check(&mut session, 3, &broken),
        expected_documents(&broken, &by_uri),
        "a failed check's file registry must not silence member diagnostics"
    );
    session.shutdown();
}

#[test]
fn entry_compile_projects_the_core_compile_outcome() {
    let compiler = opy_rs::Compiler::new().expect("compiler");
    let mut session = Session::spawn();
    session.initialize();
    let mut id = 1;
    for (name, entry) in [
        ("clean single file", BASIC_RULE),
        ("#!mainFile redirect", MAIN_FILE_ENTRY),
        (
            "#!mainFile redirect into a subdirectory",
            MAIN_FILE_SUBDIR_ENTRY,
        ),
        (
            "frontend failure before a model exists",
            MAIN_FILE_ERROR_ENTRY,
        ),
        ("integration failure on a resolved model", UNSUPPORTED),
    ] {
        id += 1;
        let path = Path::new(entry).canonicalize().expect("fixture entry");
        let project = opy_rs::FilesystemProject::load(&path).expect("project loads");
        let outcome = opy_rs::compile_with_overlay_outcome(
            project.source(),
            &project.main_path().to_string_lossy(),
            project.root(),
            &BTreeMap::new(),
        );
        let files = outcome.files.clone();
        let display_root = outcome.display_root.clone();
        let report =
            compiler.compile_outcome_report_with_language(outcome, project.root(), "en-US");

        let compiled = session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/compile",
            "params": {
                "entry": {
                    "uri": file_uri(entry),
                    "languageId": "opy",
                    "version": 7,
                }
            },
        }));
        assert_eq!(
            compiled["result"]["diagnostics"],
            json!(expected_entry_documents(
                7,
                &files,
                &display_root,
                &report.compile.diagnostics,
            )),
            "{name}: compile diagnostics must match the core compile report"
        );
        match report.compile.status {
            opy_rs::CompileStatus::Success => assert_eq!(
                compiled["result"]["artifact"],
                json!({
                    "format": TEXT_V1,
                    "content": &report.compile.workshop_exact,
                }),
                "{name}: a successful core compile projects its workshop text"
            ),
            _ => assert_eq!(
                compiled["result"]["artifact"],
                Value::Null,
                "{name}: a failed core compile must not produce an artifact"
            ),
        }
        assert_eq!(
            compiled["result"]["sourceIdentity"],
            json!(expected_source_identity(&project)),
            "{name}: sourceIdentity must hash the effective primary source"
        );
    }
    session.shutdown();
}

#[test]
fn document_compile_projects_the_core_compile_outcome() {
    let compiler = opy_rs::Compiler::new().expect("compiler");
    let mut session = Session::spawn();
    session.initialize();

    let mut id = 1;
    for (name, path) in [
        ("clean document", BASIC_RULE),
        ("frontend failure", DIAGNOSTICS),
        ("integration failure", UNSUPPORTED),
    ] {
        id += 1;
        let uri = file_uri(path);
        let text = std::fs::read_to_string(path).expect("fixture source");
        let document_path = Path::new(path).canonicalize().expect("fixture path");
        let root = document_path.parent().expect("document directory");
        let overlays =
            BTreeMap::from([(document_path.to_string_lossy().into_owned(), text.clone())]);
        let outcome = opy_rs::compile_with_overlay_outcome(
            &text,
            &document_path.to_string_lossy(),
            root,
            &overlays,
        );
        let mut diagnostics: Vec<Value> = outcome
            .diagnostics
            .iter()
            .map(|diagnostic| {
                json!({
                    "range": expected_range(&outcome.display_root, diagnostic.span.as_ref()),
                    "severity": diagnostic.severity.as_str(),
                    "code": diagnostic.code,
                    "message": diagnostic.message,
                    "source": "opy",
                })
            })
            .collect();
        let artifact = match outcome.hir.as_ref() {
            Some(hir) => match compiler.compile_hir(hir) {
                Ok(artifact) => json!({
                    "format": TEXT_V1,
                    "content": artifact.final_output,
                }),
                Err(error) => {
                    diagnostics.push(json!({
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 0 },
                        },
                        "severity": "error",
                        "code": error.diagnostic.code,
                        "message": error.diagnostic.message,
                        "source": "opy",
                    }));
                    Value::Null
                }
            },
            None => Value::Null,
        };
        let compiled = session.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "lpp/compile",
            "params": {
                "documents": {
                    &uri: {
                        "uri": &uri,
                        "languageId": "opy",
                        "version": 3,
                        "text": text,
                    },
                },
            },
        }));
        assert_eq!(
            compiled["result"]["diagnostics"],
            json!([{ "uri": uri, "version": 3, "diagnostics": diagnostics }]),
            "{name}: compile diagnostics must match the core outcome"
        );
        assert_eq!(
            compiled["result"]["artifact"], artifact,
            "{name}: artifact parity with the core compile"
        );
    }
    session.shutdown();
}

#[test]
fn check_and_compile_share_the_request_locale_for_member_spellings() {
    const LOCALE_HOOK: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../opy-rs/tests/fixtures/macros/locale-hook.opy"
    );
    let mut session = Session::spawn();
    session.initialize();
    let entry = || {
        json!({
            "uri": file_uri(LOCALE_HOOK),
            "languageId": "opy",
            "version": 7,
        })
    };

    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": { "entry": entry(), "locale": "en-US" },
    }));
    let check_diagnostic = &checked["result"]["documents"][0]["diagnostics"][0];
    assert_eq!(check_diagnostic["severity"], "error");
    assert_eq!(check_diagnostic["code"], "unknown-enum-member");

    let compiled = session.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "lpp/compile",
        "params": { "entry": entry(), "locale": "en-US" },
    }));
    assert_eq!(compiled["result"]["artifact"], Value::Null);
    let compile_diagnostic = &compiled["result"]["diagnostics"][0]["diagnostics"][0];
    assert_eq!(compile_diagnostic["code"], "unknown-enum-member");
    assert_eq!(compile_diagnostic["range"], check_diagnostic["range"]);

    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "lpp/check",
        "params": { "entry": entry(), "locale": "zh-CN" },
    }));
    assert!(
        checked["result"]["documents"]
            .as_array()
            .expect("documents")
            .iter()
            .all(|document| document["diagnostics"]
                .as_array()
                .expect("diagnostics")
                .is_empty()),
        "zh-CN check must accept the spelling: {}",
        checked["result"]["documents"]
    );

    let compiled = session.request(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "lpp/compile",
        "params": { "entry": entry(), "locale": "zh-CN" },
    }));
    assert!(compiled["result"]["artifact"].is_object());
    session.shutdown();
}
