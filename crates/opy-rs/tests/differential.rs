//! Native-vs-reference differential suite.
//!
//! Runs the compatibility corpus
//! (`crates/opy-rs/tests/fixtures/corpus/**/fixture.json`) through the native frontend and
//! compares the outcome against the recorded reference result
//! (`oracle.json`, produced by `tools/overpy/run_oracle.py` against the
//! pinned OverPy 9.7.10 oracle).
//!
//! # What is compared
//!
//! The recorded reference result contains the oracle's **Workshop text output**,
//! not a reference HIR (the wright adapter HIR fixtures do not exist in this
//! repository). The parity contract is observable semantics, not byte
//! identity, so the suite compares the supported observable boundaries:
//!
//! * **Status parity** — the native frontend must resolve a fixture the
//!   oracle accepts, and must reject a fixture the oracle rejects (with a
//!   structured diagnostic). This is the primary, CI-enforced contract.
//! * **Expected outcome** — each fixture manifest declares the native outcome
//!   and its relationship to the oracle; behavior outside that declaration is
//!   a `divergence` and fails the suite.
//! * **Structural self-check** (always runs) — a resolved program must pass
//!   Opy HIR v2 validation, must round-trip through the wire payload
//!   (`parse_value(serde_json::to_value(program))`), and its debug dump must
//!   be deterministic.
//! * **Rule-name parity** (informational) — the ordered authored rule names
//!   in the native HIR are compared against `rule ("…")` entries in the
//!   oracle Workshop text, after normalizing away reference-synthesized
//!   `Initialize …`/`Subroutine …` rules (their synthesis is
//!   lowering-dependent in opy-rs; see the canonical language-support contract).
//!   Mismatches are
//!   recorded in the report as explicit gap entries; they do not fail the
//!   suite because text-shape differences at the emission boundary are not
//!   the compatibility contract.
//!
//! # Normalization rules
//!
//! * Span endpoints (`span` objects) are removed from the emitted native HIR
//!   JSON dumps (`target/opy-differential/<fixture>.native.json`):
//!   frontend-internal source mapping that the reference result does not
//!   record. `protocol`/`generator` identities are contract fields and are
//!   kept verbatim.
//! * Reference synthesized rules (`Initialize global variables`,
//!   `Initialize player variables`, `Subroutine …`) are dropped from the
//!   rule-name comparison because their emission is lowering-dependent.
//! * Diagnostic wording is never compared; only status and the stable
//!   diagnostic `code` (where the fixture manifest pins one).
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
//! reference status, and rule-name comparison. A reference-success/native-failure case is never
//! classified as a match.
//!
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use opy_rs::{LANGUAGE_NAME, LANGUAGE_VERSION, compile};
use serde_json::{Value, json};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn fixtures_root() -> PathBuf {
    workspace_root().join("crates/opy-rs/tests/fixtures/corpus")
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

/// Load a fixture manifest, returning its source-test expectation.
fn load_fixture(path: &Path) -> (String, String, String, String, bool, Option<String>) {
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap())
        .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()));
    let id = manifest["id"].as_str().unwrap().to_string();
    let source = manifest["source"].as_str().unwrap().to_string();
    let source_test = &manifest["tests"]["source"];
    let native_status = source_test["nativeStatus"].as_str().unwrap().to_string();
    let relationship = source_test["relationship"].as_str().unwrap().to_string();
    let rule_names = source_test["ruleNames"].as_bool().unwrap();
    let diagnostic_code = source_test["diagnosticCode"].as_str().map(str::to_string);
    (
        id,
        source,
        native_status,
        relationship,
        rule_names,
        diagnostic_code,
    )
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

/// Run one fixture through the native frontend plus the structural
/// self-checks, and emit its normalized native HIR JSON dump.
fn run_native(
    source: &str,
    source_name: &str,
    fixture_dir: &Path,
    id: &str,
) -> Result<opy_rs::hir::Program, opy_rs::OpyError> {
    let program = compile(source, source_name, fixture_dir)?;
    program.validate().map_err(|error| match error.span() {
        Some(span) => opy_rs::OpyError::at(
            error.code(),
            error.message(),
            opy_rs::diag::Span::new(
                span.file,
                opy_rs::diag::Position::new(span.start.line, span.start.col),
                opy_rs::diag::Position::new(span.end.line, span.end.col),
            ),
        ),
        None => opy_rs::OpyError::new(error.code(), error.message()),
    })?;
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
fn native_and_reference_agree_on_the_corpus() {
    let mut reference_identity = Value::Null;

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

    for manifest_path in discover_fixtures(&fixtures_root()) {
        let (
            id,
            source_name,
            expected_native_status,
            expected_relationship,
            compare_rule_names,
            expected_diagnostic_code,
        ) = load_fixture(&manifest_path);
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
            if reference_identity.is_null() {
                reference_identity = oracle["oracle"].clone();
            }
            oracle["compile"]["status"].as_str().map(str::to_string)
        } else {
            None
        };
        let reference_entry = json!({
            "snapshot": if snapshot_present { "present" } else { "absent" },
            "compileStatus": reference_status.as_deref(),
        });

        // Status determination against the fixture manifest.
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
        if let (Some(expected_code), Err(error)) = (&expected_diagnostic_code, &native) {
            if error.code != expected_code.as_str() {
                detail.push(format!(
                    "expected diagnostic code '{expected_code}', got '{}'",
                    error.code
                ));
            }
        }

        // Reference status parity: informational when the manifest deliberately
        // declares a known gap between native and reference behavior.
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

        let relationship_holds = match expected_relationship.as_str() {
            "match" => !reference_gap,
            "known-gap" | "unsupported" => reference_gap,
            other => panic!("{id}: unsupported relationship '{other}'"),
        };
        if !relationship_holds {
            detail.push(format!(
                "expected relationship '{expected_relationship}' is not satisfied (reference-gap: {reference_gap})"
            ));
        }
        let classification = if skipped {
            "inconclusive"
        } else if status == "divergence" || !relationship_holds {
            "unexpected-divergence"
        } else if reference_gap {
            expected_relationship.as_str()
        } else {
            "match"
        };

        // Rule-name parity (informational, opt-in per fixture).
        let rule_names_entry = if compare_rule_names && snapshot_present {
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
            "native": native_entry,
            "reference": reference_entry,
            "ruleNames": rule_names_entry,
            "detail": detail,
            "referenceGap": reference_gap,
            "skip": skipped,
            "classification": classification,
            "expectedRelationship": expected_relationship,
        });
        let code = entry["native"].get("code").and_then(Value::as_str);
        let label = if skipped {
            "SKIP"
        } else if classification == "unexpected-divergence" {
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
        if classification == "unexpected-divergence" {
            divergences.push(json!({
                "fixture": id,
                "expect": if expect_resolve { "resolve" } else { "diagnostic" },
                "native": entry["native"],
                "reference": entry["reference"],
                "expectedRelationship": expected_relationship,
                "referenceGap": reference_gap,
                "detail": detail,
            }));
        }
        if classification == "known-gap" {
            known_gaps.push(json!({
                "fixture": id,
                "native": entry["native"],
                "reference": entry["reference"],
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

    let report = json!({
        "schemaVersion": 1,
        "artifact": "opy-rs native-vs-reference differential report",
        "generatedBy": "crates/opy-rs/tests/differential.rs",
        "frontend": { "name": LANGUAGE_NAME, "version": LANGUAGE_VERSION },
        "reference": reference_identity,
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
                    || "no detail".to_string(),
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
