//! Shared compile-time expression folding for OPY values.
//!
//! Settings use this same HIR expression path as ordinary OPY lowering. The
//! evaluator is deliberately conservative: expressions that still depend on
//! runtime values are left for normal lowering, while settings require a
//! complete primitive result.

use std::collections::HashMap;

use crate::hir::Expr;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Number(f64),
    String(String),
    Bool(bool),
    Array(Vec<Value>),
    Object(Vec<(Value, Value)>),
}

pub(crate) fn evaluate(
    expression: &Expr,
    constants: &HashMap<String, &Expr>,
    bindings: &HashMap<String, Value>,
    stack: &mut Vec<String>,
) -> Option<Value> {
    eval(expression, constants, bindings, stack, false)
}

/// Evaluation as the pinned OverPy performs it inside `settings` values: the
/// same constant folding as [`evaluate`], except format expressions and
/// string concatenation are rejected and `==`/`!=` does not fold a Boolean
/// against a non-Boolean — the reference evaluator admits literals,
/// arithmetic, comparisons, `and`/`or`/`not`, membership (`in`), indexing,
/// and the constant-foldable builtins (#512).
pub(crate) fn evaluate_settings(
    expression: &Expr,
    constants: &HashMap<String, &Expr>,
    bindings: &HashMap<String, Value>,
    stack: &mut Vec<String>,
) -> Option<Value> {
    eval(expression, constants, bindings, stack, true)
}

fn eval(
    expression: &Expr,
    constants: &HashMap<String, &Expr>,
    bindings: &HashMap<String, Value>,
    stack: &mut Vec<String>,
    settings: bool,
) -> Option<Value> {
    match expression {
        Expr::Number { value, .. } => Some(Value::Number(*value)),
        Expr::String { value, .. } => Some(Value::String(value.clone())),
        Expr::Bool { value, .. } => Some(Value::Bool(*value)),
        Expr::Array { elements, .. } => Some(Value::Array(
            elements
                .iter()
                .map(|element| eval(element, constants, bindings, stack, settings))
                .collect::<Option<Vec<_>>>()?,
        )),
        Expr::Dict { entries, .. } => Some(Value::Object(
            entries
                .iter()
                .map(|entry| {
                    Some((
                        eval(&entry.key, constants, bindings, stack, settings)?,
                        eval(&entry.value, constants, bindings, stack, settings)?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?,
        )),
        Expr::Constant { name, .. } => {
            if let Some(value) = bindings.get(name) {
                return Some(value.clone());
            }
            let value = constants.get(name)?;
            if stack.iter().any(|active| active == name) {
                return None;
            }
            stack.push(name.clone());
            let result = eval(value, constants, bindings, stack, settings);
            stack.pop();
            result
        }
        Expr::Format { text, args, .. } => {
            if settings {
                return None;
            }
            let values = args
                .iter()
                .map(|arg| eval(arg, constants, bindings, stack, settings))
                .collect::<Option<Vec<_>>>()?;
            let mut result = text.clone();
            for (index, value) in values.into_iter().enumerate() {
                result = result.replace(&format!("{{{index}}}"), &display(value)?);
            }
            Some(Value::String(result))
        }
        Expr::Binary {
            op, left, right, ..
        } => evaluate_binary(
            op,
            eval(left, constants, bindings, stack, settings)?,
            eval(right, constants, bindings, stack, settings)?,
            settings,
        ),
        Expr::Conditional {
            then_value,
            condition,
            else_value,
            ..
        } => {
            if truthy(&eval(condition, constants, bindings, stack, settings)?)? {
                eval(then_value, constants, bindings, stack, settings)
            } else {
                eval(else_value, constants, bindings, stack, settings)
            }
        }
        Expr::Unary { op, operand, .. } => {
            let operand = eval(operand, constants, bindings, stack, settings)?;
            match (op.as_str(), operand) {
                ("-", Value::Number(value)) => Some(Value::Number(-value)),
                ("+", Value::Number(value)) => Some(Value::Number(value)),
                // The reference's truthiness applies `not` to scalars and
                // arrays, not objects (#512).
                ("not", Value::Object(_)) => None,
                ("not", value) => truthy(&value).map(|value| Value::Bool(!value)),
                _ => None,
            }
        }
        Expr::Index { array, index, .. } => evaluate_index(
            eval(array, constants, bindings, stack, settings)?,
            eval(index, constants, bindings, stack, settings)?,
        ),
        Expr::Call { name, args, .. } => {
            let values = args
                .iter()
                .map(|arg| eval(arg, constants, bindings, stack, settings))
                .collect::<Option<Vec<_>>>()?;
            evaluate_builtin(name, &values)
        }
        _ => None,
    }
}

fn evaluate_builtin(name: &str, values: &[Value]) -> Option<Value> {
    match name {
        "len" | "countOf" => match values {
            [Value::Array(values)] => Some(Value::Number(values.len() as f64)),
            _ => None,
        },
        "sqrt" => match values {
            [Value::Number(value)] => Some(Value::Number(value.sqrt())),
            _ => None,
        },
        "round" => match values {
            [Value::Number(value)] => Some(Value::Number(round_half_up(*value))),
            _ => None,
        },
        "abs" => match values {
            [Value::Number(value)] => Some(Value::Number(value.abs())),
            _ => None,
        },
        "min" | "max" => {
            let mut numbers = values.iter().map(|value| match value {
                Value::Number(value) => Some(*value),
                _ => None,
            });
            let mut result = numbers.next()??;
            for value in numbers {
                let value = value?;
                result = if name == "min" {
                    result.min(value)
                } else {
                    result.max(value)
                };
            }
            Some(Value::Number(result))
        }
        _ => None,
    }
}

fn evaluate_binary(op: &str, left: Value, right: Value, settings: bool) -> Option<Value> {
    match (op, left, right) {
        ("+", Value::Number(left), Value::Number(right)) => Some(Value::Number(left + right)),
        // The settings evaluator has no string `+`; ordinary constant
        // folding keeps it for format-style sources.
        ("+", Value::String(left), Value::String(right)) if !settings => {
            Some(Value::String(left + &right))
        }
        ("-", Value::Number(left), Value::Number(right)) => Some(Value::Number(left - right)),
        ("*", Value::Number(left), Value::Number(right)) => Some(Value::Number(left * right)),
        ("/", Value::Number(left), Value::Number(right)) if right != 0.0 => {
            Some(Value::Number(left / right))
        }
        ("%", Value::Number(left), Value::Number(right)) if right != 0.0 => {
            Some(Value::Number(left % right))
        }
        ("**", Value::Number(left), Value::Number(right)) => Some(Value::Number(left.powf(right))),
        ("<", Value::Number(left), Value::Number(right)) => Some(Value::Bool(left < right)),
        (">", Value::Number(left), Value::Number(right)) => Some(Value::Bool(left > right)),
        ("<=", Value::Number(left), Value::Number(right)) => Some(Value::Bool(left <= right)),
        (">=", Value::Number(left), Value::Number(right)) => Some(Value::Bool(left >= right)),
        // `and`/`or` return an operand like the reference's settings
        // evaluator (`1 and 2` emits `2`, `0 or 3` emits `3`), not a
        // Boolean coercion (#512).
        ("and", left, right) => Some(if truthy(&left)? { right } else { left }),
        ("or", left, right) => Some(if truthy(&left)? { left } else { right }),
        // `==`/`!=` on a Boolean and a non-Boolean does not fold in the
        // reference (`1 == true` and `"a" == true` are errors there; #512).
        ("==" | "!=", left, right)
            if settings && matches!(left, Value::Bool(_)) != matches!(right, Value::Bool(_)) =>
        {
            None
        }
        ("==", left, right) => Some(Value::Bool(left == right)),
        ("!=", left, right) => Some(Value::Bool(left != right)),
        // The reference's settings evaluator tests array membership, not
        // JavaScript index existence.
        ("in", left, Value::Array(values)) => Some(Value::Bool(values.contains(&left))),
        _ => None,
    }
}

fn truthy(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(value) => Some(*value != 0.0),
        Value::String(value) => Some(!value.is_empty()),
        Value::Array(values) => Some(!values.is_empty()),
        // The reference's `and`/`or` truthiness treats objects as falsy.
        Value::Object(_) => Some(false),
    }
}

fn evaluate_index(collection: Value, index: Value) -> Option<Value> {
    match collection {
        Value::Array(values) => {
            let Value::Number(index) = index else {
                return None;
            };
            if index.fract() != 0.0 || index < 0.0 {
                return None;
            }
            values.into_iter().nth(index as usize)
        }
        Value::Object(entries) => entries
            .into_iter()
            .find(|(key, _)| *key == index)
            .map(|(_, value)| value),
        _ => None,
    }
}

pub(crate) fn display(value: Value) -> Option<String> {
    match value {
        Value::Number(value) if value.is_finite() => Some(workshop_number_text(value)),
        Value::String(value) => Some(value),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

/// Numbers substituted into text keep two decimals, as Workshop displays
/// them: `2.5` becomes `2.50`, and a value that rounds to an integer drops
/// its fraction.
/// Rounds halves toward positive infinity, as the pinned reference does.
pub(crate) fn round_half_up(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

fn workshop_number_text(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded:.2}")
    }
}
