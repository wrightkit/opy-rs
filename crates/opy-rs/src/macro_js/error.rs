//! Structured errors carrying script/source provenance.

use std::fmt;

/// A JavaScript exception with engine-provided provenance.
///
/// The QuickJS error value's `message` and `stack` are captured. Stack frames
/// that reference the invocation's script name are line-adjusted so line
/// numbers refer to the user's script text (the injected argument / `content`
/// prologue is subtracted), mirroring the OverPy reference behavior
/// (`normalizeScriptError` in `src/quickjs.ts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptError {
    /// The exception message (e.g. `"kaboom"`). Resource-limit abort messages
    /// are `"interrupted"`, `"out of memory"`, and
    /// `"Maximum call stack size exceeded"`.
    pub(crate) message: String,
    /// The script name the invocation was attributed to.
    pub(crate) source_name: Option<String>,
    /// 1-based line in the user's script of the first stack frame matching
    /// `source_name`, when the engine provided a stack.
    pub(crate) line: Option<u32>,
    /// 1-based column of that frame, when available.
    pub(crate) column: Option<u32>,
    /// The engine stack trace with line numbers adjusted to the user's script,
    /// when the engine provided one.
    pub(crate) stack: Option<String>,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Errors produced by macro or hook execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MacroError {
    /// The script threw an exception.
    Script(ScriptError),
    /// The script completed without throwing, but its completion value is not
    /// a string. `type_name` is the ECMAScript `typeof` of the value; the
    /// rendered message matches the OverPy reference (`src/quickjs.ts`).
    InvalidResult { type_name: String },
    /// Engine setup failure (runtime/context creation, builtin helper install).
    Internal(String),
}

impl fmt::Display for MacroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MacroError::Script(e) => e.fmt(f),
            MacroError::InvalidResult { type_name } => write!(
                f,
                "JavaScript macro returned value with type of {type_name}, expected string. Try using .toString()"
            ),
            MacroError::Internal(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for MacroError {}
