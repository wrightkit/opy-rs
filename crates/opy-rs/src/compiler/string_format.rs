//! Custom String formatting as the pinned OverPy writes it: nested strings
//! merge into their parent, constant arguments become text, and a string that
//! is too long or has too many distinct arguments is split into nested
//! strings.

use workshop_rs::Value;

use super::operator_optimization::same;

const MAX_LENGTH: usize = 128;
const MAX_ARGS: usize = 3;
const NUMBER_LIMIT: f64 = 1e7;

#[derive(Clone)]
pub(super) enum Token {
    Text(String),
    Argument(Value),
}

/// The tokens of a `customString` call: literal text and one argument per
/// placeholder occurrence.
pub(super) fn tokens(value: &Value) -> Option<Vec<Token>> {
    let Value::Call { name, args } = value else {
        return None;
    };
    if name != "customString" {
        return None;
    }
    let Some(Value::String(format)) = args.first() else {
        return None;
    };
    let mut tokens = Vec::new();
    let mut literal = String::new();
    let mut rest = format.as_str();
    while let Some(open) = rest.find('{') {
        literal.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('}')?;
        let index = after[..close].parse::<usize>().ok()?;
        let argument = args.get(index + 1)?.clone();
        if !literal.is_empty() {
            tokens.push(Token::Text(std::mem::take(&mut literal)));
        }
        tokens.push(Token::Argument(argument));
        rest = &after[close + 1..];
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        tokens.push(Token::Text(literal));
    }
    Some(tokens)
}

/// Splices nested strings and turns constant arguments into text.
pub(super) fn merge(tokens: Vec<Token>) -> (Vec<Token>, bool) {
    let mut changed = false;
    let mut merged: Vec<Token> = Vec::new();
    let mut pending: std::collections::VecDeque<Token> = tokens.into();
    while let Some(token) = pending.pop_front() {
        let Token::Argument(argument) = token else {
            push_text(&mut merged, token);
            continue;
        };
        if let Some(inner) = tokens_of(&argument) {
            changed = true;
            for token in inner.into_iter().rev() {
                pending.push_front(token);
            }
            continue;
        }
        match argument {
            Value::Null => {
                changed = true;
                push_text(&mut merged, Token::Text("0".to_string()));
            }
            Value::Number(number) if number.abs() <= NUMBER_LIMIT => {
                changed = true;
                let text = format!("{number:.2}").replace(".00", "");
                push_text(&mut merged, Token::Text(text));
            }
            other => merged.push(Token::Argument(other)),
        }
    }
    (merged, changed)
}

fn tokens_of(value: &Value) -> Option<Vec<Token>> {
    tokens(value)
}

fn push_text(tokens: &mut Vec<Token>, token: Token) {
    if let (Token::Text(text), Some(Token::Text(last))) = (&token, tokens.last_mut()) {
        last.push_str(text);
        return;
    }
    tokens.push(token);
}

/// A string with one numbered placeholder per argument occurrence.
pub(super) fn unsplit(tokens: Vec<Token>) -> Value {
    let mut text = String::new();
    let mut arguments = Vec::new();
    for token in tokens {
        match token {
            Token::Text(literal) => text.push_str(&literal),
            Token::Argument(argument) => {
                text.push_str(&format!("{{{}}}", arguments.len()));
                arguments.push(argument);
            }
        }
    }
    Value::Call {
        name: "customString".to_string(),
        args: std::iter::once(Value::String(text))
            .chain(arguments)
            .collect(),
    }
}

/// Splits every custom string in the tree into strings the Workshop accepts.
pub(super) fn split_all(value: &mut Value) {
    match value {
        Value::Call { args, .. } => args.iter_mut().for_each(split_all),
        Value::Array(elements) => elements.iter_mut().for_each(split_all),
        Value::Vector { x, y, z } => {
            split_all(x);
            split_all(y);
            split_all(z);
        }
        Value::PlayerVariable { player, .. } => split_all(player),
        _ => {}
    }
    if let Some(tokens) = tokens(value) {
        *value = split(tokens);
    }
}

fn length(text: &str) -> usize {
    text.chars().count()
}

fn split(mut tokens: Vec<Token>) -> Value {
    let count = tokens.len();
    if let [Token::Text(text)] = tokens.as_slice() {
        if length(text) <= MAX_LENGTH {
            return Value::Call {
                name: "customString".to_string(),
                args: vec![Value::String(text.clone())],
            };
        }
    }
    let mut result = String::new();
    let mut result_args: Vec<Value> = Vec::new();
    let mut string_length = 0;
    for index in 0..count {
        let mut should_split = false;
        if let Token::Text(text) = &tokens[index] {
            let reserve = if index == count - 1 { 0 } else { 3 };
            if string_length + length(text) > MAX_LENGTH.saturating_sub(reserve) {
                should_split = true;
                let characters: Vec<char> = text.chars().collect();
                let mut taken = 0;
                while string_length + taken < MAX_LENGTH - 3 && taken < characters.len() {
                    taken += 1;
                }
                result.extend(&characters[..taken]);
                tokens[index] = Token::Text(characters[taken..].iter().collect());
            }
        }
        if let Token::Argument(argument) = &tokens[index] {
            if index + 1 < count
                && string_length + 6 > MAX_LENGTH
                && !(index + 2 == count
                    && matches!(&tokens[count - 1], Token::Text(last)
                        if string_length + 3 + length(last) <= MAX_LENGTH))
            {
                should_split = true;
            }
            if result_args.len() >= MAX_ARGS - 1
                && !result_args.iter().any(|existing| same(existing, argument))
            {
                let mut unique: Vec<&Value> = Vec::new();
                for later in tokens[index..].iter().filter_map(|token| match token {
                    Token::Argument(value) => Some(value),
                    Token::Text(_) => None,
                }) {
                    if result_args.iter().all(|existing| !same(existing, later))
                        && unique.iter().all(|existing| !same(existing, later))
                    {
                        unique.push(later);
                        if unique.len() >= 2 {
                            should_split = true;
                            break;
                        }
                    }
                }
                if !should_split {
                    let remaining: usize = tokens[index..]
                        .iter()
                        .map(|token| match token {
                            Token::Argument(_) => 3,
                            Token::Text(text) => length(text),
                        })
                        .sum();
                    if string_length + remaining > MAX_LENGTH {
                        should_split = true;
                    }
                }
            }
        }
        if should_split {
            result.push_str(&format!("{{{}}}", result_args.len()));
            result_args.push(split(tokens.split_off(index)));
            break;
        }
        match &tokens[index] {
            Token::Text(text) => {
                result.push_str(text);
                string_length += length(text);
            }
            Token::Argument(argument) => {
                match result_args
                    .iter()
                    .position(|existing| same(existing, argument))
                {
                    Some(reused) => result.push_str(&format!("{{{reused}}}")),
                    None => {
                        result.push_str(&format!("{{{}}}", result_args.len()));
                        result_args.push(argument.clone());
                    }
                }
                string_length += 3;
            }
        }
    }
    Value::Call {
        name: "customString".to_string(),
        args: std::iter::once(Value::String(result))
            .chain(result_args)
            .collect(),
    }
}
