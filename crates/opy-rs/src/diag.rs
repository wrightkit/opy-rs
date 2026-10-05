//! Frontend diagnostics: structured, source-located failures.
//!
//! Every frontend failure is a [`OpyError`] with a stable `code`, a
//! human message, and an optional source span. The `code` is the machine
//! contract; wording is not.
//!
//! [`Position`] is serializable so tooling surfaces ([`crate::tooling`]) can
//! emit resolved source locations as JSON without introducing a parallel
//! position type.

/// A structured frontend error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpyError {
    /// A stable machine-readable code, e.g. `parse-error`.
    pub code: String,
    /// Human-readable message (not part of the machine contract).
    pub message: String,
    /// The offending source region, when known.
    pub span: Option<Span>,
    /// The valid spellings nearest the rejected input, when the error names
    /// something resolvable (an unknown builtin, enum member, or settings
    /// key). Empty when the error has no candidate set.
    pub candidates: Vec<String>,
}

/// A source span in the frontend's file registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub file: u32,
    pub start: Position,
    pub end: Position,
}

/// A 1-based line/column position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

impl Position {
    pub const fn new(line: u32, col: u32) -> Position {
        Position { line, col }
    }
}

pub(crate) fn shift_position(position: Position, origin: Position) -> Position {
    Position::new(
        origin.line + position.line.saturating_sub(1),
        if position.line == 1 {
            origin.col + position.col.saturating_sub(1)
        } else {
            position.col
        },
    )
}

impl Span {
    pub fn new(file: u32, start: Position, end: Position) -> Span {
        Span { file, start, end }
    }
}

/// A crate-wide result alias.
pub type OpyResult<T> = Result<T, OpyError>;

impl OpyError {
    /// An error without a source span.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> OpyError {
        OpyError {
            code: code.into(),
            message: message.into(),
            span: None,
            candidates: Vec::new(),
        }
    }

    /// Attach the candidate spellings nearest the rejected input.
    pub fn with_candidates(mut self, candidates: Vec<String>) -> OpyError {
        self.candidates = candidates;
        self
    }

    /// An error at a source position.
    pub fn at(code: impl Into<String>, message: impl Into<String>, span: Span) -> OpyError {
        OpyError {
            code: code.into(),
            message: message.into(),
            span: Some(span),
            candidates: Vec::new(),
        }
    }
}

impl std::fmt::Display for OpyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for OpyError {}
