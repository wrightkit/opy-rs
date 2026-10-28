pub(crate) mod compile_time;
pub(crate) mod compiler;
pub mod cst;
pub mod diag;
pub(crate) mod enums;
pub mod hir;
pub mod lexer;
pub mod lookup;
pub mod lower;
mod macro_js;
pub mod manifest;
pub(crate) mod matcher;
pub mod parser;
pub mod preprocess;
pub mod project;
#[cfg(test)]
pub(crate) mod resource_metrics;
pub mod settings;
mod string_entities;
pub mod tooling;

use std::path::Path;

pub use compiler::reconstruct;
pub use compiler::{
    COMPILE_SCHEMA_VERSION, CompilationArtifact, CompileDiagnostic, CompileFailureClass,
    CompileOutput, CompileReport, CompileResult, CompileStatus, Compiler, CompilerIdentity,
    IntegrationDiagnostic, IntegrationError, LinkReport, ScriptDiagnostic,
};
use diag::Span;
pub use diag::{OpyError, OpyResult};
pub use lower::lower;
pub use parser::parse;
pub use preprocess::{preprocess, preprocess_with_overlay};
pub use project::{FilesystemProject, FilesystemProjectError};

#[cfg(test)]
mod tests {
    use super::compile;
    use std::path::Path;

    #[test]
    fn unsupported_operator_aliases_fail_at_the_source_boundary() {
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

/// The producer identity for generated HIR.
///
/// The producer identity and the Opy HIR protocol envelope (`wright/opy-hir`
/// v2) is emitted for the ordered switch-arm wire grammar; v1 consumers must
/// reject it until they migrate to the v2 contract.
pub const LANGUAGE_NAME: &str = "opy-rs";
pub const LANGUAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compile one `.opy` source end-to-end into the Opy HIR contract:
/// preprocess (includes/defines) → parse (CST) → lower (HIR).
///
/// `main_path` is the file's display path recorded in the HIR file registry;
/// `root` is the include base. `compile` never requires Node or OverPy.
pub fn compile(source: &str, main_path: &str, root: &Path) -> OpyResult<hir::Program> {
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
) -> OpyResult<hir::Program> {
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
/// Unlike [`compile_with_overlay`], this retains the source file registry
/// even when parsing or lowering fails, so language tooling can map span file
/// ids to their actual source identities without building a diagnostics-only
/// project model.
pub struct CompileOutcome {
    pub hir: Option<hir::Program>,
    pub error: Option<OpyError>,
    pub diagnostics: Vec<tooling::Diagnostic>,
    pub files: Vec<preprocess::FileRecord>,
    /// The directory the `files` display paths resolve against — the
    /// `#!mainFile` effective directory when the entry redirects the project
    /// root, otherwise the canonicalized input root.
    pub display_root: std::path::PathBuf,
    /// Filesystem paths the frontend probed or read while producing this
    /// outcome, in the form each was accessed plus the resolved form when a
    /// probe canonicalized it — the raw record a caller turns into declared
    /// compile inputs.
    pub accessed: std::collections::BTreeSet<std::path::PathBuf>,
    /// The declared `#!postCompileHook` script, when the source declared one
    /// and the frontend stage succeeded.
    ///
    /// This is the declaration record, not an execution result: the frontend
    /// recognizes, parses, validates, and records the directive, and the
    /// compile stage (or `check`'s acceptance pass, issue #620) executes it
    /// against the emitted Workshop text and revalidates the rewritten
    /// output under the request locale. The frontend itself never
    /// fabricates a Workshop payload (issue #8).
    pub post_compile_hook: Option<PostCompileHookRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostCompileHookRecord {
    pub script: String,
    pub source: String,
    pub span: Option<Span>,
}

/// Compile with open-document overlays while retaining the source file registry
/// on parse/lower failure.
///
/// This is the frontend-stage view of [`tooling::check_with_overlay`]: the
/// two share one source pipeline, so they never disagree about whether the
/// source is clean. The compile stage runs afterwards through [`Compiler`],
/// under the caller's locale, so its failures keep their integration
/// classification.
pub fn compile_with_overlay_outcome(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> CompileOutcome {
    compile_with_overlay_outcome_with_locale(
        source,
        main_path,
        root,
        overlay,
        &workshop_rs::catalog::Locale::new("en-US"),
    )
}

/// [`compile_with_overlay_outcome`] under an explicit catalog locale — the
/// locale the caller's subsequent compile emits under (issue #620).
pub fn compile_with_overlay_outcome_with_locale(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
    locale: &workshop_rs::catalog::Locale,
) -> CompileOutcome {
    let outcome = tooling::check_frontend_with_overlay(source, main_path, root, overlay, locale);
    let error = outcome
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == tooling::DiagnosticSeverity::Error)
        .map(|diagnostic| OpyError {
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            span: diagnostic
                .span
                .as_ref()
                .map(tooling::SourceLocation::to_span),
        });
    let post_compile_hook = outcome.post_compile_hook.map(|hook| PostCompileHookRecord {
        script: hook.path,
        source: hook.source,
        span: Some(hook.span),
    });
    let outcome = CompileOutcome {
        hir: outcome.model.map(|model| model.hir),
        error,
        diagnostics: outcome.diagnostics,
        files: outcome.files,
        display_root: outcome.display_root,
        accessed: outcome.accessed,
        post_compile_hook,
    };
    debug_assert!(
        outcome.hir.is_some() || outcome.error.is_some(),
        "a failed check always carries an error"
    );
    outcome
}
