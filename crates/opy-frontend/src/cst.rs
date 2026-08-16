//! The frontend's concrete syntax tree (CST).
//!
//! Source-preserving syntax structure with spans on every node, produced by
//! [`crate::parser`] and consumed by [`crate::lower`] (and, in later
//! milestones, language services). Nodes are deliberately close to the Opy
//! HIR contract so lowering stays a small, reviewable mapping; unresolved
//! names and member accesses remain explicit until semantic resolution.

use crate::diag::Span;

/// A parsed program: declarations and rule/subroutine entries.
#[derive(Debug, Clone)]
pub struct Program {
    pub declarations: Vec<Decl>,
    pub rules: Vec<RuleEntry>,
    /// The parsed top-of-file `settings { ... }` block, when present (#86).
    pub settings: Option<Settings>,
}

/// A parsed `settings { ... }` block (JSONC, #86).
#[derive(Debug, Clone)]
pub struct Settings {
    pub span: Span,
    pub children: Vec<SettingsNode>,
}

/// One member of a settings group.
#[derive(Debug, Clone)]
pub enum SettingsNode {
    Group {
        name: String,
        children: Vec<SettingsNode>,
        span: Span,
    },
    Number {
        name: String,
        value: f64,
        span: Span,
    },
    Bool {
        name: String,
        value: bool,
        span: Span,
    },
    String {
        name: String,
        value: String,
        span: Span,
    },
    List {
        name: String,
        elements: Vec<SettingsListElement>,
        span: Span,
    },
}

/// One element of a settings list.
#[derive(Debug, Clone)]
pub struct SettingsListElement {
    pub value: String,
    pub span: Span,
}

/// A program-scope declaration.
#[derive(Debug, Clone)]
pub enum Decl {
    GlobalVariable {
        name: String,
        /// An explicit Workshop index (`globalvar x 100`), when given.
        index: Option<u32>,
        span: Span,
        /// The exact span of the declared identifier token.
        name_span: Span,
        initializer: Option<Expr>,
    },
    PlayerVariable {
        name: String,
        index: Option<u32>,
        span: Span,
        /// The exact span of the declared identifier token.
        name_span: Span,
        initializer: Option<Expr>,
    },
    Subroutine {
        name: String,
        span: Span,
        /// The exact span of the declared identifier token.
        name_span: Span,
    },
    /// A user-defined `enum`; members fold to numeric constants.
    Enum {
        name: String,
        members: Vec<(String, Span)>,
        span: Span,
    },
    /// A `macro` declaration with parameterized statement body.
    Macro {
        name: String,
        args: Vec<String>,
        body: Vec<Stmt>,
        span: Span,
    },
}

/// A rule or a subroutine definition.
#[derive(Debug, Clone)]
pub enum RuleEntry {
    Rule(Rule),
    SubroutineDef {
        name: String,
        span: Span,
        /// The exact span of the defined identifier token in `def name():`.
        name_span: Span,
        body: Vec<Stmt>,
    },
}

/// A rule with its event, conditions, and actions.
#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub span: Span,
    /// The exact span of the rule name inside its string literal.
    pub name_span: Span,
    pub disabled: bool,
    pub event: Event,
    pub conditions: Vec<Expr>,
    pub actions: Vec<Stmt>,
}

/// A rule event or an `@Event` directive.
#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

/// A statement.
#[derive(Debug, Clone)]
pub enum Stmt {
    Expr {
        expr: Expr,
        span: Span,
    },
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
    If {
        branches: Vec<IfBranch>,
        r#else: Option<Vec<Stmt>>,
        span: Span,
    },
    For {
        variable: Expr,
        iterable: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Pass {
        span: Span,
    },
}

/// One condition/body pair of an `if`.
#[derive(Debug, Clone)]
pub struct IfBranch {
    pub condition: Expr,
    pub body: Vec<Stmt>,
}

/// One call argument: either positional (`expr`) or keyword (`name = expr`,
/// issue #110). Keyword arguments keep the name token's exact span so binding
/// diagnostics are source-located on the name (unknown/duplicate keyword) or
/// the value (enum-domain, arity of the value expression) as appropriate.
#[derive(Debug, Clone)]
pub struct CallArg {
    /// The keyword name and its exact span, when this is a `name = expr`
    /// argument.
    pub keyword: Option<(String, Span)>,
    /// The argument's value expression.
    pub value: Expr,
}

/// An expression.
#[derive(Debug, Clone)]
pub enum Expr {
    Number {
        value: f64,
        text: String,
        span: Span,
    },
    String {
        value: String,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Null {
        span: Span,
    },
    Array {
        elements: Vec<Expr>,
        span: Span,
    },
    /// A plain function call.
    Call {
        name: String,
        args: Vec<CallArg>,
        span: Span,
    },
    /// A call on a receiver (`x.f(...)`).
    ReceiverCall {
        receiver: Box<Expr>,
        name: String,
        args: Vec<CallArg>,
        span: Span,
    },
    /// An unresolved identifier (resolved during lowering).
    Name {
        name: String,
        span: Span,
    },
    /// A member access `x.y` (resolved during lowering).
    Member {
        receiver: Box<Expr>,
        member: String,
        span: Span,
    },
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Unary {
        op: String,
        operand: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    /// The source span of this expression.
    pub fn span(&self) -> Span {
        match self {
            Expr::Number { span, .. }
            | Expr::String { span, .. }
            | Expr::Bool { span, .. }
            | Expr::Null { span }
            | Expr::Array { span, .. }
            | Expr::Call { span, .. }
            | Expr::ReceiverCall { span, .. }
            | Expr::Name { span, .. }
            | Expr::Member { span, .. }
            | Expr::Index { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Unary { span, .. } => *span,
        }
    }
}

impl CallArg {
    /// The source span of this argument: the keyword name when keyword, the
    /// value expression otherwise.
    pub fn span(&self) -> Span {
        match &self.keyword {
            Some((_, name_span)) => {
                let end = self.value.span().end;
                Span::new(name_span.file, name_span.start, end)
            }
            None => self.value.span(),
        }
    }
}
