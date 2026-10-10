//! OPY vocabulary lookup: display names and guesses resolve to OPY spellings,
//! parameter facts, enum members, and settings keys through the same manifest,
//! catalog, and settings tables `check`/`compile` resolve against.
//!
//! This module is the answer surface for agents; it owns no data of its own.
//! Callable spellings and parameter facts come from the compatibility manifest,
//! enum members, event entries, and display names come from the `workshop-rs`
//! catalog, and settings vocabulary comes from the `workshop-rs` emission
//! table (which it expands to every effective path, including inherited
//! per-mode keys).
//! The same scoring helpers feed the candidate lists unknown-name diagnostics
//! carry, so lookup results and rejection diagnostics cannot drift.

use serde::Serialize;
use std::sync::OnceLock;
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
    /// id, a Workshop display name, or a settings path prefix. Empty text
    /// applies no text constraint and lists the in-scope entries instead of
    /// scoring them.
    pub text: String,
    /// The namespaces the query searches; `LookupScope::ALL` is the default.
    /// `within` selects its own namespace when set.
    pub scope: LookupScope,
    /// Restricts the answer to the children of one named scope instead of
    /// searching every candidate in `scope`.
    pub within: Option<LookupWithin>,
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
            within: None,
            locale: None,
            limit: 0,
        }
    }
}

/// The namespaces a query searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupScope {
    /// Callable spellings and parameter facts.
    pub functions: bool,
    /// Enum domains and members.
    pub enums: bool,
    /// Settings keys and value forms.
    pub settings: bool,
    /// Rule events the `@Event` header accepts.
    pub events: bool,
}

impl LookupScope {
    /// Every namespace.
    pub const ALL: LookupScope = LookupScope {
        functions: true,
        enums: true,
        settings: true,
        events: true,
    };
    /// Callables only.
    pub const FUNCTIONS: LookupScope = LookupScope {
        functions: true,
        enums: false,
        settings: false,
        events: false,
    };
    /// Enum domains and members only.
    pub const ENUMS: LookupScope = LookupScope {
        functions: false,
        enums: true,
        settings: false,
        events: false,
    };
    /// Settings only.
    pub const SETTINGS: LookupScope = LookupScope {
        functions: false,
        enums: false,
        settings: true,
        events: false,
    };
    /// `@Event` spellings only.
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

/// A named scope whose children a lookup lists instead of searching every
/// namespace. `text` still filters or ranks the children; each variant names
/// the scope in the vocabulary's own terms.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum LookupWithin {
    /// The parameters of the callable with this OPY spelling, in call order.
    Callable(String),
    /// The member entries of this OPY enum domain, in domain order.
    Enum(String),
    /// The settings keys and intermediate path segments directly below this
    /// settings path prefix; an empty value lists the root.
    Settings(String),
    /// The accepted member spellings of a settings enum domain.
    SettingEnum(String),
}

impl LookupWithin {
    /// The namespace this scope's children belong to.
    fn scope(&self) -> LookupScope {
        match self {
            LookupWithin::Callable(_) => LookupScope::FUNCTIONS,
            LookupWithin::Enum(_) | LookupWithin::SettingEnum(_) => LookupScope::ENUMS,
            LookupWithin::Settings(_) => LookupScope::SETTINGS,
        }
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
    /// The `within` selector names no scope the vocabulary knows; the
    /// request carried no matches because the scope itself does not exist.
    UnknownWithin {
        /// The selector that named nothing.
        within: LookupWithin,
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
            LookupOutcome::UnknownWithin { .. } | LookupOutcome::Unsupported { .. } => &[],
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
        /// The English display name the catalog records, when linked.
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
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
        /// The English display name the catalog records, when linked.
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
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
    /// One parameter of a callable, listed inside a `LookupWithin::Callable`
    /// scope.
    Parameter {
        /// The callable's OPY spelling.
        callable: String,
        /// The parameter facts.
        param: LookupParam,
        /// Which spelling form matched.
        matched_on: MatchKind,
    },
    /// An intermediate settings path segment, listed inside a
    /// `LookupWithin::Settings` scope. It names a scope, not a key.
    SettingPath {
        /// The settings path this node represents.
        path: String,
        /// Which spelling form matched.
        matched_on: MatchKind,
    },
    /// A rule event the `@Event` header accepts. For events the OPY
    /// spelling is also the canonical catalog id (`playerDied`,
    /// `global`, `eachPlayer`).
    Event {
        /// The `@Event` spelling.
        spelling: String,
        /// Whether the event accepts `@Team`/`@Hero`/`@Slot` filters;
        /// `global` events reject them.
        accepts_filters: bool,
        /// The English display name, when the catalog provides one.
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
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
    /// The settings enum domain an `enum` key accepts members of.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
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
    if let Some(locale) = &query.locale {
        if !locale.eq_ignore_ascii_case("en-US") {
            return unsupported(
                "localized display names are owned by workshop-rs; opy-rs \
                 resolves display names in the 'en-US' locale only",
            );
        }
    }
    let (manifest, catalog) = match (Manifest::builtin(), Catalog::builtin()) {
        (Ok(manifest), Ok(catalog)) => (manifest, catalog),
        _ => return unsupported("vocabulary data (manifest or catalog) is not bundled"),
    };
    let index = Index::new(manifest, &catalog);
    if let Some(within) = &query.within {
        let Some(children) = index.children(within) else {
            return LookupOutcome::UnknownWithin {
                within: within.clone(),
            };
        };
        let mut results = index.rank(children.iter(), query);
        results.truncate(limit);
        return LookupOutcome::Matched {
            scope: within.scope(),
            limit,
            results,
        };
    }
    if !query.scope.any() {
        return unsupported("the query scope selects no namespace");
    }
    let mut results = index.rank(
        index
            .candidates
            .iter()
            .filter(|candidate| index.in_scope(candidate, scope)),
        query,
    );
    results.truncate(limit);
    LookupOutcome::Matched {
        scope,
        limit,
        results,
    }
}

/// Lookup with a default query (all namespaces, `DEFAULT_LIMIT`).
pub fn lookup_str(text: &str) -> LookupOutcome {
    lookup(&LookupQuery::new(text))
}

// ---------------------------------------------------------------------------
// lpp/resolveIds — canonical Workshop id → OPY spelling (issue #515)
// ---------------------------------------------------------------------------

/// Canonical call ids whose OPY spelling is source syntax rather than a
/// manifest-declared callable name: every entry inverts an accepted OPY
/// surface form (`compiler::lowering` emits the canonical id from this
/// spelling). Several spellings may lower to one canonical id; the table
/// records the dedicated form (`x += v` sugar aside, `appendToArray` is
/// spelled `append`, the `chase*` player calls `chaseAtRate`/
/// `chaseOverTime`).
///
/// Canonical ids absent here and unclaimed by a manifest `catalogId` have
/// no dedicated OPY spelling and stay unresolved: assignments and member
/// writes (`setGlobalVariable`, `assignMember`), declared names
/// (`globalVariable`, `playerVariable`, `callSubroutine`, `memberAccess`),
/// the dedent marker (`end`), literal, index, member-access, and ternary
/// forms (`array`, `emptyArray`, `valueInArray`, `firstOf`, `ifThenElse`),
/// lambda binders (`currentArrayElement`, `currentArrayIndex`), compound
/// conditionals (`abortIf`, `loopIf`, `skipIf`, `loopIfConditionIsTrue`,
/// `__abortIfConditionIsTrue__`), and spellings an argument selects
/// (`roundToInteger`).
const SYNTAX_SPELLINGS: &[(&str, &str)] = &[
    // Operators the lowerer emits for OPY operator syntax.
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
    // Member calls and special forms the manifest marks `special-lowering`
    // (it claims no `catalogId`, but each canonical id still has exactly
    // one dedicated OPY spelling).
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
    // Statements and reserved context values.
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
    // Canonical direction constants the language spells as `Vector` enum
    // members rather than bare calls.
    ("up", "Vector.UP"),
    ("down", "Vector.DOWN"),
    ("left", "Vector.LEFT"),
    ("right", "Vector.RIGHT"),
    ("forward", "Vector.FORWARD"),
    ("backward", "Vector.BACKWARD"),
    // The variable-kind target the manifest cannot claim for one spelling.
    ("chasePlayerVariableAtRate", "chaseAtRate"),
    ("chasePlayerVariableOverTime", "chaseOverTime"),
    ("stopChasingPlayerVariable", "stopChasingVariable"),
];

/// The bundled catalog, loaded once — `Catalog::builtin` reparses the
/// data on every call, which a `resolveIds` request paying per id cannot
/// afford.
fn resolve_catalog() -> Option<&'static Catalog> {
    static CATALOG: OnceLock<Option<Catalog>> = OnceLock::new();
    CATALOG.get_or_init(|| Catalog::builtin().ok()).as_ref()
}

/// `lpp/resolveIds` backing: the OPY spelling a canonical Workshop action,
/// value, or operator id compiles from — the manifest `catalogId` link for
/// declared callables, else [`SYNTAX_SPELLINGS`] for the ids source syntax
/// produces. `None` when no dedicated OPY spelling exists or the id is
/// unknown.
pub fn call_spelling(canonical_id: &str) -> Option<&'static str> {
    let manifest = Manifest::builtin().ok()?;
    let catalog = resolve_catalog()?;
    // A manifest entry's canonical identity is its `catalogId`, or — when
    // it declares none — its own `id` when that id is a canonical call
    // (`startRule`); a `catalogId`-free source name that is not a canonical
    // id (`log`, `all`) claims nothing.
    let is_canonical = |id: &str| {
        catalog.entry(Kind::Value, id).is_some() || catalog.entry(Kind::Action, id).is_some()
    };
    // Distinct spellings may share one canonical id (`getPlayers` and
    // `getAllPlayers` both claim `allPlayers`): prefer the entry whose own
    // id is the canonical name, matching `resolve_call_entry` in
    // `compiler::reconstruct`.
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

/// `lpp/resolveIds` backing: the OPY member spelling for a canonical enum
/// `domain`/`member` pair — the member half of the `Domain.MEMBER`
/// spelling `lpp/lookup` reports. `None` for unknown pairs, for domains
/// and members the reference does not spell in source, and for OPY-only
/// members with no canonical id.
pub fn enum_member_spelling<'a>(domain: &'a str, member: &'a str) -> Option<&'a str> {
    let catalog = resolve_catalog()?;
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
            LookupHit::Parameter {
                callable, param, ..
            } => {
                format!("4{callable}.{}", param.name)
            }
            LookupHit::SettingPath { path, .. } => format!("5{path}"),
            LookupHit::Event { spelling, .. } => format!("6{spelling}"),
        }
    }

    /// Restamp `matched_on` for a listed answer: no text constraint scored,
    /// so the entry's own primary form is what it answers to.
    fn listed(self) -> LookupHit {
        let kind = match &self {
            LookupHit::Setting { .. } | LookupHit::SettingPath { .. } => MatchKind::Path,
            _ => MatchKind::OpySpelling,
        };
        self.with_match_kind(kind, 0, false)
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
                display_name,
                ..
            } => LookupHit::Function {
                spelling,
                function_kind,
                receiver,
                unbounded,
                params,
                catalog_id,
                display_name,
                matched_on,
            },
            LookupHit::EnumDomain {
                domain,
                members,
                display_name,
                ..
            } => LookupHit::EnumDomain {
                domain,
                members,
                display_name,
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
            LookupHit::Parameter {
                callable, param, ..
            } => LookupHit::Parameter {
                callable,
                param,
                matched_on,
            },
            LookupHit::SettingPath { path, .. } => LookupHit::SettingPath { path, matched_on },
            LookupHit::Event {
                spelling,
                accepts_filters,
                display_name,
                ..
            } => LookupHit::Event {
                spelling,
                accepts_filters,
                display_name,
                matched_on,
            },
        }
    }
}

/// One searchable entry: every spelling form it answers to plus the payload
/// returned when one matches.
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

    /// Rank `candidates` under `query`: a non-empty text scores each
    /// candidate's best-matching form through the shared matcher pipeline,
    /// while an empty text lists the candidates in their construction order.
    fn rank<'c>(
        &self,
        candidates: impl Iterator<Item = &'c Candidate>,
        query: &LookupQuery,
    ) -> Vec<LookupHit> {
        if query.text.trim().is_empty() {
            return candidates
                .map(|candidate| candidate.hit.clone().listed())
                .collect();
        }
        let mut hits: Vec<ScoredHit> = candidates
            .filter_map(|candidate| {
                candidate
                    .forms
                    .iter()
                    .filter_map(|(form, kind)| {
                        // Settings paths match segment by segment: structure,
                        // template segments and `%` suffixes stay meaningful
                        // for exact/path-prefix queries; the folded `score`
                        // only serves near-name ranking. Both scorers live in
                        // `crate::matcher` — the same pipeline that produces
                        // the `did you mean` candidates on rejected names.
                        let (scored, structural) = if *kind == MatchKind::Path {
                            match crate::matcher::path_score(&query.text, form) {
                                Some(s) => (Some(s), true),
                                None => (crate::matcher::score(&query.text, form), false),
                            }
                        } else {
                            (crate::matcher::score(&query.text, form), false)
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
            .collect();
        hits.sort_by(|a, b| {
            a.score
                .cmp(&b.score)
                .then_with(|| a.hit.sort_key().cmp(&b.hit.sort_key()))
        });
        hits.into_iter().map(|scored| scored.hit).collect()
    }

    fn in_scope(&self, candidate: &Candidate, scope: LookupScope) -> bool {
        match &candidate.hit {
            LookupHit::Function { .. } | LookupHit::Parameter { .. } => scope.functions,
            LookupHit::EnumDomain { .. } | LookupHit::EnumMember { .. } => scope.enums,
            LookupHit::Setting { .. } | LookupHit::SettingPath { .. } => scope.settings,
            LookupHit::Event { .. } => scope.events,
        }
    }

    // -- within scopes -----------------------------------------------------

    /// The child candidates of one `within` scope, or `None` when `within`
    /// names no scope the index knows.
    fn children(&self, within: &LookupWithin) -> Option<Vec<Candidate>> {
        match within {
            LookupWithin::Callable(name) => self.callable_children(name),
            LookupWithin::Enum(domain) => self.enum_children(domain),
            LookupWithin::Settings(prefix) => self.settings_children(prefix),
            LookupWithin::SettingEnum(domain) => self.setting_enum_children(domain),
        }
    }

    /// One candidate per declared parameter of the callable `name` (its OPY
    /// spelling), in call order.
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

    /// One candidate per member of the enum domain `domain`, in domain
    /// order. A manifest-only domain is an existing but empty scope.
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

    /// The settings keys and intermediate path segments directly below
    /// `prefix`, in settings-table order; an empty prefix lists the root.
    /// `None` when `prefix` is not a proper segment-wise prefix of any
    /// settings path.
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

    /// One candidate per accepted member of a settings enum domain, in
    /// table order. `None` when `domain` is not a settings enum domain.
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

    // -- functions --------------------------------------------------------

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
                    if let Some(spelling) = entry.spelling(&locale) {
                        display_name = Some(spelling.to_string());
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
                entry.param_name(i).is_some_and(|name| {
                    crate::matcher::fold(name) == crate::matcher::fold(&param.name)
                })
            }) {
                Some(i) => Some(i),
                None => (candidate < count).then_some(candidate),
            }
        } else {
            (0..count).find(|&i| {
                entry.param_name(i).is_some_and(|name| {
                    crate::matcher::fold(name) == crate::matcher::fold(&param.name)
                })
            })
        }
    }

    // -- enums ------------------------------------------------------------

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

    /// The member candidate the enum domain index and a `within: enum`
    /// scope share, so a member answers to the same forms in both.
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

    /// One candidate per event the `@Event` header accepts. The
    /// acceptance set is the lowerer's: `global`, `eachPlayer`, and the
    /// `player_event_kind` names — the catalog's `subroutine` event is
    /// not an `@Event` name (subroutine rules use `def`), so it is
    /// excluded. `global` rejects player filters; the others accept
    /// `@Team`/`@Hero`/`@Slot`.
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
        domain,
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

    /// Ranking goes through the shared `crate::matcher` pipeline: exact and
    /// near matches come first, the rejected spelling is not a candidate.
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

    /// The settings candidate walk finds sibling keys of an unknown member
    /// at a per-mode path (inherited `gamemodes.general` keys included).
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

    /// A `within: callable` scope lists the callable's declared parameters
    /// as `Parameter` hits in call order, not ranked text matches.
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

    /// A `within: enum` scope lists the domain's member spellings in domain
    /// order; a `within: settings` scope lists immediate children — leaf
    /// keys as `Setting` hits, intermediate segments as `SettingPath` hits —
    /// and an empty prefix lists the root.
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

    /// A settings enum domain (`lobby.mapRotation` accepts `afterAGame`…)
    /// answers under `LookupWithin::SettingEnum` with the member spellings
    /// the settings table declares; it is not an OPY enum domain.
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

    /// An empty text lists the scope's children in construction order with
    /// each hit's primary match kind stamped; it is not a `Near` ranking.
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

    /// `call_spelling` reports a spelling for every canonical call id a
    /// manifest entry claims, and the reported spelling is one of the
    /// claiming entries — never a canonical id itself when the language
    /// spells it differently (`createHudText` -> `hudText`).
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

    /// `SYNTAX_SPELLINGS` entries resolve, and ids that are not canonical
    /// calls — source-only spellings like `evalOnce` or `all` — resolve
    /// nothing.
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

    /// `enum_member_spelling` reports the member half of the same
    /// `Domain.MEMBER` spelling `lpp/lookup` reports — renamed domains and
    /// members included — and nothing for unknown or non-source pairs.
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
}
