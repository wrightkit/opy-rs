//! Native-vs-reference differential suite (issue #7, part B; issue #2).
//!
//! Runs the declared compatibility corpus
//! (`compatibility/fixtures/**/fixture.json`) through the native frontend and
//! compares the outcome against the recorded reference evidence
//! (`oracle.json`, produced by `compatibility/run_oracle.py` against the
//! pinned OverPy 9.7.10 oracle).
//!
//! # What is compared
//!
//! The recorded reference evidence is the oracle's **Workshop text output**,
//! not a reference HIR (the wright adapter HIR fixtures do not exist in this
//! repository). The parity contract is observable semantics, not byte
//! identity, so the suite compares at the boundary the evidence supports:
//!
//! * **Status parity** — the native frontend must resolve a fixture the
//!   oracle accepts, and must reject a fixture the oracle rejects (with a
//!   structured diagnostic). This is the primary, CI-enforced contract.
//! * **Expected-outcome table** — every fixture has an explicit expectation
//!   (`resolve` or `expected-diagnostic`) with a documented rationale, so a
//!   fixture that is legitimately unsupported is a *documented* entry, not a
//!   silent failure; behavior that leaves the table is a `divergence` and
//!   fails the suite (regressions break CI, mirroring the wright contract).
//! * **Structural self-check** (always runs) — a resolved program must pass
//!   Opy HIR v2 validation, must round-trip through the wire payload
//!   (`parse_value(serde_json::to_value(program))`), and its debug dump must
//!   be deterministic.
//! * **Rule-name parity** (informational) — the ordered authored rule names
//!   in the native HIR are compared against `rule ("…")` entries in the
//!   oracle Workshop text, after normalizing away reference-synthesized
//!   `Initialize …`/`Subroutine …` rules (their synthesis is
//!   lowering-dependent in opy-rs; see the support matrix). Mismatches are
//!   recorded in the report as explicit gap entries; they do not fail the
//!   suite because text-shape differences at the emission boundary are not
//!   the compatibility contract.
//!
//! # Normalization rules
//!
//! * Span endpoints (`span` objects) are removed from the emitted native HIR
//!   JSON dumps (`target/opy-differential/<fixture>.native.json`):
//!   frontend-internal provenance that the reference evidence does not
//!   record. `protocol`/`generator` identities are contract fields and are
//!   kept verbatim.
//! * Reference synthesized rules (`Initialize global variables`,
//!   `Initialize player variables`, `Subroutine …`) are dropped from the
//!   rule-name comparison because their emission is lowering-dependent.
//! * Diagnostic wording is never compared; only status and the stable
//!   diagnostic `code` (where the expectation table pins one).
//!
//! # Degradation
//!
//! When a fixture has no `oracle.json` (reference artifacts absent), the
//! oracle comparison is skipped with a clear message and the entry is marked
//! `skip`; the structural self-check and the expected-outcome contract still
//! run. This keeps the suite runnable in `cargo test` without Node or OverPy
//! installed.
//!
//! # Report
//!
//! A machine-readable report is written to
//! `target/opy-differential-report.json` listing per-fixture native status and
//! relationship classification (`match` / `known-gap` /
//! `unexpected-divergence` / `inconclusive`), the native diagnostic code, the
//! reference status, rule-name comparison, and the support-matrix feature ids
//! the fixture evidences. A reference-success/native-failure case is never
//! classified as a match.
//!
//! # Current corpus state
//!
//! All declared fixtures run (0 skips, 0 divergences): **28 resolve** and
//! **16 produce expected diagnostics** with pinned codes; 7 fixtures are
//! documented reference gaps (the oracle accepts a surface the native
//! frontend deliberately rejects). Settings key-existence/leaf-kind
//! validation and Workshop enum member/domain validation were removed from
//! the frontend core (ownership fix): the affected fixtures resolve
//! structurally with opaque Workshop identity, and the checks are
//! `lowering-dependent` (issue #8) — the expectation table reflects that
//! contract.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use opy_rs::{LANGUAGE_NAME, LANGUAGE_VERSION, compile};
use serde_json::{Value, json};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn fixtures_root() -> PathBuf {
    workspace_root().join("compatibility").join("fixtures")
}

fn differential_expectations() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(
            workspace_root().join("compatibility/differential-expectations.json"),
        )
        .expect("differential-expectations.json must be readable"),
    )
    .expect("differential-expectations.json must parse")
}

fn expectation_for<'a>(expectations: &'a Value, id: &str) -> &'a Value {
    expectations["cases"]
        .as_array()
        .expect("differential expectations must contain cases")
        .iter()
        .find(|case| case["fixture"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("fixture '{id}' is missing from differential expectations"))
}

/// What the native frontend is expected to do for a fixture on this branch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Expect {
    /// The native frontend must resolve the fixture to an Opy HIR program.
    Resolve,
    /// The native frontend must reject the fixture with a structured
    /// diagnostic; `code` pins the stable code when known.
    Diagnostic { code: Option<&'static str> },
}

#[derive(Clone, Copy)]
struct Case {
    expect: Expect,
    /// Compare authored rule names against the oracle Workshop text.
    rule_names: bool,
    /// Documented rationale; also the divergence entry when the table is
    /// deliberately not matching the oracle status.
    note: &'static str,
}

fn case(expect: Expect, rule_names: bool, note: &'static str) -> Case {
    Case {
        expect,
        rule_names,
        note,
    }
}

/// The declared corpus expectation table. Every fixture in
/// `compatibility/fixtures` must appear here; unknown fixtures fail the suite
/// so new corpus entries are deliberate.
fn declared_corpus() -> BTreeMap<&'static str, Case> {
    let mut cases = BTreeMap::new();
    let resolve = |cases: &mut BTreeMap<&'static str, Case>, id, rule_names, note| {
        cases.insert(id, case(Expect::Resolve, rule_names, note));
    };
    let diagnostic = |cases: &mut BTreeMap<&'static str, Case>, id, code, note| {
        cases.insert(id, case(Expect::Diagnostic { code }, false, note));
    };

    // Synthetic fixtures (WrightKit-authored, AGPL-3.0-or-later).
    resolve(
        &mut cases,
        "synthetic/basic-rule",
        true,
        "minimal rule; oracle status success (parity).",
    );
    resolve(
        &mut cases,
        "synthetic/control-flow",
        true,
        "if/elif/else, for-in-range, while, pass; oracle status success (parity).",
    );
    resolve(
        &mut cases,
        "synthetic/issue-28-syntax",
        true,
        "Issue #28 pure OPY syntax: switch, do-while, hex, membership, dict indexing, comprehensions, lambda arguments, and string modifiers.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-28-string-modifiers",
        true,
        "Issue #28 inventory-backed string modifiers; translation-dependent l/t are syntax-carried outside the fixture.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-33-switch-break",
        true,
        "Issue #33 switch arms preserve source-order fallthrough and explicit break statements validate nested switch/loop context.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-33-f-string",
        true,
        "Issue #33 f-string interpolation preserves source-spanned expressions and the approved sorted lambda argument slot.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-35-integration",
        true,
        "Issue #35 OPY-to-Workshop integration fixture; the frontend resolves the source and the internal compiler module independently validates the canonical WIR slice.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-40-structural",
        false,
        "Issue #40 oracle-backed structural probe; the frontend resolves the source while the internal compiler module independently checks canonical WIR identity, allocation, and event filters.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-46-primitives",
        false,
        "Issue #46 oracle-backed non-control-flow primitives probe; the frontend resolves the source while the compiler test suite constrains native lowering against the pinned oracle through the canonical workshop-rs parser and structural equivalence.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-46-unsupported",
        false,
        "Issue #46 negative primitive-lowering probe; the frontend and the pinned oracle accept the dict-indexed assignment while the compiler rejects it with the stable source-attributed unsupported-integration-surface diagnostic.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-59-postfix-assignment",
        false,
        "Issue #59 postfix ++/-- assignment probe; global, player, and single-level indexed targets resolve and are constrained by compiler oracle tests.",
    );
    diagnostic(
        &mut cases,
        "synthetic/issue-59-postfix-negative",
        Some("parse-error"),
        "Issue #59 prefix ++ remains a stable source-attributed parse error; prefix --x remains valid consecutive unary-minus syntax.",
    );
    diagnostic(
        &mut cases,
        "synthetic/issue-59-embedded-postfix-negative",
        Some("parse-error"),
        "Issue #59 embedded postfix form remains a stable source-attributed parse error with independent pinned oracle evidence.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-113-is-dummy",
        true,
        "Issue #113 catalog-backed eventPlayer.isDummy() member predicate; canonical WIR lowering is constrained by the dedicated compiler test.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-114-hud-subheader",
        true,
        "Issue #114 shared hudSubheader action; canonical WIR lowering is constrained by the dedicated compiler test.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-47-control-flow",
        false,
        "Issue #47 oracle-backed control-flow lowering probe; the frontend resolves the source while the compiler independently constrains canonical WIR against the pinned oracle.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-47-unsupported",
        false,
        "Issue #47 negative probe; the frontend preserves the nested conditional switch-break HIR while the compiler rejects it at the canonical WIR integration boundary.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-47-switch-order",
        false,
        "Issue #47 source-order switch probe; default-before-case fallthrough remains represented in ordered HIR arms.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-47-switch-structured-target",
        false,
        "Issue #47 structured switch-target probe; nested canonical control-flow widths preserve later case/default targets.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-47-switch-multiple-break",
        false,
        "Issue #47 multi-break probe; the frontend preserves all authored arms and breaks while the compiler reports the canonical WIR capability gap.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-47-do-while-shapes",
        false,
        "Issue #47 do-while probe; direct, conditional, and nested break shapes resolve in the frontend and are constrained by compiler oracle tests.",
    );
    diagnostic(
        &mut cases,
        "synthetic/issue-47-do-while-invalid-placement",
        Some("do-while-placement"),
        "Issue #47 invalid do-while placement remains a stable source-attributed frontend diagnostic.",
    );
    diagnostic(
        &mut cases,
        "synthetic/issue-33-lambda-negative",
        Some("lambda-context"),
        "Issue #33 standalone lambda use remains rejected outside a signature-approved argument position.",
    );
    diagnostic(
        &mut cases,
        "synthetic/issue-28-invalid-syntax",
        Some("parse-error"),
        "Issue #28 malformed do-while and dictionary syntax remains a structured parse failure.",
    );
    resolve(
        &mut cases,
        "synthetic/declarations-numbers",
        true,
        "numeric literals and variable-index declarations; oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/declarations-rules",
        true,
        "globalvar/playervar/subroutine/def/enum and rule headers; oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/expressions-values",
        true,
        "expressions, arrays, strings, vectors, calls, .format; oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/preprocessing",
        false,
        "include + object/function-like defines + undef; oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-29-directives",
        true,
        "advanced directive state and source annotations; oracle status success.",
    );
    diagnostic(
        &mut cases,
        "synthetic/issue-29-invalid",
        None,
        "malformed directive and annotation forms; oracle status failure.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-29-main-file",
        true,
        "mainFile entry-point redirect and child-include scope; oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-31-positive",
        false,
        "pinned positive probe for global rulePrefixTemplate, include prefix restoration, AST macro/enum redeclaration, and translation normalization.",
    );
    diagnostic(
        &mut cases,
        "synthetic/issue-31-negative",
        Some("translations-invalid"),
        "pinned negative probe for a language code outside the exact translation set.",
    );
    resolve(
        &mut cases,
        "synthetic/issue-31-nested-scope",
        false,
        "nested include optimization directives are retained as observable scoped state; optimizer execution remains outside opy-rs.",
    );
    resolve(
        &mut cases,
        "synthetic/settings",
        true,
        "top-of-file settings block parsed into the typed HIR payload; validation is structural only (group shape, span validity, non-empty key names) — key-existence/leaf-kind checks were removed from the core and are lowering-dependent (#8); oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/receiver-calls",
        true,
        "receiver/member call forms; oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/receiver-playervar",
        true,
        "bare variable member expression is preserved as an OPY HIR member node; canonical Workshop member validation remains lowering-dependent.",
    );
    resolve(
        &mut cases,
        "synthetic/chase-enums",
        true,
        "ChaseTimeReeval/ChaseRateReeval member accesses resolve as opaque Workshop enum identities (member-existence/domain validation was removed from the core and is lowering-dependent, #8); oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/chase-condition-agentlab",
        true,
        "chaseOverTime in rule conditions (agent-lab regression); oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/chase-keywords",
        true,
        "named/keyword arguments and chase/ChaseReeval forms; the contextual member rewrites to the keyword-selected domain without membership checks (lowering-dependent, #8); oracle status success.",
    );
    resolve(
        &mut cases,
        "synthetic/for-range-agentlab",
        true,
        "for with implicit default-variable binder; oracle status success.",
    );
    diagnostic(
        &mut cases,
        "synthetic/diagnostics",
        Some("parse-error"),
        "expected-failure fixture: the native frontend rejects missing-colon with parse-error; oracle status failure (parity).",
    );
    resolve(
        &mut cases,
        "census/workshop-feature-census",
        true,
        "OPy-side Workshop feature census boundary; canonical feature identities remain owned by workshop-rs#10.",
    );

    // Real-world fixtures derived from upstream OverPy examples (GPL-3.0-only,
    // provenance-recorded evidence; oracle status success).
    resolve(
        &mut cases,
        "real-world/overpy-cake",
        true,
        "cake example; oracle status success (parity).",
    );
    resolve(
        &mut cases,
        "real-world/overpy-pixelart",
        false,
        "pixelart example; oracle status success.",
    );
    diagnostic(
        &mut cases,
        "real-world/overpy-santa",
        Some("parse-error"),
        "the postfix increment regression now resolves; the full project reaches the next unsupported for-range expression at santa.opy:356. Gap: reference accepts, native rejects (documented).",
    );
    diagnostic(
        &mut cases,
        "real-world/overpy-cronch",
        Some("parse-error"),
        "the postfix increment regression now resolves; the full project reaches the next unsupported createDummy action at cronch.opy:103. Gap: reference accepts, native rejects (documented).",
    );
    diagnostic(
        &mut cases,
        "real-world/overpy-broken-weapons",
        Some("parse-error"),
        "reference accepts; the native frontend rejects the `createWorkshopSetting(float[0.5:10], …)` numeric-range type at parse (the settings surface itself is parsed structurally; key/type validation is lowering-dependent, #8). Gap: reference accepts, native rejects (documented).",
    );
    diagnostic(
        &mut cases,
        "real-world/overpy-client-to-server",
        Some("parse-error"),
        "the chained ternary and isDummy regressions resolve; the full project reaches the next unsupported member (`getHorizontalFacingAngle`). Gap: reference accepts, native rejects (documented).",
    );
    diagnostic(
        &mut cases,
        "real-world/overpy-crosshair",
        Some("parse-error"),
        "reference accepts; the native frontend rejects the `b\"…\"` byte-string modifier (baseline category 1b, legacy-quirk/demand-driven, explicit rejection). Gap: reference accepts, native rejects (documented).",
    );
    diagnostic(
        &mut cases,
        "real-world/overpy-inputhud",
        Some("parse-error"),
        "reference accepts; the native frontend rejects multi-line parenthesized expressions with implicit string concatenation — a parse gap beyond the declared surface. Gap: reference accepts, native rejects (documented).",
    );
    diagnostic(
        &mut cases,
        "real-world/overpy-parabola",
        Some("parse-error"),
        "reference accepts; the native frontend rejects the numeric enum member `Team.2` — a parse gap beyond the declared surface. Gap: reference accepts, native rejects (documented).",
    );

    // Real-world failure fixtures (reference rejects; recorded diagnostics).
    diagnostic(
        &mut cases,
        "real-world/overpy-meipocalypse",
        Some("lex-error"),
        "reference fails with ENOENT on the __script__ macros (JS files not ported); the native frontend rejects earlier on the dict-literal surface (baseline category 1b, explicit rejection). Gap: rejection reason differs (documented); the script-macro defines are lexed as directives but never reached.",
    );
    diagnostic(
        &mut cases,
        "real-world/overpy-zencopter",
        Some("lex-error"),
        "reference rejects the example ('Invalid content before string: 'arena'', upstream example bug); the native frontend rejects the triple-quoted-string surface (baseline category 1b, declared rejection, unterminated-string lex error). Gap: rejection reasons differ (documented).",
    );
    diagnostic(
        &mut cases,
        "real-world/ow1-emulator",
        Some("main-file-placement"),
        "reference fails on semantic member checks; the native frontend now passes backslash line continuation and implicit string concatenation, then rejects the legacy #!mainFile placement (legacy-quirk/demand-driven). Gap: rejection reason differs (documented).",
    );
    diagnostic(
        &mut cases,
        "real-world/6v6-adjustments",
        Some("main-file-placement"),
        "reference fails on 'Unknown member '_hp_reset''; the native frontend now passes backslash line continuation, then rejects the legacy #!mainFile placement (legacy-quirk/demand-driven). Gap: rejection reason differs (documented).",
    );

    cases
}

/// Recursively collect `fixture.json` paths under `root`, sorted.
fn discover_fixtures(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()))
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|name| name == "fixture.json") {
                out.push(path);
            }
        }
    }
    out
}

/// Load a fixture manifest, returning `(id, source_name, expected_status)`.
fn load_fixture(path: &Path) -> (String, String, String) {
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap())
        .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()));
    let id = manifest["id"].as_str().unwrap().to_string();
    let source = manifest["source"].as_str().unwrap().to_string();
    let expected = manifest["expectedStatus"].as_str().unwrap().to_string();
    (id, source, expected)
}

/// Collect authored rule names from the native HIR in program order.
fn native_rule_names(program: &opy_rs::hir::Program) -> Vec<String> {
    program
        .rules
        .iter()
        .filter_map(|entry| match entry {
            opy_rs::hir::RuleEntry::Rule(rule) => Some(rule.name.clone()),
            // Subroutines are emitted as synthesized `rule ("Subroutine …")`
            // entries by the reference; both sides are normalized away.
            opy_rs::hir::RuleEntry::SubroutineDef { .. } => None,
        })
        .collect()
}

/// Collect `rule ("name")` occurrences from the oracle Workshop text in order,
/// dropping reference-synthesized `Initialize …`/`Subroutine …` rules.
fn reference_rule_names(workshop: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = workshop;
    while let Some(start) = rest.find("rule (\"") {
        rest = &rest[start + "rule (\"".len()..];
        let Some(end) = rest.find('"') else { break };
        let name = &rest[..end];
        if !name.starts_with("Initialize ") && !name.starts_with("Subroutine ") {
            names.push(name.to_string());
        }
        rest = &rest[end..];
    }
    names
}

/// Remove frontend-internal span endpoints from the native wire payload
/// (documented normalization; protocol/generator identities are kept).
fn strip_spans(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("span");
            for nested in map.values_mut() {
                strip_spans(nested);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_spans(item);
            }
        }
        _ => {}
    }
}

/// The support-matrix feature ids whose evidence cites `fixtures:<id>`.
fn support_matrix_linkage(matrix: &Value, id: &str) -> Vec<String> {
    let fixtures_key = format!("fixtures:{id}");
    matrix["features"]
        .as_array()
        .map(|features| {
            features
                .iter()
                .filter_map(|feature| {
                    let evidences = feature["evidence"].as_array();
                    let cited = evidences.is_some_and(|list| {
                        list.iter()
                            .any(|entry| entry.as_str() == Some(&fixtures_key))
                    });
                    cited.then(|| feature["id"].as_str().unwrap().to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Run one fixture through the native frontend plus the structural
/// self-checks, and emit its normalized native HIR JSON dump.
fn run_native(
    source: &str,
    source_name: &str,
    fixture_dir: &Path,
    id: &str,
) -> Result<opy_rs::hir::Program, opy_rs::OpyError> {
    let program = compile(source, source_name, fixture_dir)?;
    program
        .validate()
        .expect("native HIR must satisfy the v1 invariants");
    let wire = serde_json::to_value(&program).expect("HIR serialization is infallible");
    let round_trip = opy_rs::hir::parse_value(wire)
        .expect("the native wire payload must be consumable by parse_value");
    round_trip.validate().expect("round-trip HIR must validate");
    let dump = program.dump();
    assert_eq!(dump, program.dump(), "the debug dump must be deterministic");
    assert!(!dump.is_empty());

    let mut normalized = serde_json::to_value(&program).unwrap();
    strip_spans(&mut normalized);
    let out_dir = workspace_root().join("target/opy-differential");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(
        out_dir.join(format!("{}.native.json", id.replace('/', "-"))),
        serde_json::to_string_pretty(&normalized).unwrap() + "\n",
    )
    .unwrap();
    Ok(program)
}

#[test]
fn native_and_reference_agree_on_the_declared_corpus() {
    let corpus = declared_corpus();
    let expectations = differential_expectations();
    let matrix: Value = serde_json::from_str(
        &std::fs::read_to_string(workspace_root().join("compatibility/support-matrix.json"))
            .unwrap(),
    )
    .expect("support-matrix.json must parse");

    let mut fixtures = BTreeMap::<String, Value>::new();
    let mut divergences: Vec<Value> = Vec::new();
    let mut known_gaps: Vec<Value> = Vec::new();
    let mut rule_name_mismatches = 0usize;
    let mut counts = json!({
        "total": 0,
        "resolve": 0,
        "expectedDiagnostic": 0,
        "divergence": 0,
        "match": 0,
        "knownGap": 0,
        "unexpectedDivergence": 0,
        "inconclusive": 0,
        "skipped": 0
    });

    let mut seen: Vec<String> = Vec::new();
    for manifest_path in discover_fixtures(&fixtures_root()) {
        let (id, source_name, expected_status) = load_fixture(&manifest_path);
        seen.push(id.clone());
        let case = *corpus.get(id.as_str()).unwrap_or_else(|| {
            panic!(
                "fixture '{id}' is not declared in the differential expectation table; \
                 add an explicit resolve/diagnostic entry with a note"
            )
        });
        let expectation = expectation_for(&expectations, &id);
        let expected_native_status = expectation["nativeStatus"]
            .as_str()
            .unwrap_or_else(|| panic!("{id}: nativeStatus is required"));
        let expected_classification = expectation["classification"]
            .as_str()
            .unwrap_or_else(|| panic!("{id}: classification is required"));
        let expected_evidence = expectation["evidence"]
            .as_array()
            .unwrap_or_else(|| panic!("{id}: evidence is required"));
        assert!(
            !expected_evidence.is_empty(),
            "{id}: differential expectation evidence cannot be empty"
        );
        assert_eq!(
            expected_native_status,
            if matches!(case.expect, Expect::Resolve) {
                "success"
            } else {
                "failure"
            },
            "{id}: differential expectation disagrees with native expectation table"
        );
        let fixture_dir = manifest_path.parent().unwrap().to_path_buf();
        let source_path = fixture_dir.join(&source_name);
        let source = std::fs::read_to_string(&source_path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", source_path.display()));

        // Native side: preprocess -> parse -> lower (never requires Node).
        let native = run_native(&source, &source_name, &fixture_dir, &id);
        let native_ok = native.is_ok();
        let native_entry = match &native {
            Ok(_) => json!({ "status": "ok" }),
            Err(error) => json!({
                "status": "error",
                "code": error.code,
                "line": error.span.map(|span| span.start.line),
            }),
        };

        // Reference side: the recorded oracle snapshot (absent -> skip with a
        // clear message; the structural/table contract still ran above).
        let snapshot_path = fixture_dir.join("oracle.json");
        let snapshot_present = snapshot_path.is_file();
        let reference_status = if snapshot_present {
            let oracle: Value =
                serde_json::from_str(&std::fs::read_to_string(&snapshot_path).unwrap())
                    .expect("oracle.json must parse");
            oracle["compile"]["status"].as_str().map(str::to_string)
        } else {
            None
        };
        let reference_entry = json!({
            "expectedStatus": expected_status,
            "snapshot": if snapshot_present { "present" } else { "absent" },
            "compileStatus": reference_status,
        });

        // Status determination against the expectation table.
        let expect_resolve = expected_native_status == "success";
        let status = if native_ok == expect_resolve {
            if expect_resolve {
                "resolve"
            } else {
                "expected-diagnostic"
            }
        } else {
            "divergence"
        };
        // The report summary keys the counts by camelCase status labels.
        let status_key = match status {
            "expected-diagnostic" => "expectedDiagnostic",
            other => other,
        };
        let skipped = !snapshot_present;

        // Pinned diagnostic codes must match exactly.
        let mut detail = Vec::new();
        if let (
            Expect::Diagnostic {
                code: Some(expected_code),
            },
            Err(error),
        ) = (case.expect, &native)
        {
            if error.code != expected_code {
                detail.push(format!(
                    "expected diagnostic code '{expected_code}', got '{}'",
                    error.code
                ));
            }
        }

        // Reference status parity: informational when the expectation table
        // deliberately diverges from the oracle (the entry is then a
        // documented `referenceGap`; hard divergences fail the suite).
        let reference_gap = match &reference_status {
            Some(reference_status) => (*reference_status == "success") != native_ok,
            None => false,
        };
        if reference_gap && status != "divergence" {
            detail.push(format!(
                "native {} but oracle records {}",
                if native_ok { "resolves" } else { "rejects" },
                reference_status.as_deref().unwrap_or("?")
            ));
        }

        // Oracle snapshot consistency: the recorded compile status must match
        // the fixture manifest's expectedStatus.
        if let Some(compile_status) = &reference_status {
            if compile_status != &expected_status {
                detail.push(format!(
                    "oracle.json compile.status '{compile_status}' disagrees with fixture.json expectedStatus '{expected_status}'"
                ));
            }
        }

        let relationship_holds = match expected_classification {
            "match" => !reference_gap,
            "known-gap" | "unsupported" => reference_gap,
            other => panic!("{id}: unsupported expectation classification '{other}'"),
        };
        let classification = if skipped {
            "inconclusive"
        } else if status == "divergence" || !relationship_holds {
            "unexpected-divergence"
        } else if reference_gap {
            expected_classification
        } else {
            "match"
        };

        // Rule-name parity (informational, opt-in per fixture).
        let rule_names_entry = if case.rule_names && snapshot_present {
            match &native {
                Ok(program) => {
                    let oracle: Value =
                        serde_json::from_str(&std::fs::read_to_string(&snapshot_path).unwrap())
                            .unwrap();
                    let workshop = oracle["compile"]["workshop"].as_str().unwrap_or("");
                    let native_names = native_rule_names(program);
                    let reference_names = reference_rule_names(workshop);
                    let matched = native_names == reference_names;
                    if !matched {
                        rule_name_mismatches += 1;
                    }
                    json!({
                        "match": matched,
                        "native": native_names,
                        "reference": reference_names,
                    })
                }
                Err(_) => Value::Null,
            }
        } else {
            Value::Null
        };

        let entry = json!({
            "status": status,
            "expect": if expect_resolve { "resolve" } else { "diagnostic" },
            "note": case.note,
            "native": native_entry,
            "reference": reference_entry,
            "ruleNames": rule_names_entry,
            "supportMatrix": support_matrix_linkage(&matrix, &id),
            "detail": detail,
            "referenceGap": reference_gap,
            "skip": skipped,
            "classification": classification,
            "expectedClassification": expected_classification,
            "evidence": expected_evidence,
        });
        let code = entry["native"].get("code").and_then(Value::as_str);
        let label = if skipped {
            "SKIP"
        } else if status == "divergence" {
            "FAIL"
        } else if reference_gap {
            "KNOWN GAP"
        } else {
            "PASS"
        };
        println!(
            "{label} {id} ({status}{}{})",
            code.map_or(String::new(), |code| format!(", {code}")),
            if reference_gap { ", reference-gap" } else { "" }
        );
        if skipped {
            println!(
                "  reference snapshot absent ({}) — oracle comparison skipped; structural self-check ran",
                snapshot_path.display()
            );
        }
        if status == "divergence" {
            divergences.push(json!({
                "fixture": id,
                "expect": if expect_resolve { "resolve" } else { "diagnostic" },
                "native": entry["native"],
                "reference": entry["reference"],
                "note": case.note,
                "detail": detail,
            }));
        }
        if classification == "known-gap" {
            known_gaps.push(json!({
                "fixture": id,
                "native": entry["native"],
                "reference": entry["reference"],
                "note": case.note,
                "detail": detail,
            }));
        }
        if let Value::Number(count) = &mut counts[status_key] {
            *count =
                serde_json::Number::from(count.as_u64().expect("status counts start as u64") + 1);
        }
        if let Value::Number(count) = &mut counts["total"] {
            *count =
                serde_json::Number::from(count.as_u64().expect("status counts start as u64") + 1);
        }
        if skipped {
            if let Value::Number(count) = &mut counts["skipped"] {
                *count = serde_json::Number::from(
                    count.as_u64().expect("status counts start as u64") + 1,
                );
            }
        }
        let classification_key = match classification {
            "known-gap" => "knownGap",
            "unexpected-divergence" => "unexpectedDivergence",
            other => other,
        };
        if let Value::Number(count) = &mut counts[classification_key] {
            *count = serde_json::Number::from(
                count.as_u64().expect("classification counts start as u64") + 1,
            );
        }
        fixtures.insert(id.clone(), entry);
    }

    // Every declared corpus entry must exist on disk.
    let missing: Vec<&str> = corpus
        .keys()
        .filter(|id| !seen.iter().any(|s| s == *id))
        .copied()
        .collect();
    assert!(
        missing.is_empty(),
        "declared corpus entries missing from compatibility/fixtures: {missing:?}"
    );
    let extra_expectations: Vec<&str> = expectations["cases"]
        .as_array()
        .expect("differential expectations must contain cases")
        .iter()
        .filter_map(|case| case["fixture"].as_str())
        .filter(|id| !seen.iter().any(|seen_id| seen_id == id))
        .collect();
    assert!(
        extra_expectations.is_empty(),
        "differential expectations reference missing fixtures: {extra_expectations:?}"
    );

    let report = json!({
        "schemaVersion": 1,
        "artifact": "opy-rs native-vs-reference differential report (issue #25)",
        "generatedBy": "crates/opy-rs/tests/differential.rs",
        "frontend": { "name": LANGUAGE_NAME, "version": LANGUAGE_VERSION },
        "reference": matrix["reference"],
        "summary": {
            "total": counts["total"],
            "resolve": counts["resolve"],
            "expectedDiagnostic": counts["expectedDiagnostic"],
            "divergence": counts["divergence"],
            "skipped": counts["skipped"],
            "match": counts["match"],
            "knownGap": counts["knownGap"],
            "unexpectedDivergence": counts["unexpectedDivergence"],
            "inconclusive": counts["inconclusive"],
            "ruleNameMismatches": rule_name_mismatches,
        },
        "divergences": divergences,
        "knownGaps": known_gaps,
        "fixtures": Value::Object(fixtures.into_iter().collect()),
    });
    let report_path = workspace_root().join("target/opy-differential-report.json");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).unwrap() + "\n",
    )
    .unwrap();
    println!("report written to {}", report_path.display());

    assert!(
        divergences.is_empty(),
        "supported-surface divergences are not allowed:\n{}",
        divergences
            .iter()
            .map(|entry| format!(
                "- {}: expect {}; {}",
                entry["fixture"],
                entry["expect"],
                entry["detail"].as_array().map_or_else(
                    || entry["note"].to_string(),
                    |list| {
                        list.iter()
                            .map(Value::to_string)
                            .collect::<Vec<_>>()
                            .join("; ")
                    }
                )
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
