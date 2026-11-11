//! The shared source pipeline behind `check` and the compile frontend.

use super::*;

pub(super) enum CheckStage {
    FrontendOnly,
    CompileStageAcceptance,
}

pub(super) fn check_pipeline(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
    locale: &workshop_rs::catalog::Locale,
    stage: CheckStage,
) -> CheckOutcome {
    let _span = crate::trace::span("frontend");
    let preprocess = crate::trace::span("preprocess");
    let PreprocessOutcome {
        result,
        files,
        display_root,
        warnings,
        accessed,
    } = crate::preprocess::preprocess_with_overlay_outcome(source, main_path, root, overlay);
    drop(preprocess);
    let preprocessed = match result {
        Ok((preprocessed, _)) => preprocessed,
        Err(error) => {
            let mut diagnostics = warnings
                .iter()
                .map(|warning| Diagnostic::from_warning(warning, &files))
                .collect::<Vec<_>>();
            diagnostics.push(Diagnostic::from_error(error, &files));
            return CheckOutcome::failure(diagnostics, files, display_root, accessed);
        }
    };
    let parse = crate::trace::span("parse");
    let parsed = crate::parser::parse_with_macros(
        &preprocessed.tokens,
        preprocessed.preprocessing.allow_macro_redeclaration,
        &preprocessed.text_macros,
    );
    let mut diagnostics = visible_warnings(&preprocessed, &parsed.warnings)
        .map(|warning| Diagnostic::from_warning(warning, &files))
        .collect::<Vec<_>>();
    let Some(mut program) = parsed.program else {
        diagnostics.extend(
            parsed
                .errors
                .iter()
                .map(|error| Diagnostic::from_error(error.clone(), &files)),
        );
        return CheckOutcome::failure(diagnostics, files, display_root, accessed);
    };
    if let Some(block) = &preprocessed.settings {
        match crate::settings::parse_block(block) {
            Ok(parsed_settings) => program.settings = Some(parsed_settings),
            Err(error) => {
                diagnostics.push(Diagnostic::from_error(error, &files));
                return CheckOutcome::failure(diagnostics, files, display_root, accessed);
            }
        }
    }
    drop(parse);
    let lowering = crate::trace::span("hir.lower");
    let defines = preprocessed
        .defines
        .iter()
        .map(|define| Define {
            name: define.name.clone(),
            is_function: define.is_function,
            is_member: define.is_member,
            span: define.span.map(Into::into),
        })
        .collect();
    let hir_files = files
        .iter()
        .map(|file| hir::types::SourceFile {
            id: file.id,
            path: file.path.clone(),
        })
        .collect();
    match crate::lower::lower_with_preprocessing(
        &program,
        hir_files,
        defines,
        &preprocessed.preprocessing,
    ) {
        Ok(mut hir) => {
            hir.preprocessing = preprocessed.preprocessing;
            if let Err(error) = crate::settings::resolve_hir_settings(&mut hir, &program) {
                diagnostics.push(Diagnostic::from_error(error, &files));
                return CheckOutcome::failure(diagnostics, files, display_root, accessed);
            }
            match crate::compiler::settings::workshop_settings(&hir, locale) {
                Ok((settings, unknown)) => {
                    diagnostics.extend(unknown.into_iter().map(|member| {
                        Diagnostic::from_unknown_setting(member, hir.settings.as_ref(), &files)
                    }));
                    if let Some(settings) = settings {
                        // workshop-rs also reports Warning entries for carried
                        // members near a declared spelling; those members are
                        // already covered by the `unknown-setting` warnings
                        // above, and emission accepts them, so only errors
                        // belong here.
                        diagnostics.extend(
                            workshop_rs::settings::check_emission_diagnostics(&settings)
                                .into_iter()
                                .filter(|diagnostic| {
                                    diagnostic.severity
                                        == workshop_rs::settings::DiagnosticSeverity::Error
                                })
                                .map(|diagnostic| {
                                    Diagnostic::from_settings_diagnostic(
                                        diagnostic,
                                        hir.settings.as_ref(),
                                        &files,
                                    )
                                }),
                        );
                    }
                }
                Err(error) => diagnostics.push(Diagnostic::from_integration_error(error, &files)),
            }
            if diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            {
                return CheckOutcome::failure(diagnostics, files, display_root, accessed);
            }
            drop(lowering);
            if matches!(stage, CheckStage::CompileStageAcceptance) {
                let _span = crate::trace::span("acceptance");
                let hook = preprocessed.post_compile_hook.as_ref().map(|hook| {
                    crate::PostCompileHookRecord {
                        script: hook.path.clone(),
                        source: hook.source.clone(),
                        span: Some(hook.span),
                    }
                });
                if let Some(error) =
                    acceptance::compile_stage_spelling_rejection(&hir, hook, locale)
                {
                    diagnostics.push(Diagnostic::from_integration_error(error, &files));
                    return CheckOutcome::failure(diagnostics, files, display_root, accessed);
                }
            }
            let model = {
                let _span = crate::trace::span("semantic.model");
                SemanticModel::build(hir, &program)
            };
            CheckOutcome {
                diagnostics,
                model: Some(model),
                files,
                post_compile_hook: preprocessed.post_compile_hook,
                display_root,
                accessed,
            }
        }
        Err(error) => {
            diagnostics.push(Diagnostic::from_error(error, &files));
            CheckOutcome::failure(diagnostics, files, display_root, accessed)
        }
    }
}
