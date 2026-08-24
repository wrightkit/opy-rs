//! The standalone OverPy-compatible `.opy` frontend (opy-rs).
//!
//! Owns the OPY source-language surface of the `opy-rs` repository: a lexer,
//! an indentation-aware CST/parser with structured diagnostics and recovery,
//! token-level preprocessing (includes and `#!define` macros), semantic
//! resolution, and lowering into the opy-rs-owned Opy HIR contract
//! ([`hir::Program`]). Everything from source through the Opy HIR semantic
//! model is Workshop-independent: the frontend never depends on `workshop-rs`,
//! OverPy, or Node, and the integration boundary toward `workshop-rs` is
//! documented rather than implemented here.
//!
//! Pipeline: [`lexer::lex`] → [`preprocess::preprocess`] →
//! [`parser::parse`] → [`lower::lower`] → Opy HIR ([`hir`]).
//!
//! OverPy-compatible `__script__("…")` macros execute at compile time through
//! the bounded embedded runtime ([`opy_macro_js`]): script macros expand
//! during preprocessing with the reference's argument-injection ABI, and
//! resource limits mirror the pinned reference constants
//! (`opy_macro_js::Limits::default()`). Script-macro expansion is
//! compile-time behavior and is frontend-supported.
//!
//! `#!postCompileHook` is recognized, parsed, validated, and recorded only
//! (see [`preprocess`] and [`CompileOutcome::post_compile_hook`]): the
//! frontend never executes the hook. Real hook execution receives the final
//! Workshop text produced by lowering and is lowering-dependent (workshop-rs
//! emission, issue #8); the frontend never fabricates a Workshop payload.
//!
//! This crate was extracted from the mature Wright frontend (the wright
//! repository's `crates/wright-opy`); module provenance and issue references
//! follow the original implementation. Workshop→OPY reconstruction and the
//! differential harness are not part of this crate (see the opy-rs roadmap).

pub mod cst;
pub mod diag;
pub mod hir;
pub mod lexer;
pub mod lower;
pub mod manifest;
pub mod parser;
pub mod preprocess;
pub mod settings;
pub mod support;
pub mod tooling;

use std::path::Path;

use diag::Span;
pub use diag::{FrontendError, FrontendResult};
pub use lower::lower;
pub use parser::parse;
pub use preprocess::{preprocess, preprocess_with_overlay};

#[cfg(test)]
mod tests {
    use super::compile;
    use std::path::Path;

    #[test]
    fn unsupported_operator_aliases_fail_at_the_frontend_boundary() {
        for expression in ["a // 2", "a //= 2", "a ^ 2", "a && 2", "a || 2", "a = !2"] {
            let source = format!(
                "globalvar a\nrule \"unsupported operator\":\n    @Event global\n    {expression}\n"
            );
            let error = compile(&source, "unsupported-operator.opy", Path::new("."))
                .expect_err("unsupported operator alias unexpectedly compiled");
            assert!(matches!(error.code.as_str(), "lex-error" | "parse-error"));
            assert!(error.span.is_some(), "{expression}: missing source span");
        }
    }

    #[test]
    fn implicit_event_player_defaults_satisfy_hir_reference_validation() {
        let hir = compile(
            "rule \"implicit player\":\n    @Event eachPlayer\n    eventPlayer.A = 1\n",
            "implicit-player.opy",
            Path::new("."),
        )
        .expect("implicit event-player default must resolve");
        hir.validate()
            .expect("implicit event-player default must satisfy HIR invariants");
    }
}

/// The frontend's supported protocol identity for generated HIR.
///
/// The producer identity and the Opy HIR protocol envelope (`wright/opy-hir`
/// v1) are preserved from the Wright frontend so existing consumers keep
/// accepting this producer's payloads during the migration; renaming the
/// producer identity is a protocol decision for the opy-rs docs/architecture
/// workstream.
pub const FRONTEND_NAME: &str = "wright/opy-native";
pub const FRONTEND_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compile one `.opy` source end-to-end into the Opy HIR contract:
/// preprocess (includes/defines) → parse (CST) → lower (HIR).
///
/// `main_path` is the file's display path recorded in the HIR file registry;
/// `root` is the include base. `compile` never requires Node or OverPy.
pub fn compile(source: &str, main_path: &str, root: &Path) -> FrontendResult<hir::Program> {
    compile_with_overlay(source, main_path, root, &std::collections::BTreeMap::new())
}

/// Compile with open-document overlays: includes resolve to overlay text
/// (keyed by the include string or the resolved canonical path) before the
/// filesystem, so unsaved editor buffers participate in include resolution.
pub fn compile_with_overlay(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> FrontendResult<hir::Program> {
    let outcome = compile_with_overlay_outcome(source, main_path, root, overlay);
    match outcome.hir {
        Some(hir) => Ok(hir),
        None => Err(outcome
            .error
            .expect("a failed compile outcome always carries an error")),
    }
}

/// The outcome of a compile with overlays.
///
/// Unlike [`compile_with_overlay`], this retains the frontend file registry
/// even when parsing or lowering fails, so language tooling can map span file
/// ids to their actual source identities without building a diagnostics-only
/// project model.
pub struct CompileOutcome {
    pub hir: Option<hir::Program>,
    pub error: Option<FrontendError>,
    pub files: Vec<preprocess::FileRecord>,
    /// The declared `#!postCompileHook` script, when the source declared one
    /// and compilation succeeded.
    ///
    /// This is the declaration record, not an execution result: the frontend
    /// recognizes, parses, validates, and records the directive, but never
    /// executes the hook. Execution against the final Workshop text is
    /// lowering-dependent (workshop-rs emission, issue #8); the frontend
    /// never fabricates a Workshop payload.
    pub post_compile_hook: Option<PostCompileHookRecord>,
}

/// The recorded declaration of a `#!postCompileHook` script.
///
/// The declared `#!postCompileHook` script; execution against the final
/// Workshop text is lowering-dependent (workshop-rs emission, issue #8). The
/// frontend never fabricates a Workshop payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostCompileHookRecord {
    /// The script path as declared (root-relative).
    pub script: String,
    /// The directive's source span, when known.
    pub span: Option<Span>,
}

/// Compile with open-document overlays while retaining the frontend file
/// registry on parse/lower failure.
///
/// This is the compile contract view of [`tooling::check_with_overlay`]: the
/// two share one pipeline, so `check` and `compile` never disagree about
/// whether a project is clean.
pub fn compile_with_overlay_outcome(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> CompileOutcome {
    let outcome = tooling::check_with_overlay(source, main_path, root, overlay);
    // Every failed check carries at least one diagnostic, so a None model
    // always yields an error (the compile outcome invariant).
    let error = outcome.diagnostics.first().map(|diagnostic| FrontendError {
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        span: diagnostic
            .span
            .as_ref()
            .map(tooling::SourceLocation::to_span),
    });
    // The directive was parsed, validated, and recorded by preprocessing; the
    // frontend never executes the hook (real hook execution receives the
    // final Workshop text and is lowering-dependent, issue #8 — see
    // `PostCompileHookRecord`).
    let post_compile_hook = outcome.post_compile_hook.map(|hook| PostCompileHookRecord {
        script: hook.path,
        span: Some(hook.span),
    });
    CompileOutcome {
        hir: outcome.model.map(|model| model.hir),
        error,
        files: outcome.files,
        post_compile_hook,
    }
}
