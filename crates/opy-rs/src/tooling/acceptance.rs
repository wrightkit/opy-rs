use crate::{PostCompileHookRecord, compiler::IntegrationError, hir};

/// The compile-stage acceptance pass `check` performs under the active
/// catalog locale: spelling rejections that `compile` would report surface
/// here, other integration-stage failures stay `compile`-only diagnostics.
pub(super) fn compile_stage_spelling_rejection(
    hir: &hir::types::Program,
    hook: Option<PostCompileHookRecord>,
    locale: &workshop_rs::catalog::Locale,
) -> Option<IntegrationError> {
    crate::compiler::Compiler::new()
        .and_then(|compiler| {
            compiler
                .compile_hir_with_locale_and_hook(hir, hook, locale)
                .map(|_| ())
        })
        .err()
        .filter(|error| is_spelling_rejection(&error.diagnostic.code))
}

fn is_spelling_rejection(code: &str) -> bool {
    code.starts_with("unknown-") || code == "missing-mapping"
}
