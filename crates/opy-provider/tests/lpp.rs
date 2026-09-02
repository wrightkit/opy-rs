//! Process-level contract tests for the first-party OPY provider.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

const MULTI_FILE_MAIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-rs/tests/fixtures/multi-file/main.opy"
);
const BASIC_RULE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../compatibility/fixtures/synthetic/basic-rule/source.opy"
);
const UNSUPPORTED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../compatibility/fixtures/synthetic/issue-47-unsupported/source.opy"
);
const DIAGNOSTICS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../compatibility/fixtures/synthetic/diagnostics/source.opy"
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
        self.request(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "lpp/initialize",
            "params": { "protocolVersion": "1.0" },
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
    assert_eq!(initialized["result"]["capabilities"]["check"], true);
    assert_eq!(initialized["result"]["capabilities"]["compile"], true);
    assert_eq!(initialized["result"]["capabilities"]["symbols"], false);

    let checked = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/check",
        "params": { "entryUri": file_uri(MULTI_FILE_MAIN) },
    }));
    let documents = checked["result"]["documents"]
        .as_array()
        .expect("documents");
    assert_eq!(documents.len(), 2, "main plus reachable include");
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
        "params": { "entryUri": file_uri(BASIC_RULE) },
    }));
    assert_eq!(
        compiled["result"]["artifact"]["format"],
        "workshop-rs/text-v1"
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
        "params": { "entryUri": file_uri(UNSUPPORTED) },
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
fn single_document_v1_requests_keep_the_client_version() {
    let mut session = Session::spawn();
    session.initialize();
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
        "params": { "entryUri": file_uri(DIAGNOSTICS) },
    }));
    let document = &checked["result"]["documents"][0];
    assert!(
        document["uri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("/diagnostics/source.opy"))
    );
    assert_eq!(document["version"], 0);
    let diagnostic = &document["diagnostics"][0];
    assert_eq!(diagnostic["severity"], "error");
    assert_eq!(diagnostic["source"], "opy");
    assert_eq!(diagnostic["range"]["start"]["line"], 0);
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
