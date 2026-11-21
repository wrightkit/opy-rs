use super::map_variants;
use crate::diag::{OpyResult, Span};
use crate::hir;
use std::collections::HashSet;

pub(super) fn validate_evaluated(value: &crate::compile_time::Value, span: Span) -> OpyResult<()> {
    use crate::compile_time::Value;

    match value {
        Value::Object(entries) => {
            let mut keys = HashSet::new();
            for (key, value) in entries {
                let Value::String(key) = key else {
                    return Err(super::settings_expression_error(
                        span,
                        "compile-time object keys must be strings".to_string(),
                    ));
                };
                if !keys.insert(key) {
                    return Err(super::settings_expression_error(
                        span,
                        format!("duplicate key '{key}' in settings object"),
                    ));
                }
                validate_evaluated(value, span)?;
            }
        }
        Value::Array(values) | Value::Vector(values) => {
            for value in values {
                validate_evaluated(value, span)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A settings node from an evaluated compile-time value; `scope` tracks
/// whether a list is a gamemode map list.
pub(super) fn node_from_value(
    name: String,
    value: crate::compile_time::Value,
    source_span: hir::Span,
    scope: u8,
) -> OpyResult<hir::SettingsNode> {
    let span = Span::from_hir(source_span);
    let node = match value {
        // `String(numValue)` writes `NaN`/`Infinity` literally upstream —
        // `x: 5%0` is `x: NaN` and `x: Infinity` is `x: Infinity` (#512).
        crate::compile_time::Value::Number(value) => hir::SettingsNode::Number {
            name,
            value,
            span: Some(span.into()),
        },
        crate::compile_time::Value::String(value) => {
            if crate::compile_time::has_unbound_formatters(&value) {
                return Err(super::settings_expression_error(
                    span,
                    format!("settings string '{value}' has format arguments"),
                ));
            }
            hir::SettingsNode::String {
                name,
                value,
                span: Some(span.into()),
            }
        }
        crate::compile_time::Value::Bool(value) => hir::SettingsNode::Bool {
            name,
            value,
            span: Some(span.into()),
        },
        crate::compile_time::Value::Array(values) => hir::SettingsNode::List {
            name: name.clone(),
            elements: values
                .into_iter()
                .map(|value| {
                    map_variants::element_from_value(
                        value,
                        source_span,
                        map_variants::is_map_list(scope, &name),
                    )
                })
                .collect::<OpyResult<Vec<_>>>()?,
            span: Some(span.into()),
        },
        crate::compile_time::Value::Object(entries) => hir::SettingsNode::Group {
            name: name.clone(),
            children: entries
                .into_iter()
                .map(|(key, value)| {
                    let crate::compile_time::Value::String(child_name) = key else {
                        return Err(super::settings_expression_error(
                            span,
                            "compile-time object keys must be strings".to_string(),
                        ));
                    };
                    node_from_value(
                        child_name,
                        value,
                        source_span,
                        map_variants::child_scope(scope, &name),
                    )
                })
                .collect::<OpyResult<Vec<_>>>()?,
            span: Some(span.into()),
        },
        // A `vect` AST has no settings object form upstream — the
        // reference reports it unhandled (#512).
        crate::compile_time::Value::Vector(_) | crate::compile_time::Value::Null => {
            return Err(super::settings_expression_error(
                span,
                "compile-time value is not a settings value".to_string(),
            ));
        }
    };
    Ok(node)
}

/// The text JavaScript `String(value)` gives for a settings value.
pub(super) fn display_value(value: &crate::compile_time::Value) -> Result<String, String> {
    match value {
        // `String(value)` writes `NaN`/`Infinity` literally, as the pinned
        // evaluator does for `%` results and unparsed number tokens (#512).
        crate::compile_time::Value::Number(value) => {
            Ok(crate::compiler::number_format::javascript_text(*value))
        }
        crate::compile_time::Value::String(value) => {
            if crate::compile_time::has_unbound_formatters(value) {
                return Err(format!("settings string '{value}' has format arguments"));
            }
            Ok(value.clone())
        }
        crate::compile_time::Value::Bool(value) => Ok(value.to_string()),
        // JavaScript `String(value)` on containers, as the pinned evaluator
        // writes them: arrays join their element text with commas, objects
        // string-ify to the literal `[object Object]` (#512).
        crate::compile_time::Value::Array(values) => values
            .iter()
            .map(display_value)
            .collect::<Result<Vec<_>, _>>()
            .map(|text| text.join(",")),
        crate::compile_time::Value::Object(_) => Ok("[object Object]".to_string()),
        crate::compile_time::Value::Vector(_) | crate::compile_time::Value::Null => {
            Err("compile-time value is not a settings value".to_string())
        }
    }
}
