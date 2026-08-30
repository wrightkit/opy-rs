//! The reusable macro/hook runtime: script assembly, invocation, and
//! result/error mapping.

use std::time::Instant;

use super::engine::quickjs_ng::QuickJsEngine;
use super::engine::{Completion, EngineError, JsEngine};
use super::error::{MacroError, ScriptError};
use super::helpers::Helpers;
use super::limits::Limits;

/// One macro argument: the declared parameter name and the **raw** call-site
/// argument text.
///
/// The value is injected verbatim as `var <name>=<value>;` ahead of the script
/// source, exactly like the OverPy reference (`resolveMacro` in
/// `src/compiler/tokenizer.ts`). Argument-count validation against the macro
/// declaration belongs to the frontend, which knows the declared parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacroArg {
    /// Parameter name as declared in the macro.
    pub(crate) name: String,
    /// Raw textual argument from the call site.
    pub(crate) value: String,
}

impl MacroArg {
    /// Creates a macro argument from the declared parameter name and the raw
    /// call-site text.
    pub(crate) fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// The outcome of a successful macro or hook invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacroResult {
    /// The script's string completion value: expanded text for macros, the
    /// transformed content for hooks.
    pub(crate) text: String,
    /// Lines written via `console.log(...)`, in order, one entry per call
    /// (arguments rendered with `String()` semantics and joined on `" "`).
    pub(crate) console_output: Vec<String>,
}

/// Reusable runtime for JavaScript-backed macros and post-compile hooks.
///
/// Each invocation spins up a fresh embedded engine instance (runtime +
/// context) with the configured [`Limits`] and helper surface, evaluates the
/// script, and tears the engine down. The runtime itself is reusable, but no
/// JavaScript state is shared across invocations.
#[derive(Debug)]
pub(crate) struct MacroRuntime {
    limits: Limits,
    helpers: Helpers,
}

impl MacroRuntime {
    /// Creates a runtime with the given resource limits and an empty helper
    /// set.
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            helpers: Helpers::new(),
        }
    }

    /// Replaces the helper surface (constant objects) used by subsequent
    /// invocations.
    #[cfg(test)]
    pub(crate) fn set_helpers(&mut self, helpers: Helpers) {
        self.helpers = helpers;
    }

    /// The configured resource limits.
    /// Executes a macro script.
    ///
    /// `source` is the script's text (the file content the frontend resolved
    /// from `__script__("...")`). `args` are injected as `var` declarations;
    /// `script_name` is used for error attribution and must be the resolved
    /// script path/name the frontend knows.
    ///
    /// Returns the string completion value as the expanded text, or a
    /// structured error.
    pub(crate) fn run_macro(
        &self,
        source: &str,
        args: &[MacroArg],
        script_name: &str,
    ) -> Result<MacroResult, MacroError> {
        let mut prologue = String::new();
        for arg in args {
            prologue.push_str("var ");
            prologue.push_str(&arg.name);
            prologue.push('=');
            prologue.push_str(&arg.value);
            prologue.push(';');
        }
        prologue.push('\n');
        self.execute(
            &prologue,
            source,
            script_name,
            self.limits.macro_time_budget,
        )
    }

    /// Executes a post-compile hook script against `content`.
    ///
    /// Mirrors the OverPy `#!postCompileHook` ABI (`src/compiler/tokenizer.ts`):
    /// the script receives the content JSON-escaped as
    /// `var content = "...";` and must return the transformed content as a
    /// string. This works against synthetic/content inputs now; wiring to
    /// actual Workshop emission is lowering-dependent.
    pub(crate) fn run_hook(
        &self,
        source: &str,
        content: &str,
        script_name: &str,
    ) -> Result<MacroResult, MacroError> {
        let prologue = format!("var content = {};\n", json_string_literal(content));
        self.execute(&prologue, source, script_name, self.limits.hook_time_budget)
    }

    /// Shared invocation path: builds the full script text, runs it on a fresh
    /// engine, and maps the outcome.
    fn execute(
        &self,
        prologue: &str,
        source: &str,
        script_name: &str,
        time_budget: std::time::Duration,
    ) -> Result<MacroResult, MacroError> {
        // Stack line numbers count the prologue too; the reference subtracts
        // the prepended block's line count, so we adjust the same way.
        let line_adjust = prologue.matches('\n').count();
        let script_text = format!("{prologue}{source}");

        let mut engine =
            QuickJsEngine::new(&self.limits).map_err(|e| MacroError::Internal(e.to_string()))?;
        engine
            .install_console()
            .map_err(|e| MacroError::Internal(format!("failed to install console: {e}")))?;
        engine
            .evaluate(&builtins_source(&self.helpers), BUILTINS_FILENAME)
            .map_err(|e| {
                MacroError::Internal(format!("failed to evaluate builtin helpers: {e}"))
            })?;
        engine.set_interrupt_deadline(Some(Instant::now() + time_budget));
        let completion = engine
            .evaluate(&script_text, script_name)
            .map_err(|e| map_engine_error(e, script_name, line_adjust))?;
        let text = match completion {
            Completion::String(text) => text,
            Completion::NonString(type_name) => {
                return Err(MacroError::InvalidResult {
                    type_name: type_name.to_string(),
                });
            }
        };
        Ok(MacroResult {
            text,
            console_output: engine.console_output().to_vec(),
        })
    }
}

/// The constant objects the reference always defines (`src/globalVars.ts`).
const BUILTIN_OBJECTS: [&str; 6] = ["Map", "Hero", "Gamemode", "Color", "Team", "Button"];

/// Filename used for the internal helpers script; the helpers are static and
/// cannot throw, so this never surfaces in errors.
const BUILTINS_FILENAME: &str = "<opy-rs-macro-runtime-builtins>";

/// The `vect` helper from the reference's `builtInJsFunctions` block.
const VECT_HELPER: &str = r#"function vect(x, y, z) {
    return {
        x: x,
        y: y,
        z: z,
        toString: function () {
            return "vect(" + this.x + "," + this.y + "," + this.z + ")";
        },
    };
}
"#;

/// Builds the helpers script: `vect` plus the six constant objects, always
/// defined and populated from [`Helpers`] entries.
fn builtins_source(helpers: &Helpers) -> String {
    let mut source = String::from(VECT_HELPER);
    for object in BUILTIN_OBJECTS {
        source.push_str("var ");
        source.push_str(object);
        source.push_str(" = {");
        for (i, (key, value)) in helpers.entries(object).iter().enumerate() {
            if i > 0 {
                source.push(',');
            }
            source.push_str(&json_string_literal(key));
            source.push(':');
            source.push_str(&json_string_literal(value));
        }
        source.push_str("};\n");
    }
    source
}

/// Encodes `value` as a JavaScript string literal.
///
/// Uses JSON encoding (equivalent to the reference's `JSON.stringify`),
/// including the ES2019 escape of U+2028/U+2029 which `JSON.stringify`
/// produces and `serde_json` does not.
fn json_string_literal(value: &str) -> String {
    let mut literal = serde_json::to_string(value).expect("serializing a string is infallible");
    if literal.contains('\u{2028}') || literal.contains('\u{2029}') {
        literal = literal
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
    }
    literal
}

/// Maps an engine failure to a public error, adjusting stack line numbers so
/// they refer to the user's script text (the injected prologue is subtracted,
/// mirroring the reference's `normalizeScriptError`).
fn map_engine_error(error: EngineError, script_name: &str, line_adjust: usize) -> MacroError {
    match error {
        EngineError::Exception { message, stack } => {
            let position = first_frame_position(&stack, script_name, line_adjust);
            let adjusted_stack =
                (!stack.is_empty()).then(|| adjust_stack(&stack, script_name, line_adjust));
            MacroError::Script(ScriptError {
                message,
                source_name: Some(script_name.to_string()),
                line: position.map(|(line, _)| line),
                column: position.map(|(_, column)| column),
                stack: adjusted_stack,
            })
        }
        EngineError::Internal(message) => MacroError::Internal(message),
    }
}

/// Finds the first stack frame referencing `script_name` and returns its
/// line/column, with the line adjusted to the user's script.
fn first_frame_position(stack: &str, script_name: &str, line_adjust: usize) -> Option<(u32, u32)> {
    let needle = format!("{script_name}:");
    let pos = stack.find(&needle)?;
    let after = &stack[pos + needle.len()..];
    let line_digits = after.bytes().take_while(|b| b.is_ascii_digit()).count();
    if line_digits == 0 {
        return None;
    }
    let line = after[..line_digits].parse::<u32>().ok()?;
    let after_line = after[line_digits..].strip_prefix(':')?;
    let column_digits = after_line
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if column_digits == 0 {
        return None;
    }
    let column = after_line[..column_digits].parse::<u32>().ok()?;
    Some((adjusted_line(line, line_adjust), column))
}

/// Rewrites every `script_name:LINE:COL` occurrence in the stack so line
/// numbers refer to the user's script text.
fn adjust_stack(stack: &str, script_name: &str, line_adjust: usize) -> String {
    let needle = format!("{script_name}:");
    let mut out = String::with_capacity(stack.len());
    for line in stack.split_inclusive('\n') {
        let Some(pos) = line.find(&needle) else {
            out.push_str(line);
            continue;
        };
        out.push_str(&line[..pos + needle.len()]);
        let after = &line[pos + needle.len()..];
        let digits = after.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            out.push_str(after);
            continue;
        }
        let line_number = after[..digits].parse::<u32>().unwrap_or(1);
        out.push_str(&adjusted_line(line_number, line_adjust).to_string());
        out.push_str(&after[digits..]);
    }
    out
}

/// Subtracts the prologue line count, keeping a minimum of 1 (the reference
/// uses `Math.max(1, line - lineOffset)`).
fn adjusted_line(line: u32, line_adjust: usize) -> u32 {
    line.saturating_sub(line_adjust as u32).max(1)
}
