use std::path::{Path, PathBuf};

use serde_json::{Value, json};

mod common;
use common::Session;

fn compile_inputs_project(tag: &str) -> (PathBuf, Vec<String>) {
    let dir = std::env::temp_dir().join(format!("opy-lpp-inputs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.opy"),
        concat!(
            "#!postCompileHook \"hook.js\"\n",
            "#!define gen() __script__(\"gen.js\")\n",
            "#!include \"lib.opy\"\n",
            "\n",
            "rule \"main\":\n    @Event global\n    pass\n",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("lib.opy"),
        "rule \"lib\":\n    @Event global\n    pass\n",
    )
    .unwrap();
    std::fs::write(dir.join("hook.js"), "content.toString();").unwrap();
    std::fs::write(dir.join("gen.js"), "\"0\";").unwrap();
    let base = dir.canonicalize().unwrap();
    let expected = ["gen.js", "hook.js", "lib.opy", "main.opy"]
        .iter()
        .map(|name| format!("file://{}", base.join(name).display()))
        .collect();
    (base.join("main.opy"), expected)
}

fn entry_compile(session: &mut Session, id: i64, entry: &Path) -> Value {
    session.request(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "lpp/compile",
        "params": {
            "entry": {
                "uri": format!("file://{}", entry.display()),
                "languageId": "opy",
                "version": 7,
            }
        },
    }))
}

#[test]
fn compile_inputs_declares_every_file_the_entry_compile_read() {
    let (entry, expected) = compile_inputs_project("declared");
    let mut session = Session::spawn();
    let initialized = session.initialize_version("1.7");
    assert_eq!(initialized["result"]["capabilities"]["compileInputs"], true);

    let compiled = entry_compile(&mut session, 2, &entry);
    let inputs = compiled["result"]["compileInputs"]
        .as_array()
        .expect("compileInputs array");
    let inputs: Vec<&str> = inputs.iter().map(|uri| uri.as_str().unwrap()).collect();
    assert!(
        inputs.iter().all(|uri| uri.starts_with("file:///")),
        "every input is an absolute file URI: {inputs:?}"
    );
    assert_eq!(
        inputs,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    session.shutdown();
    let _ = std::fs::remove_dir_all(entry.parent().expect("project dir"));
}

#[test]
fn compile_inputs_omitted_before_lpp_1_7() {
    let (entry, _) = compile_inputs_project("gated");
    let mut session = Session::spawn();
    let initialized = session.initialize_version("1.6");
    assert!(initialized["result"]["capabilities"]["compileInputs"].is_null());

    let compiled = entry_compile(&mut session, 2, &entry);
    assert!(compiled["result"]["compileInputs"].is_null());
    session.shutdown();
    let _ = std::fs::remove_dir_all(entry.parent().expect("project dir"));
}

#[test]
fn compile_inputs_omitted_for_document_supplied_compile() {
    let mut session = Session::spawn();
    let initialized = session.initialize_version("1.7");
    assert_eq!(initialized["result"]["capabilities"]["compileInputs"], true);
    let uri = "file:///project/main.opy";
    let compiled = session.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "lpp/compile",
        "params": {
            "documents": {
                uri: {
                    "uri": uri,
                    "languageId": "opy",
                    "version": 1,
                    "text": "rule \"main\":\n    @Event global\n    pass\n",
                }
            }
        },
    }));
    assert!(compiled["result"]["compileInputs"].is_null());
    session.shutdown();
}
