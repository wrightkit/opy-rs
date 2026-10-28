use serde::Serialize;
use workshop_rs::catalog::{Catalog, Kind};
use workshop_rs::settings::{self, PathPart, SettingValueDomain};

use crate::lower::special_forms::SPECIAL_FUNCTIONS;
use crate::manifest::{Function, FunctionKind, Manifest, Param, ParamDefault, ReceiverCategory};

pub const DEFAULT_LIMIT: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct LookupQuery {
    /// The name or guess to resolve: an OPY spelling, a canonical Workshop
    /// id, a Workshop display name, or a settings path prefix. Empty text
    /// applies no text constraint and lists the in-scope entries instead of
    /// scoring them.
    pub text: String,
    pub scope: LookupScope,
    pub within: Option<LookupWithin>,
    /// The display-name locale to read; only `en-US` is supported.
    /// Localized display names remain `workshop-rs`-owned and are answered
    /// as `LookupOutcome::Unsupported`, not silently returned in English.
    pub locale: Option<String>,
    /// The maximum number of returned hits; `0` selects `DEFAULT_LIMIT`.
    pub limit: usize,
}

impl LookupQuery {
    pub fn new(text: impl Into<String>) -> LookupQuery {
        LookupQuery {
            text: text.into(),
            scope: LookupScope::ALL,
            within: None,
            locale: None,
            limit: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupScope {
    pub functions: bool,
    pub enums: bool,
    pub settings: bool,
    pub events: bool,
}

impl LookupScope {
    pub const ALL: LookupScope = LookupScope {
        functions: true,
        enums: true,
        settings: true,
        events: true,
    };
    pub const FUNCTIONS: LookupScope = LookupScope {
        functions: true,
        enums: false,
        settings: false,
        events: false,
    };
    pub const ENUMS: LookupScope = LookupScope {
        functions: false,
        enums: true,
        settings: false,
        events: false,
    };
    pub const SETTINGS: LookupScope = LookupScope {
        functions: false,
        enums: false,
        settings: true,
        events: false,
    };
    pub const EVENTS: LookupScope = LookupScope {
        functions: false,
        enums: false,
        settings: false,
        events: true,
    };

    fn any(self) -> bool {
        self.functions || self.enums || self.settings || self.events
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum LookupWithin {
    Callable(String),
    Enum(String),
    Settings(String),
    SettingEnum(String),
}

impl LookupWithin {
    fn scope(&self) -> LookupScope {
        match self {
            LookupWithin::Callable(_) => LookupScope::FUNCTIONS,
            LookupWithin::Enum(_) | LookupWithin::SettingEnum(_) => LookupScope::ENUMS,
            LookupWithin::Settings(_) => LookupScope::SETTINGS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum LookupOutcome {
    Matched {
        scope: LookupScope,
        limit: usize,
        results: Vec<LookupHit>,
    },
    /// The `within` selector names no scope the vocabulary knows; the
    /// request carried no matches because the scope itself does not exist.
    UnknownWithin {
        within: LookupWithin,
    },
    Unsupported {
        scope: LookupScope,
        reason: String,
    },
}

impl LookupOutcome {
    pub fn results(&self) -> &[LookupHit] {
        match self {
            LookupOutcome::Matched { results, .. } => results,
            LookupOutcome::UnknownWithin { .. } | LookupOutcome::Unsupported { .. } => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchKind {
    OpySpelling,
    CatalogId,
    DisplayName,
    Alias,
    Path,
    /// The opaque handle type the callable produces (e.g. `HealthPoolId`):
    /// a type-name query reaches the callables whose catalog entry returns
    /// that type, not only the calls that consume it.
    ProducedType,
    Near,
}

impl MatchKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchKind::OpySpelling => "opySpelling",
            MatchKind::CatalogId => "catalogId",
            MatchKind::DisplayName => "displayName",
            MatchKind::Alias => "alias",
            MatchKind::Path => "path",
            MatchKind::ProducedType => "producedType",
            MatchKind::Near => "near",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "hit")]
pub enum LookupHit {
    Function {
        spelling: String,
        function_kind: LookupFunctionKind,
        /// The declared receiver category of a member call — how it is
        /// invoked (`eventPlayer.setMoveSpeed(…)` takes a `Player`
        /// receiver). `None` for standalone calls.
        #[serde(skip_serializing_if = "Option::is_none")]
        receiver: Option<ReceiverCategory>,
        /// Whether the argument count is unbounded (`.format` placeholders).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        unbounded: bool,
        params: Vec<LookupParam>,
        catalog_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        matched_on: MatchKind,
    },
    EnumDomain {
        domain: String,
        /// The members the domain accepts, or `None` when the domain is
        /// manifest-only/contextual and carries no standalone member list.
        members: Option<Vec<EnumMemberEntry>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        matched_on: MatchKind,
    },
    EnumMember {
        spelling: String,
        domain: String,
        member: String,
        display_name: Option<String>,
        aliases: Vec<String>,
        matched_on: MatchKind,
    },
    Setting {
        /// The effective `settings` path; per-team/per-hero slots stay
        /// templated as `heroes.<team>.<hero>.<key>`.
        path: String,
        display_name: Option<String>,
        value: SettingValueForm,
        matched_on: MatchKind,
    },
    Parameter {
        callable: String,
        param: LookupParam,
        matched_on: MatchKind,
    },
    SettingPath {
        path: String,
        matched_on: MatchKind,
    },
    Event {
        spelling: String,
        accepts_filters: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        matched_on: MatchKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LookupFunctionKind {
    Action,
    Value,
    MemberAction,
    MemberValue,
}

impl LookupFunctionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LookupFunctionKind::Action => "action",
            LookupFunctionKind::Value => "value",
            LookupFunctionKind::MemberAction => "memberAction",
            LookupFunctionKind::MemberValue => "memberValue",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupParam {
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternate_names: Vec<String>,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<EnumMemberEntry>>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub param_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumMemberEntry {
    pub spelling: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingValueForm {
    pub kind: SettingValueKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingValueKind {
    Presence,
    Boolean,
    Number,
    Percent,
    String,
    Enum,
    MapList,
    HeroList,
}

impl SettingValueKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SettingValueKind::Presence => "presence",
            SettingValueKind::Boolean => "boolean",
            SettingValueKind::Number => "number",
            SettingValueKind::Percent => "percent",
            SettingValueKind::String => "string",
            SettingValueKind::Enum => "enum",
            SettingValueKind::MapList => "mapList",
            SettingValueKind::HeroList => "heroList",
        }
    }
}

/// Resolve a `LookupQuery` against the bundled manifest and catalog.
///
/// The answer is `LookupOutcome::Matched` (possibly with zero hits),
/// `LookupOutcome::UnknownWithin` when the named scope does not exist, or
/// `LookupOutcome::Unsupported` with a reason; vocabulary lookup never
/// fails — a name that resolves to nothing is simply absent from `results`.
/// An empty `text` applies no text constraint and lists the in-scope
/// entries in the vocabulary's stable order.
pub fn lookup(query: &LookupQuery) -> LookupOutcome {
    let limit = if query.limit == 0 {
        DEFAULT_LIMIT
    } else {
        query.limit
    };
    let scope = query.scope;
    let unsupported = |reason: &str| LookupOutcome::Unsupported {
        scope,
        reason: reason.to_string(),
    };
    if let Some(locale) = &query.locale
        && !locale.eq_ignore_ascii_case("en-US")
    {
        return unsupported(
            "localized display names are owned by workshop-rs; opy-rs \
                 resolves display names in the 'en-US' locale only",
        );
    }
    let Ok(catalog) = Catalog::builtin_ref() else {
        return unsupported("vocabulary data (manifest or catalog) is not bundled");
    };
    let Ok(manifest) = Manifest::builtin() else {
        return unsupported("vocabulary data (manifest or catalog) is not bundled");
    };
    let index = Index::new(manifest, catalog);
    if let Some(within) = &query.within {
        let Some(children) = index.children(within) else {
            return LookupOutcome::UnknownWithin {
                within: within.clone(),
            };
        };
        let results = index.rank(children.iter(), query, limit);
        return LookupOutcome::Matched {
            scope: within.scope(),
            limit,
            results,
        };
    }
    if !query.scope.any() {
        return unsupported("the query scope selects no namespace");
    }
    let results = index.rank(
        index
            .candidates
            .iter()
            .filter(|candidate| index.in_scope(candidate, scope)),
        query,
        limit,
    );
    LookupOutcome::Matched {
        scope,
        limit,
        results,
    }
}

pub fn lookup_str(text: &str) -> LookupOutcome {
    lookup(&LookupQuery::new(text))
}

const SYNTAX_SPELLINGS: &[(&str, &str)] = &[
    ("add", "+"),
    ("subtract", "-"),
    ("multiply", "*"),
    ("divide", "/"),
    ("modulo", "%"),
    ("raiseToPower", "**"),
    ("==", "=="),
    ("!=", "!="),
    ("<", "<"),
    ("<=", "<="),
    (">", ">"),
    (">=", ">="),
    ("and", "and"),
    ("or", "or"),
    ("not", "not"),
    ("-", "-"),
    ("arrayContains", "in"),
    ("appendToArray", "append"),
    ("removeFromArray", "exclude"),
    ("removeFromArrayByIndex", "del"),
    ("removeFromArrayByValue", "remove"),
    ("mappedArray", "map"),
    ("sortedArray", "sorted"),
    ("isTrueForAll", "all"),
    ("isTrueForAny", "any"),
    ("__xComponentOf__", "x"),
    ("__yComponentOf__", "y"),
    ("__zComponentOf__", "z"),
    ("abort", "return"),
    ("skip", "goto"),
    ("break", "break"),
    ("continue", "continue"),
    ("if", "if"),
    ("elseIf", "elif"),
    ("else", "else"),
    ("while", "while"),
    ("forGlobalVariable", "for"),
    ("forPlayerVariable", "for"),
    ("__forPlayerVariable__", "for"),
    ("eventPlayer", "eventPlayer"),
    ("attacker", "attacker"),
    ("victim", "victim"),
    ("localPlayer", "localPlayer"),
    ("hostPlayer", "hostPlayer"),
    ("true", "true"),
    ("false", "false"),
    ("null", "null"),
    ("up", "Vector.UP"),
    ("down", "Vector.DOWN"),
    ("left", "Vector.LEFT"),
    ("right", "Vector.RIGHT"),
    ("forward", "Vector.FORWARD"),
    ("backward", "Vector.BACKWARD"),
    ("chasePlayerVariableAtRate", "chaseAtRate"),
    ("chasePlayerVariableOverTime", "chaseOverTime"),
    ("stopChasingPlayerVariable", "stopChasingVariable"),
];

pub fn call_spelling(canonical_id: &str) -> Option<&'static str> {
    let manifest = Manifest::builtin().ok()?;
    let catalog = Catalog::builtin_ref().ok()?;
    let is_canonical = |id: &str| {
        catalog.entry(Kind::Value, id).is_some() || catalog.entry(Kind::Action, id).is_some()
    };
    manifest
        .functions()
        .iter()
        .filter(|function| match function.catalog_id.as_deref() {
            Some(catalog_id) => catalog_id == canonical_id,
            None => function.id == canonical_id && is_canonical(canonical_id),
        })
        .min_by_key(|function| function.id.as_str() != canonical_id)
        .map(|function| function.id.as_str())
        .or_else(|| {
            SYNTAX_SPELLINGS
                .iter()
                .find_map(|(id, spelling)| (*id == canonical_id).then_some(*spelling))
        })
}

pub fn enum_member_spelling<'a>(domain: &'a str, member: &'a str) -> Option<&'a str> {
    let catalog = Catalog::builtin_ref().ok()?;
    let enum_domain = catalog.enum_domain(domain)?;
    if !enum_domain
        .members
        .iter()
        .any(|entry| entry.member == member)
    {
        return None;
    }
    crate::enums::spelling_of_member(domain, member).map(|(_, member)| member)
}

struct ScoredHit<'c> {
    score: u32,
    stamp: (MatchKind, bool),
    candidate: &'c Candidate,
}

fn sort_key(hit: &LookupHit) -> (u8, String) {
    match hit {
        LookupHit::Function { spelling, .. } => (0, spelling.clone()),
        LookupHit::EnumMember { spelling, .. } => (1, spelling.clone()),
        LookupHit::EnumDomain { domain, .. } => (2, domain.clone()),
        LookupHit::Setting { path, .. } => (3, path.clone()),
        LookupHit::Parameter {
            callable, param, ..
        } => (4, format!("{callable}.{}", param.name)),
        LookupHit::SettingPath { path, .. } => (5, path.clone()),
        LookupHit::Event { spelling, .. } => (6, spelling.clone()),
    }
}

impl LookupHit {
    fn listed(mut self) -> LookupHit {
        let kind = match &self {
            LookupHit::Setting { .. } | LookupHit::SettingPath { .. } => MatchKind::Path,
            _ => MatchKind::OpySpelling,
        };
        *self.matched_on_mut() = kind;
        self
    }

    fn with_match_kind(mut self, kind: MatchKind, score: u32, structural: bool) -> LookupHit {
        *self.matched_on_mut() = if score == 0 || structural {
            kind
        } else {
            MatchKind::Near
        };
        self
    }

    fn matched_on_mut(&mut self) -> &mut MatchKind {
        match self {
            LookupHit::Function { matched_on, .. }
            | LookupHit::EnumDomain { matched_on, .. }
            | LookupHit::EnumMember { matched_on, .. }
            | LookupHit::Setting { matched_on, .. }
            | LookupHit::Parameter { matched_on, .. }
            | LookupHit::SettingPath { matched_on, .. }
            | LookupHit::Event { matched_on, .. } => matched_on,
        }
    }
}

#[derive(Clone)]
struct Candidate {
    forms: Vec<(String, MatchKind)>,
    hit: LookupHit,
}

struct Index<'a> {
    manifest: &'a Manifest,
    catalog: &'a Catalog,
    candidates: Vec<Candidate>,
}

impl<'a> Index<'a> {
    fn new(manifest: &'a Manifest, catalog: &'a Catalog) -> Index<'a> {
        let mut index = Index {
            manifest,
            catalog,
            candidates: Vec::new(),
        };
        index.index_functions();
        index.index_enums();
        index.index_settings();
        index.index_events();
        index
    }

    fn rank<'c>(
        &self,
        candidates: impl Iterator<Item = &'c Candidate>,
        query: &LookupQuery,
        limit: usize,
    ) -> Vec<LookupHit> {
        if query.text.trim().is_empty() {
            return candidates
                .take(limit)
                .map(|candidate| candidate.hit.clone().listed())
                .collect();
        }
        let (path_segments, folded) = crate::matcher::fold_path_query(&query.text);
        let trimmed = query.text.trim();
        let hits: Vec<ScoredHit<'c>> = candidates
            .filter_map(|candidate| {
                candidate
                    .forms
                    .iter()
                    .filter_map(|(form, kind)| {
                        let (scored, structural) = if *kind == MatchKind::Path {
                            match crate::matcher::path_score_folded(&path_segments, form) {
                                Some(s) => (Some(s), true),
                                None => (crate::matcher::score_folded(&folded, form), false),
                            }
                        } else {
                            (
                                crate::matcher::score_folded(&folded, form),
                                *kind == MatchKind::ProducedType,
                            )
                        };
                        scored.map(|s| (s, form.eq_ignore_ascii_case(trimmed), *kind, structural))
                    })
                    .min_by_key(|(s, exact, _, _)| (*s, !*exact))
                    .map(|(s, _, kind, structural)| ScoredHit {
                        score: s,
                        stamp: (kind, structural),
                        candidate,
                    })
            })
            .collect();
        let mut ranked: Vec<((u8, String), ScoredHit<'c>)> = hits
            .into_iter()
            .map(|hit| (sort_key(&hit.candidate.hit), hit))
            .collect();
        ranked
            .sort_by(|(key_a, a), (key_b, b)| a.score.cmp(&b.score).then_with(|| key_a.cmp(key_b)));
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, scored)| {
                let (kind, structural) = scored.stamp;
                scored
                    .candidate
                    .hit
                    .clone()
                    .with_match_kind(kind, scored.score, structural)
            })
            .collect()
    }

    fn in_scope(&self, candidate: &Candidate, scope: LookupScope) -> bool {
        match &candidate.hit {
            LookupHit::Function { .. } | LookupHit::Parameter { .. } => scope.functions,
            LookupHit::EnumDomain { .. } | LookupHit::EnumMember { .. } => scope.enums,
            LookupHit::Setting { .. } | LookupHit::SettingPath { .. } => scope.settings,
            LookupHit::Event { .. } => scope.events,
        }
    }

    fn children(&self, within: &LookupWithin) -> Option<Vec<Candidate>> {
        match within {
            LookupWithin::Callable(name) => self.callable_children(name),
            LookupWithin::Enum(domain) => self.enum_children(domain),
            LookupWithin::Settings(prefix) => self.settings_children(prefix),
            LookupWithin::SettingEnum(domain) => self.setting_enum_children(domain),
        }
    }

    fn callable_children(&self, name: &str) -> Option<Vec<Candidate>> {
        self.candidates
            .iter()
            .find_map(|candidate| match &candidate.hit {
                LookupHit::Function {
                    spelling, params, ..
                } if spelling == name => Some(
                    params
                        .iter()
                        .map(|param| {
                            let mut forms = vec![(param.name.clone(), MatchKind::OpySpelling)];
                            forms.extend(
                                param
                                    .alternate_names
                                    .iter()
                                    .map(|name| (name.clone(), MatchKind::Alias)),
                            );
                            Candidate {
                                forms,
                                hit: LookupHit::Parameter {
                                    callable: spelling.clone(),
                                    param: param.clone(),
                                    matched_on: MatchKind::OpySpelling,
                                },
                            }
                        })
                        .collect(),
                ),
                _ => None,
            })
    }

    fn enum_children(&self, domain: &str) -> Option<Vec<Candidate>> {
        let known = self.candidates.iter().any(|candidate| {
            matches!(&candidate.hit, LookupHit::EnumDomain { domain: d, .. } if d == domain)
        });
        if !known {
            return None;
        }
        Some(
            crate::enums::domain_members(domain, self.catalog)
                .map(|members| {
                    members
                        .iter()
                        .map(|member| self.member_candidate(domain, member))
                        .collect()
                })
                .unwrap_or_default(),
        )
    }

    fn settings_children(&self, prefix: &str) -> Option<Vec<Candidate>> {
        let prefix_segments: Vec<&str> =
            prefix.split('.').filter(|part| !part.is_empty()).collect();
        let mut children: Vec<Candidate> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for candidate in &self.candidates {
            let LookupHit::Setting { path, .. } = &candidate.hit else {
                continue;
            };
            let segments: Vec<&str> = path.split('.').collect();
            if !segments.starts_with(prefix_segments.as_slice()) {
                continue;
            }
            let Some(&child_segment) = segments.get(prefix_segments.len()) else {
                continue;
            };
            if seen.contains(&child_segment) {
                continue;
            }
            seen.push(child_segment);
            if segments.len() == prefix_segments.len() + 1 {
                children.push(candidate.clone());
            } else {
                let child_path = prefix_segments
                    .iter()
                    .copied()
                    .chain(std::iter::once(child_segment))
                    .collect::<Vec<_>>()
                    .join(".");
                children.push(Candidate {
                    forms: vec![
                        (child_segment.to_string(), MatchKind::OpySpelling),
                        (child_path.clone(), MatchKind::Path),
                    ],
                    hit: LookupHit::SettingPath {
                        path: child_path,
                        matched_on: MatchKind::Path,
                    },
                });
            }
        }
        (!children.is_empty()).then_some(children)
    }

    fn setting_enum_children(&self, domain: &str) -> Option<Vec<Candidate>> {
        let mut members: Vec<(String, String)> = Vec::new();
        for definition in settings::definitions() {
            for member in definition.enum_members() {
                if member.domain() == domain && !members.iter().any(|(id, _)| id == member.id()) {
                    members.push((member.id().to_string(), member.english_name().to_string()));
                }
            }
        }
        if members.is_empty() {
            return None;
        }
        Some(
            members
                .into_iter()
                .map(|(id, name)| Candidate {
                    forms: vec![
                        (id.clone(), MatchKind::OpySpelling),
                        (name.clone(), MatchKind::DisplayName),
                    ],
                    hit: LookupHit::EnumMember {
                        spelling: id.clone(),
                        domain: domain.to_string(),
                        member: id,
                        display_name: (!name.is_empty()).then_some(name),
                        aliases: Vec::new(),
                        matched_on: MatchKind::OpySpelling,
                    },
                })
                .collect(),
        )
    }

    fn index_functions(&mut self) {
        let locale = crate::enums::en_us();
        for function in self.manifest.functions() {
            let mut forms = vec![(function.id.clone(), MatchKind::OpySpelling)];
            let mut display_name = None;
            if let Some(catalog_id) = &function.catalog_id {
                forms.push((catalog_id.clone(), MatchKind::CatalogId));
                let kind = match function.kind {
                    FunctionKind::Action | FunctionKind::MemberAction => Kind::Action,
                    FunctionKind::Value | FunctionKind::MemberValue => Kind::Value,
                };
                if let Some(entry) = self.catalog.entry(kind, catalog_id) {
                    display_name = entry.spelling(&locale).map(str::to_string);
                    for spelling in entry.spellings(&locale) {
                        forms.push((spelling.clone(), MatchKind::DisplayName));
                    }
                    for produced in entry
                        .return_type()
                        .into_iter()
                        .flat_map(|return_type| return_type.split('|'))
                    {
                        let produced = produced.trim();
                        if is_handle_type(produced) {
                            forms.push((produced.to_string(), MatchKind::ProducedType));
                        }
                    }
                }
            }
            for alias in self.manifest.aliases() {
                if crate::matcher::alias_target(alias, self.manifest)
                    .is_some_and(|target| std::ptr::eq(target, function))
                {
                    forms.push((alias.source.clone(), MatchKind::Alias));
                }
            }
            let params = self.lookup_params(function);
            self.candidates.push(Candidate {
                forms,
                hit: LookupHit::Function {
                    spelling: function.id.clone(),
                    function_kind: match function.kind {
                        FunctionKind::Action => LookupFunctionKind::Action,
                        FunctionKind::Value => LookupFunctionKind::Value,
                        FunctionKind::MemberAction => LookupFunctionKind::MemberAction,
                        FunctionKind::MemberValue => LookupFunctionKind::MemberValue,
                    },
                    receiver: function.receiver,
                    unbounded: function.unbounded,
                    params,
                    catalog_id: function.catalog_id.clone(),
                    display_name,
                    matched_on: MatchKind::OpySpelling,
                },
            });
        }
        for special in SPECIAL_FUNCTIONS {
            let params: Vec<LookupParam> = special
                .params
                .iter()
                .map(|param| LookupParam {
                    name: param.name.to_string(),
                    alternate_names: Vec::new(),
                    required: param.required,
                    domain: None,
                    members: None,
                    param_type: Some(param.param_type.to_string()),
                    default: param.default.map(str::to_string),
                })
                .collect();
            self.candidates.push(Candidate {
                forms: vec![(special.name.to_string(), MatchKind::OpySpelling)],
                hit: LookupHit::Function {
                    spelling: special.name.to_string(),
                    function_kind: LookupFunctionKind::Value,
                    receiver: None,
                    unbounded: false,
                    params,
                    catalog_id: None,
                    display_name: None,
                    matched_on: MatchKind::OpySpelling,
                },
            });
        }
    }

    fn lookup_params(&self, function: &Function) -> Vec<LookupParam> {
        let catalog_entry = function.catalog_id.as_deref().and_then(|catalog_id| {
            let kind = match function.kind {
                FunctionKind::Action | FunctionKind::MemberAction => Kind::Action,
                FunctionKind::Value | FunctionKind::MemberValue => Kind::Value,
            };
            self.catalog.entry(kind, catalog_id)
        });
        function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let catalog_index = self.catalog_param_index(catalog_entry, function, param, index);
                let (param_type, catalog_domain) = catalog_index
                    .and_then(|i| {
                        catalog_entry.map(|entry| {
                            (
                                entry.param_type(i).map(str::to_string),
                                entry.param_domain(i).map(str::to_string),
                            )
                        })
                    })
                    .unwrap_or((None, None));
                // The reported domain is the reference source spelling:
                // a catalog-declared `Clipping` slot renders as `Clip`.
                let domain = param
                    .domain
                    .clone()
                    .or(catalog_domain)
                    .map(|domain| crate::enums::opy_domain(&domain).to_string());
                let required = !param.optional && param.default.is_none();
                let members = domain
                    .as_deref()
                    .and_then(|domain| crate::enums::domain_members(domain, self.catalog))
                    .map(|members| members.iter().map(member_entry).collect::<Vec<_>>());
                let default = param
                    .default
                    .as_ref()
                    .map(|d| default_text(d, domain.as_deref(), self.catalog));
                LookupParam {
                    name: param.name.clone(),
                    alternate_names: param.alternate_names.clone(),
                    required,
                    members,
                    domain,
                    param_type,
                    default,
                }
            })
            .collect()
    }

    fn catalog_param_index(
        &self,
        entry: Option<&workshop_rs::catalog::CatalogEntry>,
        function: &Function,
        param: &Param,
        index: usize,
    ) -> Option<usize> {
        let entry = entry?;
        let count = entry.param_count();
        let offset = usize::from(function.kind.is_member() && count == function.params.len() + 1);
        let folded = crate::matcher::fold(&param.name);
        let by_name = |i: usize| {
            entry
                .param_name(i)
                .is_some_and(|name| crate::matcher::fold(name) == folded)
        };
        if count == function.params.len() + offset {
            let candidate = index + offset;
            match (0..count).position(&by_name) {
                Some(i) => Some(i),
                None => (candidate < count).then_some(candidate),
            }
        } else {
            (0..count).find(|&i| by_name(i))
        }
    }

    fn index_enums(&mut self) {
        for domain in self.lookup_domains() {
            let members = crate::enums::domain_members(&domain, self.catalog);
            let mut domain_forms = vec![(domain.clone(), MatchKind::OpySpelling)];
            let mut display_name = None;
            if let Some(catalog_domain) = self
                .catalog
                .enum_domain(crate::enums::catalog_domain(&domain))
            {
                domain_forms.push((catalog_domain.domain.clone(), MatchKind::CatalogId));
                let locale = crate::enums::en_us();
                if let Some(spelling) = catalog_domain.spelling(&locale) {
                    display_name = Some(spelling.to_string());
                    domain_forms.push((spelling.to_string(), MatchKind::DisplayName));
                }
            }
            let entries: Option<Vec<EnumMemberEntry>> = members
                .as_ref()
                .map(|members| members.iter().map(member_entry).collect());
            self.candidates.push(Candidate {
                forms: domain_forms,
                hit: LookupHit::EnumDomain {
                    domain: domain.clone(),
                    members: entries,
                    display_name,
                    matched_on: MatchKind::OpySpelling,
                },
            });
            if let Some(members) = members {
                for member in &members {
                    self.candidates.push(self.member_candidate(&domain, member));
                }
            }
        }
    }

    fn member_candidate(&self, domain: &str, member: &crate::enums::DomainMember) -> Candidate {
        let entry = member_entry(member);
        let mut forms = vec![
            (
                format!("{domain}.{}", entry.spelling),
                MatchKind::OpySpelling,
            ),
            (entry.spelling.clone(), MatchKind::OpySpelling),
        ];
        if let Some(display) = &member.display_name {
            forms.push((display.clone(), MatchKind::DisplayName));
        }
        if let Some(catalog_member) = &member.catalog_member {
            forms.push((catalog_member.clone(), MatchKind::CatalogId));
        }
        for alias in &member.aliases {
            forms.push((alias.clone(), MatchKind::Alias));
        }
        Candidate {
            forms,
            hit: LookupHit::EnumMember {
                spelling: format!("{domain}.{}", entry.spelling),
                domain: domain.to_string(),
                member: entry.id,
                display_name: entry.display_name,
                aliases: entry.aliases,
                matched_on: MatchKind::OpySpelling,
            },
        }
    }

    fn lookup_domains(&self) -> Vec<String> {
        let mut domains: Vec<String> = self
            .catalog
            .enum_domains()
            .filter(|domain| !crate::enums::NON_SOURCE_DOMAINS.contains(&domain.domain.as_str()))
            .map(|domain| crate::enums::opy_domain(&domain.domain).to_string())
            .collect();
        domains.extend(
            crate::enums::DOMAIN_ALIASES
                .iter()
                .map(|(source, _)| source.to_string()),
        );
        for domain in self
            .manifest
            .functions()
            .iter()
            .flat_map(|function| function.params.iter())
            .filter_map(|param| param.domain.as_deref())
            .map(crate::enums::opy_domain)
        {
            if !crate::enums::NON_SOURCE_DOMAINS.contains(&domain)
                && !domains.iter().any(|d| d == domain)
            {
                domains.push(domain.to_string());
            }
        }
        domains
    }

    fn index_settings(&mut self) {
        let mut modes: Vec<String> = Vec::new();
        for definition in settings::definitions() {
            self.push_setting(definition.path(), &definition);
            let mut parts = definition.path().split('.');
            if let (Some("gamemodes"), Some(mode), Some(_)) =
                (parts.next(), parts.next(), parts.next())
                && mode != "general"
                && !modes.iter().any(|known| known == mode)
            {
                modes.push(mode.to_string());
            }
        }
        for definition in settings::definitions() {
            let Some(leaf) = definition.path().strip_prefix("gamemodes.general.") else {
                continue;
            };
            for mode in &modes {
                let probe = [
                    PathPart::Part("gamemodes"),
                    PathPart::Part(mode.as_str()),
                    PathPart::Part(leaf),
                ];
                if let Some(resolved) = settings::definition(&probe) {
                    let path = format!("gamemodes.{mode}.{leaf}");
                    if resolved.path() != path {
                        self.push_setting(&path, &resolved);
                    }
                }
            }
        }
    }

    fn push_setting(&mut self, path: &str, definition: &settings::SettingDefinition) {
        let leaf = path.rsplit('.').next().unwrap_or(path);
        let english_name = definition.presentation().english_name;
        let mut forms = vec![
            (path.to_string(), MatchKind::Path),
            (leaf.to_string(), MatchKind::OpySpelling),
        ];
        if !english_name.is_empty() {
            forms.push((english_name.to_string(), MatchKind::DisplayName));
        }
        self.candidates.push(Candidate {
            forms,
            hit: LookupHit::Setting {
                path: path.to_string(),
                display_name: (!english_name.is_empty()).then(|| english_name.to_string()),
                value: setting_value_form(definition),
                matched_on: MatchKind::Path,
            },
        });
    }

    fn index_events(&mut self) {
        let locale = crate::enums::en_us();
        for entry in self.catalog.entries_of(Kind::Event) {
            let accepts_filters = match entry.id.as_str() {
                "global" => false,
                id if id == "eachPlayer" || crate::compiler::player_event_kind(id).is_some() => {
                    true
                }
                _ => continue,
            };
            let display_name = entry.spelling(&locale).map(str::to_string);
            let mut forms = vec![(entry.id.clone(), MatchKind::OpySpelling)];
            for spelling in entry.spellings(&locale) {
                forms.push((spelling.clone(), MatchKind::DisplayName));
            }
            self.candidates.push(Candidate {
                forms,
                hit: LookupHit::Event {
                    spelling: entry.id.clone(),
                    accepts_filters,
                    display_name,
                    matched_on: MatchKind::OpySpelling,
                },
            });
        }
    }
}

fn is_handle_type(type_name: &str) -> bool {
    type_name.ends_with("Id")
}

fn member_entry(member: &crate::enums::DomainMember) -> EnumMemberEntry {
    EnumMemberEntry {
        spelling: member.member.clone(),
        id: member
            .catalog_member
            .clone()
            .unwrap_or_else(|| member.member.clone()),
        display_name: member.display_name.clone(),
        aliases: member.aliases.clone(),
    }
}

fn setting_value_form(definition: &settings::SettingDefinition) -> SettingValueForm {
    let (kind, domain, min, max) = match definition.domain() {
        SettingValueDomain::PresenceOnly => (SettingValueKind::Presence, None, None, None),
        SettingValueDomain::Boolean => (SettingValueKind::Boolean, None, None, None),
        SettingValueDomain::Number(bounds) => {
            (SettingValueKind::Number, None, bounds.min(), bounds.max())
        }
        SettingValueDomain::Percent(bounds) => {
            (SettingValueKind::Percent, None, bounds.min(), bounds.max())
        }
        SettingValueDomain::String => (SettingValueKind::String, None, None, None),
        SettingValueDomain::Enum { domain } => {
            (SettingValueKind::Enum, Some(domain.clone()), None, None)
        }
        SettingValueDomain::MapList => (SettingValueKind::MapList, None, None, None),
        SettingValueDomain::HeroList => (SettingValueKind::HeroList, None, None, None),
        // `SettingValueDomain` is non-exhaustive upstream; a future variant
        // must be mapped here rather than silently re-typed.
        other => unreachable!("unhandled SettingValueDomain variant {other:?}"),
    };
    let members: Vec<String> =
        if kind == SettingValueKind::Boolean && definition.enum_members().next().is_some() {
            vec!["true".to_string()]
        } else {
            definition
                .enum_members()
                .map(|member| member.id().to_string())
                .collect()
        };
    SettingValueForm {
        kind,
        domain,
        members,
        min,
        max,
    }
}

fn default_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn default_text(default: &ParamDefault, domain: Option<&str>, catalog: &Catalog) -> String {
    match default {
        ParamDefault::Null { .. } => "null".to_string(),
        ParamDefault::Bool(value) => value.to_string(),
        ParamDefault::Number(value) => default_number(*value),
        ParamDefault::Call { call, args } => {
            let args = args
                .iter()
                .map(|arg| default_number(*arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{call}({args})")
        }
        ParamDefault::EnumMember(member) => match domain {
            // Defaults render under the advertised OPY spelling: a member
            // recorded by its catalog id (`DO_NOT_CLIP`) shows as `NONE`.
            Some(domain) => {
                let spelling = crate::enums::domain_members(domain, catalog)
                    .and_then(|members| {
                        members
                            .iter()
                            .find(|entry| {
                                entry.member == *member
                                    || entry.catalog_member.as_deref() == Some(member.as_str())
                            })
                            .map(|entry| entry.member.clone())
                    })
                    .unwrap_or_else(|| member.clone());
                format!("{domain}.{spelling}")
            }
            None => member.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workshop_rs::catalog::Catalog;

    #[test]
    fn reported_member_spellings_resolve_through_lowering() {
        let catalog = Catalog::builtin().expect("bundled catalog");
        let manifest = Manifest::builtin().expect("bundled manifest");
        let index = Index::new(manifest, &catalog);
        for domain in index.lookup_domains() {
            let Some(members) = crate::enums::domain_members(&domain, &catalog) else {
                continue;
            };
            for member in &members {
                for spelling in std::iter::once(&member.member).chain(&member.aliases) {
                    let resolves = crate::enums::canonical_member(&domain, spelling, &catalog)
                        .is_some()
                        || (domain == "Color"
                            && crate::enums::extra_color_member(spelling).is_some());
                    assert!(
                        resolves,
                        "{domain}.{spelling} is lookup-visible but does not resolve"
                    );
                }
            }
        }
    }

    #[test]
    fn reported_function_spellings_resolve_through_the_manifest() {
        let manifest = Manifest::builtin().expect("bundled manifest");
        for function in manifest.functions() {
            let resolves = if function.kind.is_member() {
                manifest.resolve_member(&function.id).is_some()
            } else {
                manifest.resolve_function(&function.id).is_some()
            };
            assert!(
                resolves,
                "{} is lookup-visible but does not resolve",
                function.id
            );
        }
        for special in SPECIAL_FUNCTIONS {
            let source = format!(
                "globalvar x\nrule \"r\":\n    @Event global\n    x = {}()\n",
                special.name
            );
            let diagnostics =
                crate::tooling::check(&source, "main.opy", std::path::Path::new("")).diagnostics;
            assert!(
                !diagnostics.iter().any(|diagnostic| matches!(
                    diagnostic.code.as_str(),
                    "unknown-value" | "unknown-action" | "invalid-iterable"
                )),
                "'{}' is lookup-reported but the lowerer rejects it: {diagnostics:?}",
                special.name
            );
        }
    }

    #[test]
    fn reported_member_aliases_are_reference_spellings() {
        let catalog = Catalog::builtin().expect("bundled catalog");
        let manifest = Manifest::builtin().expect("bundled manifest");
        let index = Index::new(manifest, &catalog);
        let mut reported = Vec::new();
        for domain in index.lookup_domains() {
            let Some(members) = crate::enums::domain_members(&domain, &catalog) else {
                continue;
            };
            for member in members {
                for alias in member.aliases {
                    assert_ne!(
                        Some(alias.as_str()),
                        member.catalog_member.as_deref(),
                        "{domain}.{alias} advertises the catalog id as an OPY alias"
                    );
                    reported.push(format!("{domain}.{alias}"));
                }
            }
        }
        reported.sort();
        assert!(!reported.is_empty(), "the reference ships member aliases");
    }

    #[test]
    fn rank_prefers_close_spellings() {
        let pool = crate::matcher::bare_candidates(["hudText", "smallMessage", "destroyIcon"]);
        let ranked = crate::matcher::rank("hudTex", &pool);
        assert_eq!(ranked.first().map(String::as_str), Some("hudText"));
        assert!(
            !ranked.iter().any(|candidate| candidate == "hudTex"),
            "the rejected spelling is not a candidate"
        );
    }

    #[test]
    fn sibling_keys_cover_inherited_gamemode_keys() {
        let keys =
            crate::matcher::sibling_keys(&[PathPart::Part("gamemodes"), PathPart::Part("ffa")]);
        assert!(
            keys.iter().any(|key| key == "scoreToWin"),
            "inherited general keys must be candidates: {keys:?}"
        );
    }

    fn within_query(within: LookupWithin) -> LookupQuery {
        LookupQuery {
            text: String::new(),
            scope: LookupScope::ALL,
            within: Some(within),
            locale: None,
            limit: usize::MAX,
        }
    }

    #[test]
    fn within_callable_lists_params_in_call_order() {
        let LookupOutcome::Matched { results, .. } =
            lookup(&within_query(LookupWithin::Callable("wait".to_string())))
        else {
            panic!("wait is a known callable");
        };
        let names: Vec<&str> = results
            .iter()
            .map(|hit| {
                let LookupHit::Parameter { param, .. } = hit else {
                    panic!("callable children are parameters: {hit:?}");
                };
                param.name.as_str()
            })
            .collect();
        assert_eq!(names, ["time", "waitBehavior"]);
        let LookupOutcome::UnknownWithin { .. } = lookup(&within_query(LookupWithin::Callable(
            "notAFunction".to_string(),
        ))) else {
            panic!("an unknown callable is not a scope");
        };
    }

    #[test]
    fn within_scopes_list_their_children() {
        let LookupOutcome::Matched { results, .. } =
            lookup(&within_query(LookupWithin::Enum("Button".to_string())))
        else {
            panic!("Button is a known enum domain");
        };
        let LookupHit::EnumMember { spelling, .. } = &results[0] else {
            panic!("enum children are members: {:?}", results[0]);
        };
        assert_eq!(spelling, "Button.PRIMARY_FIRE");

        let LookupOutcome::Matched { results, .. } =
            lookup(&within_query(LookupWithin::Settings(String::new())))
        else {
            panic!("the settings root exists");
        };
        assert!(
            results
                .iter()
                .all(|hit| matches!(hit, LookupHit::SettingPath { .. })),
            "the root has only intermediate segments: {results:?}"
        );

        let LookupOutcome::Matched { results, .. } = lookup(&within_query(LookupWithin::Settings(
            "gamemodes.ffa".to_string(),
        ))) else {
            panic!("gamemodes.ffa is a settings path prefix");
        };
        assert!(
            results.iter().any(|hit| matches!(
                hit,
                LookupHit::Setting { path, .. } if path == "gamemodes.ffa.scoreToWin"
            )),
            "a leaf child reports its Setting hit: {results:?}"
        );

        let LookupOutcome::UnknownWithin { .. } = lookup(&within_query(LookupWithin::Settings(
            "gamemodes.ffa.scoreToWin".to_string(),
        ))) else {
            panic!("a leaf setting has no children");
        };
    }

    #[test]
    fn within_settings_enum_lists_accepted_members() {
        let LookupOutcome::Matched { results, .. } = lookup(&within_query(
            LookupWithin::SettingEnum("mapRotation".to_string()),
        )) else {
            panic!("mapRotation is a settings enum domain");
        };
        let spellings: Vec<&str> = results
            .iter()
            .map(|hit| {
                let LookupHit::EnumMember { spelling, .. } = hit else {
                    panic!("settings enum children are members: {hit:?}");
                };
                spelling.as_str()
            })
            .collect();
        assert_eq!(spellings, ["afterAGame", "afterMirrorMatch", "paused"]);
        let LookupOutcome::UnknownWithin { .. } =
            lookup(&within_query(LookupWithin::Enum("mapRotation".to_string())))
        else {
            panic!("mapRotation is not a source enum domain");
        };
    }

    #[test]
    fn empty_text_lists_children_in_stable_order() {
        let outcome_a = lookup(&within_query(LookupWithin::Enum("Button".to_string())));
        let outcome_b = lookup(&within_query(LookupWithin::Enum("Button".to_string())));
        assert_eq!(outcome_a, outcome_b, "repeat listings are identical");
        let LookupOutcome::Matched { results, .. } = outcome_a else {
            panic!("Button is a known enum domain");
        };
        assert!(
            results.iter().all(|hit| matches!(
                hit,
                LookupHit::EnumMember {
                    matched_on: MatchKind::OpySpelling,
                    ..
                }
            )),
            "listed members report their spelling form: {results:?}"
        );
    }

    #[test]
    fn call_spelling_reports_an_accepted_spelling() {
        let manifest = Manifest::builtin().expect("bundled manifest");
        for function in manifest.functions() {
            let Some(catalog_id) = function.catalog_id.as_deref() else {
                continue;
            };
            let spelling = call_spelling(catalog_id)
                .unwrap_or_else(|| panic!("{catalog_id} claims no spelling"));
            let claims = manifest.functions().iter().any(|entry| {
                entry.id == spelling && entry.catalog_id.as_deref() == Some(catalog_id)
            });
            assert!(
                claims,
                "{catalog_id} resolved to {spelling}, which does not claim it"
            );
        }
    }

    #[test]
    fn call_spelling_answers_only_canonical_ids() {
        for (canonical_id, spelling) in SYNTAX_SPELLINGS {
            assert_eq!(
                call_spelling(canonical_id),
                Some(*spelling),
                "{canonical_id}"
            );
        }
        for id in [
            "evalOnce",
            "all",
            "log",
            "getPlayers",
            "end",
            "valueInArray",
            "setGlobalVariable",
            "notAnId",
        ] {
            assert_eq!(call_spelling(id), None, "{id} must not resolve");
        }
    }

    #[test]
    fn enum_member_spelling_matches_the_member_table() {
        assert_eq!(enum_member_spelling("Status", "STUNNED"), Some("STUNNED"));
        assert_eq!(enum_member_spelling("Map", "ROUTE_66"), Some("ROUTE66"));
        assert_eq!(
            enum_member_spelling("Clipping", "DO_NOT_CLIP"),
            Some("NONE")
        );
        assert_eq!(enum_member_spelling("Status", "NO_SUCH_MEMBER"), None);
        assert_eq!(enum_member_spelling("Rounding", "DOWN"), None);
        assert_eq!(enum_member_spelling("NoSuchDomain", "ANY"), None);
    }

    #[test]
    fn every_produced_handle_type_reaches_its_producer() {
        let catalog = Catalog::builtin().expect("bundled catalog");
        let mut producers: Vec<(String, String)> = Vec::new();
        for entry in catalog.entries_of(Kind::Value) {
            let Some(return_type) = entry.return_type() else {
                continue;
            };
            for part in return_type.split('|') {
                let part = part.trim();
                if is_handle_type(part) {
                    producers.push((part.to_string(), entry.id.clone()));
                }
            }
        }
        producers.sort();
        producers.dedup();
        assert!(
            !producers.is_empty(),
            "the catalog declares handle-type producers"
        );
        for (handle_type, canonical_id) in producers {
            let spelling = call_spelling(&canonical_id).unwrap_or_else(|| {
                panic!("{canonical_id} produces {handle_type} but spells nothing in OPY")
            });
            let mut query = LookupQuery::new(&handle_type);
            query.limit = 10;
            let hits = lookup(&query).results().to_vec();
            assert!(
                hits.iter().any(|hit| matches!(
                    hit,
                    LookupHit::Function { spelling: s, .. } if s == spelling
                )),
                "'{handle_type}' must return producer '{spelling}' in the first 10 results: {hits:?}"
            );
        }
    }
}
