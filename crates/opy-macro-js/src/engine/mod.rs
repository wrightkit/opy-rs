//! Engine seam: the small internal contract the runtime executes against.
//!
//! Keeping the engine behind this trait is what makes the concrete embedding
//! replaceable (issue #6). Only [`quickjs_ng`] implements it today.

pub(crate) mod quickjs_ng;

use std::fmt;
use std::time::Instant;

use crate::limits::Limits;

/// Result of a script evaluation: either the string completion value or a
/// non-string value described by its ECMAScript `typeof` name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Completion {
    String(String),
    NonString(&'static str),
}

/// Engine-level failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EngineError {
    /// The script threw; `message` is the exception message and `stack` the
    /// raw engine stack trace (may be empty, e.g. for the interrupt abort).
    Exception { message: String, stack: String },
    /// Engine setup failure.
    Internal(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Exception { message, .. } => f.write_str(message),
            EngineError::Internal(message) => f.write_str(message),
        }
    }
}

/// Minimal contract an embedded JavaScript engine must satisfy.
///
/// Implementations must be single-threaded: an engine is created, used, and
/// dropped on one thread.
pub(crate) trait JsEngine {
    /// Creates an engine with `limits` applied at the runtime level (memory
    /// and stack limits must be enforced by the engine itself).
    fn new(limits: &Limits) -> Result<Self, EngineError>
    where
        Self: Sized;

    /// Installs a captured `console.log` on the global object. Subsequent
    /// `console.log(...)` calls append rendered lines to the engine's output
    /// buffer, visible through [`JsEngine::console_output`].
    fn install_console(&mut self) -> Result<(), EngineError>;

    /// Evaluates `source` as a global-scope script named `filename` and
    /// returns its completion value.
    ///
    /// Exceptions are reported with the exception's message and stack; the
    /// stack's `filename:line:column` frames refer to `source` as passed.
    fn evaluate(&mut self, source: &str, filename: &str) -> Result<Completion, EngineError>;

    /// Arms (or disarms) the wall-clock interrupt deadline. Once the deadline
    /// passes, the next interrupt poll inside the engine aborts execution.
    fn set_interrupt_deadline(&mut self, deadline: Option<Instant>);

    /// `console.log` lines captured since the engine was created.
    fn console_output(&self) -> &[String];
}
