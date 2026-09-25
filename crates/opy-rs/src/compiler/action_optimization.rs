//! Rewrites of single actions that the pinned OverPy applies while it writes
//! them, independent of `#!optimizeForSize`.

use workshop_rs::catalog::Kind;
use workshop_rs::{Action, Value};

use super::Compiler;
use super::operator_optimization::falsy;
use super::size_optimization::is_empty_string;

pub(super) struct ActionOptimizer<'a> {
    compiler: &'a Compiler,
    enabled: bool,
}

impl<'a> ActionOptimizer<'a> {
    pub(super) fn new(compiler: &'a Compiler, enabled: bool) -> Self {
        Self { compiler, enabled }
    }

    /// Rewrite an action in place. An action that does nothing, which the
    /// reference leaves out, is dropped before it gets here.
    pub(super) fn action(&self, action: &mut Action) {
        let Action::Call { name, args } = action else {
            return;
        };
        if self.enabled {
            match name.as_str() {
                "createHudText" => {
                    for text in args.iter_mut().skip(1).take(3) {
                        if is_empty_string(text) {
                            *text = Value::Null;
                        }
                    }
                }
                "startForcingOutlineFor" => {
                    if args.get(2).is_some_and(falsy) && args.len() > 3 {
                        args[3] = Value::Null;
                    }
                }
                "setUltCharge" => {
                    if let Some(charge) = args.get_mut(1)
                        && matches!(charge, Value::Number(number) if number.fract() != 0.0)
                    {
                        let number = std::mem::replace(charge, Value::Null);
                        *charge = Value::Call {
                            name: "absoluteValue".to_string(),
                            args: vec![number],
                        };
                    }
                }
                _ => {}
            }
        }
    }

    /// Wrap what a Boolean slot cannot hold; the reference does so last, after
    /// the size replacements have rewritten the argument.
    pub(super) fn wrap_booleans(&self, action: &mut Action) {
        if let Action::Call { name, args } = action {
            self.booleans(Kind::Action, name, args);
        }
    }

    /// A `Null` where a Boolean is expected is wrapped so that it reads as one.
    fn booleans(&self, kind: Kind, name: &str, args: &mut [Value]) {
        let entry = self.compiler.catalog.entry(kind, name);
        for (index, arg) in args.iter_mut().enumerate() {
            let boolean = entry.and_then(|entry| entry.param_type(index)) == Some("Boolean")
                || (kind, name, index) == (Kind::Action, "waitUntil", 0);
            if boolean && wraps_in_boolean(arg) {
                let mut inner = std::mem::replace(arg, Value::Null);
                self.nested(&mut inner);
                *arg = Value::Call {
                    name: "firstOf".to_string(),
                    args: vec![inner],
                };
            } else {
                self.nested(arg);
            }
        }
    }

    fn nested(&self, value: &mut Value) {
        match value {
            Value::Call { name, args } => self.booleans(Kind::Value, name, args),
            Value::Array(elements) => elements.iter_mut().for_each(|element| self.nested(element)),
            Value::Vector { x, y, z } => [x, y, z].into_iter().for_each(|part| self.nested(part)),
            Value::PlayerVariable { player, .. } => self.nested(player),
            _ => {}
        }
    }
}

/// Values the reference wraps in `First Of` where a Boolean is expected: `Null`,
/// a vector that is not a bare direction, some enum constants, and the results
/// of the builtins below. The sets are facts observed from the reference's
/// output by the builtin probe (source-policy.md); the reference writes every
/// other value, including strings, teams and direction constants, as it is.
fn wraps_in_boolean(value: &Value) -> bool {
    const ENUMS: [&str; 5] = ["Hero", "Map", "Color", "Button", "Gamemode"];
    const CALLS: [&str; 30] = [
        "abilityIconString",
        "allHeroes",
        "allPlayers",
        "allowedHeroes",
        "currentMap",
        "directionFromAngles",
        "directionTowards",
        "eventAbility",
        "getCurrentGamemode",
        "getEyePosition",
        "getFacingDirection",
        "getHero",
        "getHeroOfDuplication",
        "getLivingPlayers",
        "getPayloadPosition",
        "getPlayersInRadius",
        "getPlayersInSlot",
        "getPlayersOnHero",
        "getPlayersOnObjective",
        "getPosition",
        "getThrottle",
        "getVelocity",
        "heroIconString",
        "iconString",
        "inputBindingString",
        "localVector",
        "nearestWalkablePosition",
        "teamOf",
        "vectorTowards",
        "worldVector",
    ];
    let direction = |x: f64, y: f64, z: f64| {
        matches!(
            [x, y, z],
            [1.0, 0.0, 0.0]
                | [-1.0, 0.0, 0.0]
                | [0.0, 1.0, 0.0]
                | [0.0, -1.0, 0.0]
                | [0.0, 0.0, 1.0]
                | [0.0, 0.0, -1.0]
        )
    };
    match value {
        Value::Null => true,
        Value::Enum { value_type, .. } => ENUMS.contains(&value_type.as_str()),
        Value::Vector { x, y, z } => {
            !matches!((&**x, &**y, &**z), (Value::Number(x), Value::Number(y), Value::Number(z)) if direction(*x, *y, *z))
        }
        Value::Call { name, args } if name == "vector" => {
            !matches!(args.as_slice(), [Value::Number(x), Value::Number(y), Value::Number(z)] if direction(*x, *y, *z))
        }
        Value::Call { name, .. } => CALLS.contains(&name.as_str()),
        _ => false,
    }
}
