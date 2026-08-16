//! Frontend diagnostics: structured, source-located failures.
//!
//! Every frontend failure is a [`FrontendError`] with a stable `code`, a
//! human message, and an optional source span. The `code` is the machine
//! contract; wording is not.

/// A structured frontend error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendError {
    /// A stable machine-readable code, e.g. `parse-error`.
    pub code: String,
    /// Human-readable message (not part of the machine contract).
    pub message: String,
    /// The offending source region, when known.
    pub span: Option<Span>,
}

/// A source span in the frontend's file registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub file: u32,
    pub start: Position,
    pub end: Position,
}

/// A 1-based line/column position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

impl Position {
    pub const fn new(line: u32, col: u32) -> Position {
        Position { line, col }
    }
}

impl Span {
    pub fn new(file: u32, start: Position, end: Position) -> Span {
        Span { file, start, end }
    }
}

/// A crate-wide result alias.
pub type FrontendResult<T> = Result<T, FrontendError>;

impl FrontendError {
    /// An error without a source span.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> FrontendError {
        FrontendError {
            code: code.into(),
            message: message.into(),
            span: None,
        }
    }

    /// An error at a source position.
    pub fn at(code: impl Into<String>, message: impl Into<String>, span: Span) -> FrontendError {
        FrontendError {
            code: code.into(),
            message: message.into(),
            span: Some(span),
        }
    }
}

impl std::fmt::Display for FrontendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for FrontendError {}
