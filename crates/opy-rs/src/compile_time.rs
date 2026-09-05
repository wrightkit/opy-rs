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
    match expression {
        Expr::Number { value, .. } => Some(Value::Number(*value)),
        Expr::String { value, .. } => Some(Value::String(value.clone())),
        Expr::Bool { value, .. } => Some(Value::Bool(*value)),
        Expr::Array { elements, .. } => Some(Value::Array(
            elements
                .iter()
                .map(|element| evaluate(element, constants, bindings, stack))
                .collect::<Option<Vec<_>>>()?,
        )),
        Expr::Dict { entries, .. } => Some(Value::Object(
            entries
                .iter()
                .map(|entry| {
                    Some((
                        evaluate(&entry.key, constants, bindings, stack)?,
                        evaluate(&entry.value, constants, bindings, stack)?,
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
            let result = evaluate(value, constants, bindings, stack);
            stack.pop();
            result
        }
        Expr::Format { text, args, .. } => {
            let values = args
                .iter()
                .map(|arg| evaluate(arg, constants, bindings, stack))
                .collect::<Option<Vec<_>>>()?;
            let mut result = text.clone();
            for (index, value) in values.iter().enumerate() {
                result = result.replace(&format!("{{{index}}}"), &display(value)?);
            }
            Some(Value::String(result))
        }
        Expr::Binary {
            op, left, right, ..
        } => evaluate_binary(
            op,
            evaluate(left, constants, bindings, stack)?,
            evaluate(right, constants, bindings, stack)?,
        ),
        Expr::Conditional {
            then_value,
            condition,
            else_value,
            ..
        } => {
            if truthy(&evaluate(condition, constants, bindings, stack)?)? {
                evaluate(then_value, constants, bindings, stack)
            } else {
                evaluate(else_value, constants, bindings, stack)
            }
        }
        Expr::Unary { op, operand, .. } => {
            match (op.as_str(), evaluate(operand, constants, bindings, stack)?) {
                ("-", Value::Number(value)) => Some(Value::Number(-value)),
                ("not", Value::Bool(value)) => Some(Value::Bool(!value)),
                _ => None,
            }
        }
        Expr::Index { array, index, .. } => evaluate_index(
            evaluate(array, constants, bindings, stack)?,
            evaluate(index, constants, bindings, stack)?,
        ),
        Expr::Call { name, args, .. } => {
            let values = args
                .iter()
                .map(|arg| evaluate(arg, constants, bindings, stack))
                .collect::<Option<Vec<_>>>()?;
            evaluate_builtin(name, &values)
        }
        _ => None,
    }
}

fn evaluate_builtin(name: &str, values: &[Value]) -> Option<Value> {
    match name {
        "sqrt" => match values {
            [Value::Number(value)] => Some(Value::Number(value.sqrt())),
            _ => None,
        },
        "round" => match values {
            [Value::Number(value)] => Some(Value::Number(value.round())),
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

fn evaluate_binary(op: &str, left: Value, right: Value) -> Option<Value> {
    match (op, left, right) {
        ("+", Value::Number(left), Value::Number(right)) => Some(Value::Number(left + right)),
        ("+", Value::String(left), Value::String(right)) => Some(Value::String(left + &right)),
        ("-", Value::Number(left), Value::Number(right)) => Some(Value::Number(left - right)),
        ("*", Value::Number(left), Value::Number(right)) => Some(Value::Number(left * right)),
        ("/", Value::Number(left), Value::Number(right)) if right != 0.0 => {
            Some(Value::Number(left / right))
        }
        ("%", Value::Number(left), Value::Number(right)) if right != 0.0 => {
            Some(Value::Number(left % right))
        }
        ("**", Value::Number(left), Value::Number(right)) => Some(Value::Number(left.powf(right))),
        ("and", Value::Bool(left), Value::Bool(right)) => Some(Value::Bool(left && right)),
        ("or", Value::Bool(left), Value::Bool(right)) => Some(Value::Bool(left || right)),
        ("==", left, right) => Some(Value::Bool(left == right)),
        ("!=", left, right) => Some(Value::Bool(left != right)),
        _ => None,
    }
}

fn truthy(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(value) => Some(*value != 0.0),
        Value::String(value) => Some(!value.is_empty()),
        _ => None,
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

fn display(value: &Value) -> Option<String> {
    match value {
        Value::Number(value) if value.is_finite() => Some(value.to_string()),
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}
