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
    let expected = uris(&base, &["", "gen.js", "hook.js", "lib.opy", "main.opy"]);
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

fn scratch_project(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("opy-lpp-inputs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("fixture parent")).unwrap();
        std::fs::write(path, content).unwrap();
    }
    dir.canonicalize().unwrap()
}

fn compile_inputs(session: &mut Session, id: i64, entry: Value) -> Vec<String> {
    let compiled = session.request(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "lpp/compile",
        "params": { "entry": entry },
    }));
    compiled["result"]["compileInputs"]
        .as_array()
        .unwrap_or_else(|| panic!("compileInputs array: {compiled}"))
        .iter()
        .map(|uri| uri.as_str().unwrap().to_string())
        .collect()
}

fn file_entry(path: &Path) -> Value {
    json!({
        "uri": format!("file://{}", path.display()),
        "languageId": "opy",
        "version": 7,
    })
}

fn uris(dir: &Path, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|name| {
            let path = if name.is_empty() {
                dir.to_path_buf()
            } else {
                dir.join(name)
            };
            format!("file://{}", path.display())
        })
        .collect()
}

#[test]
fn compile_inputs_declares_a_directory_whose_entries_were_listed() {
    let dir = scratch_project(
        "include-dir",
        &[
            (
                "main.opy",
                "#!include \"lib\"\nrule \"main\":\n    @Event global\n    pass\n",
            ),
            ("lib/a.opy", "rule \"a\":\n    @Event global\n    pass\n"),
        ],
    );
    let mut session = Session::spawn();
    session.initialize_version("1.7");

    let inputs = compile_inputs(&mut session, 2, file_entry(&dir.join("main.opy")));
    assert_eq!(inputs, uris(&dir, &["", "lib", "lib/a.opy", "main.opy"]));
    session.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_inputs_declares_a_translation_probe_that_missed() {
    let dir = scratch_project(
        "po-miss",
        &[(
            "main.opy",
            "#!translations en fr\nrule \"main\":\n    @Event global\n    pass\n",
        )],
    );
    let mut session = Session::spawn();
    session.initialize_version("1.7");

    let inputs = compile_inputs(&mut session, 2, file_entry(&dir.join("main.opy")));
    assert_eq!(inputs, uris(&dir, &["", "main.fr.po", "main.opy"]));
    session.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_inputs_declares_a_settings_probe_that_missed() {
    let dir = scratch_project(
        "settings-miss",
        &[(
            "main.opy",
            "settings \"settings.json\"\nrule \"main\":\n    @Event global\n    pass\n",
        )],
    );
    let mut session = Session::spawn();
    session.initialize_version("1.7");

    let inputs = compile_inputs(&mut session, 2, file_entry(&dir.join("main.opy")));
    assert_eq!(inputs, uris(&dir, &["", "main.opy", "settings.json"]));
    session.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_inputs_declares_default_entry_probes() {
    let dir = scratch_project(
        "dir-entry",
        &[(
            "src/main.opy",
            "rule \"main\":\n    @Event global\n    pass\n",
        )],
    );
    let mut session = Session::spawn();
    session.initialize_version("1.7");

    let mut entry = file_entry(&dir);
    entry["kind"] = json!("directory");
    let inputs = compile_inputs(&mut session, 3, entry);
    assert_eq!(inputs, uris(&dir, &["", "main.opy", "src", "src/main.opy"]));
    session.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn compile_inputs_declares_links_followed_and_their_resolutions() {
    let dir = scratch_project(
        "link",
        &[
            (
                "real/projD/main.opy",
                "#!include \"inc.opy\"\nrule \"main\":\n    @Event global\n    pass\n",
            ),
            (
                "real/projD/inc.opy",
                "rule \"inc\":\n    @Event global\n    pass\n",
            ),
        ],
    );
    std::os::unix::fs::symlink("real/projD", dir.join("link")).unwrap();
    let mut session = Session::spawn();
    session.initialize_version("1.7");

    let inputs = compile_inputs(&mut session, 2, file_entry(&dir.join("link/main.opy")));
    assert_eq!(
        inputs,
        uris(
            &dir,
            &[
                "link",
                "link/main.opy",
                "real/projD",
                "real/projD/inc.opy",
                "real/projD/main.opy",
            ],
        )
    );
    session.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn compile_inputs_declares_a_dangling_opy_symlink_and_its_target() {
    let dir = scratch_project(
        "dangling-link",
        &[
            (
                "main.opy",
                "#!include \"lib\"\nrule \"main\":\n    @Event global\n    pass\n",
            ),
            ("lib/a.opy", "rule \"a\":\n    @Event global\n    pass\n"),
        ],
    );
    std::os::unix::fs::symlink("../elsewhere/x.opy", dir.join("lib/dangling.opy")).unwrap();
    let mut session = Session::spawn();
    session.initialize_version("1.7");

    let inputs = compile_inputs(&mut session, 2, file_entry(&dir.join("main.opy")));
    assert_eq!(
        inputs,
        uris(
            &dir,
            &[
                "",
                "elsewhere/x.opy",
                "lib",
                "lib/a.opy",
                "lib/dangling.opy",
                "main.opy",
            ],
        )
    );
    session.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
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
