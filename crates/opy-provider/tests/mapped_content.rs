use std::path::Path;

use serde_json::{Value, json};

mod common;
use common::{Session, file_uri};

const MAPPED_FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mapped-text");

#[test]
fn mapped_content_keeps_sorted_key_order_at_every_depth() {
    let main = format!("{MAPPED_FIXTURES}/helper.opy");
    assert!(Path::new(&main).exists());
    let mut session = Session::spawn();
    session.initialize_version("1.4");
    let response = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/compile",
        "params": {
            "entry": { "uri": file_uri(&main), "languageId": "opy", "version": 1 },
            "acceptedArtifactFormats": ["workshop-rs/mapped-text-v1"],
        },
    }));
    let content = response["result"]["artifact"]["content"]
        .as_str()
        .expect("mapped content is a string");
    assert!(content.starts_with(r#"{"files":"#), "{content}");
    let value: Value = serde_json::from_str(content).expect("mapped content is JSON");
    assert_eq!(content, value.to_string());
    session.shutdown();
}
