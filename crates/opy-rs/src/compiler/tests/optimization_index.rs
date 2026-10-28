use std::path::{Path, PathBuf};

use crate::compiler::{Compiler, Lowering};
use crate::hir::{self, Position, Span};
use workshop_rs::catalog::Locale;

fn lower_and_collect_queries(hir: &hir::Program) -> Vec<Option<Span>> {
    let compiler = Compiler::new().expect("Workshop contract must load");
    let mut lowering = Lowering::new(&compiler, hir);
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
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/corpus/synthetic/optimization-state-edge");
    let source = std::fs::read_to_string(dir.join("source.opy")).unwrap();
    let hir = crate::compile(&source, "source.opy", &dir).expect("fixture must resolve");

    let mut spans = lower_and_collect_queries(&hir);
    spans.push(None);

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
            spans.push(Some(directive_span));
            spans.push(Some(Span {
                start: position(
                    directive_span.start.line,
                    directive_span.start.col.saturating_sub(1),
                ),
                ..directive_span
            }));
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
