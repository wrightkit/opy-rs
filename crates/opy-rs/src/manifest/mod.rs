//! The OPY semantic compatibility manifest (issue #109).
//!
//! This module owns the Wright-authored, reference-validated semantic table
//! that the frontend resolves builtin names, member functions, receiver
//! categories, signatures/arity, parameter enum-domain identities, and
//! non-contextual source aliases against — the authoritative replacement for
//! the hardcoded `KNOWN_ENUMS` table and the semantic catalog-coverage gap
//! behind `unknown-action`/`unknown-value`/`unsupported-member` emission
//! failures.
//!
//! * The data lives in [`data/manifest.json`](data/manifest.json) (schema
//!   v1, per the compatibility-manifest spec).
//! * Every entry records the pinned-oracle probe that validates it
//!   (`probes/probes.json`); `probes/validate.py` runs each probe against the
//!   pinned OverPy 9.7.10 oracle and verifies accept/reject, emission hash,
//!   and diagnostic category deterministically.
//! * `catalogId` links each entry to the Workshop emission catalog by
//!   canonical identity without duplicating localization/output spelling
//!   data. The wright repository cross-checks every declared id against its
//!   emission catalog; opy-rs does not copy the catalog itself.
//!
//! Ownership boundary: the **function**, **alias**, and **module** tables are
//! OPY *source-language API* metadata (OverPy's documented language API;
//! Wright-authored, probe-validated) and are not Workshop content data.
//! Workshop *content/catalog* data — enum member lists, settings keys,
//! mode/team/hero/map names — is Workshop-owned and is not carried here:
//! `param.domain` and the contextual-domain machinery are catalog *identity*
//! links only (no member validation), and validation that would need the
//! canonical Workshop enum catalog is `lowering-dependent` (issue #8),
//! never approximated.
//!
//! The manifest is language-compatibility metadata, not runtime content data
//! (issue #96 stays deferred), and it is Wright-authored data validated
//! against observed oracle behavior — never a mechanical conversion of
//! OverPy's GPL-3.0 data files (ADR-0004, `docs/licensing.md` in the wright
//! repository).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The embedded schema-v1 manifest data.
pub const MANIFEST_DATA: &str = include_str!("data/manifest.json");

/// The embedded probe evidence record for the manifest data.
pub const PROBES_DATA: &str = include_str!("probes/probes.json");

/// The pinned reference identity the manifest data is validated against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub version: String,
    #[serde(rename = "contentCommit")]
    pub content_commit: String,
    pub integrity: String,
}

/// Provenance of the manifest data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub generator: String,
    pub license: String,
    pub reviewed: bool,
}

/// The kind of a builtin function entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FunctionKind {
    /// A generic action (`chaseOverTime(...)` as a statement).
    Action,
    /// A generic value (`isGameInProgress()` in an expression).
    Value,
    /// An action called on a receiver (`eventPlayer.setMoveSpeed(100)`).
    MemberAction,
    /// A value called on a receiver (`eventPlayer.isAlive()`).
    MemberValue,
}

/// How the frontend-owned function identity connects to Workshop lowering.
///
/// `canonical` entries carry a `catalogId`; the other variants are explicit
/// reasons why a source-level function does not have a direct catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogLink {
    #[default]
    Canonical,
    SpecialLowering,
    LegacyAlias,
    CatalogGap,
}

impl FunctionKind {
    /// Whether this kind is an action (statement-position builtin).
    pub fn is_action(self) -> bool {
        matches!(self, FunctionKind::Action | FunctionKind::MemberAction)
    }

    /// Whether this kind is a value (expression-position builtin).
    pub fn is_value(self) -> bool {
        matches!(self, FunctionKind::Value | FunctionKind::MemberValue)
    }

    /// Whether this kind is a receiver member function.
    pub fn is_member(self) -> bool {
        matches!(self, FunctionKind::MemberAction | FunctionKind::MemberValue)
    }
}

/// The declared receiver category of a member function.
///
/// `Player` is the metadata category for player-oriented members (the pinned
/// reference does not type-check those receivers, so the frontend does not
/// reject them); `Variable` and `String` are enforced where the reference
/// semantics are clear (`.append` requires an assignable receiver, `.format`
/// requires a string literal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ReceiverCategory {
    Player,
    Variable,
    String,
    Any,
}

impl ReceiverCategory {
    /// A human-readable description of the category for diagnostics.
    pub fn describe(self) -> &'static str {
        match self {
            ReceiverCategory::Player => "a player-valued expression",
            ReceiverCategory::Variable => "an assignable variable",
            ReceiverCategory::String => "a string literal",
            ReceiverCategory::Any => "any expression",
        }
    }
}

/// A parameter default that the frontend expands: a function call, enum
/// member (`"MEMBER"`), or scalar (`0.016`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamDefault {
    Call { call: String },
    EnumMember(String),
    Number(f64),
}

/// One ordered parameter of a function entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub name: String,
    /// The enum domain this parameter requires, when it is an enum argument.
    #[serde(default)]
    pub domain: Option<String>,
    /// An explicit default the frontend may expand; see [`ParamDefault`].
    #[serde(default)]
    pub default: Option<ParamDefault>,
    /// Whether the argument is omittable without an emitted expansion
    /// (`"optional": true`; the reference accepts the short form).
    #[serde(default)]
    pub optional: bool,
    /// Whether the argument must be passed as a keyword (`name = expr`):
    /// the reference `chase` form requires its 3rd argument to be
    /// `rate = ...` or `duration = ...` (issue #110).
    #[serde(default)]
    pub keyword_only: bool,
    /// Whether the argument can only be passed positionally (keyword
    /// binding is rejected): the reference `chase` form's leading arguments
    /// (issue #110).
    #[serde(default)]
    pub positional_only: bool,
    /// Additional accepted keyword spellings for this parameter (the
    /// reference `chase` form accepts both `rate` and `duration` for its
    /// 3rd argument).
    #[serde(default)]
    pub alternate_names: Vec<String>,
    /// Whether the argument must be a variable reference (a global variable
    /// or a player variable); the chase family requires a variable first
    /// argument to select the global/player emission form.
    #[serde(default)]
    pub variable: bool,
}

/// A call-context restriction on a function entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FunctionContext {
    /// Only valid as a `for ... in` iterable (`range`; the pinned reference
    /// rejects standalone `range` calls).
    ForIterable,
}

/// One contextual enum-domain selection: the `chase` dispatch (issue #110).
///
/// The reference `chase` form binds its 4th argument as a member of a
/// merged `ChaseReeval` domain that does not exist as a standalone enum:
/// the keyword name used for the `by` parameter selects the concrete domain
/// and the function the call lowers to (`rate` → `ChaseRateReeval` /
/// `chaseAtRate`, `duration` → `ChaseTimeReeval` / `chaseOverTime`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextualDomain {
    /// The contextual (merged) domain name; never resolvable outside the
    /// declaring function's signature context.
    pub domain: String,
    /// The parameter whose bound keyword name selects the option.
    pub by: String,
    /// The options keyed by the accepted keyword spellings of the `by`
    /// parameter.
    pub options: std::collections::BTreeMap<String, ContextualDomainOption>,
}

/// One contextual-domain option: the concrete enum domain and the function
/// name the call lowers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextualDomainOption {
    pub domain: String,
    pub target: String,
}

/// One builtin function entry (generic action/value or member function).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Function {
    pub id: String,
    pub kind: FunctionKind,
    /// The receiver category of member functions.
    #[serde(default)]
    pub receiver: Option<ReceiverCategory>,
    #[serde(default)]
    pub params: Vec<Param>,
    /// Whether the argument count is unbounded (`.format` placeholders).
    #[serde(default)]
    pub unbounded: bool,
    /// Whether keyword arguments are accepted (`name = expr`). Defaults to
    /// `true` (the reference's `parseArgs` applies to every workshop
    /// function); entries the reference routes around that mechanism
    /// (`range`, `random.*`, `.format`) declare `"keywordArgs": false`
    /// (issue #110).
    #[serde(default = "default_keyword_args")]
    pub keyword_args: bool,
    /// The contextual enum-domain dispatch (the `chase` form), when this
    /// entry has one.
    #[serde(default)]
    pub contextual_domain: Option<ContextualDomain>,
    #[serde(default)]
    pub context: Option<FunctionContext>,
    /// The canonical Workshop catalog id this entry emits through; absent
    /// when emission is special-cased or not yet catalog-covered.
    #[serde(default)]
    #[serde(rename = "catalogId")]
    pub catalog_id: Option<String>,
    /// The explicit reason a source-level function has no direct catalog id.
    #[serde(default)]
    pub catalog_link: CatalogLink,
    /// The probe ids that validate this entry against the pinned oracle.
    #[serde(default)]
    pub evidence: Vec<String>,
}

impl Function {
    /// The (minimum, maximum) argument count: the first parameter with a
    /// default makes every following parameter optional; `unbounded` entries
    /// accept any count.
    pub fn arity_bounds(&self) -> (usize, Option<usize>) {
        if self.unbounded {
            return (0, None);
        }
        let first_default = self
            .params
            .iter()
            .position(|param| param.default.is_some() || param.optional);
        let min = first_default.unwrap_or(self.params.len());
        (min, Some(self.params.len()))
    }
}

fn default_keyword_args() -> bool {
    true
}

/// A non-contextual source alias: a pure name rewrite to a declared entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alias {
    pub source: String,
    pub target: String,
    pub kind: AliasKind,
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// The alias target class; `functionAlias` targets a generic function,
/// `memberAlias` a member function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AliasKind {
    FunctionAlias,
    MemberAlias,
}

/// One recorded probe in the embedded evidence record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub id: String,
    pub source: String,
    pub sha256: String,
    pub expect: String,
    #[serde(default)]
    pub output_sha256: Option<String>,
    #[serde(default)]
    pub diagnostic_contains: Option<String>,
}

/// A validation failure while loading the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError(pub String);

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ManifestError {}

/// The validated OPY semantic compatibility manifest.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub schema_version: u32,
    pub reference: Reference,
    pub functions: Vec<Function>,
    pub aliases: Vec<Alias>,
    pub provenance: Provenance,
    /// The recorded probe evidence (`probes/probes.json`).
    pub probes: Vec<Probe>,
    by_function: HashMap<String, usize>,
    by_member: HashMap<String, usize>,
    alias_by_source: HashMap<String, usize>,
    /// The declared enum-domain identities: every `param.domain` and
    /// contextual option domain in the function table. Identity links only —
    /// member lists are Workshop-owned catalog content and are not carried
    /// here (lowering-dependent validation, #8).
    domain_identities: HashSet<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    schema_version: u32,
    reference: Reference,
    #[serde(default)]
    functions: Vec<Function>,
    #[serde(default)]
    aliases: Vec<Alias>,
    provenance: Provenance,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProbesFile {
    schema_version: u32,
    #[serde(default)]
    probes: Vec<Probe>,
}

impl Manifest {
    /// Parse and validate manifest data plus its probe evidence record.
    pub fn load(manifest_json: &str, probes_json: &str) -> Result<Manifest, ManifestError> {
        let file: ManifestFile = serde_json::from_str(manifest_json)
            .map_err(|error| ManifestError(format!("manifest data: {error}")))?;
        if file.schema_version != 1 {
            return Err(ManifestError(format!(
                "unsupported manifest schemaVersion {}",
                file.schema_version
            )));
        }
        let probes_file: ProbesFile = serde_json::from_str(probes_json)
            .map_err(|error| ManifestError(format!("probes data: {error}")))?;
        if probes_file.schema_version != 1 {
            return Err(ManifestError(format!(
                "unsupported probes schemaVersion {}",
                probes_file.schema_version
            )));
        }
        let mut manifest = Manifest {
            schema_version: file.schema_version,
            reference: file.reference.clone(),
            functions: Vec::new(),
            aliases: Vec::new(),
            provenance: file.provenance.clone(),
            probes: probes_file.probes,
            by_function: HashMap::new(),
            by_member: HashMap::new(),
            alias_by_source: HashMap::new(),
            domain_identities: HashSet::new(),
        };
        manifest.validate(file)?;
        Ok(manifest)
    }

    fn validate(&mut self, file: ManifestFile) -> Result<(), ManifestError> {
        // Probe ids must be unique and must record the accept probes the
        // entries reference.
        let mut probes: HashMap<&str, &Probe> = HashMap::new();
        for probe in &self.probes {
            if probes.insert(&probe.id, probe).is_some() {
                return Err(ManifestError(format!("duplicate probe id '{}'", probe.id)));
            }
        }

        // Functions: unique ids, member-only receiver/kind combinations,
        // declared enum domains, declared enum-default members, and probe
        // evidence that records acceptance.
        for function in &file.functions {
            if self.by_function.contains_key(&function.id) {
                return Err(ManifestError(format!(
                    "duplicate function id '{}'",
                    function.id
                )));
            }
            match function.kind {
                FunctionKind::MemberAction | FunctionKind::MemberValue => {
                    if function.receiver.is_none() {
                        return Err(ManifestError(format!(
                            "member function '{}' declares no receiver category",
                            function.id
                        )));
                    }
                }
                FunctionKind::Action | FunctionKind::Value => {
                    if function.receiver.is_some() {
                        return Err(ManifestError(format!(
                            "non-member function '{}' declares a receiver category",
                            function.id
                        )));
                    }
                }
            }
            for param in function.params.iter() {
                if let Some(domain) = &param.domain {
                    // A parameter may declare the function's own contextual
                    // domain (`chase`'s `ChaseReeval`): it resolves only in
                    // this signature's context and is not a standalone
                    // identity.
                    let is_contextual = function
                        .contextual_domain
                        .as_ref()
                        .is_some_and(|contextual| &contextual.domain == domain);
                    if !is_contextual {
                        self.domain_identities.insert(domain.clone());
                    }
                } else if matches!(param.default, Some(ParamDefault::EnumMember(_))) {
                    return Err(ManifestError(format!(
                        "function '{}' parameter '{}' has an enum-member default but no \
                         declared domain",
                        function.id, param.name
                    )));
                }
                if param.keyword_only && param.positional_only {
                    return Err(ManifestError(format!(
                        "function '{}' parameter '{}' cannot be both keyword-only and \
                         positional-only",
                        function.id, param.name
                    )));
                }
                for alternate in &param.alternate_names {
                    if alternate == &param.name {
                        return Err(ManifestError(format!(
                            "function '{}' parameter '{}' repeats its name as an \
                             alternate keyword spelling",
                            function.id, param.name
                        )));
                    }
                    if function.params.iter().any(|other| {
                        !std::ptr::eq(other, param)
                            && (&other.name == alternate
                                || other.alternate_names.contains(alternate))
                    }) {
                        return Err(ManifestError(format!(
                            "function '{}' alternate keyword spelling '{alternate}' \
                             collides with another parameter",
                            function.id
                        )));
                    }
                }
            }
            match (&function.catalog_id, function.catalog_link) {
                (Some(_), CatalogLink::Canonical)
                | (None, CatalogLink::SpecialLowering)
                | (None, CatalogLink::LegacyAlias)
                | (None, CatalogLink::CatalogGap) => {}
                (Some(id), link) => {
                    return Err(ManifestError(format!(
                        "function '{}' has catalogId '{id}' but catalogLink is {:?}",
                        function.id, link
                    )));
                }
                (None, CatalogLink::Canonical) => {
                    return Err(ManifestError(format!(
                        "function '{}' has no catalogId or explicit catalogLink reason",
                        function.id
                    )));
                }
            }
            if let Some(contextual) = &function.contextual_domain {
                let by_param = function
                    .params
                    .iter()
                    .find(|param| param.name == contextual.by)
                    .ok_or_else(|| {
                        ManifestError(format!(
                            "function '{}' contextual domain '{}' references unknown \
                             selector parameter '{}'",
                            function.id, contextual.domain, contextual.by
                        ))
                    })?;
                let contextual_param = function
                    .params
                    .iter()
                    .find(|param| param.domain.as_deref() == Some(contextual.domain.as_str()))
                    .ok_or_else(|| {
                        ManifestError(format!(
                            "function '{}' contextual domain '{}' has no parameter \
                             declaring that domain",
                            function.id, contextual.domain
                        ))
                    })?;
                let _ = contextual_param;
                let mut spellings = vec![by_param.name.clone()];
                spellings.extend(by_param.alternate_names.iter().cloned());
                for (keyword, option) in &contextual.options {
                    if !spellings.contains(keyword) {
                        return Err(ManifestError(format!(
                            "function '{}' contextual option '{keyword}' is not a \
                             keyword spelling of selector parameter '{}'",
                            function.id, by_param.name
                        )));
                    }
                    // The option's concrete domain is a catalog identity link
                    // (the domain the selected member/emission belongs to);
                    // member lists are not carried here.
                    self.domain_identities.insert(option.domain.clone());
                }
            }
            self.check_evidence(&function.id, &function.evidence, &probes)?;
            if function.kind.is_member() {
                self.by_member
                    .insert(function.id.clone(), self.functions.len());
            } else {
                self.by_function
                    .insert(function.id.clone(), self.functions.len());
            }
            self.functions.push(function.clone());
        }

        // Aliases: unique sources, declared targets of the matching class,
        // no collision with declared function ids.
        for alias in &file.aliases {
            if self.alias_by_source.contains_key(&alias.source) {
                return Err(ManifestError(format!(
                    "duplicate alias source '{}'",
                    alias.source
                )));
            }
            if self.by_function.contains_key(&alias.source)
                || self.by_member.contains_key(&alias.source)
            {
                return Err(ManifestError(format!(
                    "alias source '{}' collides with a declared function",
                    alias.source
                )));
            }
            match alias.kind {
                AliasKind::FunctionAlias => {
                    if self.function(&alias.target).is_none() {
                        return Err(ManifestError(format!(
                            "alias '{}' targets '{}' which is not a generic function",
                            alias.source, alias.target
                        )));
                    }
                }
                AliasKind::MemberAlias => {
                    if self.member(&alias.target).is_none() {
                        return Err(ManifestError(format!(
                            "alias '{}' targets '{}' which is not a member function",
                            alias.source, alias.target
                        )));
                    }
                }
            }
            self.check_evidence(&alias.source, &alias.evidence, &probes)?;
            self.alias_by_source
                .insert(alias.source.clone(), self.aliases.len());
            self.aliases.push(alias.clone());
        }

        Ok(())
    }

    fn check_evidence(
        &self,
        owner: &str,
        evidence: &[String],
        probes: &HashMap<&str, &Probe>,
    ) -> Result<(), ManifestError> {
        if evidence.is_empty() {
            return Err(ManifestError(format!(
                "entry '{owner}' records no oracle probe evidence"
            )));
        }
        for probe_id in evidence {
            let probe = probes.get(probe_id.as_str()).ok_or_else(|| {
                ManifestError(format!(
                    "entry '{owner}' references undeclared probe '{probe_id}'"
                ))
            })?;
            if probe.expect != "success" {
                return Err(ManifestError(format!(
                    "entry '{owner}' references probe '{probe_id}' which does not record \
                     oracle acceptance"
                )));
            }
        }
        Ok(())
    }

    /// The built-in manifest, loaded once from the embedded data.
    pub fn builtin() -> Result<&'static Manifest, ManifestError> {
        static MANIFEST: OnceLock<Result<Manifest, ManifestError>> = OnceLock::new();
        MANIFEST
            .get_or_init(|| Manifest::load(MANIFEST_DATA, PROBES_DATA))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// A generic (non-member) function by source name, alias-aware.
    pub fn resolve_function(&self, name: &str) -> Option<&Function> {
        self.function(name).or_else(|| {
            let alias = self.alias_by_source.get(name)?;
            let alias = &self.aliases[*alias];
            (alias.kind == AliasKind::FunctionAlias)
                .then(|| self.function(&alias.target))
                .flatten()
        })
    }

    /// A member function by source name, alias-aware.
    pub fn resolve_member(&self, name: &str) -> Option<&Function> {
        self.member(name).or_else(|| {
            let alias = self.alias_by_source.get(name)?;
            let alias = &self.aliases[*alias];
            (alias.kind == AliasKind::MemberAlias)
                .then(|| self.member(&alias.target))
                .flatten()
        })
    }

    /// The function entry with the given id, if declared.
    pub fn function(&self, id: &str) -> Option<&Function> {
        self.by_function.get(id).map(|i| &self.functions[*i])
    }

    /// The member function entry with the given id, if declared.
    pub fn member(&self, id: &str) -> Option<&Function> {
        self.by_member.get(id).map(|i| &self.functions[*i])
    }

    /// Whether the name is a declared enum-domain identity: a `param.domain`
    /// or contextual option domain in the function table. These are OPY
    /// signature metadata (catalog identity links); the domain *member
    /// lists* are Workshop-owned catalog content and are not carried here,
    /// so member validation is `lowering-dependent` (issue #8).
    pub fn domain_identity(&self, name: &str) -> bool {
        self.domain_identities.contains(name)
    }
}

/// Canonicalize manifest data: parse, validate, and re-serialize
/// deterministically (object keys sorted, stable formatting). Re-running on
/// the same input produces byte-identical output, so the data is
/// reproducible and the committed file must equal its canonical form.
pub fn canonicalize(manifest_json: &str, probes_json: &str) -> Result<String, ManifestError> {
    Manifest::load(manifest_json, probes_json)?;
    let value: serde_json::Value = serde_json::from_str(manifest_json)
        .map_err(|error| ManifestError(format!("manifest data: {error}")))?;
    serde_json::to_string_pretty(&value)
        .map(|mut out| {
            out.push('\n');
            out
        })
        .map_err(|error| ManifestError(format!("cannot serialize manifest: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_manifest_loads_and_validates() {
        let manifest = Manifest::builtin().expect("embedded manifest must validate");
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.reference.name, "overpy");
        assert_eq!(manifest.reference.version, "9.7.10");
        assert_eq!(
            manifest.reference.content_commit,
            "889d9749d1def17f146548cbddb94ea1ab015847"
        );
        assert!(!manifest.functions.is_empty());
        assert!(!manifest.aliases.is_empty());
        // Enum-domain *identities* come from the function signatures
        // (param.domain / contextual option domains); member lists are
        // Workshop-owned catalog content and are not carried here. Every
        // member entry declares a receiver; every entry has evidence.
        for domain in ["Invis", "ChaseTimeReeval", "Team", "LosCheck", "Color"] {
            assert!(manifest.domain_identity(domain), "{domain}");
        }
        assert_eq!(
            manifest
                .function("chase")
                .expect("chase entry")
                .catalog_link,
            CatalogLink::SpecialLowering
        );
        assert_eq!(
            manifest
                .member("getHero")
                .expect("getHero entry")
                .catalog_link,
            CatalogLink::CatalogGap
        );
        assert!(
            !manifest.domain_identity("ChaseReeval"),
            "contextual domains are not standalone identities"
        );
        for function in &manifest.functions {
            assert!(!function.evidence.is_empty(), "{}", function.id);
            if function.kind.is_member() {
                assert!(function.receiver.is_some(), "{}", function.id);
            }
        }
    }

    #[test]
    fn manifest_data_is_canonical() {
        // The committed data file must equal its deterministic canonical
        // rewrite (the `build` path), so the data pipeline is reproducible.
        let canonical = canonicalize(MANIFEST_DATA, PROBES_DATA).expect("canonicalizes");
        assert_eq!(canonical, MANIFEST_DATA, "manifest.json must be canonical");
        // Idempotency: re-canonicalizing the canonical form is byte-stable.
        assert_eq!(
            canonicalize(&canonical, PROBES_DATA).expect("re-canonicalizes"),
            canonical
        );
    }

    #[test]
    fn validation_rejects_duplicates_and_missing_evidence() {
        fn mutate(mutate: impl FnOnce(&mut ManifestFile)) -> Result<Manifest, ManifestError> {
            let mut file: ManifestFile = serde_json::from_str(MANIFEST_DATA).unwrap();
            mutate(&mut file);
            Manifest::load(&serde_json::to_string(&file).unwrap(), PROBES_DATA)
        }
        // duplicate function id
        let error = mutate(|file| file.functions.push(file.functions[0].clone()))
            .expect_err("duplicate function id must fail");
        assert!(error.0.contains("duplicate function id"));
        // A direct catalog link must be explicit about being canonical.
        let error = mutate(|file| file.functions[0].catalog_link = CatalogLink::CatalogGap)
            .expect_err("canonical catalog id must not carry a gap reason");
        assert!(error.0.contains("catalogLink"));
        // entry without evidence
        let error = mutate(|file| file.functions[0].evidence.clear())
            .expect_err("missing evidence must fail");
        assert!(error.0.contains("no oracle probe evidence"));
        // enum-member default without a declared domain is a data-integrity
        // error (the default cannot be expanded without an identity)
        let error = mutate(|file| {
            file.functions[0].params.push(Param {
                name: "bad".to_string(),
                domain: None,
                default: Some(ParamDefault::EnumMember("X".to_string())),
                optional: false,
                keyword_only: false,
                positional_only: false,
                alternate_names: Vec::new(),
                variable: false,
            })
        })
        .expect_err("enum default without a domain must fail");
        assert!(error.0.contains("no declared domain"));
    }

    #[test]
    fn arity_bounds_follow_defaults_and_unbounded() {
        let manifest = Manifest::builtin().expect("builtin");
        let chase = manifest.function("chaseOverTime").expect("entry");
        assert_eq!(chase.arity_bounds(), (3, Some(4)));
        let radius = manifest.function("getPlayersInRadius").expect("entry");
        assert_eq!(radius.arity_bounds(), (2, Some(4)));
        let status = manifest.member("setStatusEffect").expect("entry");
        assert_eq!(status.arity_bounds(), (3, Some(3)));
        let format = manifest.member("format").expect("entry");
        assert_eq!(format.arity_bounds(), (0, None));
        let range = manifest.function("range").expect("entry");
        assert_eq!(range.arity_bounds(), (1, Some(3)));
        assert_eq!(range.context, Some(FunctionContext::ForIterable));
    }

    #[test]
    fn aliases_resolve_to_declared_targets() {
        let manifest = Manifest::builtin().expect("builtin");
        let alias = manifest
            .resolve_function("stopChasingVariable")
            .expect("alias");
        assert_eq!(alias.id, "stopChasing");
        assert!(alias.kind.is_action());
        let member = manifest.resolve_member("getCurrentHero").expect("alias");
        assert_eq!(member.id, "getHero");
        assert!(member.kind.is_value());
        // Unknown names stay unresolved.
        assert!(manifest.resolve_function("frobnicate").is_none());
        assert!(manifest.resolve_member("frobnicate").is_none());
    }
}
