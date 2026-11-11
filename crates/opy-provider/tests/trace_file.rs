use std::path::{Path, PathBuf};

use serde_json::{Value, json};

mod common;
use common::{Session, file_uri};

const MAIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/mapped-text/main.opy"
);

fn compile_request(id: i64, format: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "lpp/compile",
        "params": {
            "entry": { "uri": file_uri(MAIN), "languageId": "opy", "version": 1 },
            "acceptedArtifactFormats": [format],
        },
    })
}

/// The compile responses for text-v1 and mapped-text-v1 in one session.
fn compile_both(trace: Option<&Path>) -> Vec<Value> {
    let mut session = Session::spawn_traced(trace);
    session.initialize_version("1.7");
    let responses = vec![
        session.request(compile_request(2, "workshop-rs/text-v1")),
        session.request(compile_request(3, "workshop-rs/mapped-text-v1")),
    ];
    session.shutdown();
    responses
}

fn scratch_file(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("opy-lpp-trace-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let path = dir.join("trace.jsonl");
    let _ = std::fs::remove_file(&path);
    path
}

fn names(line: &Value) -> Vec<&str> {
    line["spans"]
        .as_array()
        .expect("spans")
        .iter()
        .map(|span| span["name"].as_str().expect("name"))
        .collect()
}

/// Every span has the next id, a known earlier parent, and an interval
/// inside its parent's interval.
fn assert_nested(line: &Value) {
    let spans = line["spans"].as_array().expect("spans");
    for (index, span) in spans.iter().enumerate() {
        assert_eq!(span["id"], json!(index));
        assert!(
            span["name"]
                .as_str()
                .expect("name")
                .starts_with("provider.")
        );
        assert!(span["facts"].is_object());
        let start = span["startNs"].as_u64().expect("startNs");
        let end = start + span["durationNs"].as_u64().expect("durationNs");
        if let Some(parent) = span["parent"].as_u64() {
            let parent = &spans[usize::try_from(parent).expect("index")];
            assert!(parent["id"].as_u64() < span["id"].as_u64());
            let parent_start = parent["startNs"].as_u64().expect("startNs");
            let parent_end = parent_start + parent["durationNs"].as_u64().expect("durationNs");
            assert!(
                parent_start <= start && end <= parent_end,
                "{span} in {parent}"
            );
        } else {
            assert!(span["parent"].is_null());
        }
    }
}

#[test]
fn trace_file_records_one_nested_line_per_request() {
    let trace = scratch_file("lines");
    compile_both(Some(&trace));
    let lines: Vec<Value> = std::fs::read_to_string(&trace)
        .expect("trace file written")
        .lines()
        .map(|line| serde_json::from_str(line).expect("line is JSON"))
        .collect();
    let methods: Vec<&str> = lines
        .iter()
        .map(|line| line["method"].as_str().expect("method"))
        .collect();
    assert_eq!(
        methods,
        [
            "lpp/initialize",
            "lpp/compile",
            "lpp/compile",
            "lpp/shutdown"
        ]
    );
    for line in &lines {
        assert_eq!(line["schema"], "wright-provider-trace/v1");
        assert_nested(line);
    }
    for (line, source_map) in [(&lines[1], false), (&lines[2], true)] {
        let names = names(line);
        let top: Vec<&str> = line["spans"]
            .as_array()
            .expect("spans")
            .iter()
            .filter(|span| span["parent"].is_null())
            .map(|span| span["name"].as_str().expect("name"))
            .collect();
        assert_eq!(
            top,
            [
                "provider.request.decode",
                "provider.compile",
                "provider.response.encode"
            ]
        );
        for phase in [
            "provider.project.load",
            "provider.source.read",
            "provider.frontend",
            "provider.preprocess",
            "provider.parse",
            "provider.hir.lower",
            "provider.backend",
            "provider.workshop.lower",
            "provider.canonical",
            "provider.emit",
            "provider.artifact.encode",
        ] {
            assert!(names.contains(&phase), "{phase} missing from {names:?}");
        }
        assert_eq!(names.contains(&"provider.source_map.build"), source_map);
        let read = line["spans"]
            .as_array()
            .expect("spans")
            .iter()
            .find(|span| span["name"] == "provider.source.read")
            .expect("source read span");
        assert!(read["facts"]["files"].as_u64() >= Some(1));
        assert!(read["facts"]["bytes"].as_u64() > Some(0));
    }
}

#[test]
fn trace_file_leaves_responses_unchanged_and_never_fails_a_request() {
    let untraced = compile_both(None);
    assert!(
        untraced
            .iter()
            .all(|response| response["result"]["artifact"].is_object())
    );
    let trace = scratch_file("unchanged");
    assert_eq!(compile_both(Some(&trace)), untraced);
    let unwritable = trace.join("missing-directory").join("trace.jsonl");
    assert_eq!(compile_both(Some(&unwritable)), untraced);
    assert!(!unwritable.exists());
}
