//! The manifest is language-compatibility metadata, not runtime content data
//! (issue #96 stays deferred), and it is Wright-authored data validated
//! against observed oracle behavior — never a mechanical conversion of
//! OverPy's GPL-3.0 data files (ADR-0004, `docs/licensing.md` in the wright
//! repository).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub const MANIFEST_DATA: &str = include_str!("data/manifest.json");

pub const PROBES_DATA: &str = include_str!("probes/probes.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub version: String,
    #[serde(rename = "contentCommit")]
    pub content_commit: String,
    pub integrity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub generator: String,
    pub license: String,
    pub reviewed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FunctionKind {
    Action,
    Value,
    MemberAction,
    MemberValue,
}

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
    pub fn is_action(self) -> bool {
        matches!(self, FunctionKind::Action | FunctionKind::MemberAction)
    }

    pub fn is_value(self) -> bool {
        matches!(self, FunctionKind::Value | FunctionKind::MemberValue)
    }

    pub fn is_member(self) -> bool {
        matches!(self, FunctionKind::MemberAction | FunctionKind::MemberValue)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ReceiverCategory {
    Player,
    Variable,
    String,
    Vector,
    Any,
}

impl ReceiverCategory {
    pub fn describe(self) -> &'static str {
        match self {
            ReceiverCategory::Player => "a player-valued expression",
            ReceiverCategory::Variable => "an assignable variable",
            ReceiverCategory::String => "a string literal",
            ReceiverCategory::Vector => "a vector-valued expression",
            ReceiverCategory::Any => "any expression",
        }
    }

    pub fn enforced(self) -> Option<Self> {
        matches!(self, ReceiverCategory::String | ReceiverCategory::Variable).then_some(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamDefault {
    Call {
        call: String,
        #[serde(default)]
        args: Vec<f64>,
    },
    Null {
        null: bool,
    },
    EnumMember(String),
    Bool(bool),
    Number(f64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub name: String,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub default: Option<ParamDefault>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub keyword_only: bool,
    #[serde(default)]
    pub positional_only: bool,
    #[serde(default)]
    pub alternate_names: Vec<String>,
    #[serde(default)]
    pub variable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Function {
    pub id: String,
    pub kind: FunctionKind,
    #[serde(default)]
    pub receiver: Option<ReceiverCategory>,
    #[serde(default)]
    pub params: Vec<Param>,
    #[serde(default)]
    pub unbounded: bool,
    #[serde(default = "default_keyword_args")]
    pub keyword_args: bool,
    #[serde(default)]
    #[serde(rename = "catalogId")]
    pub catalog_id: Option<String>,
    #[serde(default)]
    pub catalog_link: CatalogLink,
    #[serde(default)]
    pub evidence: Vec<String>,
}

impl Function {
    pub fn arity_bounds(&self) -> (usize, Option<usize>) {
        if self.unbounded {
            return (0, None);
        }
        let min = self
            .params
            .iter()
            .rposition(|param| param.default.is_none() && !param.optional)
            .map_or(0, |index| index + 1);
        (min, Some(self.params.len()))
    }

    pub fn variable_param_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.params
            .iter()
            .enumerate()
            .filter(|(_, param)| param.variable)
            .map(|(index, _)| index)
    }
}

fn default_keyword_args() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alias {
    pub source: String,
    pub target: String,
    pub kind: AliasKind,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AliasKind {
    FunctionAlias,
    MemberAlias,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError(pub String);

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ManifestError {}

/// The validated OPY semantic compatibility manifest.
///
/// A successfully loaded `Manifest` is immutable validated state
/// ([#455](https://github.com/wrightkit/opy-rs/issues/455)): [`Manifest::load`]
/// is the validation boundary, and the lookup tables are built from the
/// validated inventory. Callers inspect the data through read-only accessors;
/// mutating inventory or validation-covered state in place is not part of the
/// API, so cached resolution cannot desynchronize from it.
///
/// ```compile_fail
/// let mut manifest = opy_rs::manifest::Manifest::builtin().unwrap().clone();
/// manifest.functions.clear(); // field is private: post-load mutation is rejected
/// ```
#[derive(Debug, Clone)]
pub struct Manifest {
    schema_version: u32,
    reference: Reference,
    functions: Vec<Function>,
    aliases: Vec<Alias>,
    provenance: Provenance,
    probes: Vec<Probe>,
    by_function: HashMap<String, usize>,
    by_member: HashMap<String, usize>,
    alias_by_source: HashMap<String, usize>,
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
        let ManifestFile {
            schema_version,
            reference,
            functions,
            aliases,
            provenance,
        } = file;
        let mut manifest = Manifest {
            schema_version,
            reference,
            functions: Vec::with_capacity(functions.len()),
            aliases: Vec::with_capacity(aliases.len()),
            provenance,
            probes: probes_file.probes,
            by_function: HashMap::new(),
            by_member: HashMap::new(),
            alias_by_source: HashMap::new(),
            domain_identities: HashSet::new(),
        };
        manifest.validate(functions, aliases)?;
        Ok(manifest)
    }

    fn validate(
        &mut self,
        functions: Vec<Function>,
        aliases: Vec<Alias>,
    ) -> Result<(), ManifestError> {
        let mut probes: HashMap<&str, &Probe> = HashMap::new();
        for probe in &self.probes {
            if probes.insert(&probe.id, probe).is_some() {
                return Err(ManifestError(format!("duplicate probe id '{}'", probe.id)));
            }
        }

        for function in functions {
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
                    let is_contextual = crate::lower::policy::is_contextual_domain(domain);
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
            crate::lower::policy::validate(&function).map_err(ManifestError)?;
            if let Some(contextual) = crate::lower::policy::contextual_domain(&function.id) {
                for option in contextual.options {
                    self.domain_identities.insert(option.domain.to_string());
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
            self.functions.push(function);
        }

        for alias in aliases {
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
            self.aliases.push(alias);
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

    pub fn builtin() -> Result<&'static Manifest, ManifestError> {
        static MANIFEST: OnceLock<Result<Manifest, ManifestError>> = OnceLock::new();
        MANIFEST
            .get_or_init(|| Manifest::load(MANIFEST_DATA, PROBES_DATA))
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn reference(&self) -> &Reference {
        &self.reference
    }

    pub fn functions(&self) -> &[Function] {
        &self.functions
    }

    pub fn aliases(&self) -> &[Alias] {
        &self.aliases
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn probes(&self) -> &[Probe] {
        &self.probes
    }

    fn resolve_alias(&self, name: &str, kind: AliasKind) -> Option<&str> {
        let alias = &self.aliases[*self.alias_by_source.get(name)?];
        (alias.kind == kind).then_some(alias.target.as_str())
    }

    pub fn resolve_function(&self, name: &str) -> Option<&Function> {
        self.function(name).or_else(|| {
            self.resolve_alias(name, AliasKind::FunctionAlias)
                .and_then(|target| self.function(target))
        })
    }

    pub fn resolve_member(&self, name: &str) -> Option<&Function> {
        self.member(name).or_else(|| {
            self.resolve_alias(name, AliasKind::MemberAlias)
                .and_then(|target| self.member(target))
        })
    }

    pub fn function(&self, id: &str) -> Option<&Function> {
        self.by_function.get(id).map(|i| &self.functions[*i])
    }

    pub fn member(&self, id: &str) -> Option<&Function> {
        self.by_member.get(id).map(|i| &self.functions[*i])
    }

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
        assert_eq!(manifest.schema_version(), 1);
        assert_eq!(manifest.reference().name, "overpy");
        assert_eq!(manifest.reference().version, "9.7.10");
        assert_eq!(
            manifest.reference().content_commit,
            "889d9749d1def17f146548cbddb94ea1ab015847"
        );
        assert!(!manifest.functions().is_empty());
        assert!(!manifest.aliases().is_empty());
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
            CatalogLink::Canonical
        );
        assert!(
            !manifest.domain_identity("ChaseReeval"),
            "contextual domains are not standalone identities"
        );
        for function in manifest.functions() {
            assert!(!function.evidence.is_empty(), "{}", function.id);
            if function.kind.is_member() {
                assert!(function.receiver.is_some(), "{}", function.id);
            }
        }
    }

    #[test]
    fn manifest_data_is_canonical() {
        let canonical = canonicalize(MANIFEST_DATA, PROBES_DATA).expect("canonicalizes");
        assert_eq!(canonical, MANIFEST_DATA, "manifest.json must be canonical");
        assert_eq!(
            canonicalize(&canonical, PROBES_DATA).expect("re-canonicalizes"),
            canonical
        );
    }

    #[test]
    fn manifest_omits_contextual_lowering_policy() {
        assert!(!MANIFEST_DATA.contains("\"contextualDomain\""));
        assert!(!MANIFEST_DATA.contains("\"context\": \"forIterable\""));
    }

    #[test]
    fn validation_rejects_duplicates_and_missing_evidence() {
        fn mutate(mutate: impl FnOnce(&mut ManifestFile)) -> Result<Manifest, ManifestError> {
            let mut file: ManifestFile = serde_json::from_str(MANIFEST_DATA).unwrap();
            mutate(&mut file);
            Manifest::load(&serde_json::to_string(&file).unwrap(), PROBES_DATA)
        }
        let error = mutate(|file| file.functions.push(file.functions[0].clone()))
            .expect_err("duplicate function id must fail");
        assert!(error.0.contains("duplicate function id"));
        let error = mutate(|file| file.functions[0].catalog_link = CatalogLink::CatalogGap)
            .expect_err("canonical catalog id must not carry a gap reason");
        assert!(error.0.contains("catalogLink"));
        let error = mutate(|file| file.functions[0].evidence.clear())
            .expect_err("missing evidence must fail");
        assert!(error.0.contains("no oracle probe evidence"));
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
        assert_eq!(
            crate::lower::policy::function_context(&range.id),
            Some(crate::lower::policy::FunctionContext::ForIterable)
        );
    }

    #[test]
    fn aliases_resolve_to_declared_targets() {
        let manifest = Manifest::builtin().expect("builtin");
        let alias = manifest
            .resolve_function("stopChasingVariable")
            .expect("alias");
        assert_eq!(alias.id, "stopChasingVariable");
        assert!(alias.kind.is_action());
        let member = manifest.resolve_member("getCurrentHero").expect("alias");
        assert_eq!(member.id, "getHero");
        assert!(member.kind.is_value());
        assert!(manifest.resolve_function("frobnicate").is_none());
        assert!(manifest.resolve_member("frobnicate").is_none());
    }

    #[test]
    fn canonical_ids_are_not_source_spellings() {
        let manifest = Manifest::builtin().expect("builtin");
        for (canonical, upstream, catalog_id) in [
            ("evaluateOnce", "evalOnce", "evaluateOnce"),
            (
                "lastCreatedEntity",
                "getLastCreatedEntity",
                "lastCreatedEntity",
            ),
            ("lastTextId", "getLastCreatedText", "lastTextId"),
            ("allTankHeroes", "getTankHeroes", "allTankHeroes"),
            ("allDamageHeroes", "getDamageHeroes", "allDamageHeroes"),
            ("allSupportHeroes", "getSupportHeroes", "allSupportHeroes"),
            (
                "destroyAllHudText",
                "destroyAllHudTexts",
                "destroyAllHudText",
            ),
        ] {
            assert!(
                manifest.resolve_function(canonical).is_none(),
                "{canonical} must not be a callable source spelling"
            );
            let entry = manifest
                .resolve_function(upstream)
                .unwrap_or_else(|| panic!("{upstream} must resolve"));
            assert_eq!(
                entry.catalog_id.as_deref(),
                Some(catalog_id),
                "{upstream} must keep the canonical identity"
            );
        }
        assert!(manifest.resolve_member("isButtonHeld").is_none());
        let held = manifest
            .resolve_member("isHoldingButton")
            .expect("isHoldingButton");
        assert_eq!(held.catalog_id.as_deref(), Some("isButtonHeld"));
        for axis in ["x", "y", "z"] {
            assert!(manifest.resolve_member(axis).is_none(), "{axis}()");
        }
    }
}
