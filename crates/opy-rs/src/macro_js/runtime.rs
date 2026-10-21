use std::time::Instant;

use super::engine::quickjs_ng::QuickJsEngine;
use super::engine::{Completion, EngineError};
use super::error::{MacroError, ScriptError};
use super::helpers::Helpers;
use super::limits::Limits;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacroArg {
    pub(crate) name: String,
    pub(crate) value: String,
}

impl MacroArg {
    pub(crate) fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacroResult {
    pub(crate) text: String,
    pub(crate) console_output: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct MacroRuntime {
    limits: Limits,
    helpers: Helpers,
}

impl MacroRuntime {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            helpers: Helpers::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_helpers(&mut self, helpers: Helpers) {
        self.helpers = helpers;
    }

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

    pub(crate) fn run_hook(
        &self,
        source: &str,
        content: &str,
        script_name: &str,
    ) -> Result<MacroResult, MacroError> {
        let prologue = format!("var content = {};\n", json_string_literal(content));
        self.execute(&prologue, source, script_name, self.limits.hook_time_budget)
    }

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
        #[cfg(test)]
        let started = Instant::now();
        engine
            .install_console()
            .map_err(|e| MacroError::Internal(format!("failed to install console: {e}")))?;
        #[cfg(test)]
        crate::resource_metrics::record_macro_phase(
            crate::resource_metrics::MacroPhase::HostRegistration,
            started.elapsed().as_nanos(),
        );

        #[cfg(test)]
        let started = Instant::now();
        engine
            .evaluate(&builtins_source(&self.helpers), BUILTINS_FILENAME)
            .map_err(|e| {
                MacroError::Internal(format!("failed to evaluate builtin helpers: {e}"))
            })?;
        #[cfg(test)]
        crate::resource_metrics::record_macro_phase(
            crate::resource_metrics::MacroPhase::BuiltinEvaluation,
            started.elapsed().as_nanos(),
        );
        engine.set_interrupt_deadline(Some(Instant::now() + time_budget));
        #[cfg(test)]
        let started = Instant::now();
        let completion = engine
            .evaluate(&script_text, script_name)
            .map_err(|e| map_engine_error(e, script_name, line_adjust))?;
        #[cfg(test)]
        crate::resource_metrics::record_macro_phase(
            crate::resource_metrics::MacroPhase::ScriptEvaluation,
            started.elapsed().as_nanos(),
        );
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

const BUILTIN_OBJECTS: [&str; 6] = ["Map", "Hero", "Gamemode", "Color", "Team", "Button"];

const BUILTINS_FILENAME: &str = "<opy-rs-macro-runtime-builtins>";

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

fn adjusted_line(line: u32, line_adjust: usize) -> u32 {
    line.saturating_sub(line_adjust as u32).max(1)
}
