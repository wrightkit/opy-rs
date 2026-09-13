use crate::macro_js::{Limits, MacroArg, MacroError, MacroRuntime};

use super::*;

impl Preprocessor {
    pub(super) fn resolve_script(
        &self,
        path: &str,
        span: Span,
        base: &Path,
    ) -> OpyResult<ScriptMacro> {
        let path = path.replace('\\', "/");
        let candidate = base.join(&path);
        let canonical = candidate.canonicalize().ok();
        let resolved_path =
            display_path(&candidate, canonical.as_deref(), &self.display_root, &path);
        let overlay_source = self
            .overlay
            .get(&path)
            .or_else(|| self.overlay.get(&candidate.to_string_lossy().into_owned()))
            .or_else(|| self.overlay.get(&resolved_path))
            .or_else(|| {
                canonical
                    .as_ref()
                    .and_then(|path| self.overlay.get(&path.to_string_lossy().into_owned()))
            })
            .cloned();
        let source = match overlay_source {
            Some(source) => source,
            None => {
                let canonical = canonical.ok_or_else(|| {
                    OpyError::at(
                        "script-not-found",
                        format!(
                            "cannot find script '{path}' under root '{}'",
                            base.display()
                        ),
                        span,
                    )
                })?;
                std::fs::read_to_string(&canonical).map_err(|error| {
                    OpyError::at(
                        "script-not-found",
                        format!("cannot read script '{path}': {error}"),
                        span,
                    )
                })?
            }
        };
        Ok(ScriptMacro {
            path: resolved_path,
            source,
        })
    }

    pub(super) fn expand_script(
        &self,
        mac: &MacroDef,
        script: &ScriptMacro,
        args: Vec<MacroArgument>,
        use_site: Span,
        line_indent: u32,
    ) -> OpyResult<Vec<Token>> {
        let macro_args: Vec<MacroArg> = mac
            .params
            .iter()
            .zip(args.iter())
            .map(|(param, argument)| MacroArg::new(param.clone(), argument.raw.clone()))
            .collect();
        // Resource limits mirror the pinned reference constants (1000 ms macro
        // budget, 64 MiB memory, 512 KiB stack; see `crate::macro_js::Limits`).
        let runtime = MacroRuntime::new(Limits::default());
        let result = runtime
            .run_macro(&script.source, &macro_args, &script.path)
            .map_err(|error| map_macro_error(&error, &script.path, use_site))?;
        // Reference indentation rule (`resolveMacro`): every newline in the
        // replacement is followed by the call line's indentation.
        let indent = " ".repeat(line_indent as usize);
        let indented = result.text.replace('\n', &format!("\n{indent}"));
        let mut tokens = lex(LexInput {
            file_id: use_site.file,
            text: &indented,
        })?;
        tokens.retain(|token| token.kind != TokenKind::Eof);
        super::macros::shift_expansion_spans(&mut tokens, use_site);
        Ok(tokens)
    }
}

/// Maps a runtime [`MacroError`] to a structured frontend diagnostic with the
/// script path as provenance and the directive/call-site span.
///
/// The runtime's QuickJS abort messages are classified into stable codes:
/// `script-timeout` (`"interrupted"`), `script-memory-limit`
/// (`"out of memory"`), `script-stack-limit`
/// (`"Maximum call stack size exceeded"`), and `script-error` for thrown
/// exceptions (with the script path and, when the engine provided one, the
/// line/column). Non-string completion values are `script-result-not-string`
/// with the reference's wording, and engine setup failures are
/// `script-internal`.
pub(crate) fn map_macro_error(error: &MacroError, script_path: &str, span: Span) -> OpyError {
    match error {
        MacroError::Script(script) => {
            let code = match script.message.as_str() {
                "interrupted" => "script-timeout",
                "out of memory" => "script-memory-limit",
                "Maximum call stack size exceeded" => "script-stack-limit",
                _ => "script-error",
            };
            let location = match (script.line, script.column) {
                (Some(line), Some(column)) => format!(" (line {line}, column {column})"),
                (Some(line), None) => format!(" (line {line})"),
                _ => String::new(),
            };
            OpyError::at(
                code,
                format!(
                    "script '{}' failed: {}{}",
                    script_path, script.message, location
                ),
                span,
            )
        }
        MacroError::InvalidResult { type_name } => OpyError::at(
            "script-result-not-string",
            format!(
                "JavaScript macro returned value with type of {type_name}, expected string. Try using .toString()"
            ),
            span,
        ),
        MacroError::Internal(message) => OpyError::at(
            "script-internal",
            format!("script '{}' runtime failure: {message}", script_path),
            span,
        ),
    }
}
