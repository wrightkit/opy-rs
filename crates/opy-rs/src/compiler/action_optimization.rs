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
        self.booleans(Kind::Action, name, args);
    }

    /// A `Null` where a Boolean is expected is wrapped so that it reads as one.
    fn booleans(&self, kind: Kind, name: &str, args: &mut [Value]) {
        let entry = self.compiler.catalog.entry(kind, name);
        for (index, arg) in args.iter_mut().enumerate() {
            let boolean = entry.and_then(|entry| entry.param_type(index)) == Some("Boolean")
                || (kind, name, index) == (Kind::Action, "waitUntil", 0);
            if boolean && matches!(arg, Value::Null) {
                *arg = Value::Call {
                    name: "firstOf".to_string(),
                    args: vec![Value::Null],
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
