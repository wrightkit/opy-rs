//! Compilation paired with the source map of its authored origin.

use super::*;

impl Compiler {
    /// Lower, validate, and emit `hir`, then pair the emitted Workshop text
    /// with the source map of its authored origin (`workshop-rs/mapped-text-v1`)
    /// in the same operation.
    ///
    /// One lowering produces both. The artifact, its `wir`, and its
    /// diagnostics keep macro definition provenance and equal a plain
    /// compile, while the map attributes macro-expanded code to its
    /// invocation site.
    pub fn compile_hir_mapped(
        &self,
        hir: &hir::Program,
    ) -> Result<(CompilationArtifact, MappedText), IntegrationError> {
        self.compile_hir_mapped_with_locale(hir, &Locale::new("en-US"))
    }

    pub fn compile_hir_mapped_with_locale(
        &self,
        hir: &hir::Program,
        locale: &Locale,
    ) -> Result<(CompilationArtifact, MappedText), IntegrationError> {
        self.compile_hir_mapped_with_hook(hir, None, locale)
    }

    pub(super) fn compile_hir_mapped_with_hook(
        &self,
        hir: &hir::Program,
        hook: Option<crate::PostCompileHookRecord>,
        locale: &Locale,
    ) -> Result<(CompilationArtifact, MappedText), IntegrationError> {
        let (artifact, attributed) = self.compile_hir_hooked(hir, hook, locale, true)?;
        let program = attributed.as_ref().unwrap_or(&artifact.wir);
        let mapped = self.mapped_text(program, &artifact.final_output, locale)?;
        Ok((artifact, mapped))
    }

    fn mapped_text(
        &self,
        program: &Program,
        text: &str,
        locale: &Locale,
    ) -> Result<MappedText, IntegrationError> {
        let _span = crate::trace::span("source_map.build");
        let map = SourceMap::extract(program);
        let verbatim_emission = workshop_rs::emitter::emit(program, self.catalog, locale)
            .is_ok_and(|emitted| emitted == text);
        if !verbatim_emission {
            let mut reparsed =
                workshop_rs::parser::parse(text, self.catalog, locale).map_err(|error| {
                    IntegrationError::new("source-map-parse", error.to_string(), None)
                })?;
            map.apply(&mut reparsed).map_err(|error| {
                IntegrationError::new("source-map-shape", error.to_string(), None)
            })?;
        }
        Ok(MappedText::new(text.to_owned(), map))
    }
}
