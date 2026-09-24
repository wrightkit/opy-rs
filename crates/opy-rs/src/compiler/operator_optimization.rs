//! Operator-level value optimization applied at the Workshop boundary.
//!
//! The pinned OverPy reference folds and simplifies operators bottom-up while
//! it builds a rule, so the emitted structure depends on those rewrites. This
//! pass applies the same rewrites to the canonical value tree of one action or
//! condition under the optimization state active at its source location.

use workshop_rs::Value;
use workshop_rs::catalog::Kind;

use super::Compiler;

/// Folded numbers beyond this magnitude keep their operator form.
const NUMBER_LIMIT: f64 = 1e7;

const RANDOM_CALLS: [&str; 4] = [
    "randomInteger",
    "randomReal",
    "randomValueInArray",
    "randomizedArray",
];

pub(super) struct OperatorOptimizer<'a> {
    compiler: &'a Compiler,
    strict: bool,
}

impl<'a> OperatorOptimizer<'a> {
    pub(super) fn new(compiler: &'a Compiler, strict: bool) -> Self {
        Self { compiler, strict }
    }

    /// Whether a value is definitely truthy or falsy, when it is constant.
    pub(super) fn constant_truth(&self, value: &Value) -> Option<bool> {
        if falsy(value) {
            Some(false)
        } else if self.truthy(value) {
            Some(true)
        } else {
            None
        }
    }

    /// The comparison a rule condition is emitted as: comparisons stay as
    /// they are, `not x` becomes `x == False`, Boolean values `x == True`, and
    /// any other value `x != False`.
    pub(super) fn wrap_condition(&self, value: Value) -> Value {
        match value {
            Value::Call { name, args }
                if matches!(name.as_str(), "==" | "!=" | "<" | "<=" | ">" | ">=") =>
            {
                Value::Call { name, args }
            }
            Value::Call { name, mut args } if name == "not" && args.len() == 1 => call(
                "==",
                vec![args.pop().expect("one argument"), Value::Bool(false)],
            ),
            value if self.boolean(&value) => call("==", vec![value, Value::Bool(true)]),
            value => call("!=", vec![value, Value::Bool(false)]),
        }
    }

    /// Apply the rule of one node; a rewrite that changes the head is applied
    /// again to its result, as the reference does.
    pub(super) fn node(&self, value: Value) -> Value {
        let head = head_of(&value);
        let rewritten = match self.rewrite(value) {
            Rewrite::Same(value) => return value,
            Rewrite::Changed(value) => value,
        };
        if head_of(&rewritten) == head {
            rewritten
        } else {
            self.node(rewritten)
        }
    }

    fn rewrite(&self, value: Value) -> Rewrite {
        let Value::Call { name, args } = value else {
            return Rewrite::Same(value);
        };
        match (name.as_str(), args.len()) {
            ("==", 2) => self.equals(args, true),
            ("!=", 2) => self.equals(args, false),
            ("<", 2) => Self::ordering("<", args, |a, b| a < b, false),
            ("<=", 2) => Self::ordering("<=", args, |a, b| a <= b, true),
            (">", 2) => Self::ordering(">", args, |a, b| a > b, false),
            (">=", 2) => Self::ordering(">=", args, |a, b| a >= b, true),
            ("not", 1) => self.not(args),
            ("and", 2) => self.and(args),
            ("or", 2) => self.or(args),
            ("ifThenElse", 3) => self.if_then_else(args),
            ("add", 2) => self.add(args),
            ("subtract", 2) => self.subtract(args),
            ("multiply", 2) => self.multiply(args),
            ("divide", 2) => self.divide(args),
            ("modulo", 2) => self.modulo(args),
            ("raiseToPower", 2) => self.power(args),
            ("-", 1) => self.negate(args),
            ("mappedArray", 2) => Self::mapped(args),
            ("filteredArray", 2) => self.filtered(args),
            ("arrayContains", 2) => self.array_contains(args),
            ("valueInArray", 2) => self.value_in_array(args),
            ("__xComponentOf__", 1) => Self::component(args, 0),
            ("__yComponentOf__", 1) => Self::component(args, 1),
            ("__zComponentOf__", 1) => Self::component(args, 2),
            _ => Rewrite::Same(Value::Call { name, args }),
        }
    }

    fn equals(&self, args: Vec<Value>, equal: bool) -> Rewrite {
        let name = if equal { "==" } else { "!=" };
        let [left, right] = two(args);
        if let (Value::Number(a), Value::Number(b)) = (&left, &right) {
            return Rewrite::Changed(Value::Bool((a == b) == equal));
        }
        if same(&left, &right) {
            return Rewrite::Changed(Value::Bool(equal));
        }
        if self.literal(&left) && self.literal(&right) {
            return Rewrite::Changed(Value::Bool(!equal));
        }
        if falsy(&right) && self.boolean(&left) {
            return Rewrite::Changed(if equal { not(left) } else { left });
        }
        if falsy(&left) && self.boolean(&right) {
            return Rewrite::Changed(if equal { not(right) } else { right });
        }
        if matches!(right, Value::Bool(true)) && self.boolean(&left) {
            return Rewrite::Changed(if equal { left } else { not(left) });
        }
        if matches!(left, Value::Bool(true)) && self.boolean(&right) {
            return Rewrite::Changed(if equal { right } else { not(right) });
        }
        Rewrite::Same(call(name, vec![left, right]))
    }

    fn ordering(
        name: &str,
        args: Vec<Value>,
        compare: fn(f64, f64) -> bool,
        reflexive: bool,
    ) -> Rewrite {
        let [left, right] = two(args);
        if let (Value::Number(a), Value::Number(b)) = (&left, &right) {
            return Rewrite::Changed(Value::Bool(compare(*a, *b)));
        }
        if same(&left, &right) {
            return Rewrite::Changed(Value::Bool(reflexive));
        }
        Rewrite::Same(call(name, vec![left, right]))
    }

    fn not(&self, args: Vec<Value>) -> Rewrite {
        let [operand] = one(args);
        if falsy(&operand) {
            return Rewrite::Changed(Value::Bool(true));
        }
        if self.truthy(&operand) {
            return Rewrite::Changed(Value::Bool(false));
        }
        if let Value::Call { name, args } = operand {
            if name == "not" && args.len() == 1 && self.boolean(&args[0]) {
                return Rewrite::Changed(args.into_iter().next().expect("one argument"));
            }
            let inverse = match name.as_str() {
                "isAlive" if args.len() == 1 => Some("isDead"),
                "isDead" if args.len() == 1 => Some("isAlive"),
                "==" => Some("!="),
                "!=" => Some("=="),
                ">" => Some("<="),
                ">=" => Some("<"),
                "<" => Some(">="),
                "<=" => Some(">"),
                _ => None,
            };
            if let Some(inverse) = inverse {
                return Rewrite::Changed(call(inverse, args));
            }
            return Rewrite::Same(not(Value::Call { name, args }));
        }
        Rewrite::Same(not(operand))
    }

    fn and(&self, args: Vec<Value>) -> Rewrite {
        let [left, right] = two(args);
        if falsy(&left) {
            return Rewrite::Changed(left);
        }
        if !self.strict && falsy(&right) {
            return Rewrite::Changed(right);
        }
        if self.truthy(&left) {
            return Rewrite::Changed(right);
        }
        if !self.strict && self.truthy(&right) {
            return Rewrite::Changed(left);
        }
        if same(&left, &right) {
            return Rewrite::Changed(left);
        }
        if !self.strict && (negates(&right, &left) || negates(&left, &right)) {
            return Rewrite::Changed(Value::Bool(false));
        }
        if let (Some(a), Some(b)) = (negated(&left), negated(&right)) {
            return Rewrite::Changed(not(call("or", vec![a.clone(), b.clone()])));
        }
        Rewrite::Same(call("and", vec![left, right]))
    }

    fn or(&self, args: Vec<Value>) -> Rewrite {
        let [left, right] = two(args);
        if falsy(&left) {
            return Rewrite::Changed(right);
        }
        if !self.strict && falsy(&right) {
            return Rewrite::Changed(left);
        }
        if self.truthy(&left) {
            return Rewrite::Changed(left);
        }
        if !self.strict && self.truthy(&right) {
            return Rewrite::Changed(right);
        }
        if same(&left, &right) {
            return Rewrite::Changed(left);
        }
        if !self.strict && (negates(&right, &left) || negates(&left, &right)) {
            return Rewrite::Changed(Value::Bool(true));
        }
        if let (Some(a), Some(b)) = (negated(&left), negated(&right)) {
            return Rewrite::Changed(not(call("and", vec![a.clone(), b.clone()])));
        }
        Rewrite::Same(call("or", vec![left, right]))
    }

    fn if_then_else(&self, args: Vec<Value>) -> Rewrite {
        let [condition, then_value, else_value] = three(args);
        if self.truthy(&condition) {
            return Rewrite::Changed(then_value);
        }
        if falsy(&condition) {
            return Rewrite::Changed(else_value);
        }
        if same(&then_value, &else_value) {
            return Rewrite::Changed(then_value);
        }
        if let Value::Call { name, args } = condition {
            if name == "not" && args.len() == 1 {
                let inner = args.into_iter().next().expect("one argument");
                return Rewrite::Changed(call("ifThenElse", vec![inner, else_value, then_value]));
            }
            return Rewrite::Same(call(
                "ifThenElse",
                vec![Value::Call { name, args }, then_value, else_value],
            ));
        }
        Rewrite::Same(call("ifThenElse", vec![condition, then_value, else_value]))
    }

    fn add(&self, args: Vec<Value>) -> Rewrite {
        let [left, right] = two(args);
        if let Some(sum) = fold(&left, &right, |a, b| a + b) {
            return Rewrite::Changed(sum);
        }
        if !self.strict {
            if is_number(&left, 0.0) {
                return Rewrite::Changed(right);
            }
            if is_number(&right, 0.0) {
                return Rewrite::Changed(left);
            }
            if zero_vector(&left) {
                return Rewrite::Changed(right);
            }
            if zero_vector(&right) {
                return Rewrite::Changed(left);
            }
        }
        if same(&left, &right) {
            return Rewrite::Changed(call("multiply", vec![Value::Number(2.0), left]));
        }
        if let Some(sum) = vector_fold(&left, &right, |a, b| a + b) {
            return Rewrite::Changed(sum);
        }
        Rewrite::Same(call("add", vec![left, right]))
    }

    fn subtract(&self, args: Vec<Value>) -> Rewrite {
        let [left, right] = two(args);
        if let Some(difference) = fold(&left, &right, |a, b| a - b) {
            return Rewrite::Changed(difference);
        }
        if is_number(&right, 0.0) || zero_vector(&right) {
            return Rewrite::Changed(left);
        }
        if same(&left, &right) {
            return Rewrite::Changed(call("multiply", vec![left, Value::Number(0.0)]));
        }
        if let Some(difference) = vector_fold(&left, &right, |a, b| a - b) {
            return Rewrite::Changed(difference);
        }
        Rewrite::Same(call("subtract", vec![left, right]))
    }

    fn multiply(&self, args: Vec<Value>) -> Rewrite {
        let [left, right] = two(args);
        if let Some(product) = fold(&left, &right, |a, b| a * b) {
            return Rewrite::Changed(product);
        }
        if !self.strict {
            if is_number(&left, 1.0) {
                return Rewrite::Changed(right);
            }
            if is_number(&right, 1.0) {
                return Rewrite::Changed(left);
            }
        }
        if (is_number(&left, 0.0) && (!self.strict || self.float(&right)))
            || (is_number(&right, 0.0) && (!self.strict || self.float(&left)))
        {
            return Rewrite::Changed(Value::Number(0.0));
        }
        if let Some(product) = vector_fold(&left, &right, |a, b| a * b) {
            return Rewrite::Changed(product);
        }
        if let (Value::Number(scale), Some(components)) = (&left, number_components(&right)) {
            return Rewrite::Changed(vector(components.map(|c| scale * c)));
        }
        if let (Some(components), Value::Number(scale)) = (number_components(&left), &right) {
            return Rewrite::Changed(vector(components.map(|c| c * scale)));
        }
        Rewrite::Same(call("multiply", vec![left, right]))
    }

    fn divide(&self, args: Vec<Value>) -> Rewrite {
        let [left, right] = two(args);
        if let Some(quotient) = fold(&left, &right, |a, b| a / b) {
            return Rewrite::Changed(quotient);
        }
        if !self.strict {
            if is_number(&right, 1.0) {
                return Rewrite::Changed(left);
            }
            if is_number(&left, 0.0) || is_number(&right, 0.0) {
                return Rewrite::Changed(Value::Number(0.0));
            }
        }
        if let Some(quotient) = vector_fold(&left, &right, |a, b| a / b) {
            return Rewrite::Changed(quotient);
        }
        if let (Some(components), Value::Number(divisor)) = (number_components(&left), &right) {
            return Rewrite::Changed(vector(components.map(|c| c / divisor)));
        }
        Rewrite::Same(call("divide", vec![left, right]))
    }

    fn modulo(&self, args: Vec<Value>) -> Rewrite {
        let [left, right] = two(args);
        if let (Value::Number(a), Value::Number(b)) = (&left, &right) {
            return Rewrite::Changed(Value::Number(a % b.abs()));
        }
        if same(&left, &right) || is_number(&left, 0.0) || is_number(&right, 0.0) {
            return Rewrite::Changed(Value::Number(0.0));
        }
        Rewrite::Same(call("modulo", vec![left, right]))
    }

    fn power(&self, args: Vec<Value>) -> Rewrite {
        let [base, exponent] = two(args);
        if let (Value::Number(a), Value::Number(b)) = (&base, &exponent) {
            if *a < 0.0 {
                return Rewrite::Changed(Value::Number(0.0));
            }
            let result = a.powf(*b);
            if result.abs() <= NUMBER_LIMIT {
                return Rewrite::Changed(Value::Number(result));
            }
        }
        if is_number(&exponent, 1.0) || is_number(&base, 1.0) {
            return Rewrite::Changed(base);
        }
        if is_number(&base, 0.0) {
            return Rewrite::Changed(Value::Number(0.0));
        }
        Rewrite::Same(call("raiseToPower", vec![base, exponent]))
    }

    fn negate(&self, args: Vec<Value>) -> Rewrite {
        let [operand] = one(args);
        if let Value::Number(number) = operand {
            return Rewrite::Changed(Value::Number(-number));
        }
        match operand {
            Value::Call { name, mut args }
                if matches!(name.as_str(), "multiply" | "divide" | "modulo")
                    && args.len() == 2
                    && matches!(args[0], Value::Number(_)) =>
            {
                if let Value::Number(number) = args[0] {
                    args[0] = Value::Number(-number);
                }
                Rewrite::Changed(Value::Call { name, args })
            }
            other => {
                if let Some(components) = number_components(&other) {
                    return Rewrite::Changed(vector(components.map(|c| -c)));
                }
                Rewrite::Changed(call("multiply", vec![Value::Number(-1.0), other]))
            }
        }
    }

    fn mapped(args: Vec<Value>) -> Rewrite {
        let [array, mapping] = two(args);
        if matches!(&mapping, Value::Call { name, .. } if name == "currentArrayElement") {
            return Rewrite::Changed(array);
        }
        Rewrite::Same(call("mappedArray", vec![array, mapping]))
    }

    fn filtered(&self, args: Vec<Value>) -> Rewrite {
        let [array, predicate] = two(args);
        let element_free = |value: &Value| !mentions_element(value);
        if let Value::Call {
            name,
            args: operands,
        } = &predicate
        {
            if name == "!=" && operands.len() == 2 {
                for (element, other) in [(0, 1), (1, 0)] {
                    if matches!(&operands[element], Value::Call { name, .. } if name == "currentArrayElement")
                        && element_free(&operands[other])
                    {
                        return Rewrite::Changed(call(
                            "removeFromArray",
                            vec![array, operands[other].clone()],
                        ));
                    }
                }
            }
        }
        Rewrite::Same(call("filteredArray", vec![array, predicate]))
    }

    fn array_contains(&self, args: Vec<Value>) -> Rewrite {
        let [array, needle] = two(args);
        let Value::Array(mut elements) = array else {
            return Rewrite::Same(call("arrayContains", vec![array, needle]));
        };
        if elements.iter().any(|element| same(element, &needle)) {
            return Rewrite::Changed(Value::Bool(true));
        }
        if self.literal(&needle) {
            elements.retain(|element| !self.literal(element));
        }
        match elements.len() {
            0 => Rewrite::Changed(Value::Bool(false)),
            1 => Rewrite::Changed(call(
                "==",
                vec![needle, elements.pop().expect("one element")],
            )),
            _ => Rewrite::Same(call("arrayContains", vec![Value::Array(elements), needle])),
        }
    }

    fn value_in_array(&self, args: Vec<Value>) -> Rewrite {
        let [array, index] = two(args);
        if let Value::Number(position) = index {
            if position < 0.0 {
                return Rewrite::Changed(Value::Null);
            }
            let position = position.round();
            if let Value::Array(mut elements) = array {
                if (position as usize) < elements.len() {
                    return Rewrite::Changed(elements.swap_remove(position as usize));
                }
                if !elements.is_empty() || !self.strict {
                    return Rewrite::Changed(Value::Null);
                }
                return Rewrite::Same(call(
                    "valueInArray",
                    vec![Value::Array(elements), Value::Number(position)],
                ));
            }
            if position == 0.0 {
                return Rewrite::Changed(call("firstOf", vec![array]));
            }
            return Rewrite::Same(call("valueInArray", vec![array, Value::Number(position)]));
        }
        Rewrite::Same(call("valueInArray", vec![array, index]))
    }

    fn component(args: Vec<Value>, axis: usize) -> Rewrite {
        let [operand] = one(args);
        let name = ["__xComponentOf__", "__yComponentOf__", "__zComponentOf__"][axis];
        match operand {
            Value::Vector { x, y, z } => Rewrite::Changed([*x, *y, *z][axis].clone()),
            other => Rewrite::Same(call(name, vec![other])),
        }
    }

    fn boolean(&self, value: &Value) -> bool {
        match value {
            Value::Bool(_) => true,
            Value::Call { name, .. } => {
                matches!(
                    name.as_str(),
                    "==" | "!=" | "<" | "<=" | ">" | ">=" | "and" | "or" | "not"
                ) || self
                    .compiler
                    .catalog
                    .entry(Kind::Value, name)
                    .and_then(|entry| entry.return_type())
                    .is_some_and(|kind| matches!(kind, "Boolean" | "BoolLiteral"))
            }
            _ => false,
        }
    }

    fn float(&self, value: &Value) -> bool {
        match value {
            Value::Number(_) => true,
            Value::Call { name, .. } => self
                .compiler
                .catalog
                .entry(Kind::Value, name)
                .and_then(|entry| entry.return_type())
                .is_some_and(|kind| kind == "Number"),
            _ => false,
        }
    }

    fn truthy(&self, value: &Value) -> bool {
        match value {
            Value::Bool(value) => *value,
            Value::Number(number) => *number != 0.0,
            Value::Vector { x, y, z } => self.truthy(x) || self.truthy(y) || self.truthy(z),
            Value::Array(elements) => elements.first().is_some_and(|first| self.truthy(first)),
            Value::Enum { value_type, .. } => matches!(
                value_type.as_str(),
                "Hero" | "Map" | "Gamemode" | "Team" | "Button" | "Color"
            ),
            Value::String(text) => has_literal_text(text),
            Value::Call { name, args } if name == "customString" => match args.first() {
                Some(Value::String(text)) => has_literal_text(text),
                _ => false,
            },
            _ => false,
        }
    }

    fn literal(&self, value: &Value) -> bool {
        match value {
            Value::Number(_) | Value::Bool(_) | Value::Null | Value::Enum { .. } => true,
            Value::Array(elements) => elements.iter().all(|element| self.literal(element)),
            Value::Vector { x, y, z } => self.literal(x) && self.literal(y) && self.literal(z),
            Value::String(_) => !self.strict,
            Value::Call { name, args } if name == "customString" => args.len() == 1 && !self.strict,
            _ => false,
        }
    }
}

enum Rewrite {
    Same(Value),
    Changed(Value),
}

fn head_of(value: &Value) -> String {
    match value {
        Value::Call { name, .. } => name.clone(),
        Value::Number(_) => "#number".to_string(),
        Value::Bool(_) => "#bool".to_string(),
        Value::Null => "#null".to_string(),
        Value::Array(_) => "#array".to_string(),
        Value::Vector { .. } => "#vector".to_string(),
        _ => "#other".to_string(),
    }
}

fn call(name: &str, args: Vec<Value>) -> Value {
    Value::Call {
        name: name.to_string(),
        args,
    }
}

fn not(value: Value) -> Value {
    call("not", vec![value])
}

fn one(args: Vec<Value>) -> [Value; 1] {
    args.try_into().expect("operator arity")
}

fn two(args: Vec<Value>) -> [Value; 2] {
    args.try_into().expect("operator arity")
}

fn three(args: Vec<Value>) -> [Value; 3] {
    args.try_into().expect("operator arity")
}

fn is_number(value: &Value, expected: f64) -> bool {
    matches!(value, Value::Number(number) if *number == expected)
}

fn zero_vector(value: &Value) -> bool {
    number_components(value).is_some_and(|components| components == [0.0, 0.0, 0.0])
}

fn number_components(value: &Value) -> Option<[f64; 3]> {
    match value {
        Value::Vector { x, y, z } => match (&**x, &**y, &**z) {
            (Value::Number(x), Value::Number(y), Value::Number(z)) => Some([*x, *y, *z]),
            _ => None,
        },
        Value::Enum { value_type, value } if value_type == "Vector" => match value.as_str() {
            "LEFT" => Some([1.0, 0.0, 0.0]),
            "RIGHT" => Some([-1.0, 0.0, 0.0]),
            "UP" => Some([0.0, 1.0, 0.0]),
            "DOWN" => Some([0.0, -1.0, 0.0]),
            "FORWARD" => Some([0.0, 0.0, 1.0]),
            "BACKWARD" => Some([0.0, 0.0, -1.0]),
            _ => None,
        },
        _ => None,
    }
}

/// A numeric vector, written as its direction constant when it is one.
fn vector(components: [f64; 3]) -> Value {
    let name = match components {
        [1.0, 0.0, 0.0] => Some("LEFT"),
        [-1.0, 0.0, 0.0] => Some("RIGHT"),
        [0.0, 1.0, 0.0] => Some("UP"),
        [0.0, -1.0, 0.0] => Some("DOWN"),
        [0.0, 0.0, 1.0] => Some("FORWARD"),
        [0.0, 0.0, -1.0] => Some("BACKWARD"),
        _ => None,
    };
    if let Some(name) = name {
        return Value::Enum {
            value_type: "Vector".to_string(),
            value: name.to_string(),
        };
    }
    let [x, y, z] = components;
    Value::Vector {
        x: Box::new(Value::Number(x)),
        y: Box::new(Value::Number(y)),
        z: Box::new(Value::Number(z)),
    }
}

fn fold(left: &Value, right: &Value, apply: fn(f64, f64) -> f64) -> Option<Value> {
    let (Value::Number(a), Value::Number(b)) = (left, right) else {
        return None;
    };
    let result = apply(*a, *b);
    (result.abs() <= NUMBER_LIMIT).then_some(Value::Number(result))
}

fn vector_fold(left: &Value, right: &Value, apply: fn(f64, f64) -> f64) -> Option<Value> {
    let (a, b) = (number_components(left)?, number_components(right)?);
    Some(vector([
        apply(a[0], b[0]),
        apply(a[1], b[1]),
        apply(a[2], b[2]),
    ]))
}

fn falsy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::Number(number) => *number == 0.0,
        Value::Array(elements) => elements.first().is_none_or(falsy),
        Value::String(text) => text.is_empty(),
        Value::Call { name, args } => match name.as_str() {
            "emptyArray" => true,
            "customString" => matches!(args.first(), Some(Value::String(text)) if text.is_empty()),
            _ => false,
        },
        _ => false,
    }
}

fn has_literal_text(text: &str) -> bool {
    let mut depth = 0;
    for character in text.chars() {
        match character {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn negated(value: &Value) -> Option<&Value> {
    match value {
        Value::Call { name, args } if name == "not" && args.len() == 1 => args.first(),
        _ => None,
    }
}

fn negates(value: &Value, other: &Value) -> bool {
    negated(value).is_some_and(|inner| same(inner, other))
}

/// Structural equality that never treats random calls as equal.
fn same(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b))
        | (Value::LocalizedString(a), Value::LocalizedString(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) | (Value::EventPlayer, Value::EventPlayer) => true,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (
            Value::Vector {
                x: ax,
                y: ay,
                z: az,
            },
            Value::Vector {
                x: bx,
                y: by,
                z: bz,
            },
        ) => same(ax, bx) && same(ay, by) && same(az, bz),
        (
            Value::Enum {
                value_type: at,
                value: av,
            },
            Value::Enum {
                value_type: bt,
                value: bv,
            },
        ) => at == bt && av == bv,
        (Value::GlobalVariable(a), Value::GlobalVariable(b))
        | (Value::Subroutine(a), Value::Subroutine(b)) => a == b,
        (
            Value::PlayerVariable {
                player: ap,
                variable: av,
            },
            Value::PlayerVariable {
                player: bp,
                variable: bv,
            },
        ) => av == bv && same(ap, bp),
        (Value::Call { name: an, args: aa }, Value::Call { name: bn, args: ba }) => {
            an == bn
                && !RANDOM_CALLS.contains(&an.as_str())
                && aa.len() == ba.len()
                && aa.iter().zip(ba).all(|(a, b)| same(a, b))
        }
        _ => false,
    }
}

fn mentions_element(value: &Value) -> bool {
    match value {
        Value::Call { name, args } => {
            matches!(name.as_str(), "currentArrayElement" | "currentArrayIndex")
                || args.iter().any(mentions_element)
        }
        Value::Array(elements) => elements.iter().any(mentions_element),
        Value::Vector { x, y, z } => [x, y, z].into_iter().any(|v| mentions_element(v)),
        Value::PlayerVariable { player, .. } => mentions_element(player),
        _ => false,
    }
}
