//! Manual compiler-resource evidence for the optimization workstream.
//!
//! Run the ignored `resource_baseline` test through the documented command in
//! `docs/benchmarks/compiler-resources.md`. Each workload is executed in a
//! child test process so its peak RSS and test-only mechanism counters are not
//! contaminated by another workload.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

use crate::macro_js::{Limits, MacroArg, MacroRuntime};
use crate::resource_metrics::{self, ResourceMetrics};
use crate::{Compiler, compile_with_overlay_outcome};

const CASE_ENV: &str = "OPY_RESOURCE_BASELINE_WORKLOAD";
const REVISION_ENV: &str = "OPY_RESOURCE_BASELINE_REVISION";
const CHILD_PREFIX: &str = "RESOURCE_BASELINE ";
const REPETITIONS: usize = 5;
const NESTED_DEPTH: usize = 24;
const WIDE_CALL_COUNT: usize = 32;
const RULE_COUNT: usize = 128;
const MACRO_INVOCATIONS: usize = 64;
const CONTRACT_CONSTRUCTIONS: usize = 64;
const EXPECTED_CONTRACT_CHECKS: usize = 1;
const SETTINGS_KEY_COUNT: usize = 1_000;
const REAL_FIXTURE: &str = "real-world/overpy-parabola";

#[derive(Debug, Serialize, Deserialize)]
struct Observation {
    workload: String,
    description: String,
    input: InputIdentity,
    sample: Sample,
}

#[derive(Debug, Serialize, Deserialize)]
struct Sample {
    elapsed_ns: u128,
    peak_rss_bytes: Option<u64>,
    metrics: ResourceMetrics,
}

#[derive(Debug, Serialize)]
struct WorkloadReport {
    workload: String,
    description: String,
    input: InputIdentity,
    samples: Vec<Sample>,
}

#[derive(Debug, Serialize, Deserialize)]
struct InputIdentity {
    kind: String,
    bytes: usize,
    sha256: String,
    source: String,
    parameters: JsonValue,
    provenance: JsonValue,
}

#[derive(Debug)]
struct Workload {
    id: &'static str,
    description: &'static str,
    input: InputIdentity,
    run: fn() -> Result<(), String>,
}

#[test]
#[ignore = "manual resource baseline; see docs/benchmarks/compiler-resources.md"]
fn resource_baseline() {
    let revision = env::var(REVISION_ENV).unwrap_or_else(|_| {
        panic!(
            "{REVISION_ENV} must identify the audited origin/main revision; see docs/benchmarks/compiler-resources.md"
        )
    });
    assert!(
        !revision.trim().is_empty(),
        "{REVISION_ENV} must not be empty"
    );

    let executable = env::current_exe().expect("resource baseline test executable exists");
    let mut workloads = Vec::new();
    for id in workload_ids() {
        let mut samples = Vec::with_capacity(REPETITIONS);
        let mut identity = None;
        let mut description = None;
        for _ in 0..REPETITIONS {
            let output = Command::new(&executable)
                .args([
                    "--exact",
                    "compiler::integration_tests::resource_baseline::run_resource_workload",
                    "--nocapture",
                ])
                .env(CASE_ENV, id)
                .output()
                .unwrap_or_else(|error| panic!("start workload {id}: {error}"));
            assert!(
                output.status.success(),
                "workload {id} failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            let line = stdout
                .lines()
                .find_map(|line| line.strip_prefix(CHILD_PREFIX))
                .unwrap_or_else(|| {
                    panic!(
                        "workload {id} did not emit a resource record:\n{}",
                        String::from_utf8_lossy(&output.stdout)
                    )
                });
            let observation = serde_json::from_str::<Observation>(line)
                .unwrap_or_else(|error| panic!("parse workload {id} record: {error}"));
            assert_mechanism_measurements(id, &observation.sample.metrics);
            identity.get_or_insert(observation.input);
            description.get_or_insert(observation.description);
            samples.push(observation.sample);
        }
        workloads.push(WorkloadReport {
            workload: id.to_string(),
            description: description.expect("workload description was recorded"),
            input: identity.expect("workload identity was recorded"),
            samples,
        });
    }

    let report = serde_json::json!({
        "schemaVersion": 1,
        "baselineRevision": revision,
        "candidateRevision": env::var("OPY_RESOURCE_CANDIDATE_REVISION").ok(),
        "compilerPackage": env!("CARGO_PKG_VERSION"),
        "workloads": workloads,
        "environment": {
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
            "rustc": rustc_version(),
            "repetitions": REPETITIONS,
            "peakRss": "getrusage(RUSAGE_SELF) per workload child process"
        }
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

fn assert_mechanism_measurements(id: &str, metrics: &ResourceMetrics) {
    match id {
        "nested-expression-lowering" | "wide-call-value-construction" => {
            assert!(
                metrics.lowering_values_peak > 0,
                "{id} did not retain any lowering values"
            );
            assert!(
                metrics.lowering_value_clone_nodes > 0,
                "{id} did not observe a lowering value-tree copy"
            );
        }
        "many-rule-compilation" => {
            assert!(
                metrics.lowering_values_peak >= RULE_COUNT,
                "{id} retained-value counter did not cover all rules: {}",
                metrics.lowering_values_peak
            );
            assert!(
                metrics.lowering_action_clone_events >= RULE_COUNT,
                "{id} action-copy counter did not cover all rules: {}",
                metrics.lowering_action_clone_events
            );
        }
        "compiler-contract-initialization" => assert_eq!(
            metrics.compiler_contract_checks, EXPECTED_CONTRACT_CHECKS,
            "{id} contract-check counter must match the workload"
        ),
        "macro-runtime" | "macro-runtime-heavy" => {
            assert_eq!(
                metrics.macro_engine_creations, MACRO_INVOCATIONS,
                "{id} engine-creation counter must match the workload"
            );
            assert_macro_phase_measurements(id, metrics);
        }
        "large-settings-source" => assert_eq!(
            metrics.settings_chars_materialized, 0,
            "{id} still materialized a whole-source character buffer"
        ),
        "real-world-parabola" => {
            assert!(
                metrics.lowering_values_peak > 0,
                "{id} did not retain any lowering values"
            );
            assert!(
                metrics.lowering_value_clone_nodes > 0,
                "{id} did not observe a lowering value-tree copy"
            );
        }
        _ => panic!("unknown resource workload {id}"),
    }
}

fn assert_macro_phase_measurements(id: &str, metrics: &ResourceMetrics) {
    for (phase, elapsed_ns) in [
        ("runtime creation", metrics.macro_runtime_creation_ns),
        ("host registration", metrics.macro_host_registration_ns),
        ("builtin evaluation", metrics.macro_builtin_evaluation_ns),
        ("script evaluation", metrics.macro_script_evaluation_ns),
        ("runtime teardown", metrics.macro_runtime_teardown_ns),
    ] {
        assert!(elapsed_ns > 0, "{id} recorded no {phase} time");
    }
}

#[test]
fn run_resource_workload() {
    let Some(id) = env::var_os(CASE_ENV) else {
        return;
    };
    let id = id.to_string_lossy();
    let workload = workload(&id).unwrap_or_else(|| panic!("unknown resource workload {id}"));
    resource_metrics::reset();
    let started = Instant::now();
    (workload.run)().unwrap_or_else(|error| panic!("workload {id}: {error}"));
    let observation = Observation {
        workload: workload.id.to_string(),
        description: workload.description.to_string(),
        input: workload.input,
        sample: Sample {
            elapsed_ns: started.elapsed().as_nanos(),
            peak_rss_bytes: peak_rss_bytes(),
            metrics: resource_metrics::snapshot(),
        },
    };
    println!(
        "{CHILD_PREFIX}{}",
        serde_json::to_string(&observation).expect("resource observation serializes")
    );
}

fn workload_ids() -> [&'static str; 8] {
    [
        "nested-expression-lowering",
        "wide-call-value-construction",
        "many-rule-compilation",
        "compiler-contract-initialization",
        "macro-runtime",
        "macro-runtime-heavy",
        "large-settings-source",
        "real-world-parabola",
    ]
}

fn workload(id: &str) -> Option<Workload> {
    Some(match id {
        "nested-expression-lowering" => Workload {
            id: "nested-expression-lowering",
            description: "nested arithmetic expression lowered into canonical Workshop values",
            input: synthetic_identity(
                "nested-expression-lowering",
                nested_expression_source(),
                serde_json::json!({"depth": NESTED_DEPTH}),
            ),
            run: run_nested_expression,
        },
        "wide-call-value-construction" => Workload {
            id: "wide-call-value-construction",
            description: "one wide value call with repeated nested vector arguments",
            input: synthetic_identity(
                "wide-call-value-construction",
                wide_call_source(),
                serde_json::json!({"vectorCount": WIDE_CALL_COUNT}),
            ),
            run: run_wide_call,
        },
        "many-rule-compilation" => Workload {
            id: "many-rule-compilation",
            description: "many independent rules compiled through the public compiler API",
            input: synthetic_identity(
                "many-rule-compilation",
                many_rule_source(),
                serde_json::json!({"ruleCount": RULE_COUNT}),
            ),
            run: run_many_rules,
        },
        "compiler-contract-initialization" => Workload {
            id: "compiler-contract-initialization",
            description: "repeated manifest/catalog contract initialization",
            input: synthetic_identity(
                "compiler-contract-initialization",
                "Compiler::new()".to_string(),
                serde_json::json!({"constructions": CONTRACT_CONSTRUCTIONS}),
            ),
            run: run_compiler_contract,
        },
        "macro-runtime" => Workload {
            id: "macro-runtime",
            description: "repeated JavaScript macro execution in one MacroRuntime workload",
            input: synthetic_identity(
                "macro-runtime",
                MACRO_SOURCE.to_string(),
                serde_json::json!({"invocations": MACRO_INVOCATIONS}),
            ),
            run: run_macro_runtime,
        },
        "macro-runtime-heavy" => Workload {
            id: "macro-runtime-heavy",
            description: "repeated helper- and collection-heavy JavaScript macro execution",
            input: synthetic_identity(
                "macro-runtime-heavy",
                MACRO_HEAVY_SOURCE.to_string(),
                serde_json::json!({"invocations": MACRO_INVOCATIONS}),
            ),
            run: run_macro_runtime_heavy,
        },
        "large-settings-source" => Workload {
            id: "large-settings-source",
            description: "large settings-containing source scanned through the frontend",
            input: synthetic_identity(
                "large-settings-source",
                large_settings_source(),
                serde_json::json!({"settingsKeyCount": SETTINGS_KEY_COUNT}),
            ),
            run: run_large_settings,
        },
        "real-world-parabola" => real_world_workload(),
        _ => return None,
    })
}

fn synthetic_identity(id: &str, source: String, parameters: JsonValue) -> InputIdentity {
    InputIdentity {
        kind: "synthetic".to_string(),
        bytes: source.len(),
        sha256: sha256(&source),
        source: format!("compiler/tests/resource_baseline.rs::{id}"),
        parameters,
        provenance: serde_json::json!({
            "kind": "original",
            "origin": "WrightKit opy-rs resource audit workload",
            "license": "AGPL-3.0-or-later",
            "relatedIssue": "wrightkit/opy-rs#296"
        }),
    }
}

fn real_world_workload() -> Workload {
    let root = fixture_root();
    let source_path = root.join("parabola.opy");
    let metadata_path = root.join("fixture.json");
    let source = std::fs::read_to_string(&source_path).expect("parabola source exists");
    let metadata: JsonValue = serde_json::from_str(
        &std::fs::read_to_string(&metadata_path).expect("parabola metadata exists"),
    )
    .expect("parabola metadata is valid JSON");
    Workload {
        id: "real-world-parabola",
        description: "provenance-linked OverPy parabola example compiled end to end",
        input: InputIdentity {
            kind: "provenance-linked-corpus".to_string(),
            bytes: source.len(),
            sha256: sha256(&source),
            source: format!("{REAL_FIXTURE}/parabola.opy"),
            parameters: serde_json::json!({
                "fixture": REAL_FIXTURE,
                "fixtureMetadata": format!(
                    "crates/opy-rs/tests/fixtures/corpus/{REAL_FIXTURE}/fixture.json"
                ),
                "entry": "parabola.opy"
            }),
            provenance: metadata["provenance"].clone(),
        },
        run: run_real_world,
    }
}

fn run_nested_expression() -> Result<(), String> {
    compile_source(&nested_expression_source())
}

fn run_wide_call() -> Result<(), String> {
    compile_source(&wide_call_source())
}

fn run_many_rules() -> Result<(), String> {
    compile_source(&many_rule_source())
}

fn run_compiler_contract() -> Result<(), String> {
    for _ in 0..CONTRACT_CONSTRUCTIONS {
        Compiler::new().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn run_macro_runtime() -> Result<(), String> {
    let runtime = MacroRuntime::new(Limits::default());
    for _ in 0..MACRO_INVOCATIONS {
        runtime
            .run_macro(
                MACRO_SOURCE,
                &[MacroArg::new("x", "40")],
                "resource-baseline.js",
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn run_macro_runtime_heavy() -> Result<(), String> {
    let runtime = MacroRuntime::new(Limits::default());
    for _ in 0..MACRO_INVOCATIONS {
        runtime
            .run_macro(MACRO_HEAVY_SOURCE, &[], "resource-heavy.js")
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn run_large_settings() -> Result<(), String> {
    let source = large_settings_source();
    let outcome = compile_with_overlay_outcome(
        &source,
        "large-settings.opy",
        Path::new("."),
        &Default::default(),
    );
    if outcome.hir.is_some() {
        Ok(())
    } else {
        Err(outcome
            .error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "large settings source failed without a diagnostic".to_string()))
    }
}

fn run_real_world() -> Result<(), String> {
    let root = fixture_root();
    let source =
        std::fs::read_to_string(root.join("parabola.opy")).map_err(|error| error.to_string())?;
    let compiler = Compiler::new().map_err(|error| error.to_string())?;
    compiler
        .compile_source_artifact(&source, "parabola.opy", &root)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn compile_source(source: &str) -> Result<(), String> {
    let compiler = Compiler::new().map_err(|error| error.to_string())?;
    compiler
        .compile_source_artifact(source, "resource-baseline.opy", Path::new("."))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn nested_expression_source() -> String {
    let mut expression = "result".to_string();
    for _ in 0..NESTED_DEPTH {
        expression = format!("result + ({expression})");
    }
    format!("globalvar result\nrule \"nested\":\n    @Event global\n    result = {expression}\n")
}

fn wide_call_source() -> String {
    let vectors = (0..WIDE_CALL_COUNT)
        .map(|index| format!("vect({index}, {index}, {index})"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("globalvar result\nrule \"wide\":\n    @Event global\n    result = [{vectors}]\n")
}

fn many_rule_source() -> String {
    let mut source = String::from("globalvar value\n");
    for index in 0..RULE_COUNT {
        source.push_str(&format!(
            "rule \"rule-{index}\":\n    @Event global\n    value = {index}\n"
        ));
    }
    source
}

const MACRO_SOURCE: &str = "(x + 2).toString();";

const MACRO_HEAVY_SOURCE: &str = r#"Array.from({length: 64}, (_, i) => vect(i, i + 1, i + 2))
    .map((value) => value.toString())
    .join(",");"#;

fn large_settings_source() -> String {
    let mut source = String::from("settings {\n    \"gamemodes\": {},\n");
    for index in 0..SETTINGS_KEY_COUNT {
        source.push_str(&format!("    \"key-{index}\": \"value-{index}\",\n"));
    }
    source.push_str("}\nrule \"settings\":\n    @Event global\n    pass\n");
    source
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus")
        .join(REAL_FIXTURE)
}

fn sha256(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|version| version.trim().to_string())
        .unwrap_or_else(|| "unavailable".to_string())
}

#[cfg(unix)]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let usage = unsafe { usage.assume_init() };
    #[cfg(target_os = "macos")]
    {
        u64::try_from(usage.ru_maxrss).ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        u64::try_from(usage.ru_maxrss).ok()?.checked_mul(1024)
    }
}

#[cfg(not(unix))]
fn peak_rss_bytes() -> Option<u64> {
    None
}
