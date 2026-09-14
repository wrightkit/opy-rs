//! Variable initializer lowering coverage.

use std::path::Path;

use crate::Compiler;

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
