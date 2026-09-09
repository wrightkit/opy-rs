use super::*;

impl Compiler {
    pub(crate) fn compile_hir_with_locale_and_hook(
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
                    IntegrationError::post_compile_hook(error, hook.span.map(hir_span_from_diag))
                })?;
            artifact.final_output = result.text;
            artifact.hook_console_output = result.console_output;
        }
        Ok(artifact)
    }
}
