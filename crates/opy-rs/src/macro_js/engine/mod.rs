pub(crate) mod quickjs_ng;

use std::fmt;

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
            EngineError::Exception { message, .. } | EngineError::Internal(message) => {
                f.write_str(message)
            }
        }
    }
}
