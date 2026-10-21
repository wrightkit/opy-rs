use std::collections::{HashMap, HashSet};
use std::fmt;

use workshop_rs::Program;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::program::{
    Action, Event, EventTarget, EventTeam, ModifyOp, Rule, Value, Variable,
};
use workshop_rs::settings::{PathPart, SettingValueDomain, SettingsNode};
use workshop_rs::source::Span;

use crate::lexer::{is_ident_continue, is_ident_start, is_identifier};
use crate::lower::special_forms::SpecialValueCall;
use crate::lower::{NameNamespace, is_reserved_name, policy};
use crate::manifest::{Function, FunctionKind, Manifest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructIssue {
    pub code: &'static str,
    pub message: String,
    pub span: Option<Span>,
}

/// All reconstruction failures for one program. The emitter never returns
/// partial output: a non-empty issue list means no OPY was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructError {
    pub issues: Vec<ReconstructIssue>,
}

impl fmt::Display for ReconstructError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, issue) in self.issues.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            let location = match issue.span {
                Some(span) => format!(" at {}:{}", span.start.line, span.start.col),
                None => String::new(),
            };
            write!(f, "{}: {}{location}", issue.code, issue.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ReconstructError {}

/// Reconstruct a canonical Workshop program into OPY source.
///
/// Resolves builtin identities through the built-in OPY semantic manifest
/// and Workshop catalog (`en-US`). Returns all diagnostics when a construct
/// cannot be represented in OPY.
pub fn reconstruct(program: &Program) -> Result<String, ReconstructError> {
    let manifest = match Manifest::builtin() {
        Ok(manifest) => manifest,
        Err(error) => {
            return Err(ReconstructError {
                issues: vec![ReconstructIssue {
                    code: "manifest-error",
                    message: format!(
                        "cannot load the OPY semantic compatibility manifest: {error}"
                    ),
                    span: None,
                }],
            });
        }
    };
    let catalog = match Catalog::builtin_ref() {
        Ok(catalog) => catalog,
        Err(error) => {
            return Err(ReconstructError {
                issues: vec![ReconstructIssue {
                    code: "catalog-error",
                    message: format!("cannot load the Workshop catalog: {error}"),
                    span: None,
                }],
            });
        }
    };
    reconstruct_with(program, manifest, catalog, &Locale::new("en-US"))
}

/// The context-sensitive form of [`reconstruct`]: resolves identities through
/// the supplied manifest and catalog. The locale selects the catalog
/// spellings used for cross-checks (reconstruction emits OPY, which is
/// locale-independent; `en-US` is the catalog's declared surface).
pub fn reconstruct_with(
    program: &Program,
    manifest: &Manifest,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<String, ReconstructError> {
    let mut emitter = Emitter::new(program, manifest, catalog, locale);
    emitter.run();
    if emitter.issues.is_empty() {
        Ok(emitter.out)
    } else {
        Err(ReconstructError {
            issues: emitter.issues,
        })
    }
}

const SHADOWED_SUBROUTINE_NAMES: &[&str] = &[
    "_",
    "__",
    "___",
    "compressed",
    "vect",
    "async",
    "startRule",
    "range",
];

const BINARY_OPS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "==", "!=", "<", "<=", ">", ">=", "and", "or",
];

const ARITHMETIC_OPS: &[(&str, &str)] = &[
    ("add", "+"),
    ("subtract", "-"),
    ("multiply", "*"),
    ("divide", "/"),
    ("modulo", "%"),
    ("raiseToPower", "**"),
];

const DEDICATED_ACTION_NAMES: &[&str] = &["append"];
const DEDICATED_VALUE_NAMES: &[&str] = &["vect", "range", "chase"];

const INDEXED_MODIFY_OPS: &[(&str, &str)] = &[
    ("add", "+="),
    ("subtract", "-="),
    ("multiply", "*="),
    ("divide", "/="),
    ("modulo", "%="),
    ("raiseToPower", "**="),
    ("min", "min="),
    ("max", "max="),
];

struct Emitter<'a> {
    program: &'a Program,
    manifest: &'a Manifest,
    catalog: &'a Catalog,
    locale: &'a Locale,
    issues: Vec<ReconstructIssue>,
    out: String,
    global_names: HashMap<String, String>,
    player_names: HashMap<String, String>,
    subroutine_emitted: HashMap<String, String>,
    subroutine_names: HashSet<String>,
    emitted_subroutine_names: HashSet<String>,
    declared_names: HashSet<String>,
    labels: HashMap<usize, Vec<String>>,
    skip_labels: HashMap<usize, String>,
    generated_counter: usize,
    lambda_binders: Vec<(String, Option<String>)>,
    loop_depth: usize,
}

type IndexedRule<'a> = (usize, &'a Rule);

type Initializers = std::collections::HashMap<usize, (Value, Option<Span>)>;

struct RuleLayout<'a> {
    global_init: Option<Initializers>,
    player_init: Option<Initializers>,
    sub_rules: Vec<IndexedRule<'a>>,
    normal_rules: Vec<IndexedRule<'a>>,
}

impl<'a> Emitter<'a> {
    fn new(
        program: &'a Program,
        manifest: &'a Manifest,
        catalog: &'a Catalog,
        locale: &'a Locale,
    ) -> Self {
        let global_names = mangle_names(
            program
                .global_variables
                .iter()
                .map(|variable| variable.name.as_str()),
            NameNamespace::Global,
        );
        let player_names = mangle_names(
            program
                .player_variables
                .iter()
                .map(|variable| variable.name.as_str()),
            NameNamespace::Player,
        );
        let subroutine_emitted = mangle_names(
            program
                .subroutines
                .iter()
                .map(|subroutine| subroutine.name.as_str()),
            NameNamespace::Subroutine,
        );
        let emitted_subroutine_names = subroutine_emitted.values().cloned().collect();
        let subroutine_names = program
            .subroutines
            .iter()
            .map(|subroutine| subroutine.name.clone())
            .collect();
        let declared_names = program
            .global_variables
            .iter()
            .chain(&program.player_variables)
            .map(|variable| variable.name.clone())
            .chain(program.subroutines.iter().map(|s| s.name.clone()))
            .collect();
        Emitter {
            program,
            manifest,
            catalog,
            locale,
            issues: Vec::new(),
            out: String::new(),
            global_names,
            player_names,
            subroutine_emitted,
            subroutine_names,
            emitted_subroutine_names,
            declared_names,
            labels: HashMap::new(),
            skip_labels: HashMap::new(),
            generated_counter: 0,
            lambda_binders: Vec::new(),
            loop_depth: 0,
        }
    }

    fn run(&mut self) {
        self.validate_tables();
        if self.issues.is_empty() {
            let layout = self.classify_rules();
            if self.issues.is_empty() {
                self.emit_program(&layout);
            }
        }
    }
    fn issue(&mut self, code: &'static str, message: impl Into<String>, span: Option<Span>) {
        self.issues.push(ReconstructIssue {
            code,
            message: message.into(),
            span,
        });
    }

    fn validate_tables(&mut self) {
        let mut taken = HashSet::new();
        for (position, variable) in self.program.global_variables.iter().enumerate() {
            self.check_duplicate_name(&variable.name, position, "global variable", None);
            self.check_index(
                variable.index.unwrap_or(position as u32),
                &mut taken,
                "global variable",
                &variable.name,
            );
        }
        taken.clear();
        for (position, variable) in self.program.player_variables.iter().enumerate() {
            self.check_duplicate_name(&variable.name, position, "player variable", None);
            self.check_index(
                variable.index.unwrap_or(position as u32),
                &mut taken,
                "player variable",
                &variable.name,
            );
        }
        taken.clear();
        for (position, subroutine) in self.program.subroutines.iter().enumerate() {
            self.check_duplicate_name(&subroutine.name, position, "subroutine", None);
            self.check_index(
                subroutine.index.unwrap_or(position as u32),
                &mut taken,
                "subroutine",
                &subroutine.name,
            );
        }
    }

    fn check_index(&mut self, index: u32, taken: &mut HashSet<u32>, kind: &str, name: &str) {
        if index > 127 {
            self.issue(
                "unsupported-index",
                format!("{kind} '{name}' index {index} is outside the 0-127 slot range"),
                None,
            );
        } else if !taken.insert(index) {
            self.issue(
                "unsupported-index",
                format!("{kind} '{name}' shares declaration index {index} with a sibling"),
                None,
            );
        }
    }

    fn check_duplicate_name(
        &mut self,
        name: &str,
        position: usize,
        kind: &str,
        span: Option<Span>,
    ) {
        let duplicate = match kind {
            "global variable" => self
                .program
                .global_variables
                .iter()
                .enumerate()
                .take(position)
                .any(|(_, other)| other.name == name),
            "player variable" => self
                .program
                .player_variables
                .iter()
                .enumerate()
                .take(position)
                .any(|(_, other)| other.name == name),
            _ => self
                .program
                .subroutines
                .iter()
                .enumerate()
                .take(position)
                .any(|(_, other)| other.name == name),
        };
        if duplicate {
            self.issue(
                "unsupported-duplicate-name",
                format!("duplicate {kind} name '{name}'"),
                span,
            );
        }
    }

    fn classify_rules(&mut self) -> RuleLayout<'a> {
        let rules: Vec<IndexedRule<'a>> = self.program.rules.iter().enumerate().collect();
        let mut index = 0;
        let mut global_init = None;
        let mut player_init = None;
        if let Some(&(rule_index, rule)) = rules.first() {
            if rule.name == "Initialize global variables" {
                if let Some(initializers) = self.canonical_init(rule_index, rule, true) {
                    global_init = Some(initializers);
                    index = 1;
                }
            } else if rule.name == "Initialize player variables"
                && let Some(initializers) = self.canonical_init(rule_index, rule, false)
            {
                player_init = Some(initializers);
                index = 1;
            }
        }
        if global_init.is_some()
            && let Some(&(rule_index, rule)) = rules.get(1)
            && rule.name == "Initialize player variables"
            && let Some(initializers) = self.canonical_init(rule_index, rule, false)
        {
            player_init = Some(initializers);
            index = 2;
        }

        let mut sub_rules = Vec::new();
        let mut normal_rules = Vec::new();
        for (rule_index, rule) in rules.iter().copied().skip(index) {
            match &rule.event {
                Event::Subroutine(_) => {
                    if !rule.disabled && !rule.conditions.is_empty() {
                        self.issue(
                            "unsupported-rule-order",
                            format!(
                                "subroutine-body rule '{}' carries conditions; `def` \
                                 bodies cannot express them",
                                rule.name
                            ),
                            self.program.rule_span(rule_index),
                        );
                    }
                    sub_rules.push((rule_index, rule));
                }
                _ => normal_rules.push((rule_index, rule)),
            }
        }

        RuleLayout {
            global_init,
            player_init,
            sub_rules,
            normal_rules,
        }
    }

    fn canonical_init(&self, rule_index: usize, rule: &Rule, global: bool) -> Option<Initializers> {
        if rule.disabled {
            return None;
        }
        let canonical_event = if global {
            matches!(&rule.event, Event::Global)
        } else {
            matches!(&rule.event, Event::EachPlayer)
        };
        if !canonical_event || !rule.conditions.is_empty() {
            return None;
        }
        let variables = if global {
            &self.program.global_variables
        } else {
            &self.program.player_variables
        };
        let mut initializers = Initializers::new();
        let mut previous: Option<usize> = None;
        for (action_index, action) in rule.actions.iter().enumerate() {
            let (player, variable, value) = match (global, action) {
                (true, Action::SetGlobalVariable { variable, value }) => (None, variable, value),
                (
                    false,
                    Action::SetPlayerVariable {
                        player,
                        variable,
                        value,
                    },
                ) => (Some(player), variable, value),
                _ => return None,
            };
            if player.is_some_and(|player| !self.is_event_player(player)) {
                return None;
            }
            let position = variables
                .iter()
                .position(|declaration| declaration.name == *variable)?;
            if previous.is_some_and(|previous| position <= previous) {
                return None;
            }
            previous = Some(position);
            initializers.insert(
                position,
                (
                    value.clone(),
                    self.program.action_argument_span(
                        rule_index,
                        action_index,
                        usize::from(!global),
                    ),
                ),
            );
        }
        Some(initializers)
    }

    fn emit_settings(&mut self) {
        let Some(settings) = self.program.settings.as_ref() else {
            return;
        };
        let mut unemittable = false;
        for error in workshop_rs::settings::check_emission(settings) {
            unemittable = true;
            self.issue("unsupported-settings", error.to_string(), error.span());
        }
        if unemittable {
            return;
        }

        let mut top: Vec<(String, SettingsJson)> = Vec::new();
        let mut extensions: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        let last = settings.children.len().saturating_sub(1);
        for (index, child) in settings.children.iter().enumerate() {
            if !seen.insert(child.name().to_string()) {
                self.refuse_settings(child, "is a duplicate top-level group");
            }
            match child {
                SettingsNode::Workshop { children, .. } => {
                    top.push(("workshop".to_string(), self.settings_workshop(children)));
                }
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. } => match name.as_str() {
                    "workshop" => self.refuse_settings(
                        child,
                        "has no OPY settings form; `workshop` reparses as workshop settings",
                    ),
                    "main" | "lobby" => {
                        let display = display_names(&[format!("{name}.")]);
                        let path = [PathPart::Part(name.as_str())];
                        top.push((
                            name.clone(),
                            self.settings_object(children, &path, None, &display),
                        ));
                    }
                    "extensions" => {
                        let flags = children
                            .iter()
                            .position(|member| matches!(member, SettingsNode::Flag { .. }));
                        match flags {
                            None => {
                                let display = display_names(&["extensions.".to_string()]);
                                top.push((
                                    name.clone(),
                                    self.settings_object(
                                        children,
                                        &[PathPart::Part("extensions")],
                                        None,
                                        &display,
                                    ),
                                ));
                            }
                            Some(first) => {
                                let mut flag_names = HashSet::new();
                                for member in &children[first..] {
                                    let SettingsNode::Flag { name, .. } = member else {
                                        self.refuse_settings(
                                            member,
                                            "follows a presence-only member in `extensions`",
                                        );
                                        continue;
                                    };
                                    if !flag_names.insert(name.clone()) {
                                        self.refuse_settings(
                                            member,
                                            "is a duplicate `#!extension` name",
                                        );
                                    }
                                    let path = [
                                        PathPart::Part("extensions"),
                                        PathPart::Part(name.as_str()),
                                    ];
                                    match workshop_rs::settings::definition(&path) {
                                        Some(definition)
                                            if matches!(
                                                definition.domain(),
                                                SettingValueDomain::PresenceOnly
                                            ) =>
                                        {
                                            extensions.push(name.clone());
                                        }
                                        _ => self.refuse_settings(
                                            member,
                                            "is not a declared Workshop extension",
                                        ),
                                    }
                                }
                                if first == 0 {
                                    if index != last {
                                        self.refuse_settings(
                                            child,
                                            "carries only `#!extension` directives outside the last group",
                                        );
                                    }
                                } else {
                                    let display = display_names(&["extensions.".to_string()]);
                                    top.push((
                                        name.clone(),
                                        self.settings_object(
                                            &children[..first],
                                            &[PathPart::Part("extensions")],
                                            None,
                                            &display,
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    "gamemodes" => {
                        top.push(("gamemodes".to_string(), self.settings_modes(children)));
                    }
                    "heroes" => {
                        top.push(("heroes".to_string(), self.settings_teams(children)));
                    }
                    _ => {
                        top.push((name.clone(), self.opaque_group_object(children)));
                    }
                },
                member => self.refuse_settings(
                    member,
                    "cannot appear at the settings top level; groups are expected",
                ),
            }
        }

        match (top.is_empty(), extensions.is_empty()) {
            (true, true) => {
                self.issue(
                    "unsupported-settings",
                    "an empty settings block has no OPY form",
                    settings.span,
                );
                return;
            }
            (true, false) => {}
            (false, _) => {
                if !top.iter().any(|(key, _)| key == "gamemodes") {
                    self.issue(
                        "unsupported-settings",
                        "settings has no `gamemodes` group, which the OPY frontend requires",
                        settings.span,
                    );
                    return;
                }
            }
        }
        if !top.is_empty() {
            self.out.push_str("settings ");
            write_settings_json(&mut self.out, &SettingsJson::Object(top), 0);
            self.out.push_str("\n\n");
        }
        for name in &extensions {
            self.out.push_str("#!extension ");
            self.out.push_str(name);
            self.out.push('\n');
        }
        if !extensions.is_empty() {
            self.out.push('\n');
        }
    }

    fn settings_modes(&mut self, children: &[SettingsNode]) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for mode in children {
            let (name, children) = match mode {
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. } => (name, children),
                _ => {
                    self.refuse_settings(
                        mode,
                        "cannot appear inside `gamemodes`; groups are expected",
                    );
                    continue;
                }
            };
            if !seen.insert(name.clone()) {
                self.refuse_settings(mode, "is a duplicate mode group");
            }
            let display = display_names(&[
                format!("gamemodes.{name}."),
                "gamemodes.general.".to_string(),
            ]);
            let path = [PathPart::Part("gamemodes"), PathPart::Part(name.as_str())];
            let mut members = Vec::new();
            let mut member_names = HashSet::new();
            for (index, member) in children.iter().enumerate() {
                if member.name() == "enabled" {
                    match (index, member) {
                        (0, SettingsNode::Bool { value: false, .. }) => {
                            members.push(("enabled".to_string(), SettingsJson::Bool(false)));
                            member_names.insert("enabled".to_string());
                        }
                        _ => self.refuse_settings(
                            member,
                            "is an `enabled` marker only `enabled: false` in first position reproduces",
                        ),
                    }
                    continue;
                }
                let Some((key, json)) = self.settings_member(member, &path, None, &display) else {
                    continue;
                };
                if !member_names.insert(key.clone()) {
                    self.refuse_settings(member, "is a duplicate settings member");
                }
                members.push((key, json));
            }
            entries.push((name.clone(), SettingsJson::Object(members)));
        }
        SettingsJson::Object(entries)
    }

    fn settings_teams(&mut self, children: &[SettingsNode]) -> SettingsJson {
        const TEAM: [PathPart<'static>; 2] = [PathPart::Part("heroes"), PathPart::Team];
        const HERO: [PathPart<'static>; 3] =
            [PathPart::Part("heroes"), PathPart::Team, PathPart::Hero];
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for team in children {
            let (name, children) = match team {
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. } => (name, children),
                _ => {
                    self.refuse_settings(
                        team,
                        "cannot appear inside `heroes`; groups are expected",
                    );
                    continue;
                }
            };
            if !seen.insert(name.clone()) {
                self.refuse_settings(team, "is a duplicate team group");
            }
            let roster = |member: &SettingsNode| {
                matches!(member, SettingsNode::List { name, .. } if matches!(
                    name.as_str(),
                    "enabledHeroes" | "disabledHeroes"
                ))
            };
            let rest_at = children
                .iter()
                .position(|member| matches!(member, SettingsNode::Group { .. }) || roster(member))
                .unwrap_or(children.len());
            let rosters_at = children[rest_at..]
                .iter()
                .position(roster)
                .map(|index| rest_at + index)
                .unwrap_or(children.len());
            let general = &children[..rest_at];
            let rest = &children[rest_at..rosters_at];
            let rosters = &children[rosters_at..];
            for member in rest {
                if roster(member) {
                    self.refuse_settings(
                        member,
                        "is a hero roster before the last hero group; rosters reparse last",
                    );
                }
            }
            for member in rosters {
                if !roster(member) {
                    self.refuse_settings(
                        member,
                        "follows a hero roster; team `general` members reparse first",
                    );
                }
            }
            if rosters
                .iter()
                .filter(|member| member.name() == "enabledHeroes")
                .count()
                + rosters
                    .iter()
                    .filter(|member| member.name() == "disabledHeroes")
                    .count()
                > 1
            {
                self.refuse_settings(
                    team,
                    "lists both `enabledHeroes` and `disabledHeroes`, which the frontend rejects",
                );
            }

            let display = display_names(&[
                "heroes.<team>.".to_string(),
                "heroes.general.".to_string(),
                "heroes.team1.".to_string(),
                "heroes.team2.".to_string(),
                "heroes.allTeams.".to_string(),
            ]);
            let mut members = Vec::new();
            let mut member_names = HashSet::new();
            for member in rest {
                let (hero_name, hero_children) = match member {
                    SettingsNode::Group { name, children, .. }
                    | SettingsNode::RawGroup { name, children, .. } => (name, children),
                    _ => {
                        self.refuse_settings(
                            member,
                            "is a non-group member among hero groups; it would reparse as an empty group",
                        );
                        continue;
                    }
                };
                if !super::settings::resolves_to_hero(hero_name) {
                    self.refuse_settings(member, "is not a declared hero");
                    continue;
                }
                if super::settings::canonical_hero_name(hero_name) != hero_name.as_str() {
                    self.refuse_settings(
                        member,
                        "is a hero alias that reparses under its canonical name",
                    );
                    continue;
                }
                if !member_names.insert(hero_name.clone()) {
                    self.refuse_settings(member, "is a duplicate hero group");
                }
                members.push((
                    hero_name.clone(),
                    self.settings_object(hero_children, &HERO, Some(hero_name.as_str()), &display),
                ));
            }
            for member in rosters {
                let SettingsNode::List { elements, .. } = member else {
                    continue;
                };
                for element in elements {
                    if super::settings::canonical_hero_name(&element.value) != element.value {
                        self.refuse_settings(
                            member,
                            "carries a hero alias that reparses under its canonical name",
                        );
                        break;
                    }
                }
                if !member_names.insert(member.name().to_string()) {
                    self.refuse_settings(member, "is a duplicate hero roster");
                }
                members.push((
                    member.name().to_string(),
                    SettingsJson::Array(
                        elements
                            .iter()
                            .map(|element| SettingsJson::String(element.value.clone()))
                            .collect(),
                    ),
                ));
            }
            if !general.is_empty() {
                if !member_names.insert("general".to_string()) {
                    self.refuse_settings(
                        team,
                        "carries a `general` member the hero-name position cannot express",
                    );
                }
                members.push((
                    "general".to_string(),
                    self.settings_object(general, &TEAM, None, &display),
                ));
            }
            for member in general {
                if member.name() == "general" || roster(member) {
                    self.refuse_settings(
                        member,
                        "cannot be reproduced at a team's `general` position",
                    );
                }
            }
            entries.push((name.clone(), SettingsJson::Object(members)));
        }
        SettingsJson::Object(entries)
    }

    fn settings_object(
        &mut self,
        children: &[SettingsNode],
        path: &[PathPart<'_>],
        hero: Option<&str>,
        display: &HashSet<String>,
    ) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for member in children {
            let Some(entry) = self.settings_member(member, path, hero, display) else {
                continue;
            };
            if !seen.insert(member.name().to_string()) {
                self.refuse_settings(member, "is a duplicate settings member");
            }
            entries.push(entry);
        }
        SettingsJson::Object(entries)
    }

    fn settings_member(
        &mut self,
        member: &SettingsNode,
        path: &[PathPart<'_>],
        hero: Option<&str>,
        display: &HashSet<String>,
    ) -> Option<(String, SettingsJson)> {
        if matches!(path, [PathPart::Part("extensions")]) {
            self.refuse_settings(
                member,
                "is not a presence-only extension; only `#!extension` members survive here",
            );
            return None;
        }
        let name = member.name().to_string();
        match member {
            SettingsNode::Number { value, .. } => {
                let definition = self.catalogued_member(member, path, hero)?;
                if !matches!(
                    definition.domain(),
                    SettingValueDomain::Number(_) | SettingValueDomain::Percent(_)
                ) {
                    self.refuse_settings(member, "is a number member under a non-numeric setting");
                    return None;
                }
                Some((name, SettingsJson::Number(format!("{value}"))))
            }
            SettingsNode::Bool { value, .. } => {
                let definition = self.catalogued_member(member, path, hero)?;
                if !matches!(definition.domain(), SettingValueDomain::Boolean) {
                    self.refuse_settings(member, "is a boolean member under a non-boolean setting");
                    return None;
                }
                Some((name, SettingsJson::Bool(*value)))
            }
            SettingsNode::String { value, .. } => {
                let definition = self.catalogued_member(member, path, hero)?;
                if !matches!(
                    definition.domain(),
                    SettingValueDomain::String | SettingValueDomain::Enum { .. }
                ) {
                    self.refuse_settings(member, "is a string member under a non-string setting");
                    return None;
                }
                if crate::compile_time::has_unbound_formatters(value) {
                    self.refuse_settings(
                        member,
                        "carries a formatter the OPY settings evaluator rejects",
                    );
                    return None;
                }
                if *value == "afterAGame" && name == "mapRotation" {
                    return Some((name, SettingsJson::String("afterGame".to_string())));
                }
                if name == "mapRotation" && value == "afterGame" {
                    self.refuse_settings(
                        member,
                        "carries a `mapRotation` value the OPY frontend rewrites",
                    );
                    return None;
                }
                if matches!(path, [PathPart::Part("main")])
                    && matches!(name.as_str(), "modeName" | "description")
                    && super::lowering::escape_bad_words(value) != *value
                {
                    self.refuse_settings(
                        member,
                        "is a `main` string the filtered-word escape rewrites",
                    );
                    return None;
                }
                Some((name, SettingsJson::String(value.clone())))
            }
            SettingsNode::List { elements, .. } => {
                let definition = self.catalogued_member(member, path, hero)?;
                match definition.domain() {
                    SettingValueDomain::MapList => {
                        if elements.iter().any(|element| element.variant.is_some()) {
                            self.refuse_settings(member, "carries a map variant qualifier");
                            return None;
                        }
                    }
                    SettingValueDomain::HeroList => {
                        for element in elements {
                            if super::settings::canonical_hero_name(&element.value) != element.value
                            {
                                self.refuse_settings(
                                    member,
                                    "carries a hero alias that reparses under its canonical name",
                                );
                                return None;
                            }
                        }
                    }
                    _ => {
                        self.refuse_settings(member, "is a list member under a non-list setting");
                        return None;
                    }
                }
                Some((
                    name,
                    SettingsJson::Array(
                        elements
                            .iter()
                            .map(|element| SettingsJson::String(element.value.clone()))
                            .collect(),
                    ),
                ))
            }
            SettingsNode::Flag { .. } => {
                self.refuse_settings(
                    member,
                    "is a presence-only member; only `#!extension` under `extensions` carries one",
                );
                None
            }
            SettingsNode::Raw { value, .. } => {
                if self.member_definition(member, path, hero).is_some()
                    || display.contains(name.as_str())
                    || display.contains(format!("{name}:{value}").as_str())
                {
                    self.refuse_settings(
                        member,
                        "reparses under a catalogued display name rather than staying raw",
                    );
                    return None;
                }
                if !emittable_member_name(name.as_str()) || !emittable_raw_value(value) {
                    self.refuse_settings(member, "carries text a `name: value` line cannot write");
                    return None;
                }
                Some((name, SettingsJson::String(value.clone())))
            }
            SettingsNode::RawValue { value, .. } => {
                if !value.is_empty() {
                    self.refuse_settings(member, "has a value that reparses as a raw member");
                    return None;
                }
                if self.member_definition(member, path, hero).is_some()
                    || display.contains(name.as_str())
                    || !emittable_member_name(name.as_str())
                {
                    self.refuse_settings(
                        member,
                        "reparses under a catalogued display name rather than staying raw",
                    );
                    return None;
                }
                Some((name, SettingsJson::String(String::new())))
            }
            SettingsNode::Group { children, .. } | SettingsNode::RawGroup { children, .. } => {
                match self.member_definition(member, path, hero) {
                    Some(definition)
                        if matches!(
                            definition.domain(),
                            SettingValueDomain::MapList | SettingValueDomain::HeroList
                        ) =>
                    {
                        self.refuse_settings(
                            member,
                            "is a block under a list setting; it reparses as a list",
                        );
                        None
                    }
                    // A boolean-domain member collapses every non-boolean
                    // value to `enabled: false` upstream-style, so a block
                    // never reaches the group form.
                    Some(definition)
                        if matches!(definition.domain(), SettingValueDomain::Boolean) =>
                    {
                        self.refuse_settings(
                            member,
                            "is a block under a boolean setting; it reparses as `false`",
                        );
                        None
                    }
                    Some(_) => Some((name, self.opaque_object(children))),
                    None => {
                        if display.contains(name.as_str()) || !emittable_member_name(name.as_str())
                        {
                            self.refuse_settings(
                                member,
                                "reparses under a catalogued display name rather than staying raw",
                            );
                            return None;
                        }
                        Some((name, self.opaque_object(children)))
                    }
                }
            }
            SettingsNode::Workshop { .. } => {
                self.refuse_settings(member, "has no OPY settings form at a leaf path");
                None
            }
        }
    }

    fn member_definition(
        &self,
        member: &SettingsNode,
        path: &[PathPart<'_>],
        hero: Option<&str>,
    ) -> Option<workshop_rs::settings::SettingDefinition> {
        let mut full = path.to_vec();
        full.push(PathPart::Part(member.name()));
        let definition = workshop_rs::settings::definition(&full)?;
        match hero {
            Some(hero) if !super::settings::hero_setting_applies(hero, member.name()) => None,
            _ => Some(definition),
        }
    }

    fn catalogued_member(
        &mut self,
        member: &SettingsNode,
        path: &[PathPart<'_>],
        hero: Option<&str>,
    ) -> Option<workshop_rs::settings::SettingDefinition> {
        match self.member_definition(member, path, hero) {
            Some(definition) => Some(definition),
            None => {
                self.refuse_settings(
                    member,
                    "is a typed member under an uncatalogued key; it reparses as raw text",
                );
                None
            }
        }
    }

    fn opaque_object(&mut self, children: &[SettingsNode]) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for member in children {
            let entry = match member {
                SettingsNode::Raw { name, value, .. }
                    if emittable_member_name(name) && emittable_raw_value(value) =>
                {
                    Some((name.clone(), SettingsJson::String(value.clone())))
                }
                SettingsNode::RawValue { name, value, .. }
                    if value.is_empty() && emittable_member_name(name) =>
                {
                    Some((name.clone(), SettingsJson::String(String::new())))
                }
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. }
                    if emittable_member_name(name) =>
                {
                    Some((name.clone(), self.opaque_object(children)))
                }
                _ => {
                    self.refuse_settings(member, "has no OPY form inside an opaque settings block");
                    None
                }
            };
            let Some(entry) = entry else { continue };
            if !seen.insert(member.name().to_string()) {
                self.refuse_settings(member, "is a duplicate settings member");
            }
            entries.push(entry);
        }
        SettingsJson::Object(entries)
    }

    fn opaque_group_object(&mut self, children: &[SettingsNode]) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for member in children {
            let entry = match member {
                SettingsNode::Group { name, children, .. }
                | SettingsNode::RawGroup { name, children, .. }
                    if emittable_member_name(name) =>
                {
                    Some((name.clone(), self.opaque_group_object(children)))
                }
                _ => {
                    self.refuse_settings(
                        member,
                        "has no OPY form inside an uncatalogued settings group",
                    );
                    None
                }
            };
            let Some(entry) = entry else { continue };
            if !seen.insert(member.name().to_string()) {
                self.refuse_settings(member, "is a duplicate settings member");
            }
            entries.push(entry);
        }
        SettingsJson::Object(entries)
    }

    fn settings_workshop(&mut self, children: &[SettingsNode]) -> SettingsJson {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for member in children {
            let entry = match member {
                SettingsNode::Raw { name, value, .. } if emittable_member_name(name) => {
                    let json = if value == "On" {
                        Some(SettingsJson::Bool(true))
                    } else if value == "Off" {
                        Some(SettingsJson::Bool(false))
                    } else if emittable_raw_value(value) && super::settings::is_number_text(value) {
                        Some(SettingsJson::String(value.clone()))
                    } else if let Some(inner) = value
                        .strip_prefix('[')
                        .and_then(|rest| rest.strip_suffix(']'))
                    {
                        if crate::compile_time::has_unbound_formatters(inner) {
                            None
                        } else {
                            Some(SettingsJson::Array(vec![SettingsJson::String(
                                inner.to_string(),
                            )]))
                        }
                    } else {
                        None
                    };
                    match json {
                        Some(json) => Some((name.clone(), json)),
                        None => {
                            self.refuse_settings(
                                member,
                                "is a `settings.workshop` value OPY cannot write",
                            );
                            None
                        }
                    }
                }
                _ => {
                    self.refuse_settings(
                        member,
                        "is not a `name: value` member inside `settings.workshop`",
                    );
                    None
                }
            };
            let Some(entry) = entry else { continue };
            if !seen.insert(member.name().to_string()) {
                self.refuse_settings(member, "is a duplicate settings member");
            }
            entries.push(entry);
        }
        SettingsJson::Object(entries)
    }

    fn refuse_settings(&mut self, member: &SettingsNode, detail: &str) {
        self.issue(
            "unsupported-settings",
            format!("settings member '{}' {detail}", member.name()),
            member.span(),
        );
    }

    fn emit_program(&mut self, layout: &RuleLayout) {
        let global_order = index_order(&self.program.global_variables);
        let player_order = index_order(&self.program.player_variables);
        let empty = Initializers::new();
        let global_initializers = layout.global_init.as_ref().unwrap_or(&empty);
        let player_initializers = layout.player_init.as_ref().unwrap_or(&empty);
        self.check_initializer_slots(true, &global_order, global_initializers);
        self.check_initializer_slots(false, &player_order, player_initializers);

        // The strict-optimization directive is part of every reconstruction:
        // without it the frontend's default folding rewrites values such as
        // `Add(x, 0)` into a different canonical shape than the input's.
        self.out.push_str("#!optimizeStrict\n\n");
        self.emit_settings();
        self.emit_variable_declarations(true, &global_order, global_initializers);
        self.emit_variable_declarations(false, &player_order, player_initializers);
        if self.program.subroutines.is_empty() {
            self.out.push('\n');
        } else {
            // A bare `subroutine name` auto-allocates the lowest free index;
            // it reproduces the canonical binding only for the leading run
            // whose index already equals the table position. Every later
            // declaration carries its index explicitly.
            let prefix = self
                .program
                .subroutines
                .iter()
                .enumerate()
                .take_while(|(position, subroutine)| {
                    subroutine.index.unwrap_or(*position as u32) == *position as u32
                })
                .count();
            for (position, subroutine) in self.program.subroutines.iter().enumerate() {
                self.out.push_str("subroutine ");
                self.out
                    .push_str(emitted(&self.subroutine_emitted, &subroutine.name));
                if position >= prefix {
                    let index = subroutine.index.unwrap_or(position as u32);
                    self.out.push_str(&format!(" {index}"));
                }
                self.out.push('\n');
            }
            self.out.push('\n');
        }

        let mut defined = std::collections::HashSet::new();
        for (rule_index, rule) in &layout.sub_rules {
            let Event::Subroutine(subroutine_name) = &rule.event else {
                continue;
            };
            if !self.subroutine_names.contains(subroutine_name) {
                self.issue(
                    "unsupported-dangling",
                    format!("unknown subroutine '{subroutine_name}'"),
                    self.program.rule_span(*rule_index),
                );
                continue;
            }
            if !defined.insert(subroutine_name.as_str()) {
                self.issue(
                    "unsupported-rule-order",
                    format!(
                        "subroutine '{subroutine_name}' has more than one def rule; \
                         the comparator accepts reordering, not duplication"
                    ),
                    self.program.rule_span(*rule_index),
                );
                continue;
            }
            self.out.push_str("def ");
            self.out
                .push_str(emitted(&self.subroutine_emitted, subroutine_name));
            self.out.push_str("():\n");
            if rule.disabled {
                self.out.push_str("    # @Disabled\n");
                if !rule.conditions.is_empty() {
                    for (condition_index, condition) in rule.conditions.iter().enumerate() {
                        let scratch = std::mem::take(&mut self.out);
                        self.out.push_str("    @Condition ");
                        self.emit_value(
                            &condition.value,
                            self.program.condition_span(*rule_index, condition_index),
                        );
                        self.out.push('\n');
                        self.flush_commented(scratch, 1);
                    }
                }
                self.emit_commented_actions(*rule_index, &rule.actions);
                self.out.push_str("    pass\n");
            } else {
                self.emit_actions(*rule_index, &rule.actions, 1);
                if rule.actions.is_empty() {
                    self.out.push_str("    pass\n");
                }
            }
            self.out.push('\n');
        }

        for (rule_index, rule) in &layout.normal_rules {
            if !rule.disabled && rule.actions.is_empty() {
                continue;
            }
            self.out.push_str("rule ");
            self.emit_string_literal(&rule.name);
            self.out.push_str(":\n");
            self.emit_event_annotation(*rule_index, rule);
            if rule.disabled {
                self.out.push_str("    @Disabled\n");
            }
            for (condition_index, condition) in rule.conditions.iter().enumerate() {
                if condition.disabled {
                    let scratch = std::mem::take(&mut self.out);
                    self.out.push_str("    @Condition ");
                    self.emit_value(
                        &condition.value,
                        self.program.condition_span(*rule_index, condition_index),
                    );
                    self.out.push('\n');
                    self.flush_commented(scratch, 1);
                    continue;
                }
                self.out.push_str("    @Condition ");
                self.emit_value(
                    &condition.value,
                    self.program.condition_span(*rule_index, condition_index),
                );
                self.out.push('\n');
            }
            self.emit_actions(*rule_index, &rule.actions, 1);
            self.out.push('\n');
        }
    }

    fn emit_event_annotation(&mut self, rule_index: usize, rule: &Rule) {
        let (kind, filters) = match &rule.event {
            Event::Global => ("global", None),
            Event::EachPlayer => ("eachPlayer", None),
            Event::EachPlayerWithFilters { team, target } => ("eachPlayer", Some((team, target))),
            Event::Player { kind, team, target } => (kind.catalog_id(), Some((team, target))),
            Event::Subroutine(_) => {
                self.issue(
                    "unsupported-rule-order",
                    format!(
                        "rule '{}' has a subroutine event outside the def layout",
                        rule.name
                    ),
                    self.program.rule_span(rule_index),
                );
                return;
            }
        };
        self.out.push_str("    @Event ");
        self.out.push_str(kind);
        self.out.push('\n');
        if let Some((team, target)) = filters {
            self.emit_event_filters(rule_index, rule, team, target);
        }
    }

    fn emit_event_filters(
        &mut self,
        rule_index: usize,
        rule: &Rule,
        team: &EventTeam,
        target: &EventTarget,
    ) {
        match team {
            EventTeam::All => {}
            EventTeam::Team1 => self.out.push_str("    @Team 1\n"),
            EventTeam::Team2 => self.out.push_str("    @Team 2\n"),
        }
        match target {
            EventTarget::All => {}
            EventTarget::Slot(slot) => {
                self.out.push_str(&format!("    @Slot {slot}\n"));
            }
            EventTarget::Hero(member) => {
                let Some((_, spelling)) = crate::enums::spelling_of_member("Hero", member) else {
                    self.issue(
                        "unsupported-enum-member",
                        format!(
                            "hero filter '{member}' on rule '{}' has no OPY spelling",
                            rule.name
                        ),
                        self.program.rule_span(rule_index),
                    );
                    return;
                };
                self.out.push_str("    @Hero ");
                self.out
                    .push_str(&crate::enums::upper_case_to_camel_case(spelling));
                self.out.push('\n');
            }
        }
    }

    fn emit_variable_declarations(
        &mut self,
        global: bool,
        order: &[(usize, u32)],
        initializers: &Initializers,
    ) {
        let (kind, variables) = if global {
            ("globalvar ", &self.program.global_variables)
        } else {
            ("playervar ", &self.program.player_variables)
        };
        for &(position, index) in order {
            let variable = &variables[position];
            let name = emitted(
                if global {
                    &self.global_names
                } else {
                    &self.player_names
                },
                &variable.name,
            )
            .to_string();
            self.out.push_str(kind);
            self.out.push_str(&name);
            match initializers.get(&position) {
                Some(value) => {
                    self.out.push_str(" = ");
                    self.emit_initializer(&value.0, value.1);
                }
                None => {
                    self.out.push(' ');
                    self.out.push_str(&index.to_string());
                }
            }
            self.out.push('\n');
        }
    }

    /// A declaration initializer: same value emission, but zero literals are
    /// spelled `0.0` because the frontend drops integer-`0` initializers
    /// (matching the reference adapter).
    fn emit_initializer(&mut self, value: &Value, span: Option<Span>) {
        if let Value::Number(number) = value
            && *number == 0.0
        {
            self.out.push_str("0.0");
            return;
        }
        self.emit_value(value, span);
    }

    fn check_initializer_slots(
        &mut self,
        global: bool,
        order: &[(usize, u32)],
        initializers: &HashMap<usize, (Value, Option<Span>)>,
    ) {
        let kind = if global { "globalvar" } else { "playervar" };
        let (variables, names) = if global {
            (&self.program.global_variables, &self.global_names)
        } else {
            (&self.program.player_variables, &self.player_names)
        };
        let mut taken = HashSet::new();
        let mut pending = Vec::new();
        for &(position, index) in order {
            if initializers.contains_key(&position) {
                let name = emitted(names, &variables[position].name);
                pending.push((
                    position,
                    index,
                    name.starts_with("__") && name.ends_with("__"),
                ));
            } else {
                taken.insert(index);
            }
        }
        // The frontend's top-allocation pass runs before the lowest-free
        // pass, so `__x__` initializer declarations claim their slots first.
        for high in [true, false] {
            for &(position, index, dunder) in &pending {
                if dunder != high {
                    continue;
                }
                let slot = if high {
                    (0..=127u32).rev().find(|slot| !taken.contains(slot))
                } else {
                    (0..=127u32).find(|slot| !taken.contains(slot))
                };
                if slot != Some(index) {
                    self.issues.push(ReconstructIssue {
                        code: "unsupported-indexed-initializer",
                        message: format!(
                            "initializer-bearing {} '{}' occupies slot {} but the \
                             OPY `{kind} name = value` form assigns slot {} on \
                             re-lowering",
                            if global { "global" } else { "player" },
                            variables[position].name,
                            index,
                            slot.map_or_else(|| "none".to_string(), |slot| slot.to_string()),
                        ),
                        span: None,
                    });
                }
                if let Some(slot) = slot {
                    taken.insert(slot);
                }
            }
        }
    }

    fn emit_actions(&mut self, rule_index: usize, actions: &[Action], level: usize) {
        self.labels.clear();
        self.skip_labels.clear();
        self.plan_labels(rule_index, actions);
        let mut position = 0;
        self.emit_action_block(rule_index, actions, &mut position, level);
        self.emit_labels_at(actions.len(), level);
        if position < actions.len() {
            self.issue(
                "unsupported-control-flow",
                "unexpected control-flow marker in Workshop action sequence",
                self.program.action_span(rule_index, position),
            );
        }
    }

    fn plan_labels(&mut self, rule_index: usize, actions: &[Action]) {
        let _ = rule_index;
        let mut depth = vec![0usize; actions.len() + 1];
        for (index, action) in actions.iter().enumerate() {
            depth[index + 1] = match action {
                Action::If { .. }
                | Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. } => depth[index] + 1,
                Action::End => depth[index].saturating_sub(1),
                _ => depth[index],
            };
        }
        for (index, action) in actions.iter().enumerate() {
            let Action::Call { name, args } = action else {
                continue;
            };
            let distance_arg = match (name.as_str(), args.as_slice()) {
                ("skip", [distance]) => distance,
                ("skipIf", [_, distance]) => distance,
                _ => continue,
            };
            let Some(distance) = literal_positive_int(distance_arg) else {
                continue;
            };
            let target = (index + 1 + distance).min(actions.len());
            let source_depth = depth[index];
            let target_depth = depth[target];
            if target_depth > source_depth {
                continue;
            }
            let dips_below_target =
                ((index + 1)..=target).any(|boundary| depth[boundary] < target_depth);
            if dips_below_target {
                continue;
            }
            let label = self.fresh_generated_name("__op_label");
            self.skip_labels.insert(index, label.clone());
            self.labels.entry(target).or_default().push(label);
        }
    }

    fn fresh_generated_name(&mut self, prefix: &str) -> String {
        loop {
            self.generated_counter += 1;
            let name = format!("{prefix}_{}", self.generated_counter);
            if !self.declared_names.contains(&name) {
                return name;
            }
        }
    }

    fn flush_commented(&mut self, prior: String, level: usize) {
        let fragment = std::mem::replace(&mut self.out, prior);
        let indent = Self::indent(level);
        for line in fragment.lines() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                self.out.push('\n');
            } else {
                self.out.push_str(&indent);
                self.out.push_str("# ");
                self.out.push_str(trimmed);
                self.out.push('\n');
            }
        }
    }

    fn emit_labels_at(&mut self, position: usize, level: usize) {
        let Some(labels) = self.labels.remove(&position) else {
            return;
        };
        for label in labels {
            self.out.push_str(&Self::indent(level));
            self.out.push_str(&label);
            self.out.push_str(":\n");
        }
    }

    fn emit_commented_actions(&mut self, rule_index: usize, actions: &[Action]) {
        if actions.is_empty() {
            return;
        }
        let scratch = std::mem::take(&mut self.out);
        self.emit_actions(rule_index, actions, 1);
        self.flush_commented(scratch, 1);
    }

    fn emit_action_block(
        &mut self,
        rule_index: usize,
        actions: &[Action],
        position: &mut usize,
        level: usize,
    ) {
        while let Some(action) = actions.get(*position) {
            self.emit_labels_at(*position, level);
            match action {
                Action::ElseIf { .. } | Action::Else | Action::End => return,
                Action::Disabled { action: inner } => {
                    let action_index = *position;
                    *position += 1;
                    match &**inner {
                        Action::If { .. }
                        | Action::While { .. }
                        | Action::ForGlobalVariable { .. }
                        | Action::ForPlayerVariable { .. } => {
                            let scratch = std::mem::take(&mut self.out);
                            self.emit_construct(
                                rule_index,
                                action_index,
                                inner,
                                actions,
                                position,
                                level,
                            );
                            self.flush_commented(scratch, level);
                        }
                        Action::ElseIf { .. }
                        | Action::Else
                        | Action::End
                        | Action::Disabled { .. } => {
                            self.issue(
                                "unsupported-control-flow",
                                "the disabled modifier applies to a single \
                                 executable action",
                                self.program.action_span(rule_index, action_index),
                            );
                        }
                        _ => {
                            let scratch = std::mem::take(&mut self.out);
                            self.emit_action(rule_index, action_index, inner, level);
                            self.flush_commented(scratch, level);
                        }
                    }
                }
                _ => {
                    let action_index = *position;
                    *position += 1;
                    self.emit_construct(rule_index, action_index, action, actions, position, level);
                }
            }
        }
    }

    fn emit_construct(
        &mut self,
        rule_index: usize,
        action_index: usize,
        action: &Action,
        actions: &[Action],
        position: &mut usize,
        level: usize,
    ) {
        match action {
            Action::If { condition } => {
                let span = self.program.action_span(rule_index, action_index);
                self.out.push_str(&Self::indent(level));
                self.out.push_str("if ");
                self.emit_value(
                    condition,
                    self.program
                        .action_argument_span(rule_index, action_index, 0),
                );
                self.out.push_str(":\n");
                self.emit_action_block(rule_index, actions, position, level + 1);
                while let Some(Action::ElseIf { condition }) = actions.get(*position) {
                    let action_index = *position;
                    *position += 1;
                    self.out.push_str(&Self::indent(level));
                    self.out.push_str("elif ");
                    self.emit_value(
                        condition,
                        self.program
                            .action_argument_span(rule_index, action_index, 0),
                    );
                    self.out.push_str(":\n");
                    self.emit_action_block(rule_index, actions, position, level + 1);
                }
                if matches!(actions.get(*position), Some(Action::Else)) {
                    *position += 1;
                    self.out.push_str(&Self::indent(level));
                    self.out.push_str("else:\n");
                    self.emit_action_block(rule_index, actions, position, level + 1);
                }
                if matches!(actions.get(*position), Some(Action::End)) {
                    *position += 1;
                } else {
                    self.issue("unsupported-control-flow", "if action is missing End", span);
                }
            }
            Action::While { condition } => {
                let span = self.program.action_span(rule_index, action_index);
                self.out.push_str(&Self::indent(level));
                self.out.push_str("while ");
                self.emit_value(
                    condition,
                    self.program
                        .action_argument_span(rule_index, action_index, 0),
                );
                self.out.push_str(":\n");
                self.loop_depth += 1;
                self.emit_action_block(rule_index, actions, position, level + 1);
                self.loop_depth -= 1;
                self.require_end(rule_index, actions, position, "while", span);
            }
            Action::ForGlobalVariable {
                variable,
                start,
                stop,
                step,
            } => {
                let span = self.program.action_span(rule_index, action_index);
                let Some(declaration) = self
                    .program
                    .global_variables
                    .iter()
                    .find(|declaration| declaration.name == *variable)
                else {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown loop variable '{variable}'"),
                        span,
                    );
                    self.skip_action_block(actions, position);
                    return;
                };
                self.out.push_str(&Self::indent(level));
                self.out.push_str("for ");
                self.out
                    .push_str(emitted(&self.global_names, &declaration.name));
                self.out.push_str(" in range(");
                for (arg_index, value) in [start, stop, step].into_iter().enumerate() {
                    if arg_index > 0 {
                        self.out.push_str(", ");
                    }
                    self.emit_value(
                        value,
                        self.program
                            .action_argument_span(rule_index, action_index, arg_index),
                    );
                }
                self.out.push_str("):\n");
                self.loop_depth += 1;
                self.emit_action_block(rule_index, actions, position, level + 1);
                self.loop_depth -= 1;
                self.require_end(rule_index, actions, position, "for", span);
            }
            Action::ForPlayerVariable {
                player,
                variable,
                start,
                stop,
                step,
            } => {
                let span = self.program.action_span(rule_index, action_index);
                if !self
                    .program
                    .player_variables
                    .iter()
                    .any(|declaration| declaration.name == *variable)
                {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown loop variable '{variable}'"),
                        span,
                    );
                    self.skip_action_block(actions, position);
                    return;
                }
                self.out.push_str(&Self::indent(level));
                self.out.push_str("for ");
                self.emit_player_variable_receiver(
                    player,
                    self.program
                        .action_argument_span(rule_index, action_index, 0),
                );
                self.out.push('.');
                self.out.push_str(emitted(&self.player_names, variable));
                self.out.push_str(" in range(");
                for (arg_index, value) in [start, stop, step].into_iter().enumerate() {
                    if arg_index > 0 {
                        self.out.push_str(", ");
                    }
                    self.emit_value(
                        value,
                        self.program
                            .action_argument_span(rule_index, action_index, arg_index + 2),
                    );
                }
                self.out.push_str("):\n");
                self.loop_depth += 1;
                self.emit_action_block(rule_index, actions, position, level + 1);
                self.loop_depth -= 1;
                self.require_end(rule_index, actions, position, "for", span);
            }
            _ => self.emit_action(rule_index, action_index, action, level),
        }
    }

    fn skip_action_block(&self, actions: &[Action], position: &mut usize) {
        let mut depth = 1usize;
        while let Some(action) = actions.get(*position) {
            *position += 1;
            match action {
                Action::If { .. }
                | Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. } => depth += 1,
                Action::End => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    fn require_end(
        &mut self,
        rule_index: usize,
        actions: &[Action],
        position: &mut usize,
        kind: &str,
        span: Option<Span>,
    ) {
        if matches!(actions.get(*position), Some(Action::End)) {
            *position += 1;
        } else {
            self.issue(
                "unsupported-control-flow",
                format!("{kind} action is missing End"),
                span.or_else(|| self.program.action_span(rule_index, *position)),
            );
        }
    }

    fn indent(level: usize) -> String {
        "    ".repeat(level)
    }

    fn emit_action(
        &mut self,
        rule_index: usize,
        action_index: usize,
        action: &Action,
        level: usize,
    ) {
        let span = self.program.action_span(rule_index, action_index);
        let indent = Self::indent(level);
        match action {
            Action::SetGlobalVariable { variable, value } => {
                self.out.push_str(&indent);
                self.out.push_str(emitted(&self.global_names, variable));
                self.out.push_str(" = ");
                self.emit_value(
                    value,
                    self.program
                        .action_argument_span(rule_index, action_index, 0),
                );
                self.out.push('\n');
            }
            Action::ModifyGlobalVariable {
                variable,
                op,
                value,
            } => {
                let variable = emitted(&self.global_names, variable).to_string();
                self.emit_modify(
                    level,
                    &variable,
                    *op,
                    value,
                    self.program
                        .action_argument_span(rule_index, action_index, 0),
                );
            }
            Action::SetPlayerVariable {
                player,
                variable,
                value,
            } => {
                let player_span = self
                    .program
                    .action_argument_span(rule_index, action_index, 0);
                let target = self.player_variable_target(player, variable, player_span);
                self.out.push_str(&indent);
                self.out.push_str(&target);
                self.out.push_str(" = ");
                self.emit_value(
                    value,
                    self.program
                        .action_argument_span(rule_index, action_index, 1),
                );
                self.out.push('\n');
            }
            Action::ModifyPlayerVariable {
                player,
                variable,
                op,
                value,
            } => {
                let player_span = self
                    .program
                    .action_argument_span(rule_index, action_index, 0);
                let target = self.player_variable_target(player, variable, player_span);
                self.emit_modify(
                    level,
                    &target,
                    *op,
                    value,
                    self.program
                        .action_argument_span(rule_index, action_index, 1),
                );
            }
            Action::AssignMember { .. } => {
                self.issue(
                    "unsupported-member-assignment",
                    "dynamic member assignments are outside the OPY reconstruction surface",
                    span,
                );
            }
            Action::CallSubroutine { subroutine } => {
                if !self.subroutine_names.contains(subroutine) {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown subroutine '{subroutine}'"),
                        span,
                    );
                    return;
                }
                self.out.push_str(&indent);
                self.out
                    .push_str(emitted(&self.subroutine_emitted, subroutine));
                self.out.push_str("()\n");
            }
            Action::If { .. } | Action::ElseIf { .. } | Action::Else | Action::End => self.issue(
                "unsupported-control-flow",
                "unexpected control-flow marker in Workshop action sequence",
                span,
            ),
            Action::While { .. } => self.issue(
                "unsupported-control-flow",
                "unexpected while action in Workshop action sequence",
                span,
            ),
            Action::ForGlobalVariable { .. } => self.issue(
                "unsupported-control-flow",
                "unexpected for action in Workshop action sequence",
                span,
            ),
            Action::ForPlayerVariable { .. } => {
                self.issue(
                    "unsupported-per-player-loop",
                    "For Player Variable is outside the reconstruction surface \
                     (the OPY `for` form binds a global variable)",
                    span,
                );
            }
            Action::Disabled { action: inner } => {
                let scratch = std::mem::take(&mut self.out);
                self.emit_action(rule_index, action_index, inner, level);
                self.flush_commented(scratch, level);
            }
            Action::Call { name, args } => {
                self.emit_call_action(rule_index, action_index, name, args, &indent, span);
            }
        }
    }

    fn is_event_player(&self, value: &Value) -> bool {
        matches!(value, Value::EventPlayer)
    }

    fn emit_player_variable_receiver(&mut self, player: &Value, span: Option<Span>) {
        if self.is_event_player(player) {
            self.out.push_str("eventPlayer");
        } else {
            self.out.push('(');
            self.emit_value(player, span);
            self.out.push(')');
        }
    }

    fn player_variable_target(
        &mut self,
        player: &Value,
        variable: &str,
        span: Option<Span>,
    ) -> String {
        let receiver = self.render_value_as(player, span, |this, player, span| {
            this.emit_player_variable_receiver(player, span);
        });
        format!("{receiver}.{}", emitted(&self.player_names, variable))
    }

    fn render_value_as(
        &mut self,
        value: &Value,
        span: Option<Span>,
        emit: impl FnOnce(&mut Self, &Value, Option<Span>),
    ) -> String {
        let saved = std::mem::take(&mut self.out);
        emit(self, value, span);
        std::mem::replace(&mut self.out, saved)
    }

    fn emit_modify(
        &mut self,
        level: usize,
        name: &str,
        op: ModifyOp,
        value: &Value,
        span: Option<Span>,
    ) {
        let indent = Self::indent(level);
        match op {
            ModifyOp::AppendToArray => {
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(".append(");
                self.emit_value(value, span);
                self.out.push_str(")\n");
            }
            ModifyOp::RemoveFromArrayByValue => {
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(".remove(");
                self.emit_value(value, span);
                self.out.push_str(")\n");
            }
            ModifyOp::RemoveFromArrayByIndex => {
                self.out.push_str(&indent);
                self.out.push_str("del ");
                self.out.push_str(name);
                self.out.push('[');
                self.emit_value(value, span);
                self.out.push_str("]\n");
            }
            ModifyOp::Min | ModifyOp::Max => {
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(match op {
                    ModifyOp::Min => " min= ",
                    ModifyOp::Max => " max= ",
                    _ => unreachable!(),
                });
                self.emit_value(value, span);
                self.out.push('\n');
            }
            ModifyOp::Add
            | ModifyOp::Subtract
            | ModifyOp::Multiply
            | ModifyOp::Divide
            | ModifyOp::Modulo
            | ModifyOp::RaiseToPower => {
                let operator = match op {
                    ModifyOp::Add => "+",
                    ModifyOp::Subtract => "-",
                    ModifyOp::Multiply => "*",
                    ModifyOp::Divide => "/",
                    ModifyOp::Modulo => "%",
                    ModifyOp::RaiseToPower => "**",
                    _ => unreachable!(),
                };
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(" = ");
                self.out.push_str(name);
                self.out.push(' ');
                self.out.push_str(operator);
                self.out.push(' ');
                self.emit_value(value, span);
                self.out.push('\n');
            }
            _ => self.issue(
                "unsupported-modify-op",
                format!("Workshop modification operation {op:?} has no OPY form"),
                span,
            ),
        }
    }

    fn emit_call_action(
        &mut self,
        rule_index: usize,
        action_index: usize,
        name: &str,
        args: &[Value],
        indent: &str,
        span: Option<Span>,
    ) {
        if DEDICATED_ACTION_NAMES.contains(&name) {
            self.issue(
                "unsupported-action-call",
                format!(
                    "action call '{name}' is lowered to a dedicated Workshop action \
                     and has no reconstructible call form"
                ),
                span,
            );
            return;
        }
        if self.emit_syntax_action(rule_index, action_index, name, args, indent, span) {
            return;
        }
        let (entry, member) =
            match Self::resolve_call_entry(self.manifest, name, FunctionKind::Action, args.len()) {
                Ok(call) => call,
                Err(message) => {
                    self.issue("unsupported-action-call", message, span);
                    return;
                }
            };
        // A bare call spelled like a declared subroutine resolves to the
        // subroutine on re-lowering — at any arity — so an emitted call
        // whose spelling collides with an emitted subroutine name cannot
        // reproduce the builtin.
        if !member && self.emitted_subroutine_names.contains(entry.id.as_str()) {
            self.issue(
                "unsupported-action-call",
                format!(
                    "action '{name}' is ambiguous with a subroutine of the same \
                     emitted name on the OPY surface"
                ),
                span,
            );
            return;
        }
        let argument_spans: Vec<Option<Span>> = (0..args.len())
            .map(|index| {
                self.program
                    .action_argument_span(rule_index, action_index, index)
            })
            .collect();
        self.out.push_str(indent);
        self.emit_manifest_call(entry, args, member, span, &argument_spans);
        self.out.push('\n');
    }

    fn emit_syntax_action(
        &mut self,
        rule_index: usize,
        action_index: usize,
        name: &str,
        args: &[Value],
        indent: &str,
        span: Option<Span>,
    ) -> bool {
        let arg_span = |i: usize| {
            self.program
                .action_argument_span(rule_index, action_index, i)
        };
        match name {
            "startRule" => {
                let [subroutine, behavior] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "startRule requires subroutine and behavior arguments",
                        span,
                    );
                    return true;
                };
                let Value::Subroutine(sub_name) = subroutine else {
                    self.issue(
                        "unsupported-action-call",
                        "startRule subroutine argument is not a subroutine value",
                        arg_span(0).or(span),
                    );
                    return true;
                };
                if !self.subroutine_names.contains(sub_name.as_str()) {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown subroutine '{sub_name}'"),
                        arg_span(0).or(span),
                    );
                    return true;
                }
                self.out.push_str(indent);
                self.out.push_str("async(");
                self.out.push_str(sub_name);
                self.out.push_str(", ");
                self.emit_value(behavior, arg_span(1));
                self.out.push_str(")\n");
                true
            }
            "skip" => {
                let [distance] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "skip requires exactly one distance argument",
                        span,
                    );
                    return true;
                };
                self.out.push_str(indent);
                if let Some(label) = self.skip_labels.get(&action_index) {
                    self.out.push_str("goto ");
                    self.out.push_str(&label.clone());
                    self.out.push('\n');
                } else {
                    self.out.push_str("goto loc+");
                    self.emit_value(distance, arg_span(0));
                    self.out.push('\n');
                }
                true
            }
            "skipIf" => {
                let [condition, distance] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "skipIf requires condition and distance arguments",
                        span,
                    );
                    return true;
                };
                if let Some(label) = self.skip_labels.get(&action_index).cloned() {
                    self.out.push_str(indent);
                    self.out.push_str("if ");
                    self.emit_value(condition, arg_span(0));
                    self.out.push_str(":\n");
                    self.out.push_str(indent);
                    self.out.push_str("    goto ");
                    self.out.push_str(&label);
                    self.out.push('\n');
                } else {
                    self.out.push_str(indent);
                    self.out.push_str("if ");
                    self.emit_value(condition, arg_span(0));
                    self.out.push_str(":\n");
                    self.out.push_str(indent);
                    self.out.push_str("    goto loc+");
                    self.emit_value(distance, arg_span(1));
                    self.out.push('\n');
                }
                true
            }
            "abort" => {
                if !args.is_empty() {
                    self.issue("unsupported-action-call", "abort takes no arguments", span);
                    return true;
                }
                self.out.push_str(indent);
                self.out.push_str("return\n");
                true
            }
            "abortIf" => {
                let [condition] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "abortIf requires exactly one condition argument",
                        span,
                    );
                    return true;
                };
                self.out.push_str(indent);
                self.out.push_str("if ");
                self.emit_value(condition, arg_span(0));
                self.out.push_str(":\n");
                self.out.push_str(indent);
                self.out.push_str("    return\n");
                true
            }
            "loop" => {
                if !args.is_empty() {
                    self.issue("unsupported-action-call", "loop takes no arguments", span);
                    return true;
                }
                self.out.push_str(indent);
                self.out.push_str("loop()\n");
                true
            }
            "loopIf" => {
                let [condition] = args else {
                    self.issue(
                        "unsupported-action-call",
                        "loopIf requires exactly one condition argument",
                        span,
                    );
                    return true;
                };
                self.out.push_str(indent);
                self.out.push_str("if ");
                self.emit_value(condition, arg_span(0));
                self.out.push_str(":\n");
                self.out.push_str(indent);
                self.out.push_str("    loop()\n");
                true
            }
            // `break` re-lowers to the same canonical call inside a loop body;
            // outside one the OPY surface rejects it, so it stays refused.
            "break" => {
                if self.loop_depth == 0 {
                    return false;
                }
                self.out.push_str(indent);
                self.out.push_str("break\n");
                true
            }
            "__abortIfConditionIsTrue__"
            | "__abortIfConditionIsFalse__"
            | "loopIfConditionIsTrue"
            | "__loopIfConditionIsFalse__" => {
                if !args.is_empty() {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} takes no arguments"),
                        span,
                    );
                    return true;
                }
                let (positive, is_loop) = match name {
                    "__abortIfConditionIsTrue__" => (true, false),
                    "__abortIfConditionIsFalse__" => (false, false),
                    "loopIfConditionIsTrue" => (true, true),
                    _ => (false, true),
                };
                self.out.push_str(indent);
                self.out.push_str("if ");
                if !positive {
                    self.out.push_str("not ");
                }
                self.out.push_str("ruleCondition():\n");
                self.out.push_str(indent);
                self.out.push_str(if is_loop {
                    "    loop()\n"
                } else {
                    "    return\n"
                });
                true
            }
            "setGlobalVariableAtIndex" | "setPlayerVariableAtIndex" => {
                let [variable, index, value] = args else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} requires variable, index and value arguments"),
                        span,
                    );
                    return true;
                };
                let Some(target) = self.indexed_target(variable, arg_span(0)) else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} variable argument is not a variable value"),
                        arg_span(0).or(span),
                    );
                    return true;
                };
                self.out.push_str(indent);
                self.out.push_str(&target);
                self.out.push('[');
                self.emit_value(index, arg_span(1));
                self.out.push_str("] = ");
                self.emit_value(value, arg_span(2));
                self.out.push('\n');
                true
            }
            "modifyGlobalVariableAtIndex" | "modifyPlayerVariableAtIndex" => {
                let [variable, index, op, value] = args else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} requires variable, index, operation and value arguments"),
                        span,
                    );
                    return true;
                };
                let Some(target) = self.indexed_target(variable, arg_span(0)) else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} variable argument is not a variable value"),
                        arg_span(0).or(span),
                    );
                    return true;
                };
                self.out.push_str(indent);
                if let Value::Call {
                    name: op_name,
                    args: op_args,
                } = op
                    && matches!(
                        op_name.as_str(),
                        "removeFromArray" | "removeFromArrayByIndex" | "appendToArray"
                    )
                    && op_args.is_empty()
                {
                    match op_name.as_str() {
                        "appendToArray" | "removeFromArray" => {
                            self.out.push_str(&target);
                            self.out.push('[');
                            self.emit_value(index, arg_span(1));
                            self.out.push_str("].");
                            self.out.push_str(if op_name == "appendToArray" {
                                "append("
                            } else {
                                "remove("
                            });
                            self.emit_value(value, arg_span(3));
                            self.out.push_str(")\n");
                        }
                        _ => {
                            self.out.push_str("del ");
                            self.out.push_str(&target);
                            self.out.push('[');
                            self.emit_value(index, arg_span(1));
                            self.out.push(']');
                            self.out.push('[');
                            self.emit_value(value, arg_span(3));
                            self.out.push(']');
                            self.out.push('\n');
                        }
                    }
                    return true;
                }
                let Some(operator) = indexed_modify_operator(op) else {
                    self.issue(
                        "unsupported-action-call",
                        format!("{name} operation argument has no compound-assignment form"),
                        arg_span(2).or(span),
                    );
                    return true;
                };
                self.out.push_str(&target);
                self.out.push('[');
                self.emit_value(index, arg_span(1));
                self.out.push_str("] ");
                self.out.push_str(operator);
                self.out.push(' ');
                self.emit_value(value, arg_span(3));
                self.out.push('\n');
                true
            }
            _ => false,
        }
    }

    fn indexed_target(&mut self, variable: &Value, span: Option<Span>) -> Option<String> {
        match variable {
            Value::GlobalVariable(name) => Some(name.clone()),
            Value::PlayerVariable { player, variable } => {
                let receiver = self.render_value_as(player, span, |this, player, span| {
                    this.emit_player_variable_receiver(player, span);
                });
                Some(format!("{receiver}.{variable}"))
            }
            _ => None,
        }
    }

    fn resolve_call_entry<'manifest>(
        manifest: &'manifest Manifest,
        name: &str,
        expected: FunctionKind,
        args_len: usize,
    ) -> Result<(&'manifest Function, bool), String> {
        let expected_action = expected.is_action();
        let by_catalog = |entry: &&Function| {
            entry.catalog_id.as_deref() == Some(name)
                && entry.kind.is_action() == expected_action
                && {
                    let provided = args_len.saturating_sub(usize::from(entry.kind.is_member()));
                    provided == entry.params.len()
                }
        };
        let (entry, member) = match manifest
            .functions()
            .iter()
            .filter(by_catalog)
            .min_by_key(|entry| entry.id.as_str() != name)
        {
            Some(entry) => (entry, entry.kind.is_member()),
            None => match manifest.resolve_function(name) {
                Some(entry) => (entry, false),
                None => match manifest.resolve_member(name) {
                    Some(entry) => (entry, true),
                    None => {
                        let message = if expected_action {
                            format!(
                                "action call '{name}' has no OPY source form on the \
                                 reconstruction surface"
                            )
                        } else {
                            format!(
                                "value call '{name}' has no OPY source form on the \
                                 reconstruction surface"
                            )
                        };
                        return Err(message);
                    }
                },
            },
        };
        if expected_action != entry.kind.is_action() {
            let message = match (expected_action, member) {
                (true, true) => format!(
                    "member value '{name}' cannot be emitted as an action on \
                     the reconstruction surface"
                ),
                (true, false) => format!(
                    "value function '{name}' cannot be emitted as an action on \
                     the reconstruction surface"
                ),
                (false, true) => format!(
                    "member action '{name}' cannot be emitted as a value on \
                     the reconstruction surface"
                ),
                (false, false) => format!(
                    "action function '{name}' cannot be emitted as a value on \
                     the reconstruction surface"
                ),
            };
            return Err(message);
        }
        Ok((entry, member))
    }

    fn emit_manifest_call(
        &mut self,
        entry: &Function,
        args: &[Value],
        member: bool,
        span: Option<Span>,
        argument_spans: &[Option<Span>],
    ) {
        let (receiver, params) = if member {
            match args.split_first() {
                Some((receiver, rest)) => (Some(receiver), rest),
                None => {
                    self.issue(
                        "unsupported-invalid-arity",
                        format!("member '{}' requires a receiver argument", entry.id),
                        span,
                    );
                    return;
                }
            }
        } else {
            (None, args)
        };
        let name = entry.id.as_str();
        if params.len() > entry.params.len() {
            self.issue(
                "unsupported-invalid-arity",
                format!(
                    "{} '{}' expects at most {} arguments but the Workshop call carries {}",
                    kind_label(entry.kind),
                    name,
                    entry.params.len(),
                    params.len()
                ),
                span,
            );
            return;
        }
        for (_index, param) in entry.params.iter().enumerate().skip(params.len()) {
            if !param.optional {
                self.issue(
                    "unsupported-missing-argument",
                    format!(
                        "{} '{}' omits parameter '{}'; the OPY frontend would \
                         reject or default-fill it and change the resulting Workshop program",
                        kind_label(entry.kind),
                        name,
                        param.name
                    ),
                    span,
                );
            }
        }

        if let Some(receiver) = receiver {
            let receiver_span = argument_spans.first().copied().flatten().or(span);
            self.emit_value(receiver, receiver_span);
            self.out.push('.');
        }
        self.out.push_str(name);
        self.out.push('(');
        if let Some(catalog_id) = &entry.catalog_id {
            let expected_kind = match entry.kind {
                FunctionKind::Action | FunctionKind::MemberAction => {
                    workshop_rs::catalog::Kind::Action
                }
                FunctionKind::Value | FunctionKind::MemberValue => {
                    workshop_rs::catalog::Kind::Value
                }
            };
            if self
                .catalog
                .spelling(expected_kind, self.locale, catalog_id)
                .is_none()
            {
                self.issue(
                    "catalog-error",
                    format!(
                        "manifest entry '{}' links catalogId '{catalog_id}' which is \
                         missing from the Workshop catalog",
                        entry.id
                    ),
                    span,
                );
            }
        }
        for (index, arg) in params.iter().enumerate() {
            if index > 0 {
                self.out.push_str(", ");
            }
            let argument_index = index + if member { 1 } else { 0 };
            let argument_span = argument_spans
                .get(argument_index)
                .copied()
                .flatten()
                .or(span);
            self.check_param_argument(entry, index, arg, argument_span);
            self.emit_value(arg, argument_span);
        }
        self.out.push(')');
    }

    fn check_param_argument(
        &mut self,
        entry: &Function,
        index: usize,
        arg: &Value,
        span: Option<Span>,
    ) {
        if let Some(domain) = entry
            .params
            .get(index)
            .and_then(|param| param.domain.as_ref())
        {
            match arg {
                Value::Enum { value_type, value } if value_type == domain => {
                    if !self.enum_member_in_domain(domain, value) {
                        self.issue(
                            "unsupported-enum-member",
                            format!(
                                "argument {} of '{}' uses enum member '{domain}.{value}' \
                                 which is outside the manifest's declared domain",
                                index + 1,
                                entry.id
                            ),
                            span,
                        );
                    }
                }
                Value::Enum { value_type, .. } => {
                    self.issue(
                        "unsupported-enum-domain-mismatch",
                        format!(
                            "argument {} of '{}' expects enum domain '{domain}' but \
                             the Workshop value carries '{value_type}'",
                            index + 1,
                            entry.id
                        ),
                        span,
                    );
                }
                _ => {
                    self.issue(
                        "unsupported-enum-domain-mismatch",
                        format!(
                            "argument {} of '{}' expects an enum member of domain \
                             '{domain}'",
                            index + 1,
                            entry.id
                        ),
                        span,
                    );
                }
            }
        }
        if policy::variable_args(&entry.id).contains(&index) {
            let is_variable =
                matches!(arg, Value::GlobalVariable(_) | Value::PlayerVariable { .. });
            if !is_variable {
                self.issue(
                    "unsupported-invalid-argument",
                    format!(
                        "argument {} of '{}' must be a variable reference",
                        index + 1,
                        entry.id
                    ),
                    span,
                );
            }
        }
    }

    fn enum_member_in_domain(&self, domain: &str, member: &str) -> bool {
        self.catalog.enum_domain(domain).is_some_and(|domain| {
            domain
                .members
                .iter()
                .any(|candidate| candidate.member == member)
        })
    }

    fn emit_value(&mut self, value: &Value, span: Option<Span>) {
        match value {
            Value::Number(value) => {
                if !value.is_finite() {
                    self.issue(
                        "unsupported-non-finite-number",
                        format!("non-finite number literal '{value}' has no OPY spelling"),
                        span,
                    );
                } else if *value < 0.0 {
                    self.out.push('-');
                    self.out
                        .push_str(&workshop_rs::format::format_number(-*value));
                } else {
                    self.out
                        .push_str(&workshop_rs::format::format_number(*value));
                }
            }
            Value::String(value) => self.emit_string_literal(value),
            Value::LocalizedString(value) => {
                self.emit_localized_literal(value, span);
            }
            Value::Bool(value) => {
                self.out.push_str(if *value { "true" } else { "false" });
            }
            Value::Null => {
                self.out.push_str("null");
            }
            Value::Array(elements) => {
                self.out.push('[');
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        self.out.push_str(", ");
                    }
                    self.emit_value(element, span);
                }
                self.out.push(']');
            }
            Value::Vector { x, y, z } => {
                self.out.push_str("vect(");
                self.emit_value(x, span);
                self.out.push_str(", ");
                self.emit_value(y, span);
                self.out.push_str(", ");
                self.emit_value(z, span);
                self.out.push(')');
            }
            Value::Enum { value_type, value } => {
                self.emit_enum(value_type, value, span);
            }
            Value::GlobalVariable(variable) => {
                if !self
                    .program
                    .global_variables
                    .iter()
                    .any(|declaration| declaration.name == *variable)
                {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown global variable '{variable}'"),
                        span,
                    );
                    return;
                }
                self.out.push_str(emitted(&self.global_names, variable));
            }
            Value::PlayerVariable { player, variable } => {
                if !self
                    .program
                    .player_variables
                    .iter()
                    .any(|declaration| declaration.name == *variable)
                {
                    self.issue(
                        "unsupported-dangling",
                        format!("unknown player variable '{variable}'"),
                        span,
                    );
                    return;
                }
                self.emit_player_variable_receiver(player, span);
                self.out.push('.');
                self.out.push_str(emitted(&self.player_names, variable));
            }
            Value::Subroutine(name) => {
                self.issue(
                    "unsupported-subroutine-value",
                    format!(
                        "subroutine value '{name}' is not representable in this \
                         value position on the OPY surface"
                    ),
                    span,
                );
            }
            Value::EventPlayer => {
                self.out.push_str("eventPlayer");
            }
            Value::Call { name, args } => {
                self.emit_value_call(name, args, span);
            }
        }
    }

    fn emit_enum(&mut self, value_type: &str, value: &str, span: Option<Span>) {
        let Some(domain) = self.catalog.enum_domain(value_type) else {
            self.issue(
                "unsupported-enum-domain",
                format!(
                    "enum domain '{value_type}' is outside the manifest's declared \
                     reconstruction surface"
                ),
                span,
            );
            return;
        };
        if !domain.members.iter().any(|member| member.member == value) {
            self.issue(
                "unsupported-enum-member",
                format!(
                    "enum member '{value_type}.{value}' is outside the manifest's \
                     declared domain"
                ),
                span,
            );
            return;
        }
        let Some((source_name, spelling)) = crate::enums::spelling_of_member(value_type, value)
        else {
            self.issue(
                "unsupported-enum-member",
                format!("enum member '{value_type}.{value}' has no OverPy source spelling"),
                span,
            );
            return;
        };
        self.out.push_str(source_name);
        self.out.push('.');
        self.out.push_str(spelling);
    }

    fn emit_localized_literal(&mut self, id: &str, span: Option<Span>) {
        let Some(spelling) = self
            .catalog
            .localized_string_spelling(&Locale::new("en-US"), id)
        else {
            self.issue(
                "unsupported-localized-string",
                format!("localized string id '{id}' has no catalog spelling"),
                span,
            );
            return;
        };
        self.out.push('l');
        self.emit_string_literal(spelling);
    }

    fn emit_lambda_body(
        &mut self,
        member_call: &str,
        receiver: &Value,
        body: &Value,
        span: Option<Span>,
    ) {
        let uses_index = uses_call(body, "currentArrayIndex");
        let element = self.fresh_generated_name("__elem");
        let index = uses_index.then(|| self.fresh_generated_name("__idx"));
        self.out.push('(');
        self.emit_value(receiver, span);
        self.out.push_str(").");
        self.out.push_str(member_call);
        self.out.push_str("(lambda ");
        self.out.push_str(&element);
        if let Some(index) = &index {
            self.out.push_str(", ");
            self.out.push_str(index);
        }
        self.out.push_str(": ");
        self.lambda_binders.push((element, index));
        self.emit_value(body, span);
        self.lambda_binders.pop();
        self.out.push(')');
    }

    fn emit_sorted(&mut self, array: &Value, key: &Value, span: Option<Span>) {
        if matches!(key, Value::Call { name, args } if name == "currentArrayElement" && args.is_empty())
        {
            self.out.push_str("sorted(");
            self.emit_value(array, span);
            self.out.push(')');
            return;
        }
        let uses_index = uses_call(key, "currentArrayIndex");
        let element = self.fresh_generated_name("__elem");
        let index = uses_index.then(|| self.fresh_generated_name("__idx"));
        self.out.push_str("sorted(");
        self.emit_value(array, span);
        self.out.push_str(", lambda ");
        self.out.push_str(&element);
        if let Some(index) = &index {
            self.out.push_str(", ");
            self.out.push_str(index);
        }
        self.out.push_str(": ");
        self.lambda_binders.push((element, index));
        self.emit_value(key, span);
        self.lambda_binders.pop();
        self.out.push(')');
    }

    fn emit_syntax_value(&mut self, name: &str, args: &[Value], span: Option<Span>) -> bool {
        if args.len() == 2
            && let Some((_, operator)) = ARITHMETIC_OPS.iter().find(|(n, _)| *n == name)
        {
            self.out.push('(');
            self.emit_value(&args[0], span);
            self.out.push(' ');
            self.out.push_str(operator);
            self.out.push(' ');
            self.emit_value(&args[1], span);
            self.out.push(')');
            return true;
        }
        match name {
            // valueInArray(a, i) → `a[i]`; a literal-zero index is
            // unreachable: every `a[i]` spelling folding to 0 normalizes to
            // `firstOf`, so it stays a structured refusal.
            "valueInArray" => {
                let [array, index] = args else {
                    return false;
                };
                if matches!(index, Value::Number(n) if *n == 0.0) {
                    self.issue(
                        "unsupported-value-call",
                        "valueInArray with a literal zero index re-lowers to firstOf \
                         and cannot round-trip",
                        span,
                    );
                    return true;
                }
                self.out.push('(');
                self.emit_value(array, span);
                self.out.push_str(")[");
                self.emit_value(index, span);
                self.out.push(']');
                true
            }
            "firstOf" => {
                let [array] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(array, span);
                self.out.push_str(")[0]");
                true
            }
            "emptyArray" if args.is_empty() => {
                self.out.push_str("[]");
                true
            }
            // slice(a, start, count) → `a.slice(start[, count])`; the count
            // parameter defaults to 999999999999 on the source side.
            "slice" => {
                let [array, start, count] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(array, span);
                self.out.push_str(").slice(");
                self.emit_value(start, span);
                if !matches!(count, Value::Number(n) if *n == 999_999_999_999.0) {
                    self.out.push_str(", ");
                    self.emit_value(count, span);
                }
                self.out.push(')');
                true
            }
            "ifThenElse" => {
                let [condition, then_value, else_value] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(then_value, span);
                self.out.push_str(" if ");
                self.emit_value(condition, span);
                self.out.push_str(" else ");
                self.emit_value(else_value, span);
                self.out.push(')');
                true
            }
            "roundToInteger" => {
                let [value, rounding] = args else {
                    return false;
                };
                let spelling = match rounding {
                    Value::Enum { value_type, value } if value_type == "Rounding" => {
                        match value.as_str() {
                            "UP" => "ceil",
                            "DOWN" => "floor",
                            "NEAREST" => "round",
                            _ => return false,
                        }
                    }
                    _ => return false,
                };
                self.out.push_str(spelling);
                self.out.push('(');
                self.emit_value(value, span);
                self.out.push(')');
                true
            }
            "sortedArray" => {
                let [array, key] = args else {
                    return false;
                };
                self.emit_sorted(array, key, span);
                true
            }
            "mappedArray" => {
                let [array, body] = args else {
                    return false;
                };
                self.emit_lambda_body("map", array, body, span);
                true
            }
            "filteredArray" => {
                let [array, condition] = args else {
                    return false;
                };
                self.emit_lambda_body("filter", array, condition, span);
                true
            }
            "isTrueForAll" | "isTrueForAny" => {
                let [array, condition] = args else {
                    return false;
                };
                let member = if name == "isTrueForAll" { "all" } else { "any" };
                if matches!(condition, Value::Call { name, args } if name == "currentArrayElement" && args.is_empty())
                {
                    self.out.push_str(member);
                    self.out.push('(');
                    self.emit_value(array, span);
                    self.out.push(')');
                } else {
                    self.emit_lambda_body(member, array, condition, span);
                }
                true
            }
            "arrayContains" => {
                let [array, needle] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(needle, span);
                self.out.push_str(" in ");
                self.emit_value(array, span);
                self.out.push(')');
                true
            }
            "string" => {
                let [localized, ..] = args else {
                    return false;
                };
                let Value::LocalizedString(id) = localized else {
                    return false;
                };
                self.emit_localized_literal(id, span);
                if args.len() > 1 {
                    self.out.push_str(".format(");
                    for (index, arg) in args.iter().skip(1).enumerate() {
                        if index > 0 {
                            self.out.push_str(", ");
                        }
                        self.emit_value(arg, span);
                    }
                    self.out.push(')');
                }
                true
            }
            "memberAccess" => {
                let [receiver, member] = args else {
                    return false;
                };
                let Value::String(member) = member else {
                    return false;
                };
                if !is_simple_ident(member) {
                    self.issue(
                        "unsupported-value-call",
                        format!("member name '{member}' is not a plain OPY identifier"),
                        span,
                    );
                    return true;
                }
                self.out.push('(');
                self.emit_value(receiver, span);
                self.out.push_str(").");
                self.out.push_str(member);
                true
            }
            "__xComponentOf__" | "__yComponentOf__" | "__zComponentOf__" => {
                let [receiver] = args else {
                    return false;
                };
                self.out.push('(');
                self.emit_value(receiver, span);
                self.out.push_str(").");
                self.out.push_str(match name {
                    "__xComponentOf__" => "x",
                    "__yComponentOf__" => "y",
                    _ => "z",
                });
                true
            }
            "currentArrayElement" if args.is_empty() => {
                match self.lambda_binders.last() {
                    Some((element, _)) => {
                        self.out.push_str(&element.clone());
                    }
                    None => {
                        self.issue(
                            "unsupported-value-call",
                            "currentArrayElement outside an array callback has no OPY binder",
                            span,
                        );
                    }
                }
                true
            }
            "currentArrayIndex" if args.is_empty() => {
                match self.lambda_binders.last() {
                    Some((_, Some(index))) => {
                        self.out.push_str(&index.clone());
                    }
                    Some(_) => {
                        self.issue(
                            "unsupported-value-call",
                            "currentArrayIndex used but the callback has no index binder",
                            span,
                        );
                    }
                    None => {
                        self.issue(
                            "unsupported-value-call",
                            "currentArrayIndex outside an array callback has no OPY binder",
                            span,
                        );
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn emit_value_call(&mut self, name: &str, args: &[Value], span: Option<Span>) {
        if name == "localPlayer" && args.is_empty() {
            self.out.push_str(name);
            return;
        }
        if self.emit_syntax_value(name, args, span) {
            return;
        }
        if BINARY_OPS.contains(&name) && args.len() == 2 {
            self.out.push('(');
            self.emit_value(&args[0], span);
            self.out.push(' ');
            self.out.push_str(name);
            self.out.push(' ');
            self.emit_value(&args[1], span);
            self.out.push(')');
            return;
        }
        if name == "not" && args.len() == 1 {
            self.out.push_str("(not ");
            self.emit_value(&args[0], span);
            self.out.push(')');
            return;
        }
        if name == "-" && args.len() == 1 {
            self.out.push_str("(-");
            self.emit_value(&args[0], span);
            self.out.push(')');
            return;
        }
        if name == "customString" {
            let Some(first) = args.first() else {
                self.issue(
                    "unsupported-value-call",
                    "format call without a receiver is outside the reconstruction surface",
                    span,
                );
                return;
            };
            let Value::String(text) = first else {
                self.issue(
                    "unsupported-value-call",
                    "format call without a string receiver is outside the \
                     reconstruction surface",
                    span,
                );
                return;
            };
            self.emit_string_literal(text);
            self.out.push_str(".format(");
            for (index, arg) in args.iter().skip(1).enumerate() {
                if index > 0 {
                    self.out.push_str(", ");
                }
                self.emit_value(arg, span);
            }
            self.out.push(')');
            return;
        }
        if DEDICATED_VALUE_NAMES.contains(&name) {
            self.issue(
                "unsupported-value-call",
                format!(
                    "value call '{name}' is lowered to a dedicated Workshop value \
                     and has no reconstructible call form"
                ),
                span,
            );
            return;
        }
        let (entry, member) =
            match Self::resolve_call_entry(self.manifest, name, FunctionKind::Value, args.len()) {
                Ok(call) => call,
                Err(message) => {
                    self.issue("unsupported-value-call", message, span);
                    return;
                }
            };
        if !member && crate::lower::policy::function_context(&entry.id).is_some() {
            self.issue(
                "unsupported-value-call",
                format!(
                    "value call '{name}' is only valid as a for-loop iterable on \
                     the OPY surface"
                ),
                span,
            );
            return;
        }
        if !member && self.emitted_subroutine_names.contains(entry.id.as_str()) {
            self.issue(
                "unsupported-value-call",
                format!(
                    "value '{name}' is ambiguous with a subroutine of the same \
                     emitted name on the OPY surface"
                ),
                span,
            );
            return;
        }
        self.emit_manifest_call(entry, args, member, span, &[]);
    }

    fn emit_string_literal(&mut self, value: &str) {
        self.out.push('"');
        for ch in value.chars() {
            match ch {
                '\\' => self.out.push_str("\\\\"),
                '"' => self.out.push_str("\\\""),
                '\n' => self.out.push_str("\\n"),
                '\t' => self.out.push_str("\\t"),
                '\r' => self.out.push_str("\\r"),
                other if other.is_control() => {
                    self.out.push_str(&format!("\\u{:04X}", other as u32));
                }
                other => self.out.push(other),
            }
        }
        self.out.push('"');
    }
}

fn uses_call(value: &Value, name: &str) -> bool {
    match value {
        Value::Call {
            name: call_name,
            args,
        } => call_name == name || args.iter().any(|arg| uses_call(arg, name)),
        Value::Array(elements) => elements.iter().any(|element| uses_call(element, name)),
        Value::Vector { x, y, z } => uses_call(x, name) || uses_call(y, name) || uses_call(z, name),
        _ => false,
    }
}

fn indexed_modify_operator(op: &Value) -> Option<&'static str> {
    let Value::Call { name, args } = op else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    INDEXED_MODIFY_OPS
        .iter()
        .find(|(op_name, _)| *op_name == name)
        .map(|(_, operator)| *operator)
}

fn literal_positive_int(value: &Value) -> Option<usize> {
    let Value::Number(number) = value else {
        return None;
    };
    (number.is_finite() && number.fract() == 0.0 && *number >= 0.0).then_some(*number as usize)
}

fn is_simple_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

enum SettingsJson {
    Object(Vec<(String, SettingsJson)>),
    Array(Vec<SettingsJson>),
    String(String),
    Number(String),
    Bool(bool),
}

/// Write `value` as JSONC the settings parser reads back — the layout the
/// pinned OverPy's `JSON.stringify(result, null, 4)` produces: four-space
/// indentation, empty containers inline, and no trailing comma.
fn write_settings_json(out: &mut String, value: &SettingsJson, indent: usize) {
    let pad = |out: &mut String| {
        for _ in 0..indent {
            out.push(' ');
        }
    };
    match value {
        SettingsJson::Object(entries) if entries.is_empty() => out.push_str("{}"),
        SettingsJson::Object(entries) => {
            out.push_str("{\n");
            for (index, (key, value)) in entries.iter().enumerate() {
                pad(out);
                for _ in 0..4 {
                    out.push(' ');
                }
                out.push('"');
                push_json_escaped(out, key);
                out.push_str("\": ");
                write_settings_json(out, value, indent + 4);
                if index + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out);
            out.push('}');
        }
        SettingsJson::Array(elements) if elements.is_empty() => out.push_str("[]"),
        SettingsJson::Array(elements) => {
            out.push_str("[\n");
            for (index, element) in elements.iter().enumerate() {
                pad(out);
                for _ in 0..4 {
                    out.push(' ');
                }
                write_settings_json(out, element, indent + 4);
                if index + 1 < elements.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out);
            out.push(']');
        }
        SettingsJson::String(value) => {
            out.push('"');
            push_json_escaped(out, value);
            out.push('"');
        }
        SettingsJson::Number(value) => out.push_str(value),
        SettingsJson::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
    }
}

fn push_json_escaped(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            other => out.push(other),
        }
    }
}

fn display_names(prefixes: &[String]) -> HashSet<String> {
    workshop_rs::settings::definitions()
        .filter(|definition| {
            let path = definition.path();
            prefixes.iter().any(|prefix| path.starts_with(prefix))
        })
        .map(|definition| definition.presentation().english_name.to_string())
        .collect()
}

fn emittable_member_name(name: &str) -> bool {
    !name.is_empty()
        && name.split_whitespace().collect::<Vec<_>>().join(" ") == name
        && !name
            .chars()
            .any(|c| matches!(c, ':' | '{' | '}' | '"' | '#' | '\'' | '\u{FEFF}'))
}

fn emittable_raw_value(value: &str) -> bool {
    !value.is_empty()
        && value.split_whitespace().collect::<Vec<_>>().join(" ") == value
        && !value
            .chars()
            .any(|c| matches!(c, '{' | '}' | '"' | '#' | '\'' | '\u{FEFF}'))
}

fn index_order(variables: &[Variable]) -> Vec<(usize, u32)> {
    let mut order: Vec<(usize, u32)> = variables
        .iter()
        .enumerate()
        .map(|(position, variable)| (position, variable.index.unwrap_or(position as u32)))
        .collect();
    order.sort_by_key(|&(_, index)| index);
    order
}

fn emitted<'a>(map: &'a HashMap<String, String>, name: &'a str) -> &'a str {
    map.get(name).map_or(name, String::as_str)
}

fn needs_mangling(name: &str, namespace: NameNamespace) -> bool {
    !is_identifier(name)
        || is_reserved_name(name, namespace)
        || (namespace == NameNamespace::Subroutine
            && (SHADOWED_SUBROUTINE_NAMES.contains(&name)
                || SpecialValueCall::from_name(name).is_some()))
}

fn mangle_names<'a>(
    names: impl Iterator<Item = &'a str>,
    namespace: NameNamespace,
) -> HashMap<String, String> {
    let names: Vec<&str> = names.collect();
    let canonical: HashSet<&str> = names.iter().copied().collect();
    let mut emitted_names = HashMap::new();
    let mut taken = HashSet::new();
    for name in names {
        if emitted_names.contains_key(name) {
            continue;
        }
        let stem = if needs_mangling(name, namespace) {
            mangle_base(name)
        } else {
            name.to_string()
        };
        let mut emitted_name = stem.clone();
        let mut counter = 0u32;
        while needs_mangling(&emitted_name, namespace)
            || taken.contains(emitted_name.as_str())
            || (emitted_name != name && canonical.contains(emitted_name.as_str()))
        {
            counter += 1;
            emitted_name = format!("{stem}_opy{counter}");
        }
        taken.insert(emitted_name.clone());
        emitted_names.insert(name.to_string(), emitted_name);
    }
    emitted_names
}

fn mangle_base(name: &str) -> String {
    let mut mangled = String::with_capacity(name.len() + 2);
    for ch in name.chars() {
        if is_ident_continue(ch) {
            mangled.push(ch);
        } else {
            mangled.push_str(&format!("_x{:x}_", ch as u32));
        }
    }
    if !mangled.starts_with(is_ident_start) {
        mangled.insert_str(0, "v_");
    }
    mangled
}

fn kind_label(kind: FunctionKind) -> &'static str {
    match kind {
        FunctionKind::Action => "action",
        FunctionKind::Value => "value",
        FunctionKind::MemberAction => "member action",
        FunctionKind::MemberValue => "member value",
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::reconstruct;
    use crate::compiler::Compiler;
    use workshop_rs::catalog::{Catalog, Locale};
    use workshop_rs::source::{Position, SourceFile, Span};
    use workshop_rs::{
        Action, Condition, Event, EventTarget, EventTeam, ModifyOp, PlayerEventKind, Program, Rule,
        Subroutine, Value, Variable,
    };

    fn compile_to_program(source: &str, path: &str) -> Program {
        let artifact = Compiler::new()
            .expect("compiler loads")
            .compile_source_artifact(source, path, Path::new("."))
            .expect("reconstructed OPY compiles");
        let catalog = Catalog::builtin().expect("catalog loads");
        workshop_rs::parser::parse(&artifact.emitted, &catalog, &Locale::new("en-US"))
            .expect("compiler output parses through the public Workshop parser")
    }

    #[test]
    fn reconstructs_public_program_control_flow() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::If {
                    condition: Value::call(
                        "==",
                        [Value::global_variable("score"), Value::number(0.0)],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                })
                .action(Action::ElseIf {
                    condition: Value::call(
                        "<",
                        [Value::global_variable("score"), Value::number(0.0)],
                    ),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "score".into(),
                    op: ModifyOp::Add,
                    value: Value::number(2.0),
                })
                .action(Action::Else)
                .action(Action::ModifyGlobalVariable {
                    variable: "score".into(),
                    op: ModifyOp::Add,
                    value: Value::number(3.0),
                })
                .action(Action::End)
                .action(Action::While {
                    condition: Value::call(
                        "<",
                        [Value::global_variable("score"), Value::number(10.0)],
                    ),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "score".into(),
                    op: ModifyOp::Add,
                    value: Value::number(1.0),
                })
                .action(Action::End)
                .action(Action::ForGlobalVariable {
                    variable: "score".into(),
                    start: Value::number(0.0),
                    stop: Value::number(2.0),
                    step: Value::number(1.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "score".into(),
                    op: ModifyOp::Add,
                    value: Value::number(1.0),
                })
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("canonical Program should reconstruct");
        assert!(source.contains("if (score == 0):"));
        assert!(source.contains("elif (score < 0):"));
        assert!(source.contains("else:"));
        assert!(source.contains("while (score < 10):"));
        assert!(source.contains("for score in range(0, 2, 1):"));

        let reparsed = compile_to_program(&source, "reconstructed.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn initializer_name_with_another_event_remains_an_ordinary_rule() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("Initialize global variables", Event::EachPlayer).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("ordinary rule should reconstruct");
        assert!(source.contains("rule \"Initialize global variables\":"));
        assert!(source.contains("@Event eachPlayer"));
        let reparsed = compile_to_program(&source, "initializer-name.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn disabled_initializer_reconstructs_as_a_disabled_ordinary_rule() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        let mut initializer = Rule::new("Initialize global variables", Event::Global).action(
            Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            },
        );
        initializer.disabled = true;
        program.rule(initializer);

        let source = reconstruct(&program).expect("disabled initializer reconstructs");
        assert!(
            source.contains(
                "rule \"Initialize global variables\":\n    @Event global\n    @Disabled\n"
            ),
            "{source}"
        );
        assert!(!source.contains("globalvar score ="), "{source}");

        let reparsed = compile_to_program(&source, "disabled-initializer.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn disabled_subroutine_rule_reconstructs_as_an_inert_def() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.subroutine(Subroutine::with_index("reset", 0));
        let mut subroutine = Rule::new("Subroutine reset", Event::Subroutine("reset".into()))
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(0.0),
            });
        subroutine.disabled = true;
        program.rule(subroutine);

        let source = reconstruct(&program).expect("disabled subroutine reconstructs");
        assert!(source.contains("def reset():\n"), "{source}");
        assert!(source.contains("    # @Disabled\n"), "{source}");
        assert!(source.contains("# score = 0"), "{source}");
        assert!(source.contains("    pass\n"), "{source}");

        let reparsed = compile_to_program(&source, "disabled-subroutine.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn escaped_rule_names_remain_valid_opy_strings() {
        let rule_name = "quoted \"rule\" \\ path\nnext\u{0001}";
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(rule_name, Event::Global).action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("special characters should be escaped");
        let artifact = Compiler::new()
            .expect("compiler loads")
            .compile_source_artifact(&source, "escaped-name.opy", Path::new("."))
            .expect("escaped OPY rule name parses");
        assert_eq!(artifact.wir.rules[0].name, rule_name);
    }

    #[test]
    fn reconstructs_public_subroutine_names() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.subroutine(Subroutine::with_index("reset", 0));
        program.rule(
            Rule::new("Subroutine reset", Event::Subroutine("reset".into())).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(0.0),
                },
            ),
        );
        program.rule(
            Rule::new("main", Event::Global).action(Action::CallSubroutine {
                subroutine: "reset".into(),
            }),
        );

        let source = reconstruct(&program).expect("canonical subroutine should reconstruct");
        assert!(source.contains("def reset():"));
        assert!(source.contains("reset()"));

        let reparsed = compile_to_program(&source, "subroutine.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn reconstructs_pinned_subroutine_index() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.subroutine(Subroutine::with_index("reset", 3));
        program.rule(
            Rule::new("Subroutine reset", Event::Subroutine("reset".into())).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(0.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("pinned subroutine index should reconstruct");
        assert!(source.contains("subroutine reset 3\n"), "{source}");

        let reparsed = compile_to_program(&source, "subroutine-index.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn action_argument_diagnostics_use_public_argument_spans() {
        let mut program = Program::new();
        let file = program.add_file(SourceFile::new("main.opy"));
        program.rule(Rule::new("main", Event::Global).action(Action::call(
            "chaseAtRate",
            [
                Value::number(1.0),
                Value::number(10.0),
                Value::number(1.0),
                Value::Enum {
                    value_type: "ChaseRateReeval".into(),
                    value: "DESTINATION_AND_RATE".into(),
                },
            ],
        )));
        let action_span = Span::new(file, Position::new(1, 1), Position::new(1, 25));
        let argument_span = Span::new(file, Position::new(1, 13), Position::new(1, 14));
        program.set_action_span(0, 0, Some(action_span)).unwrap();
        program
            .set_action_argument_span(0, 0, 0, Some(argument_span))
            .unwrap();

        let error = reconstruct(&program).expect_err("the first parameter must be a variable");
        assert_eq!(error.issues[0].code, "unsupported-invalid-argument");
        assert_eq!(error.issues[0].span, Some(argument_span));
    }

    #[test]
    fn rejects_public_program_actions_without_an_opy_form() {
        let mut program = Program::new();
        program.rule(
            Rule::new("main", Event::Global).action(Action::AssignMember {
                target: Value::EventPlayer,
                op: None,
                value: Value::number(1.0),
            }),
        );

        let error = reconstruct(&program).expect_err("dynamic member assignment is unsupported");
        assert_eq!(error.issues[0].code, "unsupported-member-assignment");
    }

    #[test]
    fn catalog_identity_prefers_the_canonical_source_spelling() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::call(
                    "getPlayersInRadius",
                    [
                        Value::EventPlayer,
                        Value::number(3.0),
                        Value::Enum {
                            value_type: "Team".into(),
                            value: "ALL".into(),
                        },
                        Value::Enum {
                            value_type: "LosCheck".into(),
                            value: "OFF".into(),
                        },
                    ],
                ),
            }),
        );

        let source = reconstruct(&program).expect("canonical call should reconstruct");
        assert!(source.contains("getPlayersInRadius("));
        assert!(!source.contains("getRealPlayersInRadius"));

        let reparsed = compile_to_program(&source, "players-in-radius.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn null_values_reconstruct_with_the_null_spelling() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::Null,
            }),
        );

        let source = reconstruct(&program).expect("null should reconstruct");
        assert!(source.contains("g = null"), "{source}");
        assert!(!source.contains("None"), "{source}");

        let reparsed = compile_to_program(&source, "null.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn custom_string_calls_reconstruct_as_format() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::call(
                    "customString",
                    [Value::string("hp: {0}"), Value::EventPlayer],
                ),
            }),
        );

        let source = reconstruct(&program).expect("customString should reconstruct");
        assert!(source.contains("\"hp: {0}\".format(eventPlayer)"));

        let reparsed = compile_to_program(&source, "customString.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    fn parse_workshop(source: &str) -> Program {
        workshop_rs::parser::parse(
            source,
            &Catalog::builtin().expect("catalog loads"),
            &Locale::new("en-US"),
        )
        .expect("workshop source parses")
    }

    fn settings_round_trip(workshop_source: &str) -> (String, Program, Program) {
        let program = parse_workshop(workshop_source);
        let source = reconstruct(&program).expect("settings reconstruct");
        let reparsed = compile_to_program(&source, "reconstructed.opy");
        (source, program, reparsed)
    }

    #[test]
    fn reconstructs_main_and_lobby_settings() {
        let (source, program, reparsed) = settings_round_trip(
            "settings\n{\n\tmain\n\t{\n\t\tMode Name: \"Recon\"\n\t\tDescription: \"test\"\n\t}\n\tlobby\n\t{\n\t\tAllow Players Who Are In Queue: Yes\n\t\tMax Team 1 Players: 4\n\t}\n\tmodes\n\t{\n\t\tSkirmish\n\t\t{\n\t\t}\n\t}\n}\n",
        );
        assert!(source.contains("\"main\""), "{source}");
        assert!(source.contains("\"lobby\""), "{source}");
        assert!(
            workshop_rs::roundtrip::equivalent(&program, &reparsed),
            "{source}"
        );
    }

    #[test]
    fn reconstructs_mode_settings() {
        let (source, program, reparsed) = settings_round_trip(
            "settings\n{\n\tmodes\n\t{\n\t\tdisabled Skirmish\n\t\t{\n\t\t\tLimit Valid Control Points: Fourth\n\t\t\tRandom Hero Role Limit Per Team: 3\n\t\t\tenabled maps\n\t\t\t{\n\t\t\t\tWorkshop Chamber\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        assert!(source.contains("\"skirmish\""), "{source}");
        assert!(source.contains("\"enabled\": false"), "{source}");
        assert!(source.contains("workshopChamber"), "{source}");
        assert!(
            workshop_rs::roundtrip::equivalent(&program, &reparsed),
            "{source}"
        );
    }

    #[test]
    fn reconstructs_hero_settings() {
        let (source, program, reparsed) = settings_round_trip(
            "settings\n{\n\tmodes\n\t{\n\t\tSkirmish\n\t\t{\n\t\t}\n\t}\n\theroes\n\t{\n\t\tGeneral\n\t\t{\n\t\t\tAbility Cooldown Time: 137%\n\t\t\tBaptiste\n\t\t\t{\n\t\t\t\tAbility 1 Cooldown Time: 60%\n\t\t\t}\n\t\t}\n\t\tTeam 1\n\t\t{\n\t\t\tenabled heroes\n\t\t\t{\n\t\t\t\tBaptiste\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        assert!(source.contains("\"allTeams\""), "{source}");
        assert!(source.contains("\"baptiste\""), "{source}");
        assert!(source.contains("\"enabledHeroes\""), "{source}");
        assert!(
            workshop_rs::roundtrip::equivalent(&program, &reparsed),
            "{source}"
        );
    }

    #[test]
    fn reconstructs_empty_and_carried_settings_groups() {
        let (source, program, reparsed) = settings_round_trip(
            "settings\n{\n\tmodes\n\t{\n\t\tSkirmish\n\t\t{\n\t\t}\n\t}\n\tmain\n\t{\n\t\tsomeCustomKey: some value\n\t\tanotherKey:\n\t}\n}\n",
        );
        assert!(
            source.contains("\"someCustomKey\": \"some value\""),
            "{source}"
        );
        assert!(source.contains("\"anotherKey\": \"\""), "{source}");
        assert!(
            workshop_rs::roundtrip::equivalent(&program, &reparsed),
            "{source}"
        );
    }

    #[test]
    fn reconstructs_extensions_as_directives() {
        let workshop =
            "settings\n{\n\textensions\n\t{\n\t\tBeam Effects\n\t\tProjectiles\n\t}\n}\n";
        let program = parse_workshop(workshop);
        let source = reconstruct(&program).expect("extensions reconstruct");
        assert!(source.contains("#!extension beamEffects"), "{source}");
        assert!(source.contains("#!extension projectiles"), "{source}");
        let reparsed = compile_to_program(&source, "extensions.opy");
        assert!(
            workshop_rs::roundtrip::equivalent(&program, &reparsed),
            "{source}"
        );
    }

    #[test]
    fn refuses_unemittable_settings_map_element() {
        let program = parse_workshop(
            "settings\n{\n\tmodes\n\t{\n\t\tSkirmish\n\t\t{\n\t\t\tenabled maps\n\t\t\t{\n\t\t\t\tWorkshop Chamber 972777519512064577\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let error = reconstruct(&program).expect_err("unresolvable map refuses");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-settings" && issue.span.is_some()),
            "{error:?}"
        );
    }

    // Pinned upstream decompiler inputs (`Zezombye/overpy` v9.7.17,
    // `src/tests/decompiler/`); see
    // `crates/opy-rs/tests/fixtures/reconstruction/README.md` for attribution.

    #[test]
    fn reconstructs_pinned_shion_settings_fixture() {
        let (source, program, reparsed) = settings_round_trip(include_str!(
            "../../tests/fixtures/reconstruction/shionSettings.txt"
        ));
        assert!(source.contains("\"shion\""), "{source}");
        assert!(source.contains("\"skirmish\": {}"), "{source}");
        assert!(
            workshop_rs::roundtrip::equivalent(&program, &reparsed),
            "{source}"
        );
    }

    #[test]
    fn refuses_unemittable_map_in_pinned_custom_game_settings_fixture() {
        let program = parse_workshop(include_str!(
            "../../tests/fixtures/reconstruction/customGameSettings.txt"
        ));
        let error = reconstruct(&program).expect_err("unresolvable map refuses");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-settings" && issue.span.is_some()),
            "{error:?}"
        );
    }
    #[test]
    fn smoke_syntax_produced_actions() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.player_variable(Variable::with_index("p", 0));
        program.subroutine(Subroutine::with_index("sub", 0));
        program.rule(
            Rule::new("Subroutine sub", Event::Subroutine("sub".into())).action(
                Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(0.0),
                },
            ),
        );
        program.rule(
            Rule::new("main", Event::EachPlayer)
                .action(Action::ForPlayerVariable {
                    player: Value::EventPlayer,
                    variable: "p".into(),
                    start: Value::number(0.0),
                    stop: Value::number(5.0),
                    step: Value::number(1.0),
                })
                .action(Action::SetPlayerVariable {
                    player: Value::call(
                        "valueInArray",
                        [
                            Value::call(
                                "allPlayers",
                                [Value::Enum {
                                    value_type: "Team".into(),
                                    value: "ALL".into(),
                                }],
                            ),
                            Value::number(1.0),
                        ],
                    ),
                    variable: "p".into(),
                    value: Value::number(-3.0),
                })
                .action(Action::End)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("add", [Value::global_variable("g"), Value::number(2.0)]),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Min,
                    value: Value::number(9.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Max,
                    value: Value::number(-9.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::RemoveFromArrayByValue,
                    value: Value::number(1.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::RemoveFromArrayByIndex,
                    value: Value::number(0.0),
                })
                .action(Action::call(
                    "setGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::number(7.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::call("add", Vec::<Value>::new()),
                        Value::number(3.0),
                    ],
                ))
                .action(Action::call(
                    "startRule",
                    [
                        Value::Subroutine("sub".into()),
                        Value::Enum {
                            value_type: "StartRuleBehavior".into(),
                            value: "RESTART".into(),
                        },
                    ],
                ))
                .action(Action::call(
                    "skipIf",
                    [
                        Value::call("==", [Value::global_variable("g"), Value::number(1.0)]),
                        Value::number(5.0),
                    ],
                ))
                .action(Action::If {
                    condition: Value::Bool(true),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(1.0),
                })
                .action(Action::Else)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(5.0),
                })
                .action(Action::End)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(2.0),
                })
                .action(Action::call(
                    "abortIf",
                    [Value::call(
                        "==",
                        [Value::global_variable("g"), Value::number(3.0)],
                    )],
                ))
                .action(Action::call(
                    "loopIf",
                    [Value::call(
                        "==",
                        [Value::global_variable("g"), Value::number(4.0)],
                    )],
                )),
        );

        let source = reconstruct(&program).expect("program should reconstruct");
        assert!(source.contains("for eventPlayer.p in range(0, 5, 1):"));
        assert!(source.contains("g min= 9"));
        assert!(source.contains("g max= -9"));
        assert!(source.contains("g.remove(1)"));
        assert!(source.contains("del g[0]"));
        assert!(source.contains("g[1] = 7"));
        assert!(source.contains("g[1] += 3"));
        assert!(source.contains("async(sub, StartRuleBehavior.RESTART)"));
        assert!(source.contains("__op_label_1"));
        assert_behaviorally_equivalent(&program, &source);
    }

    fn one_action_program(action: Action) -> Program {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.player_variable(Variable::with_index("p", 0));
        program.rule(Rule::new("main", Event::Global).action(action));
        program
    }

    fn one_value_program(value: Value) -> Program {
        one_action_program(Action::SetGlobalVariable {
            variable: "g".into(),
            value,
        })
    }

    fn assert_error_code(program: &Program, code: &str) {
        let error = reconstruct(program).expect_err("program should be refused");
        assert!(
            error.issues.iter().any(|issue| issue.code == code),
            "expected '{code}' in {error:?}"
        );
    }

    #[test]
    fn nested_player_variable_for_loops_round_trip() {
        let mut program = Program::new();
        program.player_variable(Variable::with_index("p", 0));
        program.player_variable(Variable::with_index("q", 1));
        program.rule(
            Rule::new("main", Event::EachPlayer)
                .action(Action::ForPlayerVariable {
                    player: Value::EventPlayer,
                    variable: "p".into(),
                    start: Value::number(0.0),
                    stop: Value::number(3.0),
                    step: Value::number(1.0),
                })
                .action(Action::ForPlayerVariable {
                    player: Value::call(
                        "valueInArray",
                        [
                            Value::call(
                                "allPlayers",
                                [Value::Enum {
                                    value_type: "Team".into(),
                                    value: "ALL".into(),
                                }],
                            ),
                            Value::number(1.0),
                        ],
                    ),
                    variable: "q".into(),
                    start: Value::number(1.0),
                    stop: Value::number(4.0),
                    step: Value::number(2.0),
                })
                .action(Action::ModifyPlayerVariable {
                    player: Value::EventPlayer,
                    variable: "p".into(),
                    op: ModifyOp::Add,
                    value: Value::number(1.0),
                })
                .action(Action::End)
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("nested for should reconstruct");
        assert!(
            source.contains("for eventPlayer.p in range(0, 3, 1):"),
            "{source}"
        );
        assert!(
            source.contains("for ((getPlayers(Team.ALL))[1]).q in range(1, 4, 2):"),
            "{source}"
        );
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn player_variable_reads_and_writes_on_arbitrary_players() {
        let mut program = Program::new();
        program.player_variable(Variable::with_index("p", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetPlayerVariable {
                player: Value::call(
                    "getClosestPlayer",
                    [
                        Value::call(
                            "vector",
                            [Value::number(0.0), Value::number(0.0), Value::number(0.0)],
                        ),
                        Value::Enum {
                            value_type: "Team".into(),
                            value: "ALL".into(),
                        },
                    ],
                ),
                variable: "p".into(),
                value: Value::PlayerVariable {
                    player: Box::new(Value::call(
                        "valueInArray",
                        [
                            Value::call(
                                "allPlayers",
                                [Value::Enum {
                                    value_type: "Team".into(),
                                    value: "ALL".into(),
                                }],
                            ),
                            Value::number(1.0),
                        ],
                    )),
                    variable: "p".into(),
                },
            }),
        );

        let source = reconstruct(&program).expect("player var access should reconstruct");
        assert!(
            source.contains(
                "(getClosestPlayer(vect(0, 0, 0), Team.ALL)).p = ((getPlayers(Team.ALL))[1]).p"
            ),
            "{source}"
        );
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn self_referential_set_and_modify_forms() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("add", [Value::global_variable("g"), Value::number(2.0)]),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::AppendToArray,
                    value: Value::number(1.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::RemoveFromArrayByValue,
                    value: Value::number(2.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::RemoveFromArrayByIndex,
                    value: Value::number(0.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Min,
                    value: Value::number(9.0),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Max,
                    value: Value::number(-9.0),
                }),
        );

        let source = reconstruct(&program).expect("modify forms should reconstruct");
        assert!(source.contains("g = (g + 2)"), "{source}");
        assert!(source.contains("g.append(1)"), "{source}");
        assert!(source.contains("g.remove(2)"), "{source}");
        assert!(source.contains("del g[0]"), "{source}");
        assert!(source.contains("g min= 9"), "{source}");
        assert!(source.contains("g max= -9"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn indexed_assignment_and_compound_forms() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.player_variable(Variable::with_index("p", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call(
                    "setGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::number(7.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::call("multiply", Vec::<Value>::new()),
                        Value::number(3.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(0.0),
                        Value::call("min", Vec::<Value>::new()),
                        Value::number(5.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(2.0),
                        Value::call("appendToArray", Vec::<Value>::new()),
                        Value::number(8.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(2.0),
                        Value::call("removeFromArray", Vec::<Value>::new()),
                        Value::number(8.0),
                    ],
                ))
                .action(Action::call(
                    "modifyGlobalVariableAtIndex",
                    [
                        Value::global_variable("g"),
                        Value::number(1.0),
                        Value::call("removeFromArrayByIndex", Vec::<Value>::new()),
                        Value::number(0.0),
                    ],
                ))
                .action(Action::call(
                    "setPlayerVariableAtIndex",
                    [
                        Value::PlayerVariable {
                            player: Box::new(Value::EventPlayer),
                            variable: "p".into(),
                        },
                        Value::number(1.0),
                        Value::number(4.0),
                    ],
                )),
        );

        let source = reconstruct(&program).expect("indexed forms should reconstruct");
        assert!(source.contains("g[1] = 7"), "{source}");
        assert!(source.contains("g[1] *= 3"), "{source}");
        assert!(source.contains("g[0] min= 5"), "{source}");
        assert!(source.contains("g[2].append(8)"), "{source}");
        assert!(source.contains("g[2].remove(8)"), "{source}");
        assert!(source.contains("del g[1][0]"), "{source}");
        assert!(source.contains("eventPlayer.p[1] = 4"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn skip_and_skipif_emit_deterministic_labels() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call("skip", [Value::number(1.0)]))
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(1.0),
                })
                .action(Action::call(
                    "skipIf",
                    [
                        Value::call("==", [Value::global_variable("g"), Value::number(2.0)]),
                        Value::number(1.0),
                    ],
                ))
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(3.0),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(5.0),
                }),
        );

        let source = reconstruct(&program).expect("skips should reconstruct");
        assert!(source.contains("goto __op_label_1\n"), "{source}");
        assert!(source.contains("goto __op_label_2\n"), "{source}");
        assert!(source.contains("__op_label_1:"), "{source}");
        assert!(source.contains("__op_label_2:"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn generated_labels_avoid_declared_names() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("__op_label_1", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call("skip", [Value::number(1.0)]))
                .action(Action::SetGlobalVariable {
                    variable: "__op_label_1".into(),
                    value: Value::number(1.0),
                })
                .action(Action::SetGlobalVariable {
                    variable: "__op_label_1".into(),
                    value: Value::number(2.0),
                }),
        );

        let source = reconstruct(&program).expect("labels should dodge declared names");
        assert!(source.contains("goto __op_label_2\n"), "{source}");
        assert!(source.contains("__op_label_2:"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn skip_into_a_nested_block_uses_relative_goto() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call("skip", [Value::number(2.0)]))
                .action(Action::If {
                    condition: Value::Bool(true),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(1.0),
                })
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("skip should reconstruct");
        assert!(source.contains("goto loc+2\n"), "{source}");
        assert!(!source.contains("__op_label"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn terminal_abort_and_loop_forms() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::call("abort", Vec::<Value>::new()))
                .action(Action::call(
                    "abortIf",
                    [Value::call(
                        "==",
                        [Value::global_variable("g"), Value::number(1.0)],
                    )],
                ))
                .action(Action::call("loop", Vec::<Value>::new()))
                .action(Action::call(
                    "loopIf",
                    [Value::call(
                        "==",
                        [Value::global_variable("g"), Value::number(2.0)],
                    )],
                ))
                .action(Action::call(
                    "__abortIfConditionIsTrue__",
                    Vec::<Value>::new(),
                ))
                .action(Action::call("loopIfConditionIsTrue", Vec::<Value>::new()))
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::number(9.0),
                }),
        );

        let source = reconstruct(&program).expect("terminal forms should reconstruct");
        assert!(source.contains("return\n"), "{source}");
        assert!(source.contains("    loop()\n"), "{source}");
        assert!(source.contains("if (g == 1):"), "{source}");
        assert!(source.contains("if (g == 2):"), "{source}");
        assert!(source.contains("if ruleCondition():"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn break_inside_a_while_loop_round_trips() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::While {
                    condition: Value::call("<", [Value::global_variable("g"), Value::number(10.0)]),
                })
                .action(Action::ModifyGlobalVariable {
                    variable: "g".into(),
                    op: ModifyOp::Add,
                    value: Value::number(1.0),
                })
                .action(Action::call("break", Vec::<Value>::new()))
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("break should reconstruct");
        assert!(source.contains("while (g < 10):"), "{source}");
        assert!(source.contains("    break\n"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn member_assignment_stays_refused() {
        let program = one_action_program(Action::AssignMember {
            target: Value::call(
                "memberAccess",
                [Value::global_variable("g"), Value::string("health")],
            ),
            op: Some(ModifyOp::Add),
            value: Value::number(1.0),
        });
        assert_error_code(&program, "unsupported-member-assignment");
    }

    #[test]
    fn syntax_produced_values_round_trip() {
        let array = Value::call(
            "allPlayers",
            [Value::Enum {
                value_type: "Team".into(),
                value: "ALL".into(),
            }],
        );
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("valueInArray", [array.clone(), Value::number(1.0)]),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "sortedArray",
                        [
                            array.clone(),
                            Value::call("currentArrayElement", Vec::<Value>::new()),
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "sortedArray",
                        [
                            array.clone(),
                            Value::call(
                                "getSpeed",
                                [Value::call("currentArrayElement", Vec::<Value>::new())],
                            ),
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "mappedArray",
                        [
                            array.clone(),
                            Value::call(
                                "+",
                                [
                                    Value::call("currentArrayElement", Vec::<Value>::new()),
                                    Value::call("currentArrayIndex", Vec::<Value>::new()),
                                ],
                            ),
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "filteredArray",
                        [
                            array.clone(),
                            Value::call(
                                ">",
                                [
                                    Value::call("currentArrayIndex", Vec::<Value>::new()),
                                    Value::number(0.0),
                                ],
                            ),
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("arrayContains", [array.clone(), Value::EventPlayer]),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("firstOf", [array.clone()]),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("emptyArray", Vec::<Value>::new()),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "slice",
                        [array.clone(), Value::number(1.0), Value::number(3.0)],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "ifThenElse",
                        [
                            Value::call("==", [Value::global_variable("g"), Value::number(1.0)]),
                            Value::number(1.0),
                            Value::number(2.0),
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "roundToInteger",
                        [
                            Value::global_variable("g"),
                            Value::Enum {
                                value_type: "Rounding".into(),
                                value: "NEAREST".into(),
                            },
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "roundToInteger",
                        [
                            Value::global_variable("g"),
                            Value::Enum {
                                value_type: "Rounding".into(),
                                value: "DOWN".into(),
                            },
                        ],
                    ),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "roundToInteger",
                        [
                            Value::global_variable("g"),
                            Value::Enum {
                                value_type: "Rounding".into(),
                                value: "UP".into(),
                            },
                        ],
                    ),
                }),
        );

        let source = reconstruct(&program).expect("syntax values should reconstruct");
        assert!(source.contains("(getPlayers(Team.ALL))[1]"), "{source}");
        assert!(source.contains("sorted(getPlayers(Team.ALL))"), "{source}");
        assert!(
            source.contains("sorted(getPlayers(Team.ALL), lambda __elem"),
            "{source}"
        );
        assert!(source.contains(".map(lambda __elem"), "{source}");
        assert!(source.contains(".filter(lambda __elem"), "{source}");
        assert!(
            source.contains("(eventPlayer in getPlayers(Team.ALL))"),
            "{source}"
        );
        assert!(source.contains("(getPlayers(Team.ALL))[0]"), "{source}");
        assert!(source.contains("g = []"), "{source}");
        assert!(
            source.contains("getPlayers(Team.ALL)).slice(1, 3)"),
            "{source}"
        );
        assert!(source.contains("(1 if (g == 1) else 2)"), "{source}");
        assert!(source.contains("round(g)"), "{source}");
        assert!(source.contains("floor(g)"), "{source}");
        assert!(source.contains("ceil(g)"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn mapped_array_lambda_uses_element_and_index_binders() {
        let program = one_value_program(Value::call(
            "mappedArray",
            [
                Value::global_variable("g"),
                Value::call(
                    "add",
                    [
                        Value::call("currentArrayElement", Vec::<Value>::new()),
                        Value::call("currentArrayIndex", Vec::<Value>::new()),
                    ],
                ),
            ],
        ));

        let source = reconstruct(&program).expect("lambda should reconstruct");
        assert!(
            source.contains("g = (g).map(lambda __elem_1, __idx_2: (__elem_1 + __idx_2))"),
            "{source}"
        );
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn localized_string_literals_round_trip() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("string", [Value::LocalizedString("hello".into())]),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "string",
                        [Value::LocalizedString("goodbye".into()), Value::EventPlayer],
                    ),
                }),
        );

        let source = reconstruct(&program).expect("localized strings should reconstruct");
        assert!(source.contains("l\"Hello\""), "{source}");
        assert!(
            source.contains("l\"Goodbye\".format(eventPlayer)"),
            "{source}"
        );
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn negative_numbers_emit_unary_negation() {
        let program = one_value_program(Value::number(-3.5));
        let source = reconstruct(&program).expect("negative number should reconstruct");
        assert!(source.contains("g = -3.5"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn member_access_and_vector_components() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call("__xComponentOf__", [Value::global_variable("g")]),
                })
                .action(Action::SetGlobalVariable {
                    variable: "g".into(),
                    value: Value::call(
                        "memberAccess",
                        [Value::global_variable("g"), Value::string("health")],
                    ),
                }),
        );

        let source = reconstruct(&program).expect("member access should reconstruct");
        assert!(source.contains("(g).x"), "{source}");
        assert!(source.contains("(g).health"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn nested_lambdas_keep_distinct_binders() {
        let inner = Value::call(
            "filteredArray",
            [
                Value::call("currentArrayElement", Vec::<Value>::new()),
                Value::call(
                    "==",
                    [
                        Value::call("currentArrayIndex", Vec::<Value>::new()),
                        Value::number(0.0),
                    ],
                ),
            ],
        );
        let program = one_value_program(Value::call(
            "mappedArray",
            [Value::global_variable("g"), inner],
        ));

        let source = reconstruct(&program).expect("nested lambdas should reconstruct");
        assert!(source.contains(".map(lambda __elem_"), "{source}");
        assert!(source.contains(".filter(lambda __elem_"), "{source}");
        assert_behaviorally_equivalent(&program, &source);
    }

    #[test]
    fn refusals_stay_diagnostic() {
        assert_error_code(
            &one_value_program(Value::number(f64::NAN)),
            "unsupported-non-finite-number",
        );
        assert_error_code(
            &one_value_program(Value::number(f64::INFINITY)),
            "unsupported-non-finite-number",
        );

        assert_error_code(
            &one_value_program(Value::Enum {
                value_type: "NotARealDomain".into(),
                value: "X".into(),
            }),
            "unsupported-enum-domain",
        );

        assert_error_code(
            &one_value_program(Value::call("definitelyNotAValue", [Value::number(1.0)])),
            "unsupported-value-call",
        );

        assert_error_code(
            &one_value_program(Value::call(
                "valueInArray",
                [Value::global_variable("g"), Value::number(0.0)],
            )),
            "unsupported-value-call",
        );

        assert_error_code(
            &one_value_program(Value::Subroutine("sub".into())),
            "unsupported-subroutine-value",
        );

        assert_error_code(
            &one_value_program(Value::call("currentArrayElement", Vec::<Value>::new())),
            "unsupported-value-call",
        );
        assert_error_code(
            &one_value_program(Value::call("currentArrayIndex", Vec::<Value>::new())),
            "unsupported-value-call",
        );

        assert_error_code(
            &one_value_program(Value::call(
                "memberAccess",
                [Value::global_variable("g"), Value::string("not an ident")],
            )),
            "unsupported-value-call",
        );
    }

    fn assert_behaviorally_equivalent(program: &Program, source: &str) {
        let reparsed = match compile_to_program_checked(source) {
            Ok(program) => program,
            Err(message) => panic!("recompiled failed:\n{source}\n{message}"),
        };
        assert!(
            workshop_rs::compare::equivalent(program, &reparsed),
            "reconstructed source is not behaviorally equivalent:\n{source}\ndifference: {:?}",
            workshop_rs::compare::difference(program, &reparsed),
        );
    }

    fn compile_to_program_checked(source: &str) -> Result<Program, String> {
        let artifact = Compiler::new()
            .map_err(|e| e.to_string())?
            .compile_source_artifact(source, "reconstructed.opy", Path::new("."))
            .map_err(|e| format!("{e:?}"))?;
        let catalog = Catalog::builtin().map_err(|e| e.to_string())?;
        workshop_rs::parser::parse(&artifact.emitted, &catalog, &Locale::new("en-US"))
            .map_err(|e| format!("{e:?}"))
    }

    #[test]
    fn each_player_filters_emit_team_and_slot_annotations() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(
                "filtered",
                Event::EachPlayerWithFilters {
                    team: EventTeam::Team1,
                    target: EventTarget::Slot(0),
                },
            )
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("filtered event reconstructs");
        assert!(
            source.contains("    @Event eachPlayer\n    @Team 1\n    @Slot 0\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "each-player-filters.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn each_player_hero_filter_emits_the_source_hero_spelling() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(
                "hero",
                Event::EachPlayerWithFilters {
                    team: EventTeam::All,
                    target: EventTarget::Hero("SOLDIER_76".into()),
                },
            )
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("hero filter reconstructs");
        assert!(
            source.contains("    @Event eachPlayer\n    @Hero soldier\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "each-player-hero.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn player_event_emits_the_catalog_id_and_filters() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(
                "died",
                Event::Player {
                    kind: PlayerEventKind::Died,
                    team: EventTeam::Team2,
                    target: EventTarget::Hero("CASSIDY".into()),
                },
            )
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("player event reconstructs");
        assert!(
            source.contains("    @Event playerDied\n    @Team 2\n    @Hero cassidy\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "player-event.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn unspellable_hero_filter_is_refused() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new(
                "hero",
                Event::EachPlayerWithFilters {
                    team: EventTeam::All,
                    target: EventTarget::Hero("DOCTRINE".into()),
                },
            )
            .action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            }),
        );

        let error = reconstruct(&program).expect_err("a hero with no OPY spelling is refused");
        assert_eq!(error.issues[0].code, "unsupported-enum-member");
    }

    #[test]
    fn disabled_rule_emits_the_disabled_annotation() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        let mut rule = Rule::new("off", Event::Global).action(Action::SetGlobalVariable {
            variable: "score".into(),
            value: Value::number(1.0),
        });
        rule.disabled = true;
        program.rule(rule);

        let source = reconstruct(&program).expect("disabled rule reconstructs");
        assert!(
            source.contains("rule \"off\":\n    @Event global\n    @Disabled\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "disabled-rule.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn disabled_condition_emits_a_commented_line() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .condition(Value::call(
                    "==",
                    [Value::global_variable("score"), Value::number(0.0)],
                ))
                .condition(Condition::disabled(Value::call(
                    "<",
                    [Value::global_variable("score"), Value::number(1.0)],
                )))
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(2.0),
                }),
        );

        let source = reconstruct(&program).expect("disabled condition reconstructs");
        assert!(source.contains("    @Condition (score == 0)\n"), "{source}");
        assert!(
            source.contains("    # @Condition (score < 1)\n"),
            "{source}"
        );

        let reparsed = compile_to_program(&source, "disabled-condition.opy");
        assert!(
            workshop_rs::compare::equivalent(&program, &reparsed),
            "{source}\n{:?}",
            workshop_rs::compare::difference(&program, &reparsed)
        );
    }

    #[test]
    fn disabled_action_emits_a_commented_line() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(Rule::new("main", Event::Global).action(Action::disabled(
            Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(1.0),
            },
        )));

        let source = reconstruct(&program).expect("disabled action reconstructs");
        assert!(source.contains("    # score = 1\n"), "{source}");

        let reparsed = compile_to_program(&source, "disabled-action.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn disabled_control_flow_emits_a_commented_block() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::disabled(Action::If {
                    condition: Value::call(
                        "==",
                        [Value::global_variable("score"), Value::number(0.0)],
                    ),
                }))
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                })
                .action(Action::Else)
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(2.0),
                })
                .action(Action::End),
        );

        let source = reconstruct(&program).expect("disabled control flow reconstructs");
        assert!(source.contains("    # if (score == 0):\n"), "{source}");
        assert!(source.contains("    # score = 1\n"), "{source}");
        assert!(source.contains("    # else:\n"), "{source}");

        let reparsed = compile_to_program(&source, "disabled-if.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn subroutine_rule_without_actions_emits_pass() {
        let mut program = Program::new();
        program.subroutine(Subroutine::with_index("reset", 0));
        program.rule(Rule::new(
            "Subroutine reset",
            Event::Subroutine("reset".into()),
        ));

        let source = reconstruct(&program).expect("empty subroutine rule reconstructs");
        assert!(source.contains("def reset():\n    pass\n"), "{source}");

        let reparsed = compile_to_program(&source, "empty-subroutine.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn subroutine_rules_reconstruct_in_def_form_in_any_order() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.subroutine(Subroutine::with_index("first", 0));
        program.subroutine(Subroutine::with_index("second", 1));
        program.rule(
            Rule::new("main", Event::Global).action(Action::CallSubroutine {
                subroutine: "first".into(),
            }),
        );
        program.rule(
            Rule::new("my second body", Event::Subroutine("second".into())).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(2.0),
                },
            ),
        );
        program.rule(
            Rule::new("my first body", Event::Subroutine("first".into())).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("out-of-order defs reconstruct");
        assert!(source.contains("def second():"), "{source}");
        assert!(source.contains("def first():"), "{source}");

        let reparsed = compile_to_program(&source, "def-order.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn subroutine_rule_with_conditions_stays_refused() {
        let mut program = Program::new();
        program.subroutine(Subroutine::with_index("reset", 0));
        program.rule(
            Rule::new("Subroutine reset", Event::Subroutine("reset".into()))
                .condition(Value::number(1.0))
                .action(Action::CallSubroutine {
                    subroutine: "reset".into(),
                }),
        );

        let error = reconstruct(&program).expect_err("subroutine conditions stay refused");
        assert_eq!(error.issues[0].code, "unsupported-rule-order");
    }

    #[test]
    fn duplicate_subroutine_rules_stay_refused() {
        let mut program = Program::new();
        program.subroutine(Subroutine::with_index("reset", 0));
        for name in ["first def", "second def"] {
            program.rule(Rule::new(name, Event::Subroutine("reset".into())));
        }

        let error = reconstruct(&program).expect_err("one subroutine, one def");
        assert_eq!(error.issues[0].code, "unsupported-rule-order");
    }

    #[test]
    fn initializer_named_rule_after_an_ongoing_rule_keeps_its_position() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "score".into(),
                value: Value::number(2.0),
            }),
        );
        program.rule(
            Rule::new("Initialize global variables", Event::Global).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("demoted initializer reconstructs");
        let main = source.find("rule \"main\":").expect("main rule emitted");
        let init = source
            .find("rule \"Initialize global variables\":")
            .expect("initializer-named rule emitted");
        assert!(main < init, "{source}");
        assert!(!source.contains("globalvar score ="), "{source}");

        let reparsed = compile_to_program(&source, "init-position.opy");
        assert!(
            workshop_rs::compare::equivalent(&program, &reparsed),
            "{:?}",
            workshop_rs::compare::difference(&program, &reparsed)
        );
    }

    #[test]
    fn initializer_named_rule_with_conditions_stays_an_ordinary_rule() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("Initialize global variables", Event::Global)
                .condition(Value::call(
                    "==",
                    [Value::global_variable("score"), Value::number(0.0)],
                ))
                .action(Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                }),
        );

        let source = reconstruct(&program).expect("conditioned initializer-name reconstructs");
        assert!(
            source.contains("rule \"Initialize global variables\":"),
            "{source}"
        );
        assert!(source.contains("    @Condition (score == 0)\n"), "{source}");

        let reparsed = compile_to_program(&source, "init-condition.opy");
        assert!(
            workshop_rs::compare::equivalent(&program, &reparsed),
            "{source}\n{:?}",
            workshop_rs::compare::difference(&program, &reparsed)
        );
    }

    #[test]
    fn initializer_named_rule_with_out_of_order_sets_stays_an_ordinary_rule() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("first", 0));
        program.global_variable(Variable::with_index("second", 1));
        program.rule(
            Rule::new("Initialize global variables", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "second".into(),
                    value: Value::number(2.0),
                })
                .action(Action::SetGlobalVariable {
                    variable: "first".into(),
                    value: Value::number(1.0),
                }),
        );

        let source = reconstruct(&program).expect("out-of-order initializer-name reconstructs");
        assert!(
            source.contains("rule \"Initialize global variables\":"),
            "{source}"
        );
        assert!(!source.contains("globalvar second ="), "{source}");

        let reparsed = compile_to_program(&source, "init-order.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn canonical_initializer_still_folds_into_declarations() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("score", 0));
        program.rule(
            Rule::new("Initialize global variables", Event::Global).action(
                Action::SetGlobalVariable {
                    variable: "score".into(),
                    value: Value::number(1.0),
                },
            ),
        );

        let source = reconstruct(&program).expect("canonical initializer reconstructs");
        assert!(source.contains("globalvar score = 1\n"), "{source}");
        assert!(!source.contains("rule \""), "{source}");

        let reparsed = compile_to_program(&source, "canonical-init.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn reconstruction_begins_with_optimize_strict() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::number(1.0),
            }),
        );

        let source = reconstruct(&program).expect("canonical Program should reconstruct");
        assert!(source.starts_with("#!optimizeStrict\n"), "{source}");
    }

    #[test]
    fn optimize_strict_preserves_add_zero() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 0));
        program.global_variable(Variable::with_index("h", 1));
        program.rule(
            Rule::new("main", Event::Global).action(Action::SetGlobalVariable {
                variable: "g".into(),
                value: Value::call("+", [Value::global_variable("h"), Value::number(0.0)]),
            }),
        );

        let source = reconstruct(&program).expect("Add(x, 0) should reconstruct");
        assert!(source.starts_with("#!optimizeStrict"), "{source}");
        assert!(source.contains("h + 0"), "{source}");

        let reparsed = compile_to_program(&source, "add-zero.opy");
        assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
    }

    #[test]
    fn out_of_order_global_indices_reconstruct_sorted_with_explicit_indices() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("zed", 3));
        program.global_variable(Variable::with_index("alpha", 0));
        program.global_variable(Variable::with_index("mid", 2));
        program.rule(
            Rule::new("main", Event::Global)
                .action(Action::SetGlobalVariable {
                    variable: "zed".into(),
                    value: Value::number(1.0),
                })
                .action(Action::SetGlobalVariable {
                    variable: "alpha".into(),
                    value: Value::global_variable("mid"),
                }),
        );

        let source = reconstruct(&program).expect("out-of-order indices should reconstruct");
        let alpha = source
            .find("globalvar alpha 0")
            .expect("alpha declares its index");
        let mid = source
            .find("globalvar mid 2")
            .expect("mid declares its index");
        let zed = source
            .find("globalvar zed 3")
            .expect("zed declares its index");
        assert!(alpha < mid && mid < zed, "{source}");

        let reparsed = compile_to_program(&source, "out-of-order.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn out_of_order_player_indices_reconstruct_sorted_with_explicit_indices() {
        let mut program = Program::new();
        program.player_variable(Variable::with_index("zed", 3));
        program.player_variable(Variable::with_index("alpha", 0));
        program.rule(
            Rule::new("main", Event::EachPlayer).action(Action::SetPlayerVariable {
                player: Value::EventPlayer,
                variable: "alpha".into(),
                value: Value::player_variable(Value::EventPlayer, "zed"),
            }),
        );

        let source = reconstruct(&program).expect("out-of-order player indices reconstruct");
        let alpha = source
            .find("playervar alpha 0")
            .expect("alpha declares its index");
        let zed = source
            .find("playervar zed 3")
            .expect("zed declares its index");
        assert!(alpha < zed, "{source}");

        let reparsed = compile_to_program(&source, "player-out-of-order.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));
    }

    #[test]
    fn names_outside_the_opy_surface_reconstruct_with_recorded_mangling() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("for", 0));
        program.global_variable(Variable::with_index("1st place", 1));
        program.player_variable(Variable::with_index("x", 0));
        program.subroutine(Subroutine::with_index("sorted", 0));
        program.rule(
            Rule::new("Subroutine sorted", Event::Subroutine("sorted".into())).action(
                Action::SetGlobalVariable {
                    variable: "for".into(),
                    value: Value::global_variable("1st place"),
                },
            ),
        );
        program.rule(
            Rule::new("main", Event::EachPlayer)
                .action(Action::SetPlayerVariable {
                    player: Value::EventPlayer,
                    variable: "x".into(),
                    value: Value::number(0.0),
                })
                .action(Action::CallSubroutine {
                    subroutine: "sorted".into(),
                }),
        );

        let source = reconstruct(&program).expect("mangled names should reconstruct");
        assert!(source.contains("globalvar for_opy1 0"), "{source}");
        assert!(source.contains("globalvar v_1st_x20_place 1"), "{source}");
        assert!(source.contains("playervar x_opy1 0"), "{source}");
        assert!(source.contains("subroutine sorted_opy1\n"), "{source}");
        assert!(source.contains("def sorted_opy1():"), "{source}");
        assert!(source.contains("for_opy1 = v_1st_x20_place"), "{source}");
        assert!(source.contains("eventPlayer.x_opy1 = 0"), "{source}");
        assert!(source.contains("sorted_opy1()"), "{source}");

        let reparsed = compile_to_program(&source, "mangled.opy");
        assert!(workshop_rs::compare::equivalent(&program, &reparsed));

        let again = reconstruct(&program).expect("reconstruction is deterministic");
        assert_eq!(source, again);
    }

    #[test]
    fn duplicate_table_names_still_refuse() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("dup", 0));
        program.global_variable(Variable::with_index("dup", 1));

        let error = reconstruct(&program).expect_err("duplicate names refuse");
        assert_eq!(error.issues[0].code, "unsupported-duplicate-name");
    }

    #[test]
    fn declaration_indices_outside_the_slot_range_refuse() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("g", 200));

        let error = reconstruct(&program).expect_err("out-of-range index refuses");
        assert_eq!(error.issues[0].code, "unsupported-index");
    }

    #[test]
    fn colliding_declaration_indices_refuse() {
        let mut program = Program::new();
        program.global_variable(Variable::with_index("a", 0));
        program.global_variable(Variable::with_index("b", 0));

        let error = reconstruct(&program).expect_err("colliding indices refuse");
        assert_eq!(error.issues[0].code, "unsupported-index");
    }
}
