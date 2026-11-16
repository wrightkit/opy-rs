use crate::compile_time::Value;
use crate::diag::{OpyError, OpyResult};
use crate::{cst, hir};
use std::collections::HashMap;

pub(crate) fn lower_element(element: &cst::SettingsListElement) -> hir::SettingsListElement {
    hir::SettingsListElement {
        value: element.value.clone(),
        evaluated: (!element.expr).then(|| element.value.clone()),
        variant: None,
        span: Some(element.span.into()),
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
    let is_map_list = scope == 2 && matches!(name.as_str(), "enabledMaps" | "disabledMaps");
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
            if is_map_list && matches!(value, Value::Object(_)) {
                resolve(&mut element, value, span)?;
            } else {
                element.evaluated = super::display_value(&value).ok();
            }
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
    let Value::Array(variants) = variants else {
        return Err(error("map variants must be an array"));
    };
    let variants = variants
        .iter()
        .map(super::display_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|message| error(&message))?;
    element.evaluated = Some(key);
    element.variant = Some(variants.join(" "));
    Ok(())
}
