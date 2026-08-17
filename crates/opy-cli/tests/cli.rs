//! Integration tests for the `opy-cli` binary (issue #7): exit-code contract
//! (0 clean / 1 diagnostics / 2 usage and I/O errors), stderr diagnostics,
//! and the JSON surfaces. Uses `std::process::Command` on the built binary
//! via `CARGO_BIN_EXE_opy-cli` — no extra test dependencies.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opy-cli"))
}

/// The WrightKit-authored multi-file fixture shared with the frontend tests.
const MULTI_MAIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../opy-frontend/tests/fixtures/multi-file/main.opy"
);

fn run(args: &[&str]) -> std::process::Output {
    bin().args(args).output().expect("the binary runs")
}

/// A fresh temp directory for per-test source files (recoverable and
/// task-scoped; removed by the caller like the rest of the workspace tests).
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("opy-cli-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn check_clean_project_exits_zero() {
    let output = run(&["check", MULTI_MAIN]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "clean project must exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("check passed"), "summary line: {stdout}");
    assert!(stdout.contains("2 file(s)"), "registry count: {stdout}");
}

#[test]
fn check_reports_diagnostics_on_stderr_and_exits_one() {
    let dir = temp_dir("check-bad");
    let main = dir.join("bad.opy");
    std::fs::write(&main, "rule \"r\":\n    @Event global\n    frobnicate()\n").unwrap();
    let output = run(&["check", main.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1), "diagnostics must exit 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error[unknown-action]"),
        "stable code: {stderr}"
    );
    assert!(stderr.contains("bad.opy:3"), "source location: {stderr}");
    assert!(stderr.contains("-->"), "source arrow: {stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_missing_file_is_an_io_usage_error() {
    let output = run(&["check", "/nonexistent/opy/nope.opy"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
}

#[test]
fn check_without_arguments_is_a_usage_error() {
    let output = run(&["check"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn inspect_prints_the_resolved_model_as_json() {
    let output = run(&["inspect", MULTI_MAIN]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "clean project must inspect, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("stdout must be JSON: {error}"));
    assert_eq!(json["hir"]["protocol"]["name"], "wright/opy-hir");
    assert_eq!(json["hir"]["rules"].as_array().expect("rules").len(), 2);
    let symbols = json["symbols"].as_array().expect("symbols");
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "total" && symbol["kind"] == "global")
    );
    assert!(json["enums"].as_array().expect("enums").len() == 1);
}

#[test]
fn inspect_reports_diagnostics_and_exits_one() {
    let dir = temp_dir("inspect-bad");
    let main = dir.join("bad.opy");
    std::fs::write(
        &main,
        "globalvar x\nrule \"r\":\n    @Event global\n    x = nope\n",
    )
    .unwrap();
    let output = run(&["inspect", main.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("error[unknown-identifier]"));
    assert!(output.stdout.is_empty(), "no model JSON on failure");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn support_prints_the_embedded_matrix_as_json() {
    let output = run(&["support", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("matrix JSON");
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["reference"]["name"], "overpy");
    assert!(json["features"].as_array().expect("features").len() >= 30);
}

#[test]
fn support_filters_by_feature_id_and_category() {
    let by_id = run(&["support", "compilation/workshop-lowering"]);
    assert_eq!(by_id.status.code(), Some(0));
    let feature: serde_json::Value = serde_json::from_slice(&by_id.stdout).expect("feature JSON");
    assert_eq!(feature["state"], "lowering-dependent");

    let by_category = run(&["support", "syntax"]);
    assert_eq!(by_category.status.code(), Some(0));
    let slice: serde_json::Value =
        serde_json::from_slice(&by_category.stdout).expect("category JSON");
    assert_eq!(slice["category"], "syntax");
    assert_eq!(slice["count"], 14);

    let unknown = run(&["support", "nope/nothing"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown feature id or category"));
}

#[test]
fn version_prints_crate_and_protocol_identity() {
    let output = run(&["version"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("opy-cli"), "crate identity: {stdout}");
    assert!(
        stdout.contains("wright/opy-hir"),
        "protocol identity: {stdout}"
    );
    assert!(
        stdout.contains("wright/opy-native"),
        "frontend identity: {stdout}"
    );
}

#[test]
fn unknown_command_is_a_usage_error() {
    let output = run(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command"));
}
