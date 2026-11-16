use crate::compile_time::Value;
use crate::diag::{OpyError, OpyResult};
use crate::{cst, hir};
use std::collections::HashMap;

fn is_map_list(scope: u8, name: &str) -> bool {
    scope == 2 && matches!(name, "enabledMaps" | "disabledMaps")
}

fn apply_value(
    element: &mut hir::SettingsListElement,
    value: Value,
    span: hir::Span,
    is_map_list: bool,
) -> OpyResult<()> {
    if is_map_list && matches!(value, Value::Object(_)) {
        resolve(element, value, span)?;
    } else {
        element.evaluated = super::display_value(&value).ok();
    }
    Ok(())
}

fn element_from_value(
    value: Value,
    span: hir::Span,
    is_map_list: bool,
) -> OpyResult<hir::SettingsListElement> {
    let text = super::display_value(&value).map_err(|message| {
        super::settings_expression_error(super::hir_span_to_diag_span(span), message)
    })?;
    let mut element = hir::SettingsListElement {
        value: text,
        evaluated: None,
        variant: None,
        span: Some(span),
    };
    apply_value(&mut element, value, span, is_map_list)?;
    Ok(element)
}

pub(crate) fn lower_settings(settings: &cst::Settings) -> hir::Settings {
    hir::Settings {
        span: Some(settings.span.into()),
        children: settings
            .children
            .iter()
            .map(crate::lower::lower_settings_node)
            .collect(),
    }
}

pub(crate) fn lower_group(
    name: &str,
    children: &[cst::SettingsNode],
    span: crate::diag::Span,
) -> hir::SettingsNode {
    hir::SettingsNode::Group {
        name: name.to_string(),
        children: children
            .iter()
            .map(crate::lower::lower_settings_node)
            .collect(),
        span: Some(span.into()),
    }
}

pub(super) fn child_scope(scope: u8, name: &str) -> u8 {
    match (scope, name) {
        (0, "gamemodes") => 1,
        (1, _) => 2,
        _ => 3,
    }
}

/// The pinned OverPy evaluates each settings list element through its
/// expression path and errors on what it cannot resolve (#512). Evaluation is
/// recorded on the element rather than emitted eagerly: elements under
/// catalogued enum keys and hero lists are member names, not expressions, and
/// only the downstream pass knows which. A one-key object in a gamemode map
/// list is a map with its variant IDs.
pub(super) fn resolve_list(
    name: String,
    elements: Vec<hir::SettingsListElement>,
    span: Option<hir::Span>,
    scope: u8,
    constants: &HashMap<String, &hir::Expr>,
    expander: &mut crate::compiler::MacroExpander<'_>,
    lowerer: &mut crate::lower::SettingsLowerer,
) -> OpyResult<hir::SettingsNode> {
    let is_map_list = is_map_list(scope, &name);
    let elements = elements
        .into_iter()
        .map(|mut element| {
            let Some(span) = element.span.filter(|_| element.evaluated.is_none()) else {
                return Ok(element);
            };
            let Ok(value) =
                super::eval_settings_text(&element.value, span, constants, expander, lowerer)
            else {
                return Ok(element);
            };
            apply_value(&mut element, value, span, is_map_list)?;
            Ok(element)
        })
        .collect::<OpyResult<Vec<_>>>()?;
    Ok(hir::SettingsNode::List {
        name,
        elements,
        span,
    })
}

fn resolve(element: &mut hir::SettingsListElement, value: Value, span: hir::Span) -> OpyResult<()> {
    let Value::Object(entries) = value else {
        unreachable!()
    };
    let error = |message: &str| {
        OpyError::at(
            "settings-expression",
            message.to_string(),
            super::hir_span_to_diag_span(span),
        )
    };
    if entries.len() != 1 {
        return Err(error("Malformed map object, should only have 1 key"));
    }
    let (key, variants) = entries.into_iter().next().unwrap();
    let Value::String(key) = key else {
        return Err(error("map name must be a string"));
    };
    let variants = match variants {
        Value::Array(variants) => variants,
        Value::String(variants) => variants
            .chars()
            .map(|c| Value::String(c.to_string()))
            .collect(),
        _ => return Err(error("map variants must be iterable")),
    };
    let variants = variants
        .iter()
        .map(super::display_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|message| error(&message))?;
    element.evaluated = Some(key);
    element.variant = (!variants.is_empty()).then(|| variants.join(" "));
    Ok(())
}

pub(super) fn node_from_value(
    name: String,
    value: crate::compile_time::Value,
    source_span: hir::Span,
    scope: u8,
) -> OpyResult<hir::SettingsNode> {
    let span = super::hir_span_to_diag_span(source_span);
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
                .map(|value| element_from_value(value, source_span, is_map_list(scope, &name)))
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
                    node_from_value(child_name, value, source_span, child_scope(scope, &name))
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
