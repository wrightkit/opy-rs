use std::path::Path;

use opy_rs::hir::types::{Declaration, Expr, RuleEntry, Stmt};
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

#[test]
fn visibility_uses_interleaved_top_level_source_order() {
    let source = "def worker():\n    pass\n\nmacro call_worker():\n    worker()\n\nrule \"call macro\":\n    @Event global\n    call_worker()\n";
    let outcome = check(source, "main.opy", Path::new(""));
    assert!(
        outcome.is_clean(),
        "a macro must see an earlier def: {:?}",
        outcome.diagnostics
    );

    let later_macro = check(
        "rule \"call macro\":\n    @Event global\n    later_macro()\n\nmacro later_macro():\n    pass\n",
        "main.opy",
        Path::new(""),
    );
    assert_eq!(
        later_macro
            .diagnostics
            .first()
            .expect("forward macro diagnostic")
            .code,
        "unknown-action"
    );

    let later_global = check(
        "rule \"use global\":\n    @Event global\n    later = 1\n\nglobalvar later\n",
        "main.opy",
        Path::new(""),
    );
    assert_eq!(
        later_global
            .diagnostics
            .first()
            .expect("forward global diagnostic")
            .code,
        "unknown-identifier"
    );
}

#[test]
fn player_variable_receivers_cover_all_source_player_contexts() {
    let source = "\
playervar isWeaponBroken
globalvar target
rule \"player contexts\":
    @Event global
    @Condition eventPlayer.isWeaponBroken == true
    @Condition hostPlayer.isWeaponBroken == false
    @Condition attacker.isWeaponBroken == true
    @Condition victim.isWeaponBroken == false
    @Condition attacker.isAlive()
    @Condition target.isWeaponBroken == true
    @Condition getAllPlayers().isWeaponBroken == true
    @Condition localPlayer.isWeaponBroken == false
    target = attacker
";
    let outcome = check(source, "main.opy", Path::new(""));
    assert!(
        outcome.is_clean(),
        "player context expressions must resolve: {:?}",
        outcome.diagnostics
    );
    let model = outcome.model.expect("clean semantic model");
    let RuleEntry::Rule(rule) = &model.hir.rules[0] else {
        panic!("expected a rule");
    };

    let expected_contexts = ["eventPlayer", "hostPlayer", "attacker", "victim"];
    for (condition, expected_context) in rule
        .conditions
        .iter()
        .take(expected_contexts.len())
        .zip(expected_contexts)
    {
        let Expr::Binary { left, .. } = condition else {
            panic!("expected comparison condition");
        };
        let Expr::PlayerVar {
            player,
            name,
            member_span,
            ..
        } = left.as_ref()
        else {
            panic!("expected player-variable condition");
        };
        assert_eq!(name, "isWeaponBroken");
        assert!(member_span.is_some(), "member source identity must survive");
        match expected_context {
            "eventPlayer" => assert!(matches!(&**player, Expr::EventPlayer { .. })),
            "hostPlayer" => assert!(matches!(&**player, Expr::HostPlayer { .. })),
            "attacker" | "victim" => assert!(matches!(
                &**player,
                Expr::Call { name, args, .. }
                    if name == expected_context && args.is_empty()
            )),
            _ => unreachable!(),
        }
    }

    assert!(matches!(
        &rule.conditions[7],
        Expr::Binary { left, .. }
            if matches!(left.as_ref(), Expr::PlayerVar { player, .. }
                if matches!(player.as_ref(), Expr::Call { name, args, .. }
                    if name == "localPlayer" && args.is_empty()))
    ));

    assert!(matches!(
        &rule.conditions[4],
        Expr::ReceiverCall { receiver, name, .. }
            if name == "isAlive"
                && matches!(receiver.as_ref(), Expr::Call { name, args, .. } if name == "attacker" && args.is_empty())
    ));
    assert!(matches!(
        &rule.conditions[5],
        Expr::Binary { left, .. }
            if matches!(left.as_ref(), Expr::PlayerVar { player, name, member_span, .. }
                if name == "isWeaponBroken"
                    && member_span.is_some()
                    && matches!(player.as_ref(), Expr::GlobalVar { name, .. } if name == "target"))
    ));
    assert!(matches!(
        &rule.conditions[6],
        Expr::Binary { left, .. }
            if matches!(left.as_ref(), Expr::PlayerVar { player, name, member_span, .. }
                if name == "isWeaponBroken"
                    && member_span.is_some()
                    && matches!(player.as_ref(), Expr::Call { name, args, .. } if name == "getAllPlayers" && args.is_empty()))
    ));
    assert!(matches!(
        rule.actions.first(),
        Some(Stmt::Assign { value, .. })
            if matches!(value.as_ref(), Expr::Call { name, args, .. } if name == "attacker" && args.is_empty())
    ));
}

#[test]
fn unknown_context_player_variable_keeps_a_structured_member_diagnostic() {
    let outcome = check(
        "playervar isWeaponBroken\n\
rule \"invalid player member\":
    @Event playerDied
    @Condition attacker.notDeclared
",
        "main.opy",
        Path::new(""),
    );
    let diagnostic = outcome
        .diagnostics
        .first()
        .expect("unknown player member diagnostic");
    assert_eq!(diagnostic.code, "unknown-member");
    assert_eq!(diagnostic.span.as_ref().expect("member span").start.line, 4);
    assert_eq!(diagnostic.span.as_ref().expect("member span").start.col, 25);
}
