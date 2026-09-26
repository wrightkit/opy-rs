//! `#!optimizeForSize` literal replacements, applied to canonical values at the
//! Workshop boundary so the emitted structure converges on pinned OverPy.

use workshop_rs::catalog::{Kind, ParamCoercions};
use workshop_rs::{Action, ModifyOp, Value};

use self::literal_slots::{Slot, slot};
use super::Compiler;
use super::operator_optimization::falsy;

mod literal_slots;

pub(super) struct SizeOptimizer<'a> {
    compiler: &'a Compiler,
}

impl<'a> SizeOptimizer<'a> {
    pub(super) fn new(compiler: &'a Compiler) -> Self {
        Self { compiler }
    }

    pub(super) fn action(&self, action: &mut Action) {
        match action {
            Action::SetGlobalVariable { value, .. } | Action::SetPlayerVariable { value, .. } => {
                self.assigned(value);
            }
            Action::ModifyGlobalVariable { op, value, .. }
            | Action::ModifyPlayerVariable { op, value, .. } => self.modified(*op, value),
            Action::ForGlobalVariable { stop, step, .. }
            | Action::ForPlayerVariable { stop, step, .. } => {
                for bound in [stop, step] {
                    match bound {
                        Value::Number(zero) if *zero == 0.0 => *bound = Value::Bool(false),
                        Value::Number(one) if *one == 1.0 => *bound = Value::Bool(true),
                        _ => {}
                    }
                }
            }
            Action::Call { name, args } => {
                self.indexed_variable_call(name, args);
                self.chase_call(name, args);
                Self::hud_text_call(name, args);
                Self::beam_call(name, args);
                Self::progress_bar_call(name, args);
                self.call_arguments(Kind::Action, name, args);
            }
            _ => {}
        }
        for value in action_values(action) {
            self.nested(value);
        }
    }

    fn assigned(&self, value: &mut Value) {
        if matches!(value, Value::Number(zero) if *zero == 0.0) {
            *value = Value::Null;
        } else if is_zero_vector(value) {
            *value = self.zero_vector_sum();
        } else if is_empty_string(value) {
            *value = self.empty_string(false);
        }
    }

    /// An empty string is spelled as the first character of nothing, or as an
    /// empty array where the parameter accepts one.
    fn empty_string(&self, empty_array: bool) -> Value {
        if empty_array {
            return Value::Call {
                name: "emptyArray".to_string(),
                args: Vec::new(),
            };
        }
        Value::Call {
            name: "charAt".to_string(),
            args: vec![Value::String(String::new()), Value::Number(0.0)],
        }
    }

    fn modified(&self, op: ModifyOp, value: &mut Value) {
        let Value::Number(number) = value else {
            return;
        };
        match op {
            ModifyOp::Add
            | ModifyOp::Subtract
            | ModifyOp::Modulo
            | ModifyOp::Max
            | ModifyOp::Min
            | ModifyOp::RemoveFromArrayByIndex => {
                if *number == 0.0 {
                    *value = Value::Bool(false);
                } else if *number == 1.0 {
                    *value = Value::Bool(true);
                }
            }
            ModifyOp::AppendToArray | ModifyOp::RemoveFromArrayByValue if *number == 0.0 => {
                *value = Value::Null;
            }
            _ => {}
        }
    }

    fn indexed_variable_call(&self, name: &str, args: &mut [Value]) {
        let (index, value) = match name {
            "setGlobalVariableAtIndex" | "setPlayerVariableAtIndex" => (1, 2),
            "modifyGlobalVariableAtIndex" | "modifyPlayerVariableAtIndex" => (1, 3),
            _ => return,
        };
        if let Some(index) = args.get_mut(index) {
            match index {
                Value::Number(number) if *number == 0.0 => *index = Value::Bool(false),
                Value::Number(number) if *number == 1.0 => *index = Value::Bool(true),
                _ => {}
            }
        }
        let is_modify = name.starts_with("modify");
        let op = if is_modify {
            args.get(value - 1).and_then(modify_op_of)
        } else {
            None
        };
        if let Some(target) = args.get_mut(value) {
            match (is_modify, op) {
                (false, _) => self.assigned(target),
                (true, Some(op)) => self.modified(op, target),
                (true, None) => {}
            }
        }
    }

    /// An unused HUD text drops its color.
    fn hud_text_call(name: &str, args: &mut [Value]) {
        if name != "createHudText" {
            return;
        }
        for (text, color) in [(1, 6), (2, 7), (3, 8)] {
            if matches!(args.get(text), Some(Value::Null)) && args.len() > color {
                args[color] = Value::Null;
            }
        }
    }

    /// A grapple beam has no colour.
    fn beam_call(name: &str, args: &mut [Value]) {
        if name == "createBeamEffect"
            && matches!(args.get(1), Some(Value::Enum { value, .. }) if value == "GRAPPLE")
            && args.len() > 4
        {
            args[4] = Value::Null;
        }
    }

    /// A progress bar whose value or text is falsy drops the matching colour.
    fn progress_bar_call(name: &str, args: &mut [Value]) {
        let colors: [(usize, usize); 2] = match name {
            "progressBarHud" => [(1, 5), (2, 6)],
            "createProgressBarInWorldText" => [(1, 6), (2, 7)],
            _ => return,
        };
        for (source, color) in colors {
            if args.get(source).is_some_and(falsy) && args.len() > color {
                args[color] = Value::Null;
            }
        }
    }

    /// Chase destinations and rates spell `0` and `1` as `False` and `True`.
    fn chase_call(&self, name: &str, args: &mut [Value]) {
        let positions: &[usize] = match name {
            "chasePlayerVariableAtRate" | "chasePlayerVariableOverTime" => &[2, 3],
            _ => return,
        };
        for position in positions {
            if let Some(arg @ Value::Number(_)) = args.get_mut(*position) {
                match arg {
                    Value::Number(number) if *number == 0.0 => *arg = Value::Bool(false),
                    Value::Number(number) if *number == 1.0 => *arg = Value::Bool(true),
                    _ => {}
                }
            }
        }
    }

    fn call_arguments(&self, kind: Kind, name: &str, args: &mut [Value]) {
        let entry = self.compiler.catalog.entry(kind, name);
        for (index, arg) in args.iter_mut().enumerate() {
            let mut coercions = entry
                .and_then(|entry| entry.param_coercions(index))
                .copied()
                .unwrap_or_default();
            // The reference also writes an empty separator as an empty array.
            coercions.empty_array_as_string |= (name, index) == ("stringSplit", 1);
            self.argument(name, index, coercions, arg);
        }
    }

    /// Where OverPy replaces a literal is a per-parameter fact of its own
    /// argument tables, not the catalog's broader acceptance coercions.
    fn argument(&self, name: &str, index: usize, coercions: ParamCoercions, value: &mut Value) {
        // OverPy reads every element of an array, and every substitution of a
        // custom string, from the parameter of its first repeated position.
        let index = match name {
            "array" => 0,
            "customString" => 1,
            _ => index,
        };
        let slot = slot(name, index);
        if let Value::Number(number) = value {
            match (slot, *number) {
                (Some(Slot::Boolean | Slot::FalseOnly), 0.0) => *value = Value::Bool(false),
                (Some(Slot::ZeroAsNull), 0.0) => *value = Value::Null,
                (Some(Slot::Boolean | Slot::TrueOnly), 1.0) => *value = Value::Bool(true),
                _ => {}
            }
        } else if is_empty_string(value) {
            *value = self.empty_string(coercions.empty_array_as_string);
        } else if is_zero_vector(value) {
            *value = if slot == Some(Slot::ZeroVectorAsNull) {
                Value::Null
            } else {
                self.zero_vector_sum()
            };
        }
    }

    /// A rule condition is written as a comparison of its operands, which
    /// the reference does not treat as arguments: they only lose `0` and
    /// (beside an ordering) `1`, and their contents are optimized as usual.
    pub(super) fn condition(&self, value: &mut Value) {
        match value {
            Value::Call { name, args }
                if matches!(name.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=") =>
            {
                let ordering = matches!(name.as_str(), "<" | "<=" | ">" | ">=");
                for arg in args.iter_mut().take(2) {
                    match arg {
                        Value::Number(number) if *number == 0.0 => *arg = Value::Null,
                        Value::Number(number) if *number == 1.0 && ordering => {
                            *arg = Value::Bool(true);
                        }
                        _ => self.nested(arg),
                    }
                }
            }
            Value::Call { name, args } if name == "not" && args.len() == 1 => {
                self.nested(&mut args[0]);
            }
            other => self.nested(other),
        }
    }

    fn nested(&self, value: &mut Value) {
        match value {
            Value::Call { name, args } if name == "vector" && args.len() == 3 => {
                if let Some(form) = compact_vector(&args[0], &args[1], &args[2]) {
                    *value = form;
                    return self.nested(value);
                }
                self.call_arguments(Kind::Value, "vector", args);
                for arg in args.iter_mut() {
                    self.nested(arg);
                }
            }
            Value::Call { name, args } => {
                self.compared(name, args);
                self.call_arguments(Kind::Value, name, args);
                for arg in args {
                    self.nested(arg);
                }
            }
            Value::Array(elements) => {
                let coercions = self
                    .compiler
                    .catalog
                    .entry(Kind::Value, "array")
                    .and_then(|entry| entry.param_coercions(0))
                    .copied()
                    .unwrap_or_default();
                for element in elements {
                    self.argument("array", 0, coercions, element);
                    self.nested(element);
                }
            }
            Value::Vector { x, y, z } => {
                if let Some(form) = compact_vector(x, y, z) {
                    *value = form;
                    return self.nested(value);
                }
                for (index, component) in [&mut **x, &mut **y, &mut **z].into_iter().enumerate() {
                    self.argument("vector", index, ParamCoercions::default(), component);
                    self.nested(component);
                }
            }
            Value::PlayerVariable { player, .. } => self.nested(player),
            _ => {}
        }
    }

    fn compared(&self, name: &str, args: &mut [Value]) {
        if !matches!(name, "<" | "<=" | ">" | ">=" | "==" | "!=") {
            return;
        }
        for arg in args.iter_mut().take(2) {
            if matches!(arg, Value::Number(number) if *number == 0.0) {
                *arg = Value::Null;
            }
        }
    }

    fn zero_vector_sum(&self) -> Value {
        Value::Call {
            name: "add".to_string(),
            args: vec![
                Value::Enum {
                    value_type: "Vector".to_string(),
                    value: "LEFT".to_string(),
                },
                Value::Enum {
                    value_type: "Vector".to_string(),
                    value: "RIGHT".to_string(),
                },
            ],
        }
    }
}

fn direction(name: &str) -> Value {
    Value::Enum {
        value_type: "Vector".to_string(),
        value: name.to_string(),
    }
}

fn number_of(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => Some(*number),
        _ => None,
    }
}

/// The shorter spellings of a vector: the sum of two directions, or one
/// component scaling its direction.
fn compact_vector(x: &Value, y: &Value, z: &Value) -> Option<Value> {
    let (nx, ny, nz) = (number_of(x), number_of(y), number_of(z));
    if (nx, ny, nz) == (Some(0.0), Some(0.0), Some(0.0)) {
        return None;
    }
    if let (Some(nx), Some(ny), Some(nz)) = (nx, ny, nz) {
        let pair = match (nx, ny, nz) {
            (1.0, 1.0, 0.0) => Some(("LEFT", "UP")),
            (1.0, -1.0, 0.0) => Some(("LEFT", "DOWN")),
            (1.0, 0.0, 1.0) => Some(("LEFT", "FORWARD")),
            (1.0, 0.0, -1.0) => Some(("LEFT", "BACKWARD")),
            (-1.0, 1.0, 0.0) => Some(("RIGHT", "UP")),
            (-1.0, -1.0, 0.0) => Some(("RIGHT", "DOWN")),
            (-1.0, 0.0, 1.0) => Some(("RIGHT", "FORWARD")),
            (-1.0, 0.0, -1.0) => Some(("RIGHT", "BACKWARD")),
            (0.0, 1.0, 1.0) => Some(("UP", "FORWARD")),
            (0.0, 1.0, -1.0) => Some(("UP", "BACKWARD")),
            (0.0, -1.0, 1.0) => Some(("DOWN", "FORWARD")),
            (0.0, -1.0, -1.0) => Some(("DOWN", "BACKWARD")),
            _ => None,
        };
        if let Some((first, second)) = pair {
            return Some(Value::Call {
                name: "add".to_string(),
                args: vec![direction(first), direction(second)],
            });
        }
    }
    let scaled = |component: &Value, axis: &str| Value::Call {
        name: "multiply".to_string(),
        args: vec![component.clone(), direction(axis)],
    };
    if ny == Some(0.0) && nz == Some(0.0) {
        Some(scaled(x, "LEFT"))
    } else if nx == Some(0.0) && nz == Some(0.0) {
        Some(scaled(y, "UP"))
    } else if nx == Some(0.0) && ny == Some(0.0) {
        Some(scaled(z, "FORWARD"))
    } else {
        None
    }
}

pub(super) fn is_empty_string(value: &Value) -> bool {
    match value {
        Value::String(text) => text.is_empty(),
        Value::Call { name, args } => {
            name == "customString"
                && args.len() == 1
                && matches!(&args[0], Value::String(text) if text.is_empty())
        }
        _ => false,
    }
}

fn is_zero_vector(value: &Value) -> bool {
    let is_zero = |value: &Value| matches!(value, Value::Number(number) if *number == 0.0);
    match value {
        Value::Vector { x, y, z } => is_zero(x) && is_zero(y) && is_zero(z),
        Value::Call { name, args } => {
            name == "vector" && args.len() == 3 && args.iter().all(is_zero)
        }
        _ => false,
    }
}

fn modify_op_of(value: &Value) -> Option<ModifyOp> {
    let operation = match value {
        Value::Enum { value, .. } => value.as_str(),
        Value::Call { name, args } if args.is_empty() => name.as_str(),
        _ => return None,
    };
    Some(match operation {
        "ADD" | "add" => ModifyOp::Add,
        "SUBTRACT" | "subtract" => ModifyOp::Subtract,
        "MODULO" | "modulo" => ModifyOp::Modulo,
        "MAX" | "max" => ModifyOp::Max,
        "MIN" | "min" => ModifyOp::Min,
        "REMOVE_FROM_ARRAY_BY_INDEX" | "removeFromArrayByIndex" => ModifyOp::RemoveFromArrayByIndex,
        "APPEND_TO_ARRAY" | "appendToArray" => ModifyOp::AppendToArray,
        "REMOVE_FROM_ARRAY_BY_VALUE" | "removeFromArrayByValue" => ModifyOp::RemoveFromArrayByValue,
        _ => return None,
    })
}

pub(super) fn action_values(action: &mut Action) -> Vec<&mut Value> {
    match action {
        Action::SetGlobalVariable { value, .. } | Action::ModifyGlobalVariable { value, .. } => {
            vec![value]
        }
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => vec![player, value],
        Action::If { condition } | Action::ElseIf { condition } | Action::While { condition } => {
            vec![condition]
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => vec![start, stop, step],
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => vec![player, start, stop, step],
        Action::Call { args, .. } => args.iter_mut().collect(),
        _ => Vec::new(),
    }
}
