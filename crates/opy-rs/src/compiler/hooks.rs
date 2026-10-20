use super::*;

impl Compiler {
    /// [`Self::compile_hir_with_locale`], then run the declared
    /// `#!postCompileHook` against the emitted Workshop text under the same
    /// locale. This is an advanced integration API.
    ///
    /// A hook rewrite must still produce Workshop text that parses,
    /// validates, and emits under `locale`: the rewrite can introduce
    /// spellings the emitter never produced, and an enum member spelling
    /// valid in one locale is not necessarily valid in the active one
    /// (issue #620).
    pub fn compile_hir_with_locale_and_hook(
        &self,
        hir: &hir::Program,
        hook: Option<crate::PostCompileHookRecord>,
        locale: &Locale,
    ) -> Result<CompilationArtifact, IntegrationError> {
        let mut artifact = self.compile_hir_with_locale(hir, locale)?;
        if let Some(hook) = hook {
            let runtime = crate::macro_js::MacroRuntime::new(crate::macro_js::Limits::default());
            let result = runtime
                .run_hook(&hook.source, &artifact.emitted, &hook.script)
                .map_err(|error| {
                    IntegrationError::post_compile_hook(error, hook.span.map(Into::into))
                })?;
            artifact.final_output = result.text;
            artifact.hook_console_output = result.console_output;
            if artifact.final_output != artifact.emitted {
                self.validate_final_output(
                    hir,
                    &artifact.final_output,
                    locale,
                    hook.span.map(Into::into),
                )?;
            }
        }
        Ok(artifact)
    }

    /// [`Self::compile_hir_with_locale_and_hook`], and on success also
    /// returns the mapped Workshop text of the same compile. This is an
    /// advanced integration API.
    pub fn compile_hir_mapped_with_locale_and_hook(
        &self,
        hir: &hir::Program,
        hook: Option<crate::PostCompileHookRecord>,
        locale: &Locale,
    ) -> Result<(CompilationArtifact, MappedText), IntegrationError> {
        self.compile_hir_mapped_with_hook(hir, hook, locale)
    }
}
