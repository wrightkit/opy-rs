use std::path::Path;

use crate::Compiler;
use workshop_rs::{Action, Value};

#[test]
fn nested_player_collection_deletion_uses_the_canonical_indexed_action() {
    let source = r#"
playervar values
globalvar index
rule "delete nested player value":
    @Event eachPlayer
    del eventPlayer.values[index][0]
"#;
    let hir = crate::compile(source, "nested-player-delete.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    let Action::Call { name, args } = &artifact.wir.rules[0].actions[0] else {
        panic!("nested deletion must lower to an indexed player-variable action");
    };
    assert_eq!(name, "modifyPlayerVariableAtIndex");
    let [
        Value::PlayerVariable { player, variable },
        Value::GlobalVariable(outer_index),
        Value::Call {
            name: operation, ..
        },
        Value::Number(inner_index),
    ] = args.as_slice()
    else {
        panic!("nested deletion must preserve its variable, indices, and operation");
    };
    assert!(matches!(player.as_ref(), Value::EventPlayer));
    assert_eq!(variable, "values");
    assert_eq!(outer_index, "index");
    assert_eq!(operation, "removeFromArrayByIndex");
    assert_eq!(*inner_index, 0.0);
}

#[test]
fn indexed_assignment_indices_keep_the_authored_boolean_spelling() {
    let source = r#"
globalvar values = [0]
globalvar nested = [[0]]

rule "boolean indices":
    @Event global
    values[true] = 1
    values[false] += 1
    nested[true][false] = 2
"#;
    let hir = crate::compile(source, "indexed-assignment-indices.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable At Index(values, True, 1);")
    );
    assert!(
        artifact
            .emitted
            .contains("Modify Global Variable At Index(values, False, Add, 1);")
    );
    assert!(
        artifact
            .emitted
            .contains("Set Global Variable At Index(nested, True,")
    );
}

#[test]
fn indexed_receiver_mutations_update_only_the_selected_variable_slot() {
    let source = r#"
globalvar values
playervar slots

rule "indexed receiver mutations":
    @Event eachPlayer
    values[eventPlayer.getSlot()].append(getCurrentMap())
    eventPlayer.slots[eventPlayer.getSlot()].remove(eventPlayer.getHero())
"#;
    let hir = crate::compile(source, "indexed-receiver-mutations.opy", Path::new(".")).unwrap();
    let artifact = Compiler::new().unwrap().compile_hir(&hir).unwrap();

    let calls = artifact
        .wir
        .rules
        .iter()
        .flat_map(|rule| rule.actions.iter())
        .filter_map(|action| match action {
            Action::Call { name, args } => Some((name.as_str(), args.as_slice())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(calls.iter().any(|(name, args)| {
        *name == "modifyGlobalVariableAtIndex"
            && matches!(
                *args,
                [
                    Value::GlobalVariable(variable),
                    Value::Call { name: index, .. },
                    Value::Call { name: operation, .. },
                    Value::Call { name: value, .. },
                ] if variable == "values"
                    && index == "getSlot"
                    && operation == "appendToArray"
                    && value == "currentMap"
            )
    }));
    assert!(calls.iter().any(|(name, args)| {
        *name == "modifyPlayerVariableAtIndex"
            && matches!(
                *args,
                [
                    Value::PlayerVariable { player, variable },
                    Value::Call { name: index, .. },
                    Value::Call { name: operation, .. },
                    Value::Call { name: value, .. },
                ] if matches!(player.as_ref(), Value::EventPlayer)
                    && variable == "slots"
                    && index == "getSlot"
                    && operation == "removeFromArray"
                    && value == "getHero"
            )
    }));

    assert_eq!(artifact.wir.action_span(0, 0).unwrap().start.line, 7);
    assert_eq!(artifact.wir.action_span(0, 1).unwrap().start.line, 8);
    for (action, line) in [(0, 7), (1, 8)] {
        for argument in [0, 1, 3] {
            assert_eq!(
                artifact
                    .wir
                    .action_argument_span(0, action, argument)
                    .unwrap()
                    .start
                    .line,
                line
            );
        }
        assert!(artifact.wir.action_argument_span(0, action, 2).is_none());
    }
}
