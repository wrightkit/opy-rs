//! Oracle-backed catalog/member/enum lowering evidence for issue #42.

use std::path::{Path, PathBuf};

use opy_compiler::Compiler;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::roundtrip::equivalent;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures/synthetic")
        .join(name)
}

fn compile_fixture(name: &str) -> opy_compiler::CompilationArtifact {
    let dir = fixture_dir(name);
    let source = std::fs::read_to_string(dir.join("source.opy")).expect("source must be readable");
    let hir = opy_rs::compile(&source, "source.opy", &dir).expect("fixture must resolve");
    Compiler::new()
        .expect("released workshop contract must load")
        .compile_hir(&hir)
        .expect("fixture must lower to canonical WIR")
}

fn oracle_workshop(name: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir(name).join("oracle.json"))
            .expect("oracle must be readable"),
    )
    .expect("oracle must parse");
    value["compile"]["workshop"]
        .as_str()
        .expect("oracle must contain Workshop output")
        .to_string()
}

fn assert_matches_oracle(name: &str) {
    let artifact = compile_fixture(name);
    let catalog = Catalog::builtin().expect("catalog must load");
    let locale = Locale::new("en-US");
    let native =
        workshop_rs::parser::parse(&artifact.emitted, &catalog, &locale).unwrap_or_else(|error| {
            panic!("native output must reparse: {error}\n{}", artifact.emitted)
        });
    let oracle = workshop_rs::parser::parse(&oracle_workshop(name), &catalog, &locale)
        .expect("oracle output must reparse");
    assert!(
        equivalent(&native, &oracle),
        "issue #42 lowering diverged for {name}\n--- native ---\n{}\n--- oracle ---\n{}",
        artifact.emitted,
        oracle_workshop(name)
    );
}

#[test]
fn catalog_backed_receiver_calls_match_the_pinned_oracle() {
    assert_matches_oracle("receiver-calls");
}

#[test]
fn catalog_backed_contextual_chase_calls_match_the_pinned_oracle() {
    let artifact = compile_fixture("chase-condition-agentlab");
    assert!(
        artifact
            .emitted
            .contains("Chase Global Variable Over Time(Global.Round_Attack_Time, 0, 30, None);")
    );
}

#[test]
fn catalog_enum_members_lower_and_validate() {
    let artifact = compile_fixture("chase-enums");
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(time_reeval, None);")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(time_reeval, Destination and Duration);")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(rate_reeval, None);")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(rate_reeval, Destination and Rate);")
    );
}

#[test]
fn unknown_catalog_enum_member_is_a_source_attributed_gap() {
    let source = "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseTimeReeval.NOPE\n";
    let hir = opy_rs::compile(source, "source.opy", Path::new(".")).expect("frontend resolves");
    let error = match Compiler::new().expect("compiler loads").compile_hir(&hir) {
        Ok(_) => panic!("unknown catalog enum member must not be emitted"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert_eq!(error.diagnostic.span.expect("source span").start.line, 4);
}

#[test]
fn catalog_gap_member_is_explicitly_rejected() {
    let source =
        "rule \"r\":\n    @Event eachPlayer\n    @Condition eventPlayer.getHero() == None\n";
    let hir = opy_rs::compile(source, "source.opy", Path::new(".")).expect("frontend resolves");
    let error = match Compiler::new().expect("compiler loads").compile_hir(&hir) {
        Ok(_) => panic!("catalog-gap member must remain explicit"),
        Err(error) => error,
    };
    assert_eq!(error.diagnostic.code, "unsupported-integration-surface");
    assert!(
        error
            .diagnostic
            .message
            .contains("canonical catalog identity")
    );
}

#[test]
fn append_receiver_uses_the_canonical_modify_operation() {
    let source = "globalvar values\nrule \"r\":\n    @Event global\n    values.append(1)\n";
    let hir = opy_rs::compile(source, "source.opy", Path::new(".")).expect("frontend resolves");
    let artifact = Compiler::new()
        .expect("compiler loads")
        .compile_hir(&hir)
        .expect("append lowers to canonical WIR");
    assert!(
        artifact
            .emitted
            .contains("Modify Global Variable(values, Append To Array, 1);")
    );
}
