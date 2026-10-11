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
    /// A `vect(x, y, z)` value: only `distance`/`magnitude`-family folds
    /// and the `.x`/`.y`/`.z` component reads consume it (#512).
    Vector(Vec<Value>),
}

pub(crate) fn evaluate(
    expression: &Expr,
    constants: &HashMap<String, &Expr>,
    bindings: &HashMap<String, Value>,
    stack: &mut Vec<String>,
) -> Option<Value> {
    eval(expression, constants, bindings, stack, false)
}

/// `NUMBER_LIMIT` of the pinned optimizer: arithmetic folds only when the
/// result's magnitude stays within it, so `NaN` and `Infinity` arithmetic
/// stays unevaluated (and is rejected by the settings object walk) while
/// identity rules still pass non-finite operands through (#512).
const NUMBER_LIMIT: f64 = 10_000_000.0;

/// Evaluation as the pinned OverPy performs it inside `settings` values: the
/// same constant folding as [`evaluate`], except string concatenation is
/// rejected — the reference evaluator admits literals, arithmetic,
/// comparisons, `and`/`or`/`not`, membership (`in`), indexing, format
/// expressions, and the constant-foldable builtins (#512).
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
        Expr::Number { value, text, .. } => {
            // The pinned tokenizer splits `1e-7` into `1e`, `-`, `7` — the
            // sign ends the word token — so it is an unsupported `-`/`+`
            // operation there, not one number token. This lexer does read
            // the signed exponent, and settings evaluation still follows
            // the pinned split by refusing it (#512).
            if settings
                && ["e-", "e+", "E-", "E+"]
                    .iter()
                    .any(|mark| text.contains(mark))
            {
                return None;
            }
            Some(Value::Number(*value))
        }
        Expr::String { value, .. } => Some(Value::String(value.clone())),
        Expr::Bool { value, .. } => Some(Value::Bool(*value)),
        Expr::Array { elements, .. } => Some(Value::Array(
            elements
                .iter()
                .map(|element| eval(element, constants, bindings, stack, settings))
                .collect::<Option<Vec<_>>>()?,
        )),
        Expr::Vector { x, y, z, .. } => Some(Value::Vector(vec![
            eval(x, constants, bindings, stack, settings)?,
            eval(y, constants, bindings, stack, settings)?,
            eval(z, constants, bindings, stack, settings)?,
        ])),
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
            let Some(&value) = constants.get(name) else {
                // The pinned settings evaluator resolves bare `Infinity` as
                // the JavaScript global; `NaN`, `null`, and `undefined`
                // remain unresolved and error (#512).
                return (settings && name == "Infinity").then_some(Value::Number(f64::INFINITY));
            };
            if stack.iter().any(|active| active == name) {
                return None;
            }
            stack.push(name.clone());
            let result = eval(value, constants, bindings, stack, settings);
            stack.pop();
            result
        }
        Expr::Format { text, args, .. } => {
            let values = args
                .iter()
                .map(|arg| eval(arg, constants, bindings, stack, settings))
                .collect::<Option<Vec<_>>>()?;
            evaluate_format(text, &values)
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
            // The pinned compiler resolves every operand before folding, so
            // an unresolvable untaken branch still fails the expression.
            let condition = eval(condition, constants, bindings, stack, settings)?;
            let then_value = eval(then_value, constants, bindings, stack, settings)?;
            let else_value = eval(else_value, constants, bindings, stack, settings)?;
            // The settings evaluator's conditional has no fold for a
            // condition it cannot call truthy or falsy — `1 if {} else 2`
            // and `1 if [] else 2` error upstream (#512).
            if matches!(condition, Value::Object(_))
                || matches!(&condition, Value::Array(values) if values.is_empty())
            {
                return None;
            }
            match truthiness(&condition) {
                Some(false) => Some(else_value),
                Some(true) | None => Some(then_value),
            }
        }
        Expr::Unary { op, operand, .. } => {
            let operand = eval(operand, constants, bindings, stack, settings)?;
            match (op.as_str(), operand) {
                ("-", Value::Number(value)) => Some(Value::Number(-value)),
                // Unary `+` passes its operand through unchanged.
                ("+", value) => Some(value),
                ("not", value) => truthiness(&value).map(|value| Value::Bool(!value)),
                _ => None,
            }
        }
        Expr::Index { array, index, .. } => evaluate_index(
            eval(array, constants, bindings, stack, settings)?,
            eval(index, constants, bindings, stack, settings)?,
        ),
        // Member access folds only the pinned vector-component reads:
        // `vect(1, 2, 3).x` is `__xComponentOf__` upstream (#512).
        Expr::Member {
            receiver, member, ..
        } => {
            match (
                eval(receiver, constants, bindings, stack, settings)?,
                member.as_str(),
            ) {
                (Value::Vector(values), "x") => values.into_iter().next(),
                (Value::Vector(values), "y") => values.into_iter().nth(1),
                (Value::Vector(values), "z") => values.into_iter().nth(2),
                _ => None,
            }
        }
        // Member calls fold the pinned array/string methods; the receiver
        // is the first argument upstream (`content.args[0]`; #512).
        Expr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } => {
            let mut values = vec![eval(receiver, constants, bindings, stack, settings)?];
            for arg in args {
                values.push(eval(arg, constants, bindings, stack, settings)?);
            }
            evaluate_builtin(name, &values)
        }
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

/// The pinned optimizer's foldable builtins: every function that folds on
/// literal arguments in the reference, reproduced with its quirks —
/// `*Deg` multiplies by `π/180` (upstream reproduces the radians factor
/// instead of converting to degrees), `sqrt` maps `NaN` to `0` through
/// JavaScript's `|| 0`, and `min`/`max` require two or more numeric
/// arguments (#512).
///
/// Transcendentals go through `libm`: the pinned compiler runs on V8's
/// fdlibm, whose `Math.*` results differ from the platform libm behind
/// Rust's `f64` methods by ulps — and deferring to the platform would
/// also make emission nondeterministic across hosts.
fn evaluate_builtin(name: &str, values: &[Value]) -> Option<Value> {
    let degrees = std::f64::consts::PI / 180.0;
    let number = |value: &Value| match value {
        Value::Number(value) => Some(*value),
        _ => None,
    };
    fn vector(value: &Value) -> Option<&[Value]> {
        match value {
            Value::Vector(components) => Some(components.as_slice()),
            _ => None,
        }
    }

    let unary = |function: &dyn Fn(f64) -> f64| match values {
        [value] => number(value).map(function).map(Value::Number),
        _ => None,
    };
    match name {
        "abs" => unary(&f64::abs),
        "acos" => unary(&|value| libm::acos(js_clamp(value))),
        "acosDeg" => unary(&|value| libm::acos(js_clamp(value)) * degrees),
        "asin" => unary(&|value| libm::asin(js_clamp(value))),
        "asinDeg" => unary(&|value| libm::asin(js_clamp(value)) * degrees),
        "atan2" => match values {
            [y, x] => Some(Value::Number(libm::atan2(number(y)?, number(x)?))),
            _ => None,
        },
        "atan2Deg" => match values {
            [y, x] => Some(Value::Number(libm::atan2(number(y)?, number(x)?) * degrees)),
            _ => None,
        },
        "ceil" => unary(&f64::ceil),
        // `str.charAt` indexes UTF-16 units and truncates its index; the
        // pinned fold clamps at `Math.max(0, i)` and yields `""` out of
        // range (#512).
        "charAt" => match values {
            [Value::String(text), index] => {
                let index = number(index)?.max(0.0);
                if index.is_nan() || index >= text.encode_utf16().count() as f64 {
                    return Some(Value::String(String::new()));
                }
                let unit = text.encode_utf16().nth(index as usize)?;
                Some(Value::String(char::from_u32(unit as u32)?.to_string()))
            }
            _ => None,
        },
        "cos" => unary(&libm::cos),
        "cosDeg" => unary(&|value| libm::cos(value * degrees)),
        // `.concat` pushes a literal or splices an array (#512).
        "concat" => match values {
            [Value::Array(elements), Value::Array(extension)] => {
                let mut elements = elements.clone();
                elements.extend(extension.iter().cloned());
                Some(Value::Array(elements))
            }
            [Value::Array(elements), value] => {
                let mut elements = elements.clone();
                elements.push(value.clone());
                Some(Value::Array(elements))
            }
            _ => None,
        },
        "crossProduct" => match values {
            [a, b] => {
                let [ax, ay, az] = vector(a)? else {
                    return None;
                };
                let [bx, by, bz] = vector(b)? else {
                    return None;
                };
                let (ax, ay, az) = (number(ax)?, number(ay)?, number(az)?);
                let (bx, by, bz) = (number(bx)?, number(by)?, number(bz)?);
                Some(Value::Vector(vec![
                    Value::Number(ay * bz - az * by),
                    Value::Number(az * bx - ax * bz),
                    Value::Number(ax * by - ay * bx),
                ]))
            }
            _ => None,
        },
        "distance" => match values {
            [a, b] => {
                let [x1, y1, z1] = vector(a)? else {
                    return None;
                };
                let [x2, y2, z2] = vector(b)? else {
                    return None;
                };
                Some(Value::Number(libm::sqrt(
                    libm::pow(number(x1)? - number(x2)?, 2.0)
                        + libm::pow(number(y1)? - number(y2)?, 2.0)
                        + libm::pow(number(z1)? - number(z2)?, 2.0),
                )))
            }
            _ => None,
        },
        "dotProduct" => match values {
            [a, b] => {
                let [x1, y1, z1] = vector(a)? else {
                    return None;
                };
                let [x2, y2, z2] = vector(b)? else {
                    return None;
                };
                Some(Value::Number(
                    number(x1)? * number(x2)?
                        + number(y1)? * number(y2)?
                        + number(z1)? * number(z2)?,
                ))
            }
            _ => None,
        },
        // `.exclude` drops every element structurally equal to the value,
        // or to any member of an array value (#512).
        "exclude" => match values {
            [Value::Array(elements), Value::Array(excluded)] => Some(Value::Array(
                elements
                    .iter()
                    .filter(|element| !excluded.iter().any(|other| values_equal(element, other)))
                    .cloned()
                    .collect(),
            )),
            [Value::Array(elements), excluded] => Some(Value::Array(
                elements
                    .iter()
                    .filter(|element| !values_equal(element, excluded))
                    .cloned()
                    .collect(),
            )),
            _ => None,
        },
        "floor" => unary(&f64::floor),
        // `.index` is `Array.prototype.indexOf` on structural equality:
        // the first match position or `-1` (#512).
        "index" => match values {
            [Value::Array(elements), searched] => Some(Value::Number(
                elements
                    .iter()
                    .position(|element| values_equal(element, searched))
                    .map(|index| index as f64)
                    .unwrap_or(-1.0),
            )),
            _ => None,
        },
        "last" => match values {
            [Value::Array(elements)] if !elements.is_empty() => {
                Some(elements[elements.len() - 1].clone())
            }
            _ => None,
        },
        "len" => match values {
            [Value::Array(values)] => Some(Value::Number(values.len() as f64)),
            _ => None,
        },
        "log" => match values {
            [value] => number(value).map(libm::log).map(Value::Number),
            [value, base] => Some(Value::Number(
                libm::log(number(value)?) / libm::log(number(base)?),
            )),
            _ => None,
        },
        "magnitude" => match values {
            [arg] => {
                let [x, y, z] = vector(arg)? else { return None };
                Some(Value::Number(libm::sqrt(
                    libm::pow(number(x)?, 2.0)
                        + libm::pow(number(y)?, 2.0)
                        + libm::pow(number(z)?, 2.0),
                )))
            }
            _ => None,
        },
        "min" | "max" => {
            let numbers = values.iter().map(&number).collect::<Option<Vec<_>>>()?;
            if numbers.len() < 2 {
                return None;
            }
            let result = numbers
                .into_iter()
                .reduce(if name == "min" { js_min } else { js_max })?;
            Some(Value::Number(result))
        }
        // `.replace` is `String.prototype.replaceAll` (#512).
        "replace" => match values {
            [
                Value::String(text),
                Value::String(pattern),
                Value::String(replacement),
            ] => Some(Value::String(text.replace(pattern.as_str(), replacement))),
            _ => None,
        },
        "round" => unary(&round_half_up),
        "sin" => unary(&libm::sin),
        "sinDeg" => unary(&|value| libm::sin(value * degrees)),
        // `.slice` rounds start and count through `Math.round`; a
        // nonpositive count yields `[]` and a negative start eats into
        // the count from index `0` (#512).
        "slice" => match values {
            [Value::Array(elements), start, count] => {
                let mut count = round_half_up(number(count)?);
                if count <= 0.0 {
                    return Some(Value::Array(Vec::new()));
                }
                let mut start = round_half_up(number(start)?);
                if start < 0.0 {
                    count += start;
                    start = 0.0;
                }
                let start = start as usize;
                let end = (start as f64 + count).min(elements.len() as f64) as usize;
                Some(Value::Array(
                    elements
                        .get(start.min(elements.len())..end.min(elements.len()))
                        .unwrap_or(&[])
                        .to_vec(),
                ))
            }
            _ => None,
        },
        // `Math.sqrt(value) || 0` in the reference maps `NaN` to `0`.
        "sqrt" => unary(&|value| {
            let result = value.sqrt();
            if result.is_nan() { 0.0 } else { result }
        }),
        "strContains" => match values {
            [Value::String(text), Value::String(member)] => {
                Some(Value::Bool(text.contains(member.as_str())))
            }
            _ => None,
        },
        // `strLen` counts characters, not UTF-8 bytes (#512).
        "strLen" => match values {
            [Value::String(text)] => Some(Value::Number(text.chars().count() as f64)),
            _ => None,
        },
        // `.substring(string, start, length)` clamps both numbers at 0
        // and reads UTF-16 units (#512).
        "substring" => match values {
            [Value::String(text), start, length] => {
                let start = number(start)?.max(0.0);
                let length = number(length)?.max(0.0);
                if start.is_nan() || length.is_nan() {
                    return None;
                }
                let units: Vec<u16> = text.encode_utf16().collect();
                let end = (start + length).min(units.len() as f64) as usize;
                let start = (start as usize).min(units.len());
                Some(Value::String(String::from_utf16_lossy(
                    &units[start..end.max(start)],
                )))
            }
            _ => None,
        },
        "tan" => unary(&libm::tan),
        "tanDeg" => unary(&|value| libm::tan(value * degrees)),
        "vectorTowards" => match values {
            [a, b] => {
                let [x1, y1, z1] = vector(a)? else {
                    return None;
                };
                let [x2, y2, z2] = vector(b)? else {
                    return None;
                };
                Some(Value::Vector(vec![
                    Value::Number(number(x2)? - number(x1)?),
                    Value::Number(number(y2)? - number(y1)?),
                    Value::Number(number(z2)? - number(z1)?),
                ]))
            }
            _ => None,
        },
        _ => None,
    }
}

/// JavaScript `Math.min`/`Math.max`: `NaN` poisons the result, unlike the
/// Rust `f64` methods which skip it.
fn js_min(left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() {
        f64::NAN
    } else {
        left.min(right)
    }
}

fn js_max(left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() {
        f64::NAN
    } else {
        left.max(right)
    }
}

/// `Math.max(-1, Math.min(1, x))` — the clamp the pinned `acos`/`asin`
/// folds apply — propagates `NaN` where `f64::min`/`f64::max` would
/// silently drop it.
fn js_clamp(value: f64) -> f64 {
    if value.is_nan() {
        f64::NAN
    } else {
        value.clamp(-1.0, 1.0)
    }
}

/// Constant folding of the pinned optimizer's binary operators. Numeric
/// folds only apply while the result stays within `NUMBER_LIMIT`;
/// afterwards the identity rules (`a + 0`, `a * 1`, `b / 1`, …) still
/// pass operands through for any value kind (#512).
fn evaluate_binary(op: &str, left: Value, right: Value, settings: bool) -> Option<Value> {
    let is_zero = |value: &Value| matches!(value, Value::Number(number) if *number == 0.0);
    let is_one = |value: &Value| matches!(value, Value::Number(number) if *number == 1.0);
    let is_bool = |value: &Value| matches!(value, Value::Bool(_));
    let is_literal = |value: &Value| matches!(value, Value::Number(_) | Value::String(_));
    let is_falsy = |value: &Value| truthiness(value) == Some(false);
    let folded = |result: f64| (result.abs() <= NUMBER_LIMIT).then_some(Value::Number(result));
    match op {
        "+" => {
            if let (Value::Number(left), Value::Number(right)) = (&left, &right) {
                if let result @ Some(_) = folded(left + right) {
                    return result;
                }
            }
            if is_zero(&left) {
                return Some(right);
            }
            if is_zero(&right) {
                return Some(left);
            }
            // The settings evaluator has no string `+`; ordinary constant
            // folding keeps it for `#!define` composition.
            if !settings && let (Value::String(left), Value::String(right)) = (&left, &right) {
                return Some(Value::String(left.clone() + right));
            }
            None
        }
        "-" => {
            if let (Value::Number(left), Value::Number(right)) = (&left, &right)
                && let result @ Some(_) = folded(left - right)
            {
                return result;
            }
            if is_zero(&right) {
                return Some(left);
            }
            if values_equal(&left, &right) {
                return Some(Value::Number(0.0));
            }
            None
        }
        "*" => {
            if let (Value::Number(left), Value::Number(right)) = (&left, &right)
                && let result @ Some(_) = folded(left * right)
            {
                return result;
            }
            if is_one(&left) {
                return Some(right);
            }
            if is_one(&right) {
                return Some(left);
            }
            if is_zero(&left) || is_zero(&right) {
                return Some(Value::Number(0.0));
            }
            None
        }
        "/" => {
            if let (Value::Number(left), Value::Number(right)) = (&left, &right)
                && let result @ Some(_) = folded(left / right)
            {
                return result;
            }
            if is_one(&right) {
                return Some(left);
            }
            if is_zero(&left) || is_zero(&right) {
                return Some(Value::Number(0.0));
            }
            None
        }
        // `a % |b|` folds without the number limit, so `5 % 0` emits `NaN`.
        "%" => {
            if let (Value::Number(left), Value::Number(right)) = (&left, &right) {
                return Some(Value::Number(left % right.abs()));
            }
            if values_equal(&left, &right) || is_zero(&left) || is_zero(&right) {
                return Some(Value::Number(0.0));
            }
            None
        }
        "**" => {
            if let (Value::Number(base), Value::Number(exponent)) = (&left, &right) {
                if *base < 0.0 {
                    return Some(Value::Number(0.0));
                }
                if let result @ Some(_) = folded(base.powf(*exponent)) {
                    return result;
                }
            }
            if is_one(&right) {
                return Some(left);
            }
            if is_zero(&left) {
                return Some(Value::Number(0.0));
            }
            if is_one(&left) {
                return Some(left);
            }
            None
        }
        // `and`/`or` return an operand like the reference's operand-return
        // rules, which also decide truthy/falsy on the *other* operand
        // (`{} and 5` emits the object, `{} or 5` emits `5`; #512).
        "and" => {
            if is_falsy(&left) {
                Some(left)
            } else if is_falsy(&right) || truthiness(&left) == Some(true) {
                Some(right)
            } else if truthiness(&right) == Some(true) || values_equal(&left, &right) {
                Some(left)
            } else {
                None
            }
        }
        "or" => {
            if is_falsy(&left) {
                Some(right)
            } else if is_falsy(&right) || truthiness(&left) == Some(true) {
                Some(left)
            } else if truthiness(&right) == Some(true) {
                Some(right)
            } else if values_equal(&left, &right) {
                Some(left)
            } else {
                None
            }
        }
        // `==`/`!=` reproduce the reference's rules: numbers compare,
        // equal structures fold, number/string literals never equal other
        // kinds, and a falsy operand can compare against a Boolean —
        // `0 == false` emits `true` and `5 == false` stays unevaluated
        // because non-Booleans are never bool-suitable (#512).
        "==" => {
            if let (Value::Number(left), Value::Number(right)) = (&left, &right) {
                return Some(Value::Bool(left == right));
            }
            if values_equal(&left, &right) {
                return Some(Value::Bool(true));
            }
            if is_literal(&left) && is_literal(&right) {
                return Some(Value::Bool(false));
            }
            match (&left, &right) {
                (Value::Bool(left), right) if is_falsy(right) => {
                    return Some(Value::Bool(!left));
                }
                (left, Value::Bool(right)) if is_falsy(left) => {
                    return Some(Value::Bool(!right));
                }
                (left, right) if is_bool(left) && *right == Value::Bool(true) => {
                    return Some(left.clone());
                }
                (left, right) if is_bool(right) && *left == Value::Bool(true) => {
                    return Some(right.clone());
                }
                _ => {}
            }
            None
        }
        "!=" => {
            if let (Value::Number(left), Value::Number(right)) = (&left, &right) {
                return Some(Value::Bool(left != right));
            }
            if values_equal(&left, &right) {
                return Some(Value::Bool(false));
            }
            if is_literal(&left) && is_literal(&right) {
                return Some(Value::Bool(true));
            }
            match (&left, &right) {
                (left, right) if is_bool(left) && is_falsy(right) => {
                    return Some(left.clone());
                }
                (left, right) if is_bool(right) && is_falsy(left) => {
                    return Some(right.clone());
                }
                (Value::Bool(left), right) if *right == Value::Bool(true) => {
                    return Some(Value::Bool(!left));
                }
                (left, Value::Bool(right)) if *left == Value::Bool(true) => {
                    return Some(Value::Bool(!right));
                }
                _ => {}
            }
            None
        }
        // Comparisons fold on numbers; equal structures fold `<`/`>` to
        // false and `<=`/`>=` to true (`[1] <= [1]` emits `true`; #512).
        "<" | "<=" | ">" | ">=" => {
            if let (Value::Number(left), Value::Number(right)) = (&left, &right) {
                return Some(Value::Bool(match op {
                    "<" => left < right,
                    "<=" => left <= right,
                    ">" => left > right,
                    _ => left >= right,
                }));
            }
            if values_equal(&left, &right) {
                return Some(Value::Bool(matches!(op, "<=" | ">=")));
            }
            None
        }
        // Membership tests array contents structurally; `x in y` on a
        // non-array does not fold (`[[1]] in [[1],[2]]` is an index read
        // upstream, handled by `Expr::Index`; #512).
        "in" => match right {
            Value::Array(values) => Some(Value::Bool(
                values.iter().any(|value| values_equal(value, &left)),
            )),
            _ => None,
        },
        "not in" => match right {
            Value::Array(values) => Some(Value::Bool(
                !values.iter().any(|value| values_equal(value, &left)),
            )),
            _ => None,
        },
        _ => None,
    }
}

/// The pinned evaluator's `isDefinitelyTruthy`/`isDefinitelyFalsy`:
/// numbers are truthy when nonzero (`NaN` included), strings when
/// non-empty, and arrays by their first element. Objects are neither —
/// `not {}` and `{} and x` do not fold (#512).
fn truthiness(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(value) => Some(*value != 0.0),
        Value::String(value) => Some(!value.is_empty()),
        Value::Array(values) if values.is_empty() => Some(false),
        Value::Array(values) => truthiness(&values[0]),
        Value::Object(_) | Value::Vector(_) => None,
    }
}

/// The pinned `areAstsAlwaysEqual`: structural equality where numbers
/// compare by their JavaScript `toString`, so `NaN` equals `NaN` and
/// `1` equals `1.0` (#512).
fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => {
            crate::compiler::number_format::javascript_text(*left)
                == crate::compiler::number_format::javascript_text(*right)
        }
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Array(left), Value::Array(right)) | (Value::Vector(left), Value::Vector(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| values_equal(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|((lk, lv), (rk, rv))| values_equal(lk, rk) && values_equal(lv, rv))
        }
        _ => false,
    }
}

/// `__valueInArray__` folds a numeric index through `Math.round`, so
/// `[1, 2][0.6]` reads index `1`; out-of-range and negative indexes stay
/// unevaluated. Dictionary lookup compares keys structurally (#512).
fn evaluate_index(collection: Value, index: Value) -> Option<Value> {
    match collection {
        Value::Array(values) => {
            let Value::Number(index) = index else {
                return None;
            };
            let index = round_half_up(index);
            if index < 0.0 || index >= values.len() as f64 {
                return None;
            }
            values.into_iter().nth(index as usize)
        }
        Value::Object(entries) => entries
            .into_iter()
            .find(|(key, _)| values_equal(key, &index))
            .map(|(_, value)| value),
        _ => None,
    }
}

/// The pinned `__customString__` fold: numbered (`{0}`) and unnumbered
/// (`{}`) formatters each substitute one argument and must not mix;
/// unnumbered formatters consume exactly one argument each, numbered
/// formatters index into the argument list. Arguments substitute as
/// numbers (`toFixed(2)` minus `.00`, within `NUMBER_LIMIT`), string
/// splices, the first array element, or `0` for null-like gaps; any other
/// argument leaves a formatter and fails the fold (#512).
fn evaluate_format(text: &str, args: &[Value]) -> Option<Value> {
    let mut output = String::new();
    let mut unnumbered = 0usize;
    let mut numbered = false;
    let mut unnumbered_args = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            // Doubled braces are literal text upstream — the reference
            // emits `{{0}}` verbatim rather than unescaping to `{0}`.
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                output.push_str("{{");
            }
            '{' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    unnumbered += 1;
                    unnumbered_args += 1;
                    output.push_str(&format_substitute(args.get(unnumbered_args - 1)?)?);
                    continue;
                }
                let mut index_text = String::new();
                let mut closed = false;
                for character in chars.by_ref() {
                    if character == '}' {
                        closed = true;
                        break;
                    }
                    index_text.push(character);
                }
                if closed
                    && !index_text.is_empty()
                    && index_text.chars().all(|c| c.is_ascii_digit())
                {
                    numbered = true;
                    let index = index_text.parse::<usize>().ok()?;
                    output.push_str(&format_substitute(args.get(index)?)?);
                } else {
                    output.push('{');
                    output.push_str(&index_text);
                    if closed {
                        output.push('}');
                    }
                }
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                output.push_str("}}");
            }
            character => output.push(character),
        }
    }
    // Numbered and unnumbered formatters must not mix, and unnumbered
    // formatters require exactly one argument each — the reference errors
    // in both cases. Extra arguments with numbered formatters (or none at
    // all) are dropped silently (`"{{0}}".format(5)` emits `{0}`; #512).
    if numbered && unnumbered > 0 || unnumbered_args != args.len() && unnumbered > 0 {
        return None;
    }
    Some(Value::String(output))
}

/// The text an argument contributes inside a folded format expression.
fn format_substitute(value: &Value) -> Option<String> {
    match value {
        Value::Number(value) if value.abs() <= NUMBER_LIMIT => Some(workshop_number_text(*value)),
        Value::String(value) => Some(value.clone()),
        // Arrays display their first element only (#512).
        Value::Array(values) if values.is_empty() => Some(String::new()),
        Value::Array(values) => format_substitute(&values[0]),
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
