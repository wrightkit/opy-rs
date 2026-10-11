use super::*;
use workshop_rs::source::{Position as WorkshopPosition, Span as WorkshopSpan};

/// Convert resolved HIR settings into the canonical Workshop settings
/// carrier exactly as lowering does: constant expansion, then `#!extension`
/// merging, then pass-through of members outside the settings catalog.
/// Shared by lowering and by `check`'s emission-acceptance pass so `check`
/// and `compile` see the same settings tree (#411). The second value lists
/// the members passed through unchanged.
pub(crate) fn workshop_settings(
    hir: &crate::hir::Program,
) -> Result<(Option<workshop_rs::settings::Settings>, Vec<UnknownSetting>), IntegrationError> {
    let settings_constants: HashMap<String, &Expr> = hir
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            hir::Declaration::Constant { name, value, .. } => Some((name.clone(), value.as_ref())),
            _ => None,
        })
        .collect();
    let (mut settings, evals) = merge_extensions(
        hir.settings
            .clone()
            .map(|settings| expand_settings_constants(settings, &settings_constants)),
        &hir.preprocessing.directives,
    )?;
    let unknown = settings
        .as_mut()
        .map(|settings| pass_through_unknown_members(settings, &evals))
        .transpose()?
        .unwrap_or_default();
    Ok((settings, unknown))
}

/// A settings member the catalog does not declare, emitted as written.
pub(crate) struct UnknownSetting {
    /// The member's key.
    pub(crate) name: String,
    /// The undeclared value of a catalogued key; `None` for an unknown key.
    pub(crate) value: Option<String>,
    pub(crate) span: Option<WorkshopSpan>,
}

/// Whether the pinned OverPy schema applies a hero settings key to a
/// specific hero. Its post-load schema merge expands
/// `heroes.values.__generalAndEachHero__` plus the `__eachHero__` keys whose
/// `include`/`exclude` hero filters admit the hero into that hero's `values`
/// set; `compileCustomGameSettingsDict` then looks authored keys up in the
/// merged set and writes the ones it cannot find back verbatim. The
/// `data/hero_applicability.json` artifact records each merged key's
/// applying heroes in the smaller of include/exclude form and is generated
/// from the pinned compiler's own merge by
/// `tools/overpy/gen_hero_applicability.cjs`.
fn hero_setting_applies(hero: &str, key: &str) -> bool {
    let data = applicability();
    if data.all.iter().any(|candidate| candidate == key) {
        return true;
    }
    if let Some(heroes) = data.only.get(key) {
        return heroes.iter().any(|candidate| candidate == hero);
    }
    if let Some(heroes) = data.except.get(key) {
        return !heroes.iter().any(|candidate| candidate == hero);
    }
    false
}

#[derive(serde::Deserialize)]
struct HeroApplicability {
    all: Vec<String>,
    only: std::collections::HashMap<String, Vec<String>>,
    except: std::collections::HashMap<String, Vec<String>>,
}

fn applicability() -> &'static HeroApplicability {
    static DATA: std::sync::OnceLock<HeroApplicability> = std::sync::OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("data/hero_applicability.json"))
            .expect("hero applicability data is valid JSON")
    })
}

/// The pinned compiler rewrites these authored hero spellings under
/// `heroes.<team>` — group names and hero-list elements alike — before the
/// schema lookup (`compileCustomGameSettingsDict`). The order matters: group
/// renames run as one pass per alias in this order.
const HERO_NAME_ALIASES: &[(&str, &str)] = &[("mccree", "cassidy"), ("hammond", "wreckingBall")];

fn canonical_hero_name(name: &str) -> &str {
    HERO_NAME_ALIASES
        .iter()
        .find_map(|(alias, canonical)| (*alias == name).then_some(*canonical))
        .unwrap_or(name)
}

fn rename_member(member: &mut workshop_rs::settings::SettingsNode, name: &str) {
    use workshop_rs::settings::SettingsNode;
    match member {
        SettingsNode::Group { name: slot, .. }
        | SettingsNode::Number { name: slot, .. }
        | SettingsNode::Bool { name: slot, .. }
        | SettingsNode::Flag { name: slot, .. }
        | SettingsNode::String { name: slot, .. }
        | SettingsNode::List { name: slot, .. }
        | SettingsNode::Raw { name: slot, .. }
        | SettingsNode::RawValue { name: slot, .. } => *slot = name.to_string(),
        SettingsNode::Workshop { .. } => {}
    }
}

/// The pinned OverPy writes a `main`, `lobby`, mode, team `general`, or hero
/// member it cannot translate as authored instead of rejecting it: an unknown
/// key with its value (`key: value`, a list or object as a block), and a
/// catalogued enum key with a value outside its domain. Projects rely on this
/// for settings the catalog lacks, often rewriting them in a post-compile
/// hook. Replace each such member with its written form, drop a mode's
/// non-Boolean `enabled` as upstream does, and lift each team's `general`
/// members ahead of its hero groups as the pinned OverPy writes them.
fn pass_through_unknown_members(
    settings: &mut workshop_rs::settings::Settings,
    evals: &EvalMap,
) -> Result<Vec<UnknownSetting>, IntegrationError> {
    use workshop_rs::settings::{PathPart, SettingsNode};

    let mut unknown = Vec::new();
    for group in &mut settings.children {
        let SettingsNode::Group { name, children, .. } = group else {
            continue;
        };
        match name.as_str() {
            "main" | "lobby" => {
                pass_through_members(children, &[PathPart::Part(name)], &mut unknown, None, evals)?;
            }
            "gamemodes" => {
                for mode in children {
                    if let SettingsNode::Group { name, children, .. } = mode {
                        children.retain(|member| {
                            member.name() != "enabled"
                                || matches!(member, SettingsNode::Bool { .. })
                        });
                        let path = [PathPart::Part("gamemodes"), PathPart::Part(name)];
                        pass_through_members(children, &path, &mut unknown, None, evals)?;
                    }
                }
            }
            "heroes" => {
                for team in children {
                    let SettingsNode::Group {
                        name: team_name,
                        children,
                        ..
                    } = team
                    else {
                        continue;
                    };
                    // Any other team member is a hero name to upstream.
                    let mut general = Vec::new();
                    let mut rest = Vec::new();
                    for child in std::mem::take(children) {
                        match child {
                            SettingsNode::Group { name, children, .. } if name == "general" => {
                                general.extend(children);
                            }
                            // Upstream iterates the value's `Object.keys`: a
                            // non-dict `general` yields index-keyed members for
                            // strings and lists, and drops numbers/booleans.
                            child if child.name() == "general" => match child {
                                SettingsNode::String { value, span, .. } => {
                                    general.extend(value.chars().enumerate().map(|(index, ch)| {
                                        SettingsNode::Raw {
                                            name: index.to_string(),
                                            value: ch.to_string(),
                                            span,
                                        }
                                    }));
                                }
                                SettingsNode::List { elements, .. } => {
                                    for (index, element) in elements.iter().enumerate() {
                                        general.push(SettingsNode::Raw {
                                            name: index.to_string(),
                                            value: list_element_text(element, evals)?,
                                            span: element.span,
                                        });
                                    }
                                }
                                _ => {}
                            },
                            child => rest.push(child),
                        }
                    }
                    let team_path = [PathPart::Part("heroes"), PathPart::Team];
                    pass_through_members(&mut general, &team_path, &mut unknown, None, evals)?;
                    let mut enabled = false;
                    let mut disabled = false;
                    for child in &rest {
                        enabled |= child.name() == "enabledHeroes";
                        disabled |= child.name() == "disabledHeroes";
                    }
                    if enabled && disabled {
                        return Err(IntegrationError::new(
                            "settings-hero-lists",
                            format!(
                                "Cannot have both 'enabledHeroes' and 'disabledHeroes' in team '{team_name}'"
                            ),
                            None,
                        ));
                    }
                    // The pinned compiler emits `enabledHeroes`/`disabledHeroes`
                    // after every hero group regardless of authored position.
                    let (mut hero_lists, mut rest): (Vec<_>, Vec<_>) =
                        rest.into_iter().partition(|child| {
                            matches!(child, SettingsNode::List { name, .. } if matches!(
                                name.as_str(),
                                "enabledHeroes" | "disabledHeroes"
                            ))
                        });
                    // Canonical hero-group names take upstream's assign+delete
                    // rename: the source group lands at the destination's
                    // position, collapsing a duplicate, or at the end of the
                    // hero groups when the destination is absent. Upstream
                    // runs one pass per alias in `HERO_NAME_ALIASES` order, so
                    // appended groups follow that order, not authored order.
                    for (alias, canonical) in HERO_NAME_ALIASES {
                        let mut index = 0;
                        while index < rest.len() {
                            let SettingsNode::Group { name, .. } = &rest[index] else {
                                index += 1;
                                continue;
                            };
                            if name != alias {
                                index += 1;
                                continue;
                            }
                            let mut moved = rest.remove(index);
                            if let SettingsNode::Group { name, .. } = &mut moved {
                                *name = (*canonical).to_string();
                            }
                            match rest.iter_mut().find(|m| m.name() == *canonical) {
                                Some(dest) => *dest = moved,
                                None => rest.push(moved),
                            }
                        }
                    }
                    // The flattened `general` members are not reprocessed:
                    // only authored hero groups and hero rosters take the
                    // hero-name and applicability passes.
                    for hero in rest.iter_mut().chain(hero_lists.iter_mut()) {
                        match hero {
                            SettingsNode::Group { name, children, .. } => {
                                let canonical = canonical_hero_name(name).to_string();
                                // The pinned compiler rewrites `ability1KB%`
                                // to `ability1Kb%` inside hero settings before
                                // the schema lookup; the rewritten member takes
                                // the source value at the destination key's
                                // position, collapsing a duplicate, and moves to
                                // the end when the destination is absent.
                                if let Some(source) =
                                    children.iter().position(|m| m.name() == "ability1KB%")
                                {
                                    let mut moved = children.remove(source);
                                    rename_member(&mut moved, "ability1Kb%");
                                    match children.iter_mut().find(|m| m.name() == "ability1Kb%") {
                                        Some(dest) => *dest = moved,
                                        None => children.push(moved),
                                    }
                                }
                                let path =
                                    [PathPart::Part("heroes"), PathPart::Team, PathPart::Hero];
                                pass_through_members(
                                    children,
                                    &path,
                                    &mut unknown,
                                    Some(canonical.as_str()),
                                    evals,
                                )?;
                            }
                            SettingsNode::List { name, elements, .. }
                                if matches!(name.as_str(), "enabledHeroes" | "disabledHeroes") =>
                            {
                                for element in elements {
                                    let text = list_element_text(element, evals)?;
                                    let canonical = canonical_hero_name(&text);
                                    element.value = canonical.to_string();
                                }
                            }
                            _ => {}
                        }
                    }
                    general.extend(rest);
                    general.extend(hero_lists);
                    *children = general;
                }
            }
            _ => {}
        }
    }
    Ok(unknown)
}

/// The source span of each list element the settings evaluator could not
/// resolve, keyed by the converted element span (#512). Such an element is
/// an error wherever the list is written.
pub(super) type EvalMap = HashMap<(u32, u32, u32, u32), HirSpan>;

fn eval_key(span: &WorkshopSpan) -> (u32, u32, u32, u32) {
    (span.start.line, span.start.col, span.end.line, span.end.col)
}

/// The emitted text of a settings list element: the display text the
/// element already carries (evaluated for expressions, authored for
/// literals). An authored expression the evaluator could not resolve is an
/// error, as the pinned compiler rejects it wherever the list appears
/// (#512).
fn list_element_text(
    element: &workshop_rs::settings::SettingsListElement,
    evals: &EvalMap,
) -> Result<String, IntegrationError> {
    match element
        .span
        .as_ref()
        .and_then(|span| evals.get(&eval_key(span)))
    {
        Some(&span) => Err(IntegrationError::new(
            "settings-expression",
            format!(
                "settings list element '{}' is not a compile-time value",
                element.value
            ),
            Some(span),
        )),
        None => Ok(element.value.clone()),
    }
}

fn pass_through_members(
    members: &mut [workshop_rs::settings::SettingsNode],
    path: &[workshop_rs::settings::PathPart<'_>],
    unknown: &mut Vec<UnknownSetting>,
    hero: Option<&str>,
    evals: &EvalMap,
) -> Result<(), IntegrationError> {
    use workshop_rs::settings::{PathPart, SettingValueDomain, SettingsNode};

    for member in members {
        // A mode's Boolean `enabled` is consumed by the mode header.
        if member.name() == "enabled" && matches!(path, [PathPart::Part("gamemodes"), _]) {
            continue;
        }
        let name = member.name().to_string();
        let span = member.span();
        let mut full = path.to_vec();
        full.push(PathPart::Part(&name));
        let definition = workshop_rs::settings::definition(&full);
        // A catalogued key the pinned schema does not apply to this hero is
        // unknown for it and passes through like any other unknown key, but
        // fully verbatim: a `RawValue` at the catalogued path would still
        // emit the canonical name.
        let inapplicable = definition.is_some()
            && hero.is_some_and(|hero| !hero_setting_applies(hero, name.as_str()));
        match definition.filter(|_| !inapplicable) {
            None => {
                if matches!(member, SettingsNode::Raw { .. }) {
                    continue;
                }
                *member = if inapplicable {
                    verbatim_form(member.clone(), evals)?
                } else {
                    written_form(member.clone(), evals)?
                };
                unknown.push(UnknownSetting {
                    name,
                    value: None,
                    span,
                });
            }
            Some(definition) => {
                if !matches!(definition.domain(), SettingValueDomain::Enum { .. }) {
                    continue;
                }
                let value = match member {
                    SettingsNode::String { value, .. }
                        if !definition.enum_members().any(|member| member.id() == value) =>
                    {
                        value.clone()
                    }
                    SettingsNode::Number { .. } | SettingsNode::Bool { .. } => {
                        scalar_text(member).expect("scalar member")
                    }
                    // A list or object under a catalogued non-list key is
                    // carried as a written block, the shape the pinned
                    // reference emits (`Map Rotation { a }`,
                    // `Map Rotation { a: 1 }`), not a kind-mismatch error
                    // (#496). The group form is what workshop-rs accepts and
                    // writes under the catalogued key's display name.
                    SettingsNode::List { .. } | SettingsNode::Group { .. } => {
                        *member = written_form(member.clone(), evals)?;
                        continue;
                    }
                    _ => continue,
                };
                *member = SettingsNode::RawValue {
                    name: name.clone(),
                    value: value.clone(),
                    span,
                };
                unknown.push(UnknownSetting {
                    name,
                    value: Some(value),
                    span,
                });
            }
        }
    }
    Ok(())
}

/// The written form of a member under an unknown key, as the pinned OverPy
/// serializes it: scalars as `key: value`, lists as a block of bare lines,
/// and objects as a block of their members. List elements write their
/// evaluated expression value (#512).
fn written_form(
    node: workshop_rs::settings::SettingsNode,
    evals: &EvalMap,
) -> Result<workshop_rs::settings::SettingsNode, IntegrationError> {
    use workshop_rs::settings::SettingsNode;

    Ok(match node {
        SettingsNode::List {
            name,
            elements,
            span,
        } => {
            let mut children = Vec::with_capacity(elements.len());
            for element in elements {
                let span = element.span;
                children.push(SettingsNode::Raw {
                    name: list_element_text(&element, evals)?,
                    value: String::new(),
                    span,
                });
            }
            SettingsNode::Group {
                name,
                children,
                span,
            }
        }
        SettingsNode::Group {
            name,
            children,
            span,
        } => SettingsNode::Group {
            name,
            children: children
                .into_iter()
                .map(|child| written_form(child, evals))
                .collect::<Result<_, _>>()?,
            span,
        },
        node => match scalar_text(&node) {
            Some(value) => SettingsNode::RawValue {
                name: node.name().to_string(),
                value,
                span: node.span(),
            },
            None => node,
        },
    })
}

/// The fully verbatim form of a member: like [`written_form`], but a scalar
/// becomes `Raw` so its name is written as authored even when the key
/// resolves in the catalog.
fn verbatim_form(
    node: workshop_rs::settings::SettingsNode,
    evals: &EvalMap,
) -> Result<workshop_rs::settings::SettingsNode, IntegrationError> {
    use workshop_rs::settings::SettingsNode;

    match scalar_text(&node) {
        Some(value) => Ok(SettingsNode::Raw {
            name: node.name().to_string(),
            value,
            span: node.span(),
        }),
        None => written_form(node, evals),
    }
}

/// A scalar value as JavaScript's `String(value)` writes it.
fn scalar_text(node: &workshop_rs::settings::SettingsNode) -> Option<String> {
    use workshop_rs::settings::SettingsNode;

    match node {
        SettingsNode::Number { value, .. } => Some(super::number_format::javascript_text(*value)),
        SettingsNode::Bool { value, .. } => Some(value.to_string()),
        SettingsNode::String { value, .. } => Some(value.clone()),
        _ => None,
    }
}

pub(super) fn merge_extensions(
    settings: Option<crate::hir::Settings>,
    directives: &[crate::hir::DirectiveRecord],
) -> Result<(Option<workshop_rs::settings::Settings>, EvalMap), IntegrationError> {
    let mut extension_nodes: Vec<workshop_rs::settings::SettingsNode> = Vec::new();
    for directive in directives
        .iter()
        .filter(|directive| directive.name == "extension")
    {
        let Some(name) = directive.value.as_deref() else {
            return Err(IntegrationError::new(
                "directive-invalid",
                "`#!extension` expects one argument",
                directive.span,
            ));
        };
        let span = directive.span;
        if extension_nodes.iter().any(|node| node.name() == name) {
            continue;
        }
        let path = [
            workshop_rs::settings::PathPart::Part("extensions"),
            workshop_rs::settings::PathPart::Part(name),
        ];
        let Some(definition) = workshop_rs::settings::definition(&path) else {
            return Err(IntegrationError::new(
                "directive-invalid",
                format!("unknown Workshop extension `{name}`"),
                span,
            ));
        };
        if !matches!(
            definition.domain(),
            workshop_rs::settings::SettingValueDomain::PresenceOnly
        ) {
            return Err(IntegrationError::new(
                "directive-invalid",
                format!("Workshop extension `{name}` is not a presence-only setting"),
                span,
            ));
        }
        extension_nodes.push(workshop_rs::settings::SettingsNode::Flag {
            name: name.to_string(),
            span: span.map(convert_settings_span),
        });
    }
    let (settings, evals) = settings
        .map(convert_settings)
        .map(|(settings, evals)| (Some(settings), evals))
        .unwrap_or_default();
    if extension_nodes.is_empty() {
        return Ok((settings, evals));
    }

    let mut settings = settings.unwrap_or(workshop_rs::settings::Settings {
        span: None,
        children: Vec::new(),
    });
    if let Some(workshop_rs::settings::SettingsNode::Group { children, .. }) = settings
        .children
        .iter_mut()
        .find(|node| node.name() == "extensions")
    {
        for extension in extension_nodes {
            if !children.iter().any(|node| node.name() == extension.name()) {
                children.push(extension);
            }
        }
    } else {
        settings
            .children
            .push(workshop_rs::settings::SettingsNode::Group {
                name: "extensions".to_string(),
                children: extension_nodes,
                span: None,
            });
    }
    Ok((Some(settings), evals))
}

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

pub(super) fn convert_settings(
    settings: crate::hir::Settings,
) -> (workshop_rs::settings::Settings, EvalMap) {
    let mut evals = EvalMap::new();
    let settings = workshop_rs::settings::Settings {
        span: settings.span.map(convert_settings_span),
        children: settings
            .children
            .into_iter()
            .map(|node| convert_settings_node(node, &mut evals))
            .collect(),
    };
    (settings, evals)
}

fn convert_settings_node(
    node: crate::hir::SettingsNode,
    evals: &mut EvalMap,
) -> workshop_rs::settings::SettingsNode {
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
            children: children
                .into_iter()
                .map(|child| escape_main_string(&name, child))
                .map(|child| convert_settings_node(child, evals))
                .collect(),
            name,
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
                .map(|element| {
                    let hir_span = element.span;
                    let span = hir_span.map(convert_settings_span);
                    if element.evaluated.is_none()
                        && let (Some(span), Some(hir_span)) = (span.as_ref(), hir_span)
                    {
                        evals.insert(eval_key(span), hir_span);
                    }
                    // Catalogued list keys (`enabledMaps`, hero rosters) read
                    // the display text the evaluator produced (#512); the
                    // element's authored text only survives through the
                    // verbatim `workshop` group.
                    SettingsListElement {
                        value: element.evaluated.unwrap_or(element.value),
                        span,
                    }
                })
                .collect(),
            span: span.map(convert_settings_span),
        },
    }
}

/// `main.modeName` and `main.description` are the settings strings the pinned
/// OverPy escapes for filtered words.
fn escape_main_string(group: &str, node: crate::hir::SettingsNode) -> crate::hir::SettingsNode {
    use crate::hir::SettingsNode;
    match node {
        SettingsNode::String { name, value, span }
            if group == "main" && matches!(name.as_str(), "modeName" | "description") =>
        {
            SettingsNode::String {
                value: super::lowering::escape_bad_words(&value),
                name,
                span,
            }
        }
        node => node,
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
            value: number_format::javascript_text(value),
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
