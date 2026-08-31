use std::path::Path;

use opy_rs::hir::types::{Declaration, RuleEntry, Stmt};
use opy_rs::tooling::{SymbolKind, check};

#[test]
fn def_materializes_a_tooling_usable_subroutine_declaration() {
    let source =
        "def worker():\n    pass\n\nrule \"call worker\":\n    @Event global\n    worker()\n";
    let outcome = check(source, "main.opy", Path::new(""));
    assert!(
        outcome.is_clean(),
        "def-only call must resolve: {:?}",
        outcome.diagnostics
    );
    let model = outcome.model.expect("clean semantic model");

    assert!(model.declarations().iter().any(|declaration| matches!(
        declaration,
        Declaration::Subroutine { name, name_span, .. }
            if name == "worker" && name_span.is_some()
    )));
    assert!(model.rules().iter().any(|entry| matches!(
        entry,
        RuleEntry::Rule(rule)
            if rule.actions.iter().any(|statement| matches!(
                statement,
                Stmt::CallSubroutine { name, .. } if name == "worker"
            ))
    )));

    let worker_symbols: Vec<_> = model
        .symbols()
        .iter()
        .filter(|symbol| symbol.name == "worker")
        .collect();
    assert_eq!(worker_symbols.len(), 2);
    assert!(
        worker_symbols
            .iter()
            .all(|symbol| !symbol.references.is_empty())
    );
    assert!(
        worker_symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Subroutine)
    );
    assert!(
        worker_symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Def)
    );
}

#[test]
fn subroutine_visibility_follows_source_order_and_rejects_duplicate_defs() {
    let forward_call = check(
        "rule \"call worker\":\n    @Event global\n    worker()\n\ndef worker():\n    pass\n",
        "main.opy",
        Path::new(""),
    );
    let diagnostic = forward_call
        .diagnostics
        .first()
        .expect("forward call diagnostic");
    assert_eq!(diagnostic.code, "unknown-action");
    assert_eq!(diagnostic.span.as_ref().expect("source span").start.line, 3);

    let duplicate = check(
        "def worker():\n    pass\n\ndef worker():\n    pass\n",
        "main.opy",
        Path::new(""),
    );
    let diagnostic = duplicate
        .diagnostics
        .first()
        .expect("duplicate definition diagnostic");
    assert_eq!(diagnostic.code, "duplicate-definition");
    assert_eq!(diagnostic.span.as_ref().expect("source span").start.line, 4);
}
