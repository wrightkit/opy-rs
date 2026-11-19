use crate::cst;
use crate::hir::{self, SettingsNode as HirSettingsNode};

pub(crate) fn lower_settings(settings: &cst::Settings) -> hir::Settings {
    hir::Settings {
        span: Some(settings.span.into()),
        children: settings.children.iter().map(lower_settings_node).collect(),
    }
}

fn lower_settings_node(node: &cst::SettingsNode) -> HirSettingsNode {
    match node {
        cst::SettingsNode::Group {
            name,
            children,
            span,
        } => HirSettingsNode::Group {
            name: name.clone(),
            children: children.iter().map(lower_settings_node).collect(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::Number { name, value, span } => HirSettingsNode::Number {
            name: name.clone(),
            value: *value,
            span: Some((*span).into()),
        },
        cst::SettingsNode::Bool { name, value, span } => HirSettingsNode::Bool {
            name: name.clone(),
            value: *value,
            span: Some((*span).into()),
        },
        cst::SettingsNode::String { name, value, span } => HirSettingsNode::String {
            name: name.clone(),
            value: value.clone(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::Raw { name, value, span } => HirSettingsNode::Raw {
            name: name.clone(),
            value: value.clone(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::List {
            name,
            elements,
            span,
        } => HirSettingsNode::List {
            name: name.clone(),
            elements: elements
                .iter()
                .map(|element| hir::SettingsListElement {
                    value: element.value.clone(),
                    evaluated: (!element.expr).then(|| element.value.clone()),
                    variant: None,
                    span: Some(element.span.into()),
                })
                .collect(),
            span: Some((*span).into()),
        },
    }
}
