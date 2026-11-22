use super::*;
use workshop_rs::settings::SettingsNode;

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

        let mut program = lowering.program;
        collapse_modes(&mut program.settings, locale);
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

/// The mode header the pinned OverPy writes for a mode: the locale's display
/// name for a known mode id, else the authored key verbatim (its `tows` lookup
/// throws and the catch keeps the key), prefixed by `disabled` for
/// `enabled: false`. The emitter owns that spelling, so read it back from a
/// one-mode emission; `None` when the locale cannot spell the mode.
fn written_mode_header(node: &SettingsNode, locale: &Locale) -> Option<String> {
    let SettingsNode::Group { name, children, .. } = node else {
        return None;
    };
    let probe = SettingsNode::Group {
        name: name.clone(),
        children: children
            .iter()
            .filter(|member| matches!(member, SettingsNode::Bool { name, .. } if name == "enabled"))
            .cloned()
            .collect(),
        span: None,
    };
    let mut program = workshop_rs::Program::default();
    program.settings = Some(workshop_rs::settings::Settings {
        span: None,
        children: vec![SettingsNode::Group {
            name: "gamemodes".to_string(),
            children: vec![probe],
            span: None,
        }],
    });
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().ok()?;
    let text = workshop_rs::emitter::emit(&program, catalog, locale).ok()?;
    text.lines()
        .nth(2)
        .and_then(|line| line.trim().strip_suffix('{'))
        .map(|header| header.trim().to_string())
}

/// The pinned OverPy stores each mode in an object keyed by the written
/// header (`result.modes[header] = {}`), so a later mode with the same header
/// replaces the earlier one at the earlier one's position. Keys differing in
/// case collapse only when they write the same header (`skirmish` and
/// `Skirmish` in en-US); the header depends on the output locale.
pub(super) fn collapse_modes(
    settings: &mut Option<workshop_rs::settings::Settings>,
    locale: &Locale,
) {
    for group in settings
        .iter_mut()
        .flat_map(|settings| &mut settings.children)
    {
        if let SettingsNode::Group { name, children, .. } = group
            && name == "gamemodes"
        {
            collapse_written_modes(children, locale);
        }
    }
}

fn collapse_written_modes(modes: &mut Vec<SettingsNode>, locale: &Locale) {
    if modes.len() < 2 {
        return;
    }
    let mut kept: Vec<(Option<String>, SettingsNode)> = Vec::new();
    for mode in std::mem::take(modes) {
        let header = written_mode_header(&mode, locale);
        match header.as_ref().and_then(|header| {
            kept.iter_mut()
                .find(|(known, _)| known.as_ref() == Some(header))
        }) {
            Some((_, slot)) => *slot = mode,
            None => kept.push((header, mode)),
        }
    }
    modes.extend(kept.into_iter().map(|(_, mode)| mode));
}
