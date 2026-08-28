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
    "/../opy-rs/tests/fixtures/multi-file/main.opy"
);
const BASIC_RULE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../compatibility/fixtures/synthetic/basic-rule/source.opy"
);
const ISSUE_46_UNSUPPORTED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../compatibility/fixtures/synthetic/issue-46-unsupported/source.opy"
);

fn run(args: &[&str]) -> std::process::Output {
    run_with_env(args, &[])
}

fn run_with_env(args: &[&str], vars: &[(&str, &str)]) -> std::process::Output {
    let mut command = bin();
    command.env_clear().args(args).envs(vars.iter().copied());
    command.output().expect("the binary runs")
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
fn compile_text_prints_only_final_workshop_to_stdout() {
    let output = run(&["compile", BASIC_RULE]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "compile must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("rule (\"setup\")"));
    assert!(stdout.contains("Disable Inspector Recording;"));
    assert!(output.stderr.is_empty());
}

#[test]
fn compile_json_reports_success_identity_and_normalized_output() {
    let output = run(&["compile", "--format", "json", BASIC_RULE]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("compile JSON");
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["compile"]["status"], "success");
    assert_eq!(json["compile"]["exitCode"], 0);
    assert_eq!(json["compile"]["diagnostics"].as_array().unwrap().len(), 0);
    assert_eq!(
        json["compile"]["workshop"],
        json["compile"]["workshopExact"]
            .as_str()
            .unwrap()
            .trim_end_matches('\n')
            .to_owned()
            + "\n"
    );
    assert_eq!(json["compiler"]["name"], "opy-compiler");
    assert_eq!(json["catalog"]["implementation-version"], "0.1.11");
}

#[test]
fn compile_json_reports_frontend_diagnostics_and_exit_one() {
    let dir = temp_dir("compile-bad");
    let main = dir.join("bad.opy");
    std::fs::write(&main, "rule \"r\":\n    @Event global\n    frobnicate()\n").unwrap();
    let output = run(&["compile", "--format", "json", main.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("compile JSON");
    assert_eq!(json["compile"]["status"], "failure");
    assert_eq!(json["compile"]["failureClass"], "frontend");
    assert_eq!(json["compile"]["diagnostics"][0]["code"], "unknown-action");
    assert!(
        json["compile"]["diagnostics"][0]["span"]["path"]
            .as_str()
            .unwrap()
            .ends_with("/bad.opy")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_missing_file_is_an_io_usage_error() {
    let output = run(&["compile", "--format", "json", "/nonexistent/opy/nope.opy"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
}

#[test]
fn compile_json_reports_unsupported_lowering_as_integration_failure() {
    let output = run(&["compile", "--format", "json", ISSUE_46_UNSUPPORTED]);
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("compile JSON");
    assert_eq!(json["compile"]["failureClass"], "integration");
    assert_eq!(
        json["compile"]["diagnostics"][0]["code"],
        "unsupported-integration-surface"
    );
}

#[test]
fn compile_rejects_unsupported_locale_with_structured_diagnostic() {
    let output = run(&[
        "compile",
        "--format",
        "json",
        "--language",
        "xx-XX",
        BASIC_RULE,
    ]);
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("compile JSON");
    assert_eq!(json["compile"]["failureClass"], "integration");
    assert_eq!(
        json["compile"]["diagnostics"][0]["code"],
        "locale-unsupported"
    );
    assert_eq!(
        json["compile"]["diagnostics"][0]["span"],
        serde_json::Value::Null
    );
}

#[test]
fn check_without_arguments_is_a_usage_error() {
    let output = run(&["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("<MAIN.OPY>"));
    assert!(output.stdout.is_empty());
}

#[test]
fn check_json_input_errors_stay_human_stderr_only() {
    for args in [
        vec!["check", "--format", "json"],
        vec!["check", "--format", "json", "/nonexistent/opy/nope.opy"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
        assert!(output.stdout.is_empty(), "args: {args:?}");
        assert!(!output.stderr.is_empty(), "args: {args:?}");
    }

    let unreadable = run(&["check", "--format", "json", "/nonexistent/opy/nope.opy"]);
    assert!(String::from_utf8_lossy(&unreadable.stderr).contains("cannot read"));
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
    assert!(stdout.contains("opy-rs"), "language identity: {stdout}");
}

#[test]
fn unknown_command_is_a_usage_error() {
    let output = run(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command"));
}

#[test]
fn help_and_parse_are_driven_by_the_structured_command_model() {
    let help = run(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&help.stdout);
    for expected in [
        "opy-cli",
        "check",
        "compile",
        "inspect",
        "support",
        "completion",
        "--renderer",
        "--color",
    ] {
        assert!(
            stdout.contains(expected),
            "help missing {expected}: {stdout}"
        );
    }
    assert!(help.stderr.is_empty(), "help belongs on stdout");

    let invalid = run(&["completion", "invalid-shell"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("possible values"));
}

#[test]
fn all_static_completion_shells_are_generated_from_the_same_model() {
    let cases = [
        ("bash", "_opy__cli"),
        ("zsh", "#compdef opy-cli"),
        ("fish", "complete"),
        ("powershell", "Register-ArgumentCompleter"),
    ];
    for (shell, marker) in cases {
        let output = run(&["completion", shell]);
        assert_eq!(output.status.code(), Some(0), "shell: {shell}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(marker), "shell {shell}: {stdout}");
        assert!(
            stdout.contains("completion"),
            "model command missing: {shell}"
        );
        assert!(output.stderr.is_empty(), "completion stderr: {shell}");
    }
}

#[test]
fn github_actions_renderer_uses_annotations_and_step_summary_without_stdout() {
    let dir = temp_dir("github");
    let main = dir.join("bad.opy");
    let summary = dir.join("summary.md");
    std::fs::write(&main, "rule \"r\":\n    @Event global\n    frobnicate()\n").unwrap();
    let main = main.to_str().unwrap();
    let summary = summary.to_str().unwrap();
    let output = run_with_env(
        &["check", main],
        &[
            ("GITHUB_ACTIONS", "true"),
            ("CI", "true"),
            ("GITHUB_STEP_SUMMARY", summary),
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "GitHub presentation uses stderr");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("::error file="), "annotation: {stderr}");
    assert!(stderr.contains("::group::opy-cli check"), "group: {stderr}");
    assert!(stderr.contains("ERROR check"), "status: {stderr}");
    assert!(
        String::from_utf8_lossy(&std::fs::read(summary).unwrap()).contains("**ERROR**"),
        "step summary"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn explicit_renderer_and_color_override_injected_environment() {
    let terminal = run_with_env(
        &[
            "check",
            "--renderer",
            "terminal",
            "--color",
            "always",
            MULTI_MAIN,
        ],
        &[
            ("GITHUB_ACTIONS", "true"),
            ("CI", "true"),
            ("NO_COLOR", "1"),
        ],
    );
    assert_eq!(terminal.status.code(), Some(0));
    assert!(terminal.stdout.starts_with(b"\x1b[32m"));
    assert!(!String::from_utf8_lossy(&terminal.stdout).contains("::"));

    let plain = run_with_env(
        &[
            "check",
            "--renderer",
            "plain",
            "--color",
            "always",
            MULTI_MAIN,
        ],
        &[("GITHUB_ACTIONS", "true"), ("CI", "true")],
    );
    assert_eq!(plain.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&plain.stdout).contains("\x1b["));
    assert!(!String::from_utf8_lossy(&plain.stdout).contains("::"));

    let no_color = run_with_env(
        &[
            "check",
            "--renderer",
            "terminal",
            "--color",
            "auto",
            MULTI_MAIN,
        ],
        &[("NO_COLOR", "1")],
    );
    assert_eq!(no_color.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&no_color.stdout).contains("\x1b["));
}

#[test]
fn machine_json_stays_pure_under_github_and_color_environment() {
    let dir = temp_dir("json-purity");
    let main = dir.join("bad.opy");
    std::fs::write(&main, "rule \"r\":\n    @Event global\n    frobnicate()\n").unwrap();
    let output = run_with_env(
        &[
            "check",
            "--format",
            "json",
            "--renderer",
            "github-actions",
            "--color",
            "always",
            main.to_str().unwrap(),
        ],
        &[("GITHUB_ACTIONS", "true"), ("NO_COLOR", "")],
    );
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("pure JSON");
    assert_eq!(json["ok"], false);
    assert!(!json["diagnostics"].as_array().unwrap().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\x1b["));
    assert!(!stdout.contains("::"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn github_workflow_path_properties_are_escaped() {
    let dir = temp_dir("workflow-%,:");
    let main = dir.join("bad.opy");
    std::fs::write(&main, "rule \"r\":\n    @Event global\n    frobnicate()\n").unwrap();
    let output = run_with_env(
        &[
            "check",
            "--renderer",
            "github-actions",
            main.to_str().unwrap(),
        ],
        &[("GITHUB_ACTIONS", "true")],
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let encoded = main
        .to_str()
        .unwrap()
        .replace('%', "%25")
        .replace(':', "%3A")
        .replace(',', "%2C");
    assert!(
        stderr.contains(&format!("file={encoded}")),
        "escaped path: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "workflow output must stay off stdout"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
