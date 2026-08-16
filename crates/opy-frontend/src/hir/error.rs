//! Structured errors for Opy HIR v1 ingestion.
//!
//! Every failure carries a stable code, a message, and — when the offending
//! source position is known — a span. Human-readable wording is not part of
//! the stable contract; `code` is.

use crate::hir::types::Span;

/// A structured Opy HIR v1 ingestion error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirError {
    /// The payload is not valid JSON or does not fit the protocol envelope.
    Malformed { code: &'static str, message: String },
    /// Protocol identity or major version is not supported. Reported before
    /// any program-body inspection.
    IncompatibleProtocol { expected: String, received: String },
    /// A node kind the consumer does not understand.
    UnsupportedNode { kind: String, span: Option<Span> },
    /// A structural, provenance, identifier, or reference invariant failed.
    Invalid {
        code: &'static str,
        message: String,
        span: Option<Span>,
    },
}

impl HirError {
    /// Stable machine-readable code for this error.
    pub fn code(&self) -> &'static str {
        match self {
            HirError::Malformed { code, .. } => code,
            HirError::IncompatibleProtocol { .. } => "incompatible-protocol",
            HirError::UnsupportedNode { .. } => "unsupported-node",
            HirError::Invalid { code, .. } => code,
        }
    }

    /// Human-readable message.
    pub fn message(&self) -> String {
        match self {
            HirError::Malformed { message, .. } => message.clone(),
            HirError::IncompatibleProtocol { expected, received } => {
                format!("incompatible protocol: expected {expected}, received {received}")
            }
            HirError::UnsupportedNode { kind, .. } => {
                format!("unsupported node kind '{kind}'")
            }
            HirError::Invalid { message, .. } => message.clone(),
        }
    }

    /// The offending source span, when known.
    pub fn span(&self) -> Option<&Span> {
        match self {
            HirError::UnsupportedNode { span, .. } => span.as_ref(),
            HirError::Invalid { span, .. } => span.as_ref(),
            _ => None,
        }
    }
}

impl std::fmt::Display for HirError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for HirError {}

impl From<serde_json::Error> for HirError {
    fn from(error: serde_json::Error) -> Self {
        HirError::Malformed {
            code: "malformed-payload",
            message: error.to_string(),
        }
    }
}

/// Shorthand for an invalid-payload error with a stable code.
pub(crate) fn invalid(
    code: &'static str,
    message: impl Into<String>,
    span: Option<Span>,
) -> HirError {
    HirError::Invalid {
        code,
        message: message.into(),
        span,
    }
}
