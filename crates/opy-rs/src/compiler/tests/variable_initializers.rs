use std::path::Path;

use crate::Compiler;

#[test]
fn global_initializers_reject_event_player_inside_expressions() {
    for value in [
        "eventPlayer",
        "eventPlayer()",
        "[eventPlayer]",
        "eventPlayer.getPosition()",
    ] {
        let source = format!("globalvar target = {value}\n");
        let error = crate::compile(&source, "initializer.opy", Path::new(".")).unwrap_err();
        assert_eq!(error.code, "rule-event-gate", "{value}");
        assert_eq!(error.span.unwrap().start.line, 1, "{value}");
    }
}

#[test]
fn player_initializers_keep_event_player_context() {
    let source = "playervar target = eventPlayer\n";
    let hir = crate::compile(source, "initializer.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(matches!(
        artifact.wir.rules[0].actions.first(),
        Some(workshop_rs::Action::SetPlayerVariable { player, value, .. })
            if matches!(player, workshop_rs::Value::EventPlayer)
                && matches!(value, workshop_rs::Value::EventPlayer)
    ));
}

#[test]
fn explicit_null_initializers_are_emitted_for_global_and_player_variables() {
    let source = r#"
globalvar gamblerHeartsteelJackpot = null
globalvar targetPlayerIndex = null
globalvar integer_zero = 0
playervar player_null = null
playervar player_zero = 0

rule "initializer contract":
    @Event global
    pass
"#;
    let hir = crate::compile(source, "initializer-contract.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(matches!(
        artifact.wir.rules[0].actions.first(),
        Some(workshop_rs::Action::SetGlobalVariable { variable, value })
            if variable == "gamblerHeartsteelJackpot" && matches!(value, workshop_rs::Value::Null)
    ));
    assert!(matches!(
        artifact.wir.rules[0].actions.get(1),
        Some(workshop_rs::Action::SetGlobalVariable { variable, value })
            if variable == "targetPlayerIndex" && matches!(value, workshop_rs::Value::Null)
    ));
    assert_eq!(artifact.wir.rules[0].actions.len(), 2);
    assert!(matches!(
        artifact.wir.rules[1].actions.first(),
        Some(workshop_rs::Action::SetPlayerVariable { variable, value, .. })
            if variable == "player_null" && matches!(value, workshop_rs::Value::Null)
    ));
    assert_eq!(artifact.wir.rules[1].actions.len(), 1);
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(gamblerHeartsteelJackpot, Null);")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(targetPlayerIndex, Null);")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Player Variable(Event Player, player_null, Null);")
    );

    assert_eq!(artifact.wir.action_span(0, 0).unwrap().start.line, 2);
    assert_eq!(
        artifact
            .wir
            .action_argument_span(0, 0, 0)
            .unwrap()
            .start
            .line,
        2
    );
    assert_eq!(artifact.wir.action_span(0, 1).unwrap().start.line, 3);
    assert_eq!(
        artifact
            .wir
            .action_argument_span(0, 1, 0)
            .unwrap()
            .start
            .line,
        3
    );
    assert_eq!(artifact.wir.action_span(1, 0).unwrap().start.line, 5);
    assert_eq!(
        artifact
            .wir
            .action_argument_span(1, 0, 1)
            .unwrap()
            .start
            .line,
        5
    );
}

#[test]
fn size_directive_does_not_rewrite_variable_initializers() {
    let source = r#"#!optimizeForSize

globalvar g = ""
globalvar v = vect(0, 0, 0)
globalvar n
playervar ps = ""
playervar pv = vect(0, 0, 0)

rule "r":
    @Event global
    n = 0
    g = ""
    v = vect(0, 0, 0)
"#;
    let hir = crate::compile(source, "size-initializers.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(g, Custom String(\"\"));")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(v, Vector(0, 0, 0));")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Player Variable(Event Player, ps, Custom String(\"\"));")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Player Variable(Event Player, pv, Vector(0, 0, 0));")
    );
    assert!(artifact.emitted.contains("Set Global Variable(n, Null);"));
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(g, Char In String(Empty Array, False));")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable(v, Add(Left, Right));")
    );
}
