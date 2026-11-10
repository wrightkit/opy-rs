use super::*;

impl Compiler {
    /// Validate `program` and emit it as Workshop text. The canonical form is
    /// built once and shared by both validations and emission.
    pub(super) fn validate_and_emit(
        &self,
        program: &Program,
        hir: &hir::Program,
        locale: &Locale,
    ) -> Result<String, IntegrationError> {
        let to_error = |code: &str, error: workshop_rs::WorkshopError| {
            IntegrationError::new(
                code,
                error.to_string(),
                workshop_error_span(&error).and_then(|span| hir_span_from_workshop(span, hir)),
            )
        };
        let canonical = program
            .canonical()
            .map_err(|error| to_error("workshop-validation", error))?;
        canonical
            .validate()
            .map_err(|error| to_error("workshop-validation", error))?;
        canonical
            .validate_canonical_ids(self.catalog)
            .map_err(|error| to_error("catalog-validation", error))?;
        canonical
            .emit_with_options(
                self.catalog,
                locale,
                &workshop_rs::emitter::EmitOptions::default(),
            )
            .map(|output| output.text)
            .map_err(|error| {
                let span =
                    workshop_error_span(&error).and_then(|span| hir_span_from_workshop(span, hir));
                self.emission_error(hir, error, "workshop-emission", span)
            })
    }
}
