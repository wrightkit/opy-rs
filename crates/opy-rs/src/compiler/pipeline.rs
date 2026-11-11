//! One compile of OPY HIR: macro expansion, lowering to Workshop WIR,
//! canonical validation, and emission.

use super::*;

impl Compiler {
    /// Compile once. Macro expansion records the invocation site of each
    /// expanded node, and optimization decisions read it. With `attributed`,
    /// the second result is the program whose provenance uses those sites
    /// (`None` when no node was attributed).
    pub(super) fn compile_hir_attributed(
        &self,
        hir: &hir::Program,
        locale: &Locale,
        attributed: bool,
    ) -> Result<(CompilationArtifact, Option<Program>), IntegrationError> {
        if !self.catalog.supports(locale) {
            return Err(IntegrationError::new(
                "locale-unsupported",
                format!("workshop catalog does not declare locale '{locale}'"),
                None,
            ));
        }
        reject_unlowered_directives(hir)?;
        let expanded_hir = {
            let _span = crate::trace::span("hir.expand");
            expand_macros(hir)?
        };
        let lowering = self.fully_lowered(&expanded_hir, locale)?;
        let translation_files = lowering.translation_files();
        let attributed_program = if attributed {
            let _span = crate::trace::span("source_map.attribute");
            lowering.attributed_program()?
        } else {
            None
        };

        let program = lowering.program;
        let emitted = self.validate_and_emit(&program, &expanded_hir, locale)?;
        let exclude_variables = has_directive(&expanded_hir, "excludeVariablesInCompilation");
        let emitted = if exclude_variables {
            omit_declaration_sections(&emitted, &program)
        } else {
            emitted
        };
        let emitted = if has_directive(&expanded_hir, "debugElementCount") {
            emit_debug_element_counts(&emitted, &program, self.catalog, &expanded_hir)?
        } else {
            emitted
        };

        let artifact = CompilationArtifact {
            wir: program,
            final_output: emitted.clone(),
            emitted,
            catalog_identity: self.catalog.identity(),
            hook_console_output: Vec::new(),
            translation_files,
        };
        Ok((artifact, attributed_program))
    }

    fn fully_lowered<'a>(
        &'a self,
        hir: &'a hir::Program,
        locale: &Locale,
    ) -> Result<Lowering<'a>, IntegrationError> {
        let _span = crate::trace::span("workshop.lower");
        let mut lowering = Lowering::new(self, hir);
        lowering.copy_files(locale)?;
        lowering.lower_declarations()?;
        lowering.lower_rules()?;
        Ok(lowering)
    }
}
