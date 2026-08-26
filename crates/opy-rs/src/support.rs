//! Compatibility support-matrix accessor (read-only).
//!
//! Loads the packaged `support-matrix.json` (copied from the evidence
//! workstream) and exposes feature-state queries by id and category, so
//! consumers and the CLI can report OPY support status without duplicating
//! the matrix.
//!
//! The matrix is embedded at build time via [`include_str!`] rather than
//! loaded at runtime: the shipped artifact always carries the exact matrix CI
//! validated, no runtime file lookup or dependency is required, and cargo
//! rebuilds the crate automatically when the file changes. The module is a
//! strict read-only consumer — it never writes, rewrites, or caches a
//! modified copy of the matrix.
//!
//! The five declared feature states (`planned`, `source-supported`,
//! `semantic-supported`, `lowering-dependent`, `end-to-end-supported`) are
//! documented in the matrix itself; Workshop-dependent items stay
//! `lowering-dependent` and are never approximated here (repo ownership
//! boundary: see `AGENTS.md`).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The embedded support matrix shipped with this crate.
const SUPPORT_MATRIX_JSON: &str = include_str!("../support-matrix.json");

/// The matrix schema version this module understands.
pub const SUPPORT_MATRIX_SCHEMA_VERSION: u32 = 1;

/// The declared feature states (see the matrix's `states` map for wording).
pub const FEATURE_STATES: [&str; 5] = [
    "planned",
    "source-supported",
    "semantic-supported",
    "lowering-dependent",
    "end-to-end-supported",
];

/// The pinned reference identity recorded in the matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixReference {
    pub name: String,
    pub version: String,
    #[serde(rename = "contentCommit")]
    pub content_commit: String,
    pub integrity: String,
}

/// The snapshot provenance recorded in the matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixSnapshot {
    pub date: String,
    pub note: String,
}

/// One support-matrix feature entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feature {
    pub id: String,
    pub name: String,
    pub category: String,
    pub state: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

/// The matrix summary (counts by state and category).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixSummary {
    #[serde(rename = "byState")]
    pub by_state: BTreeMap<String, u32>,
    #[serde(rename = "byCategory")]
    pub by_category: BTreeMap<String, u32>,
}

/// The machine-readable support matrix, mirroring
/// `compatibility/support-matrix.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportMatrix {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub artifact: String,
    pub reference: MatrixReference,
    pub snapshot: MatrixSnapshot,
    /// The declared state domain: state name → description.
    pub states: BTreeMap<String, String>,
    pub categories: Vec<String>,
    pub features: Vec<Feature>,
    pub summary: MatrixSummary,
}

impl SupportMatrix {
    /// Load and validate a matrix payload. Rejects an unknown schema version;
    /// state/category consistency is additionally enforced by the evidence
    /// workstream's harness test suite.
    fn load(data: &str) -> Result<SupportMatrix, SupportMatrixError> {
        let matrix: SupportMatrix = serde_json::from_str(data)
            .map_err(|error| SupportMatrixError(format!("invalid support matrix JSON: {error}")))?;
        if matrix.schema_version != SUPPORT_MATRIX_SCHEMA_VERSION {
            return Err(SupportMatrixError(format!(
                "unsupported support-matrix schema version {} \
                 (this module understands v{SUPPORT_MATRIX_SCHEMA_VERSION})",
                matrix.schema_version
            )));
        }
        Ok(matrix)
    }

    /// The embedded support matrix, parsed once and cached.
    ///
    /// An embedded matrix that fails to parse or validate is a build/provenance
    /// error in this repository, so this is a hard error rather than a
    /// recoverable lookup failure.
    pub fn builtin() -> Result<&'static SupportMatrix, SupportMatrixError> {
        static MATRIX: OnceLock<Result<SupportMatrix, SupportMatrixError>> = OnceLock::new();
        MATRIX
            .get_or_init(|| SupportMatrix::load(SUPPORT_MATRIX_JSON))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The feature entry with `id`, when declared.
    pub fn feature(&self, id: &str) -> Option<&Feature> {
        self.features.iter().find(|feature| feature.id == id)
    }

    /// The support state of a feature id (e.g. `planned`,
    /// `semantic-supported`, `lowering-dependent`).
    pub fn feature_state(&self, id: &str) -> Option<&str> {
        self.feature(id).map(|feature| feature.state.as_str())
    }

    /// Every feature entry of a category (e.g. `syntax`, `semantics`).
    pub fn features_by_category(&self, category: &str) -> Vec<&Feature> {
        self.features
            .iter()
            .filter(|feature| feature.category == category)
            .collect()
    }

    /// Every feature entry in a state (e.g. `lowering-dependent`).
    pub fn features_by_state(&self, state: &str) -> Vec<&Feature> {
        self.features
            .iter()
            .filter(|feature| feature.state == state)
            .collect()
    }

    /// The declared categories.
    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    /// The declared state domain (state name → description).
    pub fn declared_states(&self) -> &BTreeMap<String, String> {
        &self.states
    }

    /// The feature-count summary by state and category.
    pub fn summary(&self) -> &MatrixSummary {
        &self.summary
    }
}

/// A support-matrix load failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportMatrixError(pub String);

impl fmt::Display for SupportMatrixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SupportMatrixError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_matrix_loads_and_validates() {
        let matrix = SupportMatrix::builtin().expect("embedded matrix must load");
        assert_eq!(matrix.schema_version, SUPPORT_MATRIX_SCHEMA_VERSION);
        assert_eq!(matrix.reference.name, "overpy");
        assert!(matrix.reference.version.contains('.'));
        assert!(!matrix.features.is_empty());
    }

    #[test]
    fn feature_lookup_by_id_and_state() {
        let matrix = SupportMatrix::builtin().unwrap();
        let lexing = matrix.feature("syntax/lexing").expect("declared feature");
        assert_eq!(lexing.category, "syntax");
        assert_eq!(lexing.state, "source-supported");
        assert_eq!(
            matrix.feature_state("syntax/lexing"),
            Some("source-supported")
        );
        assert_eq!(
            matrix.feature_state("compilation/workshop-lowering"),
            Some("lowering-dependent")
        );
        assert_eq!(matrix.feature("syntax/nope"), None);
        assert_eq!(matrix.feature_state("syntax/nope"), None);
    }

    #[test]
    fn category_and_state_filters_match_the_summary() {
        let matrix = SupportMatrix::builtin().unwrap();
        let syntax = matrix.features_by_category("syntax");
        assert_eq!(syntax.len(), 14);
        assert!(syntax.iter().all(|feature| feature.category == "syntax"));
        let lowering = matrix.features_by_state("lowering-dependent");
        assert_eq!(lowering.len(), 13);
        assert!(
            lowering
                .iter()
                .all(|feature| feature.state == "lowering-dependent")
        );
        assert_eq!(matrix.summary().by_state["planned"], 0);
        assert_eq!(matrix.summary().by_category["semantics"], 14);
        // Every feature id is unique.
        let mut ids: Vec<&str> = matrix
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), matrix.features.len());
    }

    #[test]
    fn declared_states_and_categories_are_exposed() {
        let matrix = SupportMatrix::builtin().unwrap();
        assert_eq!(matrix.declared_states().len(), FEATURE_STATES.len());
        for state in FEATURE_STATES {
            assert!(
                matrix.declared_states().contains_key(state),
                "declared state '{state}' missing from the matrix"
            );
        }
        assert!(matrix.categories().contains(&"syntax".to_string()));
        assert!(matrix.categories().contains(&"decompilation".to_string()));
    }

    #[test]
    fn unknown_schema_version_is_rejected() {
        let error = SupportMatrix::load(
            r#"{"schemaVersion": 99, "artifact": "x",
                "reference": {"name": "overpy", "version": "1", "contentCommit": "c", "integrity": "i"},
                "snapshot": {"date": "d", "note": "n"},
                "states": {}, "categories": [], "features": [],
                "summary": {"byState": {}, "byCategory": {}}}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("schema version"));
    }
}
