//! `#!optimizeForSize` literal replacements, applied to canonical values at the
//! Workshop boundary so the emitted structure converges on pinned OverPy.

use workshop_rs::catalog::{Kind, ParamCoercions};
use workshop_rs::{Action, ModifyOp, Value};

use super::Compiler;

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
            Action::ForGlobalVariable { step, .. } | Action::ForPlayerVariable { step, .. } => {
                if matches!(step, Value::Number(one) if *one == 1.0) {
                    *step = Value::Bool(true);
                }
            }
            Action::Call { name, args } => {
                self.indexed_variable_call(name, args);
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

    fn call_arguments(&self, kind: Kind, name: &str, args: &mut [Value]) {
        let entry = self.compiler.catalog.entry(kind, name);
        for (index, arg) in args.iter_mut().enumerate() {
            let coercions = entry
                .and_then(|entry| entry.param_coercions(index))
                .copied()
                .unwrap_or_default();
            self.argument(coercions, arg);
        }
    }

    fn argument(&self, coercions: ParamCoercions, value: &mut Value) {
        if is_zero_vector(value) {
            *value = if coercions.null_vector_as_null {
                Value::Null
            } else {
                self.zero_vector_sum()
            };
        }
    }

    /// A rule condition also replaces `1` beside an ordering comparison.
    pub(super) fn condition(&self, value: &mut Value) {
        if let Value::Call { name, args } = value {
            if matches!(name.as_str(), "<" | "<=" | ">" | ">=") {
                for arg in args.iter_mut().take(2) {
                    if matches!(arg, Value::Number(number) if *number == 1.0) {
                        *arg = Value::Bool(true);
                    }
                }
            }
        }
        self.nested(value);
    }

    fn nested(&self, value: &mut Value) {
        match value {
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
                    self.argument(coercions, element);
                    self.nested(element);
                }
            }
            Value::Vector { x, y, z } => {
                self.nested(x);
                self.nested(y);
                self.nested(z);
                if let Some(form) = compact_vector(x, y, z) {
                    *value = form;
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
    let Value::Enum { value, .. } = value else {
        return None;
    };
    Some(match value.as_str() {
        "ADD" => ModifyOp::Add,
        "SUBTRACT" => ModifyOp::Subtract,
        "MODULO" => ModifyOp::Modulo,
        "MAX" => ModifyOp::Max,
        "MIN" => ModifyOp::Min,
        "REMOVE_FROM_ARRAY_BY_INDEX" => ModifyOp::RemoveFromArrayByIndex,
        "APPEND_TO_ARRAY" => ModifyOp::AppendToArray,
        "REMOVE_FROM_ARRAY_BY_VALUE" => ModifyOp::RemoveFromArrayByValue,
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
