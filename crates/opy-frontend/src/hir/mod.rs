//! Opy HIR v1 — the OPY semantic model owned by `opy-rs`.
//!
//! The wire contract is the `wright/opy-hir` protocol, major version 1
//! (produced as `1.1.0`), specified in the WrightKit HIR protocol document
//! (`docs/hir/opy-hir-v1.md` in the wright repository; the opy-rs docs
//! workstream re-homes the specification). This module provides the serde
//! protocol types, envelope and structural validation, and a deterministic
//! debug dump.
//!
//! Ingestion order follows the spec (§8): envelope identity/version first,
//! then unknown-node-kind rejection, then deserialization, then invariant
//! validation. Every failure is a structured [`HirError`].

pub mod dump;
pub mod error;
pub mod types;
mod validate;

pub use error::HirError;
pub use types::{
    Annotation, AnnotationArg, Declaration, DirectiveRecord, DirectiveValue, Event, Expr,
    Generator, OptimizationState, Position, PreprocessingSnapshot, PreprocessingState, Program,
    Protocol, Rule, RuleEntry, Settings, SettingsListElement, SettingsNode, SourceFile, Span, Stmt,
    TranslationState, default_var_index,
};

use serde_json::Value;

/// Parse and validate an Opy HIR v1 payload from a JSON string.
///
/// Returns a structured [`HirError`] for malformed JSON, an unsupported
/// protocol identity or major version, unknown node kinds, or invariant
/// violations.
pub fn parse_str(input: &str) -> Result<Program, HirError> {
    let value: Value = serde_json::from_str(input)?;
    parse_value(value)
}

/// Parse and validate an Opy HIR v1 payload from a JSON value.
pub fn parse_value(value: Value) -> Result<Program, HirError> {
    validate::check_envelope(&value)?;
    validate::check_unknown_kinds(&value)?;
    let program: Program = serde_json::from_value(value)?;
    program.validate()?;
    Ok(program)
}

impl Program {
    /// Validate structural invariants of this program (spans, identifiers,
    /// references). Envelope and node-kind checks are performed by
    /// [`parse_str`]/[`parse_value`].
    pub fn validate(&self) -> Result<(), HirError> {
        validate::validate_program(self)
    }

    /// Render a deterministic debug dump suitable for tests and issue
    /// reports.
    pub fn dump(&self) -> String {
        dump::dump(self)
    }
}
