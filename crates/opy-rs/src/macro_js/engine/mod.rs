#[allow(unsafe_code)]
pub(crate) mod quickjs_ng;

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Completion {
    String(String),
    NonString(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EngineError {
    Exception { message: String, stack: String },
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
