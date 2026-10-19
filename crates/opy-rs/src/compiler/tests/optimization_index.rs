//! Equivalence checks for the per-file `#!` directive index behind
//! `optimization_state_at`.
//!
//! The lowering path once answered every query with a linear scan over all
//! recorded directive events; the index must return the same state for every
//! span lowering ever asks about. The old scan is retained as
//! `Lowering::optimization_state_scan` (test-only) and is the oracle here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::compiler::{Compiler, Lowering};
use crate::hir::{self, Position, Span};
use workshop_rs::catalog::Locale;

/// Bastion project checkout used by the issue-596 measurement harness.
const BASTION_PROJECT_ENV: &str = "OPY_BASTION_PROJECT";

fn bastion_project() -> Option<PathBuf> {
    std::env::var_os(BASTION_PROJECT_ENV).map(PathBuf::from)
}

fn lower_and_collect_queries(hir: &hir::Program) -> Vec<Option<Span>> {
    let compiler = Compiler::new().expect("Workshop contract must load");
    let mut lowering = Lowering::new(&compiler, hir);
    // Lower as much as the program supports; a fixture that reaches an
    // unsupported backend surface still exercises every query recorded on
    // the way to the error.
    let _ = lowering
        .copy_files(&Locale::new("en-US"))
        .and_then(|_| lowering.lower_declarations())
        .and_then(|_| lowering.lower_rules());
    lowering.optimization_queries.take()
}

fn assert_index_matches_scan(hir: &hir::Program, spans: &[Option<Span>]) {
    let compiler = Compiler::new().expect("Workshop contract must load");
    let lowering = Lowering::new(&compiler, hir);
    for (index, span) in spans.iter().enumerate() {
        let via_index = lowering.optimization_state_at(span.as_ref());
        // `optimization_state_at` records the query again; drop the extra
        // entries so the collected span set is compared once.
        lowering.optimization_queries.take();
        let via_scan = lowering.optimization_state_scan(span.as_ref());
        assert_eq!(
            via_index, via_scan,
            "optimization index disagrees with the linear scan for query {index}: {span:?}"
        );
    }
}

#[test]
fn optimization_index_matches_scan_on_synthetic_edge_cases() {
    // Exercise every span shape `optimization_state_at` sees: no span,
    // positions before the first directive of a file, between directives,
    // on the same line/col boundary as a directive, in included files, and
    // spans referencing files that carry no directives at all.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic/optimization-state-edge");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");

    let mut spans = lower_and_collect_queries(&hir);
    spans.push(None);

    // Hand-picked edge spans: unknown file ids, positions before any
    // directive of a directive-bearing file, exact directive positions, and
    // end-of-file positions.
    let files: Vec<u32> = hir
        .preprocessing
        .directives
        .iter()
        .filter_map(|directive| directive.span.map(|span| span.file))
        .collect();
    let unknown_file = files.iter().copied().max().unwrap_or(0) + 1;
    let position = |line, col| Position { line, col };
    let span = |file, line, col| Span {
        file,
        start: position(line, col),
        end: position(line, col),
    };
    for directive in &hir.preprocessing.directives {
        if let Some(directive_span) = directive.span {
            // At the directive itself and one column before it.
            spans.push(Some(directive_span));
            spans.push(Some(Span {
                start: position(
                    directive_span.start.line,
                    directive_span.start.col.saturating_sub(1),
                ),
                ..directive_span
            }));
            // The end position of the directive line's file.
            spans.push(Some(span(
                directive_span.file,
                directive_span.end.line,
                u32::MAX,
            )));
        }
    }
    spans.push(Some(span(unknown_file, 1, 1)));
    spans.push(Some(span(unknown_file, 500, 1)));
    spans.push(Some(span(u32::MAX, 9, 9)));

    assert!(!spans.is_empty());
    assert_index_matches_scan(&hir, &spans);
}

#[test]
fn optimization_index_matches_scan_over_corpus_fixtures() {
    // Every span lowered by the shipped corpus fixtures must agree between
    // the index and the linear scan. This is the broad regression net for
    // `#!` directive scoping (`#!optimizeFor`, `#!optimizeSize`, …) across
    // multi-file projects.
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("crates/opy-rs/tests/fixtures/corpus");
    let mut checked = 0usize;
    for entry in walk_fixture_dirs(&corpus) {
        let source_path = entry.join("source.opy");
        if !source_path.exists() {
            continue;
        }
        let source = std::fs::read_to_string(&source_path).unwrap();
        let Ok(hir) = crate::compile(&source, "source.opy", &entry) else {
            continue;
        };
        let spans = lower_and_collect_queries(&hir);
        assert_index_matches_scan(&hir, &spans);
        checked += 1;
    }
    assert!(checked > 10, "corpus must provide fixtures to check");
}

#[test]
fn optimization_index_matches_scan_on_bastion_project() {
    // The full Bastion workload from issue #596: every span the lowering
    // pass queries must return the same state from the index as from the
    // original linear scan. Skipped unless OPY_BASTION_PROJECT points at a
    // Bastion checkout.
    let Some(project_dir) = bastion_project() else {
        eprintln!("skipping Bastion equivalence check: {BASTION_PROJECT_ENV} is not set");
        return;
    };
    let project = crate::FilesystemProject::load(&project_dir.join("src/main.opy"))
        .expect("Bastion project must load");
    let main_path = project.main_path().to_string_lossy().to_string();
    let outcome = crate::compile_with_overlay_outcome(
        project.source(),
        &main_path,
        project.root(),
        &BTreeMap::new(),
    );
    let hir = outcome.hir.expect("Bastion frontend must succeed");
    let expanded = crate::compiler::expand_macros(&hir).expect("Bastion macros must expand");

    let spans = lower_and_collect_queries(&expanded);
    assert!(!spans.is_empty(), "Bastion must lower optimization queries");
    assert_index_matches_scan(&expanded, &spans);
    eprintln!("checked {} Bastion optimization queries", spans.len());
}

fn walk_fixture_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|name| name == "fixture.json") {
                dirs.push(dir.clone());
            }
        }
    }
    dirs.sort();
    dirs
}
