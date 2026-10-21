use crate::diag::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub declarations: Vec<Decl>,
    pub rules: Vec<RuleEntry>,
    pub top_level: Vec<TopLevel>,
    pub settings: Option<Settings>,
}

#[derive(Debug, Clone)]
pub enum TopLevel {
    Declaration(Decl),
    Rule(RuleEntry),
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub span: Span,
    pub children: Vec<SettingsNode>,
}

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
    Raw {
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

#[derive(Debug, Clone)]
pub struct SettingsListElement {
    pub value: String,
    pub expr: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Decl {
    GlobalVariable {
        name: String,
        index: Option<u32>,
        span: Span,
        name_span: Span,
        initializer: Option<Expr>,
    },
    PlayerVariable {
        name: String,
        index: Option<u32>,
        span: Span,
        name_span: Span,
        initializer: Option<Expr>,
    },
    Subroutine {
        name: String,
        index: Option<u32>,
        span: Span,
        name_span: Span,
    },
    Enum {
        name: String,
        members: Vec<(String, Option<Expr>, Span)>,
        span: Span,
    },
    Constant {
        name: String,
        value: Expr,
        span: Span,
    },
    Macro {
        name: String,
        args: Vec<String>,
        body: Vec<Stmt>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum RuleEntry {
    Rule(Rule),
    SubroutineDef {
        name: String,
        presentation_name: Option<String>,
        span: Span,
        name_span: Span,
        body: Vec<Stmt>,
        annotations: Vec<Annotation>,
        rule_prefix: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub span: Span,
    pub name_span: Span,
    pub disabled: bool,
    pub delimiter: bool,
    pub new_page: Option<String>,
    pub annotations: Vec<Annotation>,
    pub rule_prefix: Option<String>,
    pub event: Event,
    pub conditions: Vec<Expr>,
    pub actions: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<AnnotationArg>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AnnotationArg {
    pub text: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

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
        else_span: Option<Span>,
        end_span: Span,
        /// The chain began with an `elif`/`else` in statement position (no
        /// preceding `if`): the reference emits `Else If`/`Else` markers
        /// rather than `If` (#516).
        orphan: bool,
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
    DoWhile {
        condition: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Switch {
        value: Expr,
        arms: Vec<SwitchArm>,
        span: Span,
    },
    Delete {
        target: Expr,
        span: Span,
    },
    Break {
        span: Span,
    },
    Return {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Goto {
        label: Option<String>,
        offset: Option<Expr>,
        rule_start: bool,
        span: Span,
    },
    Label {
        name: String,
        span: Span,
    },
    Pass {
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum SwitchArm {
    Case {
        value: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Default {
        body: Vec<Stmt>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct IfBranch {
    pub marker: Span,
    pub condition: Expr,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct CallArg {
    pub keyword: Option<(String, Span)>,
    pub value: Expr,
}

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
    Dict {
        entries: Vec<DictEntry>,
        span: Span,
    },
    Comprehension {
        element: Box<Expr>,
        variable: String,
        variable_span: Span,
        index: Option<(String, Span)>,
        iterable: Box<Expr>,
        condition: Option<Box<Expr>>,
        span: Span,
    },
    Lambda {
        params: Vec<(String, Span)>,
        body: Box<Expr>,
        /// `true` when the lambda was written wrapped in parentheses. The
        /// reference detects binder lambdas at the token level, so
        /// `(lambda x: e)` never satisfies a lambda-argument position
        /// (issue #445); the flag lets lowering enforce the same rule
        /// without reparsing source.
        parenthesized: bool,
        span: Span,
    },
    StringModifier {
        /// The modifier letters; the reference accepts combinations like
        /// `fw` and applies them in a fixed pipeline order.
        modifier: String,
        value: String,
        format_text: Option<String>,
        interpolations: Vec<Expr>,
        span: Span,
    },
    Call {
        name: String,
        args: Vec<CallArg>,
        debug_source: Option<String>,
        span: Span,
    },
    ReceiverCall {
        receiver: Box<Expr>,
        name: String,
        args: Vec<CallArg>,
        span: Span,
    },
    Name {
        name: String,
        span: Span,
    },
    Member {
        receiver: Box<Expr>,
        member: String,
        member_span: Span,
        span: Span,
    },
    Type {
        name: String,
        args: Vec<Expr>,
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
    Conditional {
        then_value: Box<Expr>,
        condition: Box<Expr>,
        else_value: Box<Expr>,
        span: Span,
    },
    Unary {
        op: String,
        operand: Box<Expr>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct DictEntry {
    pub key: Expr,
    pub value: Expr,
    pub span: Span,
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Number { span, .. }
            | Expr::String { span, .. }
            | Expr::Bool { span, .. }
            | Expr::Null { span }
            | Expr::Array { span, .. }
            | Expr::Dict { span, .. }
            | Expr::Comprehension { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::StringModifier { span, .. }
            | Expr::Call { span, .. }
            | Expr::ReceiverCall { span, .. }
            | Expr::Name { span, .. }
            | Expr::Member { span, .. }
            | Expr::Type { span, .. }
            | Expr::Index { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Conditional { span, .. }
            | Expr::Unary { span, .. } => *span,
        }
    }
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Expr { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::If { span, .. }
            | Stmt::For { span, .. }
            | Stmt::While { span, .. }
            | Stmt::DoWhile { span, .. }
            | Stmt::Switch { span, .. }
            | Stmt::Delete { span, .. }
            | Stmt::Break { span }
            | Stmt::Return { span }
            | Stmt::Continue { span }
            | Stmt::Goto { span, .. }
            | Stmt::Label { span, .. }
            | Stmt::Pass { span } => *span,
        }
    }
}

impl CallArg {
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
