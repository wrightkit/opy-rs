//! The argument positions where pinned OverPy 9.7.10 rewrites a literal under
//! `#!optimizeForSize`, keyed by catalog call name and argument position.
//! `tools/overpy/upstream-literal-flags.json` records OverPy's own flags and
//! `slots_cover_the_upstream_flags` checks this policy against them.

/// How OverPy rewrites a literal in one argument position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    /// `0` becomes `False` and `1` becomes `True`.
    Boolean,
    /// Only `1` becomes `True`.
    TrueOnly,
    /// Only `0` becomes `False`.
    FalseOnly,
    /// `0` becomes `Null`.
    ZeroAsNull,
    /// A zero vector becomes `Null`.
    ZeroVectorAsNull,
}

pub(super) fn slot(name: &str, index: usize) -> Option<Slot> {
    Some(match (name, index) {
        ("startForcingSpawn", 1) => Slot::FalseOnly,
        ("add", 0 | 1) | ("divide", 0) | ("modulo", 0 | 1) | ("subtract", 1) => Slot::TrueOnly,
        ("attachTo", 2) | ("createDummyBot", 3 | 4) | ("createInWorldText", 2) => {
            Slot::ZeroVectorAsNull
        }
        ("appendToArray" | "arrayContains" | "indexOfArrayValue" | "removeFromArray", 1)
        | ("array", 0)
        | ("customString", 1..=3)
        | ("ifThenElse", 1 | 2)
        | ("bigMessage" | "smallMessage" | "setObjectiveDescription", 1)
        | ("createInWorldText", 1)
        | ("startForcingPosition" | "teleport", 1) => Slot::ZeroAsNull,
        _ if is_boolean_slot(name, index) => Slot::Boolean,
        _ => return None,
    })
}

fn is_boolean_slot(name: &str, index: usize) -> bool {
    match name {
        // Waits, skips and arithmetic.
        "wait"
        | "skip"
        | "setMatchTime"
        | "getObjectivePosition"
        | "getPlayersInSlot"
        | "isObjectiveComplete" => index == 0,
        "skipIf" | "charAt" | "valueInArray" | "addToScore" | "addToTeamScore" | "setScore"
        | "setTeamScore" | "destroyDummy" | "getAmmo" | "getMaxAmmo" => index == 1,
        "max" | "min" | "randomInteger" => index <= 1,
        "subtract" => index == 0,
        "slice" => matches!(index, 1 | 2),
        "vector" => index <= 2,
        // Chase destinations and rates.
        "chaseAtRate" | "chaseOverTime" => matches!(index, 1 | 2),
        // Player stats and abilities.
        "setAbilityCharge" | "setAbilityCooldown" | "setAbilityResource" => index == 2,
        "setAmmo" | "setMaxAmmo" => matches!(index, 1 | 2),
        "setGravity"
        | "setMoveSpeed"
        | "setProjectileGravity"
        | "setProjectileSpeed"
        | "setRespawnTime"
        | "setUltCharge"
        | "setWeapon"
        | "startModifyingVoicelinePitch"
        | "startScalingBarriers"
        | "startScalingSize" => index == 1,
        "setStatusEffect" => index == 3,
        "startForcingThrottle" => (1..=6).contains(&index),
        // Effects, HUD and dummies.
        "createDummyBot" => index == 2,
        "createEffect" => index == 4,
        "createHudText" => index == 5,
        "createInWorldText" => index == 3,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{Slot, slot};

    /// Every flag pinned OverPy sets is honored, and no position it leaves
    /// unflagged is rewritten.
    #[test]
    fn slots_cover_the_upstream_flags() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/overpy/upstream-literal-flags.json"
        );
        let upstream: Vec<(String, usize, Vec<String>)> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for (name, index, flags) in &upstream {
            let has = |flag: &str| flags.iter().any(|candidate| candidate == flag);
            let expected = match (
                has("ZERO_BY_FALSE"),
                has("ONE_BY_TRUE"),
                has("ZERO_BY_NULL"),
                has("NULL_VECTOR_BY_NULL"),
            ) {
                (true, true, ..) => Slot::Boolean,
                (false, true, ..) => Slot::TrueOnly,
                (true, false, ..) => Slot::FalseOnly,
                (_, _, true, _) => Slot::ZeroAsNull,
                (_, _, _, true) => Slot::ZeroVectorAsNull,
                _ => unreachable!("{name}:{index} carries no flag"),
            };
            assert_eq!(slot(name, *index), Some(expected), "{name}:{index}");
        }
        let listed = |name: &str, index: usize| {
            upstream
                .iter()
                .any(|(candidate, position, _)| candidate == name && *position == index)
        };
        for name in upstream.iter().map(|(name, ..)| name.as_str()) {
            for index in 0..8 {
                assert!(
                    listed(name, index) || slot(name, index).is_none(),
                    "{name}:{index} is rewritten but not flagged upstream"
                );
            }
        }
    }
}
