use super::*;
use workshop_rs::source::{Position as WorkshopPosition, Span as WorkshopSpan};

pub(super) fn expand_settings_constants(
    settings: crate::hir::Settings,
    constants: &HashMap<String, &Expr>,
) -> crate::hir::Settings {
    crate::hir::Settings {
        span: settings.span,
        children: settings
            .children
            .into_iter()
            .map(|node| expand_settings_node(node, constants))
            .collect(),
    }
}

fn expand_settings_node(
    node: crate::hir::SettingsNode,
    constants: &HashMap<String, &Expr>,
) -> crate::hir::SettingsNode {
    use crate::hir::SettingsNode;
    match node {
        SettingsNode::Group {
            name,
            children,
            span,
        } => {
            let children = children
                .into_iter()
                .map(|child| expand_settings_node(child, constants))
                .collect::<Vec<_>>();
            let children = if matches!(name.as_str(), "team1" | "team2" | "allTeams") {
                children
                    .into_iter()
                    .flat_map(|child| match child {
                        SettingsNode::Group { name, children, .. } if name == "general" => children,
                        child => vec![child],
                    })
                    .collect()
            } else {
                children
            };
            SettingsNode::Group {
                name,
                children,
                span,
            }
        }
        SettingsNode::Raw { name, value, span } => constants
            .get(&value)
            .and_then(|expr| settings_node_from_expr(name.clone(), expr, constants, span))
            .unwrap_or(SettingsNode::Raw { name, value, span }),
        node => node,
    }
}

fn settings_node_from_expr(
    name: String,
    expr: &Expr,
    constants: &HashMap<String, &Expr>,
    span: Option<HirSpan>,
) -> Option<crate::hir::SettingsNode> {
    use crate::hir::SettingsNode;
    match expr {
        Expr::Constant { name: value, .. } => constants
            .get(value)
            .and_then(|expr| settings_node_from_expr(name, expr, constants, span)),
        Expr::Dict { entries, .. } => Some(SettingsNode::Group {
            name,
            children: entries
                .iter()
                .filter_map(|entry| {
                    let Expr::String {
                        value: child_name, ..
                    } = entry.key.as_ref()
                    else {
                        return None;
                    };
                    settings_node_from_expr(child_name.clone(), &entry.value, constants, entry.span)
                })
                .flat_map(|child| match child {
                    SettingsNode::Group { name, children, .. } if name == "general" => children,
                    child => vec![child],
                })
                .collect(),
            span,
        }),
        Expr::Number { value, .. } => Some(SettingsNode::Number {
            name,
            value: *value,
            span,
        }),
        Expr::Bool { value, .. } => Some(SettingsNode::Bool {
            name,
            value: *value,
            span,
        }),
        Expr::String { value, .. } => Some(SettingsNode::String {
            name,
            value: value.clone(),
            span,
        }),
        _ => None,
    }
}

pub(super) fn convert_settings(settings: crate::hir::Settings) -> workshop_rs::settings::Settings {
    let mut settings = workshop_rs::settings::Settings {
        span: settings.span.map(convert_settings_span),
        children: settings
            .children
            .into_iter()
            .map(convert_settings_node)
            .collect(),
    };
    clear_settings_spans(&mut settings);
    settings
}

fn clear_settings_spans(settings: &mut workshop_rs::settings::Settings) {
    settings.span = None;
    for node in &mut settings.children {
        clear_settings_node_spans(node);
    }
}

fn clear_settings_node_spans(node: &mut workshop_rs::settings::SettingsNode) {
    use workshop_rs::settings::SettingsNode;
    match node {
        SettingsNode::Workshop { children, span } | SettingsNode::Group { children, span, .. } => {
            *span = None;
            for child in children {
                clear_settings_node_spans(child);
            }
        }
        SettingsNode::Number { span, .. }
        | SettingsNode::Bool { span, .. }
        | SettingsNode::Flag { span, .. }
        | SettingsNode::String { span, .. }
        | SettingsNode::Raw { span, .. } => *span = None,
        SettingsNode::List { elements, span, .. } => {
            *span = None;
            for element in elements {
                element.span = None;
            }
        }
    }
}

fn convert_settings_node(node: crate::hir::SettingsNode) -> workshop_rs::settings::SettingsNode {
    use crate::hir::SettingsNode as SourceNode;
    use workshop_rs::settings::{SettingsListElement, SettingsNode as TargetNode};

    match node {
        SourceNode::Group {
            name,
            children,
            span,
        } if name == "workshop" => TargetNode::Workshop {
            children: children.into_iter().map(convert_workshop_node).collect(),
            span: span.map(convert_settings_span),
        },
        SourceNode::Group {
            name,
            children,
            span,
        } => TargetNode::Group {
            name,
            children: children.into_iter().map(convert_settings_node).collect(),
            span: span.map(convert_settings_span),
        },
        SourceNode::Number { name, value, span } => TargetNode::Number {
            name,
            value,
            span: span.map(convert_settings_span),
        },
        SourceNode::Bool { name, value, span } => TargetNode::Bool {
            name,
            value,
            span: span.map(convert_settings_span),
        },
        SourceNode::String { name, value, span } => TargetNode::String {
            name: name.clone(),
            value: if name == "mapRotation" && value == "afterGame" {
                "afterAGame".to_string()
            } else {
                value
            },
            span: span.map(convert_settings_span),
        },
        SourceNode::Raw { name, value, span } => TargetNode::Raw {
            name,
            value,
            span: span.map(convert_settings_span),
        },
        SourceNode::List {
            name,
            elements,
            span,
        } => TargetNode::List {
            name,
            elements: elements
                .into_iter()
                .map(|element| SettingsListElement {
                    value: element.value,
                    span: element.span.map(convert_settings_span),
                })
                .collect(),
            span: span.map(convert_settings_span),
        },
    }
}

fn convert_workshop_node(node: crate::hir::SettingsNode) -> workshop_rs::settings::SettingsNode {
    use crate::hir::SettingsNode as SourceNode;
    use workshop_rs::settings::SettingsNode as TargetNode;

    match node {
        SourceNode::Group {
            name,
            children,
            span,
        } => TargetNode::Group {
            name,
            children: children.into_iter().map(convert_workshop_node).collect(),
            span: span.map(convert_settings_span),
        },
        SourceNode::Number { name, value, span } => TargetNode::Raw {
            name,
            value: value.to_string(),
            span: span.map(convert_settings_span),
        },
        SourceNode::Bool { name, value, span } => TargetNode::Raw {
            name,
            value: value.to_string(),
            span: span.map(convert_settings_span),
        },
        SourceNode::String { name, value, span } | SourceNode::Raw { name, value, span } => {
            TargetNode::Raw {
                name,
                value,
                span: span.map(convert_settings_span),
            }
        }
        SourceNode::List {
            name,
            elements,
            span,
        } => TargetNode::Raw {
            name,
            value: format!(
                "[{}]",
                elements
                    .into_iter()
                    .map(|element| element.value)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            span: span.map(convert_settings_span),
        },
    }
}

fn convert_settings_span(span: HirSpan) -> WorkshopSpan {
    WorkshopSpan::new(
        workshop_rs::source::FileId::from_index(span.file as usize),
        WorkshopPosition::new(span.start.line, span.start.col),
        WorkshopPosition::new(span.end.line, span.end.col),
    )
}
