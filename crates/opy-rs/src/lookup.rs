//! OPY vocabulary lookup: display names and guesses resolve to OPY spellings,
//! signatures, enum members, and settings keys through the same manifest,
//! catalog, and settings tables `check`/`compile` resolve against.
//!
//! This module is the answer surface for agents; it owns no data of its own.
//! Callable spellings and signatures come from the compatibility manifest,
//! enum members and display names come from the `workshop-rs` catalog, and
//! settings vocabulary comes from the `workshop-rs` emission table (which it
//! expands to every effective path, including inherited per-mode keys).
//! The same scoring helpers feed the candidate lists unknown-name diagnostics
//! carry, so lookup results and rejection diagnostics cannot drift.

use serde::Serialize;
use workshop_rs::catalog::{Catalog, Kind};
use workshop_rs::settings::{self, PathPart, SettingValueDomain};

use crate::manifest::{
    AliasKind, Function, FunctionKind, Manifest, Param, ParamDefault, ReceiverCategory,
};

/// The default bound on returned hits; also the diagnostic candidate bound.
pub const DEFAULT_LIMIT: usize = 8;

/// A source-level special form: a call name the lowerer handles by name
/// rather than through the manifest (`crate::lower::expressions`). Their
/// parameter facts mirror the reference's declared signatures — the same
/// `(name, type, required, default)` facts manifest-backed callables report.
struct SpecialFunction {
    name: &'static str,
    params: &'static [SpecialParam],
}

/// One ordered parameter of a special call, from the reference signature.
struct SpecialParam {
    name: &'static str,
    /// The reference's declared argument type (`IntLiteral`, `Lambda`, …).
    param_type: &'static str,
    required: bool,
    /// The declared default rendered in source syntax, when declared.
    default: Option<&'static str>,
}

const fn required(name: &'static str, param_type: &'static str) -> SpecialParam {
    SpecialParam {
        name,
        param_type,
        required: true,
        default: None,
    }
}

const fn defaulted(
    name: &'static str,
    param_type: &'static str,
    default: &'static str,
) -> SpecialParam {
    SpecialParam {
        name,
        param_type,
        required: false,
        default: Some(default),
    }
}

const fn optional(name: &'static str, param_type: &'static str) -> SpecialParam {
    SpecialParam {
        name,
        param_type,
        required: false,
        default: None,
    }
}

/// The special-call spellings an `unknown-value` diagnostic can suggest and
/// the lookup can describe, with the parameter facts the reference declares.
const SPECIAL_FUNCTIONS: &[SpecialFunction] = &[
    SpecialFunction {
        name: "sorted",
        params: &[
            required("array", "Array"),
            // The reference's signature names the slot `lambda`; `key` is
            // the keyword spelling callers write (#437).
            optional("key", "Lambda"),
        ],
    },
    SpecialFunction {
        name: "createWorkshopSetting",
        params: &[
            required("type", "Type"),
            required("category", "CustomStringLiteral"),
            required("name", "CustomStringLiteral"),
            required("default", "BoolLiteral|IntLiteral|FloatLiteral|HeroLiteral"),
            defaulted("sortOrder", "IntLiteral", "0"),
        ],
    },
    SpecialFunction {
        name: "createWorkshopSettingBool",
        params: &[
            required("category", "CustomStringLiteral"),
            required("name", "CustomStringLiteral"),
            required("default", "BoolLiteral"),
            defaulted("sortOrder", "IntLiteral", "0"),
        ],
    },
    SpecialFunction {
        name: "createWorkshopSettingEnum",
        params: &[
            required("category", "CustomStringLiteral"),
            required("name", "CustomStringLiteral"),
            required("default", "UnsignedIntLiteral"),
            required("options", "Array<CustomStringLiteral>"),
            defaulted("sortOrder", "IntLiteral", "0"),
        ],
    },
    SpecialFunction {
        name: "createWorkshopSettingInt",
        params: &[
            required("category", "CustomStringLiteral"),
            required("name", "CustomStringLiteral"),
            required("default", "IntLiteral"),
            required("min", "IntLiteral"),
            required("max", "IntLiteral"),
            defaulted("sortOrder", "IntLiteral", "0"),
        ],
    },
    SpecialFunction {
        name: "createWorkshopSettingFloat",
        params: &[
            required("category", "CustomStringLiteral"),
            required("name", "CustomStringLiteral"),
            required("default", "FloatLiteral"),
            required("min", "FloatLiteral"),
            required("max", "FloatLiteral"),
            defaulted("sortOrder", "IntLiteral", "0"),
        ],
    },
    SpecialFunction {
        name: "createWorkshopSettingHero",
        params: &[
            required("category", "CustomStringLiteral"),
            required("name", "CustomStringLiteral"),
            required("default", "HeroLiteral"),
            defaulted("sortOrder", "IntLiteral", "0"),
        ],
    },
];

/// A lookup request.
#[derive(Debug, Clone, PartialEq)]
pub struct LookupQuery {
    /// The name or guess to resolve: an OPY spelling, a canonical Workshop
    /// id, a Workshop display name, or a settings path prefix.
    pub text: String,
    /// The namespaces the query searches; `LookupScope::ALL` is the default.
    pub scope: LookupScope,
    /// The display-name locale to read; only `en-US` is supported.
    /// Localized display names remain `workshop-rs`-owned and are answered
    /// as `LookupOutcome::Unsupported`, not silently returned in English.
    pub locale: Option<String>,
    /// The maximum number of returned hits; `0` selects `DEFAULT_LIMIT`.
    pub limit: usize,
}

impl LookupQuery {
    /// A query across every namespace.
    pub fn new(text: impl Into<String>) -> LookupQuery {
        LookupQuery {
            text: text.into(),
            scope: LookupScope::ALL,
            locale: None,
            limit: 0,
        }
    }
}

/// The namespaces a query searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupScope {
    /// Callable spellings and signatures.
    pub functions: bool,
    /// Enum domains and members.
    pub enums: bool,
    /// Settings keys and value forms.
    pub settings: bool,
}

impl LookupScope {
    /// Every namespace.
    pub const ALL: LookupScope = LookupScope {
        functions: true,
        enums: true,
        settings: true,
    };
    /// Callables only.
    pub const FUNCTIONS: LookupScope = LookupScope {
        functions: true,
        enums: false,
        settings: false,
    };
    /// Enum domains and members only.
    pub const ENUMS: LookupScope = LookupScope {
        functions: false,
        enums: true,
        settings: false,
    };
    /// Settings only.
    pub const SETTINGS: LookupScope = LookupScope {
        functions: false,
        enums: false,
        settings: true,
    };

    fn any(self) -> bool {
        self.functions || self.enums || self.settings
    }
}

/// The result of a lookup.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum LookupOutcome {
    /// The query was answered (possibly with no hits when nothing matched).
    Matched {
        /// The namespaces searched.
        scope: LookupScope,
        /// The maximum number of hits the bound permitted.
        limit: usize,
        /// The hits, best matches first.
        results: Vec<LookupHit>,
    },
    /// The query is valid but the requested capability is not served:
    /// nothing was searched.
    Unsupported {
        /// The namespaces that would have been searched.
        scope: LookupScope,
        /// Why the query cannot be answered (e.g. localized display names
        /// are `workshop-rs`-owned).
        reason: String,
    },
}

impl LookupOutcome {
    /// The hits of a `Matched` outcome, else an empty slice.
    pub fn results(&self) -> &[LookupHit] {
        match self {
            LookupOutcome::Matched { results, .. } => results,
            LookupOutcome::Unsupported { .. } => &[],
        }
    }
}

/// Which spelling form a hit matched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchKind {
    /// The OPY source spelling (e.g. `hudText`, `Hero.SOLDIER`).
    OpySpelling,
    /// The canonical Workshop id (e.g. `createHudText`).
    CatalogId,
    /// A Workshop display name (e.g. `Create HUD Text`).
    DisplayName,
    /// A declared OPY alias spelling (e.g. `getPlayers`).
    Alias,
    /// A settings path (e.g. `gamemodes.ffa.scoreToWin`).
    Path,
    /// No form matched exactly; the hit is the closest near-spelling.
    Near,
}

impl MatchKind {
    /// The serialized kind label.
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchKind::OpySpelling => "opySpelling",
            MatchKind::CatalogId => "catalogId",
            MatchKind::DisplayName => "displayName",
            MatchKind::Alias => "alias",
            MatchKind::Path => "path",
            MatchKind::Near => "near",
        }
    }
}

/// One lookup hit.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "hit")]
pub enum LookupHit {
    /// A callable.
    Function {
        /// The OPY call spelling (member calls carry their member name).
        spelling: String,
        /// Whether the callable is an action, a value, or a member call.
        function_kind: LookupFunctionKind,
        /// The declared receiver category of a member call — how it is
        /// invoked (`eventPlayer.setMoveSpeed(…)` takes a `Player`
        /// receiver). `None` for standalone calls.
        #[serde(skip_serializing_if = "Option::is_none")]
        receiver: Option<ReceiverCategory>,
        /// Whether the argument count is unbounded (`.format` placeholders).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        unbounded: bool,
        /// The parameters in call order: the owner facts Wright renders
        /// into a signature string.
        params: Vec<LookupParam>,
        /// The canonical catalog id the callable emits, when linked.
        catalog_id: Option<String>,
        /// Which spelling form matched.
        matched_on: MatchKind,
    },
    /// A callable enum domain.
    EnumDomain {
        /// The OPY domain spelling (`Domain.MEMBER` head).
        domain: String,
        /// The members the domain accepts, or `None` when the domain is
        /// manifest-only/contextual and carries no standalone member list.
        members: Option<Vec<EnumMemberEntry>>,
        /// Which spelling form matched.
        matched_on: MatchKind,
    },
    /// One enum member.
    EnumMember {
        /// The OPY spelling (`Hero.SOLDIER`).
        spelling: String,
        /// The domain it belongs to.
        domain: String,
        /// The canonical catalog member id it lowers to.
        member: String,
        /// The English display name, when the catalog provides one.
        display_name: Option<String>,
        /// The other source spellings the member accepts.
        aliases: Vec<String>,
        /// Which spelling form matched.
        matched_on: MatchKind,
    },
    /// A settings key.
    Setting {
        /// The effective `settings` path; per-team/per-hero slots stay
        /// templated as `heroes.<team>.<hero>.<key>`.
        path: String,
        /// The English display name.
        display_name: Option<String>,
        /// The accepted value form.
        value: SettingValueForm,
        /// Which spelling form matched.
        matched_on: MatchKind,
    },
}

/// The call position a function hit occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LookupFunctionKind {
    /// A statement-position action.
    Action,
    /// A value-position function or special form.
    Value,
    /// An action that takes an event-player receiver.
    MemberAction,
    /// A value that takes a receiver.
    MemberValue,
}

impl LookupFunctionKind {
    /// The serialized kind label.
    pub fn as_str(&self) -> &'static str {
        match self {
            LookupFunctionKind::Action => "action",
            LookupFunctionKind::Value => "value",
            LookupFunctionKind::MemberAction => "memberAction",
            LookupFunctionKind::MemberValue => "memberValue",
        }
    }
}

/// One ordered parameter of a callable: the owner facts a caller renders.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupParam {
    /// The canonical OPY keyword spelling.
    pub name: String,
    /// The alternate keyword spellings the slot accepts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternate_names: Vec<String>,
    /// Whether the slot must be supplied.
    pub required: bool,
    /// The enum domain the slot takes, when it takes one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Every member spelling of that domain, when it is an enum slot —
    /// the owner supplies the full inventory; which members a signature
    /// renders is the caller's policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<EnumMemberEntry>>,
    /// The semantic type the catalog records for the slot.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub param_type: Option<String>,
    /// The default value, rendered in source syntax.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// One member of an enum domain.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumMemberEntry {
    /// The OPY member spelling (`Hero.SOLDIER` minus its head).
    pub spelling: String,
    /// The canonical catalog member id it lowers to.
    pub id: String,
    /// The English display name, when the catalog provides one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The other source spellings the member accepts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

/// The value form a settings key accepts.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingValueForm {
    /// The value shape.
    pub kind: SettingValueKind,
    /// The member spellings an `enum` slot accepts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    /// The numeric bounds the table declares, when it declares them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// The numeric upper bound, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

/// The value shape a settings key accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingValueKind {
    /// `key` with no value.
    Presence,
    /// `true` or `false`.
    Boolean,
    /// A number literal.
    Number,
    /// A number interpreted as a percent.
    Percent,
    /// A string literal.
    String,
    /// One of the `members` spellings.
    Enum,
    /// A list of map names.
    MapList,
    /// A list of hero names.
    HeroList,
}

impl SettingValueKind {
    /// The serialized kind label.
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
/// The answer is `LookupOutcome::Matched` (possibly with zero hits) or
/// `LookupOutcome::Unsupported` with a reason; vocabulary lookup never
/// fails — a name that resolves to nothing is simply absent from `results`.
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
    if let Some(locale) = &query.locale {
        if !locale.eq_ignore_ascii_case("en-US") {
            return unsupported(
                "localized display names are owned by workshop-rs; opy-rs \
                 resolves display names in the 'en-US' locale only",
            );
        }
    }
    if query.text.trim().is_empty() {
        return unsupported("the query text is empty");
    }
    if !query.scope.any() {
        return unsupported("the query scope selects no namespace");
    }
    let (manifest, catalog) = match (Manifest::builtin(), Catalog::builtin()) {
        (Ok(manifest), Ok(catalog)) => (manifest, catalog),
        _ => return unsupported("vocabulary data (manifest or catalog) is not bundled"),
    };
    let index = Index::new(manifest, &catalog);
    let mut hits = index.search(query);
    hits.sort_by(|a, b| {
        a.score
            .cmp(&b.score)
            .then_with(|| a.hit.sort_key().cmp(&b.hit.sort_key()))
    });
    hits.truncate(limit);
    LookupOutcome::Matched {
        scope,
        limit,
        results: hits.into_iter().map(|scored| scored.hit).collect(),
    }
}

/// Lookup with a default query (all namespaces, `DEFAULT_LIMIT`).
pub fn lookup_str(text: &str) -> LookupOutcome {
    lookup(&LookupQuery::new(text))
}

// ---------------------------------------------------------------------------
// Scoring — shared by lookup hits and diagnostic candidate lists
// ---------------------------------------------------------------------------

/// Fold a spelling or phrase to a comparable form: lowercase letters and
/// digits only; `_`, `.`, `:`, `%`, and whitespace are dropped.
fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Score a query against a valid spelling; lower is closer, `None` is
/// unrelated. Exact folded equality beats prefix beats substring beats
/// edit distance.
fn score(query: &str, spelling: &str) -> Option<u32> {
    let query = fold(query);
    let spelling = fold(spelling);
    if query.is_empty() || spelling.is_empty() {
        return None;
    }
    if query == spelling {
        return Some(0);
    }
    if spelling.starts_with(&query) {
        return Some((spelling.len() - query.len()) as u32 + 2);
    }
    if query.starts_with(&spelling) {
        return Some((query.len() - spelling.len()) as u32 + 4);
    }
    // Substring matching on very short spellings is pure noise (a one-letter
    // display name like Icon's "X" hides inside unrelated folds).
    if spelling.len().min(query.len()) >= 3
        && (spelling.contains(&query) || query.contains(&spelling))
    {
        return Some(
            (spelling.len().max(query.len()) - spelling.len().min(query.len())) as u32 + 10,
        );
    }
    let distance = levenshtein(&query, &spelling);
    (distance * 3 <= query.len() as u32 + 1).then_some(distance * 10 + 20)
}

/// Rank `pool` by closeness to `query`, best first, bounded to
/// `DEFAULT_LIMIT` entries. This is the candidate list diagnostics carry.
pub(crate) fn rank(query: &str, pool: &[String]) -> Vec<String> {
    let mut scored: Vec<(u32, &str)> = pool
        .iter()
        .map(String::as_str)
        .filter_map(|spelling| score(query, spelling).map(|s| (s, spelling)))
        .collect();
    scored.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.cmp(y)));
    scored.dedup_by(|a, b| a.1 == b.1);
    scored
        .into_iter()
        .take(DEFAULT_LIMIT)
        .map(|(_, spelling)| spelling.to_string())
        .collect()
}

/// Append a `(did you mean …?)` suffix naming the closest `ranked`
/// candidates (at most three; the list must already be [`rank`]ed).
pub(crate) fn did_you_mean(message: String, ranked: &[String]) -> String {
    match ranked {
        [] => message,
        [one] => format!("{message} (did you mean '{one}'?)"),
        [first, rest @ ..] => {
            let others = rest
                .iter()
                .take(2)
                .map(|s| format!("'{s}'"))
                .collect::<Vec<_>>()
                .join(" or ");
            format!("{message} (did you mean '{first}', {others}?)")
        }
    }
}

/// Segment-aware scoring for a settings path: the query's dot-separated
/// segments must be a prefix of the path's segments, where a template
/// segment (`<team>`, `<hero>`) accepts any segment spelling and the last
/// query segment may be a prefix of its path segment. `None` when the
/// query is not a path prefix — near-name ranking falls back to `score`.
fn path_score(query: &str, path: &str) -> Option<u32> {
    let segments: Vec<String> = query.trim().split('.').map(fold).collect();
    if segments.iter().any(String::is_empty) {
        return None;
    }
    let path_segments: Vec<&str> = path.split('.').collect();
    if segments.len() > path_segments.len() {
        return None;
    }
    let mut penalty = 0u32;
    for (index, segment) in segments.iter().enumerate() {
        let target = path_segments[index];
        if (target.starts_with('<') && target.ends_with('>')) || fold(target) == *segment {
            continue;
        }
        let target_folded = fold(target);
        if index + 1 == segments.len() && target_folded.starts_with(segment.as_str()) {
            penalty += (target_folded.len() - segment.len()) as u32 + 2;
            continue;
        }
        return None;
    }
    Some(penalty + (path_segments.len() - segments.len()) as u32)
}

fn levenshtein(a: &str, b: &str) -> u32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<u32> = (0..=b.len() as u32).collect();
    for (i, &x) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i as u32 + 1;
        for (j, &y) in b.iter().enumerate() {
            let next = (row[j] + 1)
                .min(row[j + 1] + 1)
                .min(prev + u32::from(x != y));
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

// ---------------------------------------------------------------------------
// Diagnostic candidate pools — the names each unknown-name failure compares
// ---------------------------------------------------------------------------

/// The standalone spellings callable in the given position.
pub(crate) fn function_spellings(manifest: &Manifest, actions: bool) -> Vec<String> {
    let mut pool: Vec<String> = manifest
        .functions()
        .iter()
        .filter(|function| {
            !function.kind.is_member()
                && if actions {
                    function.kind.is_action()
                } else {
                    function.kind.is_value()
                }
        })
        .map(|function| function.id.clone())
        .collect();
    pool.extend(
        manifest
            .aliases()
            .iter()
            .filter(|alias| {
                alias.kind == AliasKind::FunctionAlias
                    && manifest.function(&alias.target).is_some_and(|function| {
                        if actions {
                            function.kind.is_action()
                        } else {
                            function.kind.is_value()
                        }
                    })
            })
            .map(|alias| alias.source.clone()),
    );
    if !actions {
        pool.extend(
            SPECIAL_FUNCTIONS
                .iter()
                .map(|special| special.name.to_string()),
        );
    }
    pool
}

/// The member-call spellings (`eventPlayer.member(...)`, `player.member`).
pub(crate) fn member_function_spellings(manifest: &Manifest) -> Vec<String> {
    let mut pool: Vec<String> = manifest
        .functions()
        .iter()
        .filter(|function| function.kind.is_member())
        .map(|function| function.id.clone())
        .collect();
    pool.extend(
        manifest
            .aliases()
            .iter()
            .filter(|alias| alias.kind == AliasKind::MemberAlias)
            .map(|alias| alias.source.clone()),
    );
    pool
}

/// The candidates a settings-emission rejection carries: the leaf keys or
/// enum members valid at the rejected member's path, resolved by walking
/// the source `settings` tree to the reported span. The owner's own single
/// `suggestion` (already in the message) is the candidate of record for
/// positions whose spelling pool is not enumerable through the public
/// settings API (hero group names, list element spellings).
pub(crate) fn settings_member_candidates(
    hir_settings: Option<&crate::hir::types::Settings>,
    error: &workshop_rs::WorkshopError,
    suggestion: Option<&str>,
) -> Vec<String> {
    let Some(hir_settings) = hir_settings else {
        return Vec::new();
    };
    let Some(span) = crate::compiler::workshop_error_span(error) else {
        return Vec::new();
    };
    let mut path = Vec::new();
    match find_settings_node(&hir_settings.children, &mut path, &span) {
        // A leaf member: the key either resolves (the value or kind is the
        // problem) or is itself outside the emission table.
        Some(SettingsMatch::Member { path, name }) => {
            let member_path: Vec<PathPart<'_>> = {
                let mut full = path;
                full.push(PathPart::Part(name));
                full
            };
            match settings::definition(&member_path) {
                Some(definition) => {
                    // A `BoolEnum` key is Boolean-typed over an enum domain
                    // and accepts `true` only.
                    if matches!(definition.domain(), SettingValueDomain::Boolean)
                        && definition.enum_members().next().is_some()
                    {
                        vec!["true".to_string()]
                    } else {
                        definition
                            .enum_members()
                            .map(|member| member.id().to_string())
                            .collect()
                    }
                }
                None => rank(name, &sibling_keys(&member_path[..member_path.len() - 1])),
            }
        }
        // A hero group under `heroes.<team>`: hero names are the accepted
        // spelling pool, which the owner only exposes through `suggestion`.
        Some(SettingsMatch::Group { path })
            if matches!(
                path.as_slice(),
                [PathPart::Part("heroes"), PathPart::Team, PathPart::Hero]
            ) =>
        {
            suggestion.into_iter().map(str::to_string).collect()
        }
        // A team group or a structural rejection: no enumerable pool.
        Some(SettingsMatch::Group { .. }) => Vec::new(),
        // No node matched the reported span (list-element spans land here):
        // the owner's suggestion is the only available candidate.
        None => suggestion.into_iter().map(str::to_string).collect(),
    }
}

/// The leaf keys valid at `parent_path` — every declaration whose last part
/// resolves at that parent (so `gamemodes.ffa` inherits `gamemodes.general`).
fn sibling_keys(parent_path: &[PathPart<'_>]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for definition in settings::definitions() {
        let Some(leaf) = definition.path().rsplit('.').next() else {
            continue;
        };
        let mut probe = parent_path.to_vec();
        probe.push(PathPart::Part(leaf));
        if settings::definition(&probe).is_some() && !keys.iter().any(|key| key == leaf) {
            keys.push(leaf.to_string());
        }
    }
    keys
}

/// The settings node found at a reported span, with its emission-table path.
enum SettingsMatch<'a> {
    Member {
        path: Vec<PathPart<'a>>,
        name: &'a str,
    },
    Group {
        path: Vec<PathPart<'a>>,
    },
}

/// Walk the settings tree to the node whose span `check` reported, building
/// the emission-table path (`gamemodes.<mode>`, `heroes.<team>`,
/// `heroes.<team>.<hero>`) on the way down.
fn find_settings_node<'a>(
    children: &'a [crate::hir::types::SettingsNode],
    path: &mut Vec<PathPart<'a>>,
    span: &workshop_rs::source::Span,
) -> Option<SettingsMatch<'a>> {
    use crate::hir::types::SettingsNode as Node;
    for node in children {
        if let Node::Group { name, children, .. } = node {
            path.push(settings_path_part(name, path));
            if let Some(found) = find_settings_node(children, path, span) {
                return Some(found);
            }
            if settings_span_matches(node.span(), span) {
                return Some(SettingsMatch::Group {
                    path: std::mem::take(path),
                });
            }
            path.pop();
        } else if settings_span_matches(node.span(), span) {
            return Some(SettingsMatch::Member {
                path: path.clone(),
                name: settings_member_name(node),
            });
        }
    }
    None
}

/// The path part a group name occupies, given the path so far.
fn settings_path_part<'a>(name: &'a str, path: &[PathPart<'a>]) -> PathPart<'a> {
    match path {
        [PathPart::Part("heroes")] => PathPart::Team,
        [PathPart::Part("heroes"), PathPart::Team] => PathPart::Hero,
        _ => PathPart::Part(name),
    }
}

/// Whether the HIR node span is the span a `WorkshopError` reported.
fn settings_span_matches(
    span: Option<&crate::hir::Span>,
    reported: &workshop_rs::source::Span,
) -> bool {
    let Some(span) = span else { return false };
    span.file == reported.file.index() as u32
        && span.start.line == reported.start.line
        && span.start.col == reported.start.col
        && span.end.line == reported.end.line
        && span.end.col == reported.end.col
}

fn settings_member_name(node: &crate::hir::types::SettingsNode) -> &str {
    use crate::hir::types::SettingsNode as Node;
    match node {
        Node::Number { name, .. }
        | Node::Bool { name, .. }
        | Node::String { name, .. }
        | Node::Raw { name, .. }
        | Node::List { name, .. } => name,
        Node::Group { .. } => unreachable!("groups are not leaf members"),
    }
}

// ---------------------------------------------------------------------------
// The search index — spellings projected from manifest + catalog data
// ---------------------------------------------------------------------------

struct ScoredHit {
    score: u32,
    hit: LookupHit,
}

impl LookupHit {
    fn sort_key(&self) -> String {
        match self {
            LookupHit::Function { spelling, .. } => format!("0{spelling}"),
            LookupHit::EnumMember { spelling, .. } => format!("1{spelling}"),
            LookupHit::EnumDomain { domain, .. } => format!("2{domain}"),
            LookupHit::Setting { path, .. } => format!("3{path}"),
        }
    }

    /// Stamp the matched-on kind: a folded-equal or structurally matching
    /// (settings path-prefix) form reports its own kind; other hits are
    /// `Near` near-misses.
    fn with_match_kind(self, kind: MatchKind, score: u32, structural: bool) -> LookupHit {
        let matched_on = if score == 0 || structural {
            kind
        } else {
            MatchKind::Near
        };
        match self {
            LookupHit::Function {
                spelling,
                function_kind,
                receiver,
                unbounded,
                params,
                catalog_id,
                ..
            } => LookupHit::Function {
                spelling,
                function_kind,
                receiver,
                unbounded,
                params,
                catalog_id,
                matched_on,
            },
            LookupHit::EnumDomain {
                domain, members, ..
            } => LookupHit::EnumDomain {
                domain,
                members,
                matched_on,
            },
            LookupHit::EnumMember {
                spelling,
                domain,
                member,
                display_name,
                aliases,
                ..
            } => LookupHit::EnumMember {
                spelling,
                domain,
                member,
                display_name,
                aliases,
                matched_on,
            },
            LookupHit::Setting {
                path,
                display_name,
                value,
                ..
            } => LookupHit::Setting {
                path,
                display_name,
                value,
                matched_on,
            },
        }
    }
}

/// One searchable entry: every spelling form it answers to plus the payload
/// returned when one matches.
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
        index
    }

    fn search(&self, query: &LookupQuery) -> Vec<ScoredHit> {
        self.candidates
            .iter()
            .filter(|candidate| self.in_scope(candidate, query.scope))
            .filter_map(|candidate| {
                candidate
                    .forms
                    .iter()
                    .filter_map(|(form, kind)| {
                        // Settings paths match segment by segment: structure,
                        // template segments and `%` suffixes stay meaningful
                        // for exact/path-prefix queries; the folded `score`
                        // only serves near-name ranking.
                        let (scored, structural) = if *kind == MatchKind::Path {
                            match path_score(&query.text, form) {
                                Some(s) => (Some(s), true),
                                None => (score(&query.text, form), false),
                            }
                        } else {
                            (score(&query.text, form), false)
                        };
                        scored.map(|s| {
                            // A raw (case-insensitive) hit reports the form
                            // the user actually typed when folds collide.
                            (
                                s,
                                form.eq_ignore_ascii_case(query.text.trim()),
                                *kind,
                                structural,
                            )
                        })
                    })
                    .min_by_key(|(s, exact, _, _)| (*s, !*exact))
                    .map(|(s, _, kind, structural)| ScoredHit {
                        score: s,
                        hit: candidate.hit.clone().with_match_kind(kind, s, structural),
                    })
            })
            .collect()
    }

    fn in_scope(&self, candidate: &Candidate, scope: LookupScope) -> bool {
        match &candidate.hit {
            LookupHit::Function { .. } => scope.functions,
            LookupHit::EnumDomain { .. } | LookupHit::EnumMember { .. } => scope.enums,
            LookupHit::Setting { .. } => scope.settings,
        }
    }

    // -- functions --------------------------------------------------------

    fn index_functions(&mut self) {
        let locale = crate::enums::en_us();
        for function in self.manifest.functions() {
            let mut forms = vec![(function.id.clone(), MatchKind::OpySpelling)];
            if let Some(catalog_id) = &function.catalog_id {
                forms.push((catalog_id.clone(), MatchKind::CatalogId));
                let kind = match function.kind {
                    FunctionKind::Action | FunctionKind::MemberAction => Kind::Action,
                    FunctionKind::Value | FunctionKind::MemberValue => Kind::Value,
                };
                if let Some(entry) = self.catalog.entry(kind, catalog_id) {
                    if let Some(spelling) = entry.spelling(&locale) {
                        forms.push((spelling.to_string(), MatchKind::DisplayName));
                    }
                    for spelling in entry.spellings(&locale) {
                        forms.push((spelling.clone(), MatchKind::DisplayName));
                    }
                }
            }
            for alias in self.manifest.aliases() {
                if alias_target(alias, self.manifest)
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
                // An enum slot carries its domain's full member inventory:
                // which members a rendered signature shows is the caller's
                // (Wright's) policy, not the owner's.
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

    /// The catalog parameter index a manifest parameter occupies: member
    /// calls carry a receiver slot the manifest does not list, so they shift
    /// by one when the catalog side names a receiver first.
    fn catalog_param_index(
        &self,
        entry: Option<&workshop_rs::catalog::CatalogEntry>,
        function: &Function,
        param: &Param,
        index: usize,
    ) -> Option<usize> {
        let entry = entry?;
        let count = entry.param_count();
        let offset = if function.kind.is_member() && count == function.params.len() + 1 {
            1
        } else {
            0
        };
        if count == function.params.len() + offset {
            // Aligned declarations: the semantic name resolves the slot,
            // else position wins.
            let candidate = index + offset;
            match (0..count).position(|i| {
                entry
                    .param_name(i)
                    .is_some_and(|name| fold(name) == fold(&param.name))
            }) {
                Some(i) => Some(i),
                None => (candidate < count).then_some(candidate),
            }
        } else {
            (0..count).find(|&i| {
                entry
                    .param_name(i)
                    .is_some_and(|name| fold(name) == fold(&param.name))
            })
        }
    }

    // -- enums ------------------------------------------------------------

    fn index_enums(&mut self) {
        for domain in self.lookup_domains() {
            let members = crate::enums::domain_members(&domain, self.catalog);
            let mut domain_forms = vec![(domain.clone(), MatchKind::OpySpelling)];
            if let Some(catalog_domain) = self
                .catalog
                .enum_domain(crate::enums::catalog_domain(&domain))
            {
                domain_forms.push((catalog_domain.domain.clone(), MatchKind::CatalogId));
                let locale = crate::enums::en_us();
                if let Some(spelling) = catalog_domain.spelling(&locale) {
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
                    matched_on: MatchKind::OpySpelling,
                },
            });
            if let Some(members) = members {
                for member in &members {
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
                    self.candidates.push(Candidate {
                        forms,
                        hit: LookupHit::EnumMember {
                            spelling: format!("{domain}.{}", entry.spelling),
                            domain: domain.clone(),
                            member: entry.id.clone(),
                            display_name: entry.display_name.clone(),
                            aliases: entry.aliases.clone(),
                            matched_on: MatchKind::OpySpelling,
                        },
                    });
                }
            }
        }
    }

    /// Every enum domain the lookup reports: catalog domains under their
    /// reference source spelling (`Clipping` reports as `Clip`), plus
    /// source-level alias domains (`AsyncBehavior`) and manifest-only
    /// signature domains. Catalog domains that are not `Domain.MEMBER`
    /// sources in the reference (`EventTeam`, `Rounding`, …) are not
    /// reported — their members are spellings upstream rejects.
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

    // -- settings ---------------------------------------------------------

    fn index_settings(&mut self) {
        for definition in settings::definitions() {
            self.push_setting(definition.path(), &definition);
        }
        // Per-mode keys inherited from `gamemodes.general` answer to their
        // effective path: the table resolves them through lookup, so index
        // the same way emission does rather than re-declaring them.
        let modes: Vec<String> = settings::definitions()
            .filter_map(|definition| {
                let mut parts = definition.path().split('.');
                match (parts.next(), parts.next(), parts.next()) {
                    (Some("gamemodes"), Some(mode), Some(_)) if mode != "general" => {
                        Some(mode.to_string())
                    }
                    _ => None,
                }
            })
            .collect();
        let modes: Vec<String> = {
            let mut seen = Vec::new();
            for mode in modes {
                if !seen.contains(&mode) {
                    seen.push(mode);
                }
            }
            seen
        };
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
                        // The resolution is the inherited general entry.
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
}

/// The function an alias resolves to, through its declared kind.
fn alias_target<'a>(
    alias: &crate::manifest::Alias,
    manifest: &'a Manifest,
) -> Option<&'a Function> {
    match alias.kind {
        AliasKind::FunctionAlias => manifest.function(&alias.target),
        AliasKind::MemberAlias => manifest.member(&alias.target),
    }
}

/// Render a manifest `DomainMember` as a lookup member entry.
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

/// The value form of one settings table definition.
fn setting_value_form(definition: &settings::SettingDefinition) -> SettingValueForm {
    let (kind, min, max) = match definition.domain() {
        SettingValueDomain::PresenceOnly => (SettingValueKind::Presence, None, None),
        SettingValueDomain::Boolean => (SettingValueKind::Boolean, None, None),
        SettingValueDomain::Number(bounds) => {
            (SettingValueKind::Number, bounds.min(), bounds.max())
        }
        SettingValueDomain::Percent(bounds) => {
            (SettingValueKind::Percent, bounds.min(), bounds.max())
        }
        SettingValueDomain::String => (SettingValueKind::String, None, None),
        SettingValueDomain::Enum { .. } => (SettingValueKind::Enum, None, None),
        SettingValueDomain::MapList => (SettingValueKind::MapList, None, None),
        SettingValueDomain::HeroList => (SettingValueKind::HeroList, None, None),
        // `SettingValueDomain` is non-exhaustive upstream; a future variant
        // must be mapped here rather than silently re-typed.
        other => unreachable!("unhandled SettingValueDomain variant {other:?}"),
    };
    // A `BoolEnum` key is Boolean-typed over an enum domain and accepts
    // `true` only; a plain `Enum` key accepts its member ids.
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
        members,
        min,
        max,
    }
}

/// Render a parameter default in source syntax.
fn default_text(default: &ParamDefault, domain: Option<&str>, catalog: &Catalog) -> String {
    match default {
        ParamDefault::Null { .. } => "null".to_string(),
        ParamDefault::Bool(value) => value.to_string(),
        ParamDefault::Number(value) => {
            if value.fract() == 0.0 {
                format!("{}", *value as i64)
            } else {
                value.to_string()
            }
        }
        ParamDefault::Call { call, args } => {
            let args = args
                .iter()
                .map(|arg| {
                    if arg.fract() == 0.0 {
                        format!("{}", *arg as i64)
                    } else {
                        arg.to_string()
                    }
                })
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

    /// The lookup cannot claim a member spelling the lowerer would reject:
    /// every member and alias `domain_members` reports must resolve through
    /// `canonical_member`, the same function lowering calls.
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
                    // OPY-only `Color` members lower to `rgb(...)` calls
                    // through `extra_color_member` rather than a canonical
                    // member id.
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

    /// Every function spelling a lookup hit reports must resolve through
    /// the same manifest the lowerer resolves: lookup data is never a
    /// second table.
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
        // Special-call spellings must be exactly the names `lower_call`
        // dispatches on: `SPECIAL_VALUE_CALLS` is the descriptor the resolver
        // consults, so a dropped or renamed special form makes the reported
        // set disagree and fails here.
        let reported: Vec<&str> = SPECIAL_FUNCTIONS
            .iter()
            .map(|special| special.name)
            .collect();
        let dispatched: Vec<&str> = crate::lower::special_forms::SPECIAL_VALUE_CALLS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(
            reported, dispatched,
            "reported special-call spellings must be the dispatched set"
        );
        // Each reported special must resolve through the real lowerer: a
        // call in value position is `unknown-value` when the dispatch table
        // no longer recognizes the name.
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

    /// Every member alias the lookup reports must be an OPY spelling the
    /// pinned reference accepts — never a canonical catalog id
    /// (`Hero.SOLDIER_76` is a reference rejection, not an alias). The
    /// upstream-accepted extra spellings are the legacy hero names
    /// `MCCREE`/`HAMMOND` and the `HudPosition.ACTUALLY_LEFT` emit alias
    /// (overpy 9.7.10 rewrites all three in member access); `opy-compat
    /// probe-generate` emits a `:spelling` probe per reported spelling so
    /// the same set is checked against the oracle.
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
        assert_eq!(
            reported,
            ["Hero.HAMMOND", "Hero.MCCREE", "HudPosition.ACTUALLY_LEFT"]
        );
    }

    /// The diagnostic candidate pool is the same member list the lookup
    /// reports: `member_spellings` feeds `unknown-enum-member`.
    #[test]
    fn enum_member_candidates_match_lookup_members() {
        let catalog = Catalog::builtin().expect("bundled catalog");
        let reported: Vec<String> = crate::enums::domain_members("Clip", &catalog)
            .expect("Clip members")
            .iter()
            .map(|member| member.member.clone())
            .collect();
        assert_eq!(reported, crate::enums::member_spellings("Clip", &catalog));
    }

    /// Ranking puts exact and near matches first and bounds the list.
    #[test]
    fn rank_prefers_close_spellings() {
        let pool: Vec<String> = ["hudText", "smallMessage", "destroyIcon"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let ranked = rank("hudTex", &pool);
        assert_eq!(ranked.first().map(String::as_str), Some("hudText"));
        assert!(
            !ranked.iter().any(|candidate| candidate == "hudTex"),
            "the rejected spelling is not a candidate"
        );
    }

    /// The settings candidate walk finds sibling keys of an unknown member
    /// at a per-mode path (inherited `gamemodes.general` keys included).
    #[test]
    fn sibling_keys_cover_inherited_gamemode_keys() {
        let keys = sibling_keys(&[PathPart::Part("gamemodes"), PathPart::Part("ffa")]);
        assert!(
            keys.iter().any(|key| key == "scoreToWin"),
            "inherited general keys must be candidates: {keys:?}"
        );
    }
}
