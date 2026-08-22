//! Serde protocol types for the `wright/opy-hir` protocol, major version 1.
//!
//! These types mirror the Opy HIR v1 specification (`docs/hir/opy-hir-v1.md`,
//! `wright/opy-hir` v1.1.0 wire payloads). Unknown
//! fields on known nodes are tolerated so an additive producer change inside
//! the same major version does not break the consumer; unknown node *kinds*
//! are rejected during validation (see [`super::validate`]).

use serde::{Deserialize, Serialize};

/// The `wright/opy-hir` protocol name.
pub const PROTOCOL_NAME: &str = "wright/opy-hir";
/// The protocol major version this consumer understands.
pub const PROTOCOL_MAJOR: u32 = 1;

/// The number of Workshop variable slots per variable set.
///
/// OverPy's `defaultVarNames` table covers exactly these slots: the 128
/// uppercase letter spellings `A`–`Z`, `AA`–`AZ`, …, `DA`–`DX` (bijective
/// base-26, Excel-style, zero-based). The pinned OverPy 9.7.10 reference
/// accepts these names as *implicit* global variables anywhere a variable
/// may appear — including as a `for ... in range(...)` loop binder — without
/// requiring a `globalvar` declaration, and assigns each its fixed slot.
/// Names outside the table (lowercase, mixed case, longer spellings) stay
/// ordinary unresolved identifiers (see `docs/opy/support-matrix.md`).
const DEFAULT_VAR_SLOTS: u32 = 128;

/// The fixed Workshop slot for an OverPy default variable name (`A`–`Z`,
/// `AA`–`AZ`, …, `DA`–`DX`), or `None` for any other spelling.
///
/// The index is the zero-based bijective base-26 value of the uppercase
/// spelling (`A` = 0, `Z` = 25, `AA` = 26, `DX` = 127); spellings beyond the
/// 128-slot table (`DY`, `EA`, …, three-letter names, lowercase) return
/// `None`, matching the pinned reference's `defaultVarNames` table exactly.
pub fn default_var_index(name: &str) -> Option<u32> {
    if name.is_empty() || name.len() > 2 {
        return None;
    }
    let mut value: u32 = 0;
    for byte in name.bytes() {
        if !byte.is_ascii_uppercase() {
            return None;
        }
        value = value * 26 + u32::from(byte - b'A' + 1);
    }
    let index = value - 1;
    (index < DEFAULT_VAR_SLOTS).then_some(index)
}

/// Protocol envelope identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Protocol {
    pub name: String,
    pub version: String,
}

/// Producer identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generator {
    pub name: String,
    pub version: String,
    pub frontend: String,
}

/// A source file in the protocol's file registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceFile {
    pub id: u32,
    pub path: String,
}

/// A preprocessing define recorded for provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Define {
    pub name: String,
    #[serde(default)]
    pub is_function: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

/// A 1-based, half-open source interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub file: u32,
    pub start: Position,
    pub end: Position,
}

/// A 1-based line/column position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

/// A top-level program payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub protocol: Protocol,
    pub generator: Generator,
    pub files: Vec<SourceFile>,
    #[serde(default)]
    pub defines: Vec<Define>,
    #[serde(default)]
    pub declarations: Vec<Declaration>,
    #[serde(default)]
    pub rules: Vec<RuleEntry>,
    /// The typed custom-game-settings block, when the source had one (#86).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    /// Frontend preprocessing state. Workshop execution of optimizer,
    /// translation, and replacement choices remains lowering-dependent.
    #[serde(default)]
    pub preprocessing: PreprocessingState,
}

/// The source-level preprocessing state observed by the frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PreprocessingState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_file: Option<DirectiveValue>,
    pub allow_macro_redeclaration: bool,
    #[serde(default)]
    pub rule_prefix: Option<DirectiveValue>,
    #[serde(default)]
    pub rule_prefix_template: Option<DirectiveValue>,
    #[serde(default)]
    pub translations: Option<TranslationState>,
    #[serde(default)]
    pub optimization: OptimizationState,
    #[serde(default)]
    pub replacements: Vec<DirectiveValue>,
    #[serde(default)]
    pub suppressed_warnings: Vec<String>,
    #[serde(default)]
    pub directives: Vec<DirectiveRecord>,
}

/// A directive value plus its source provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectiveValue {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

/// Translation language selection. Locale/catalog data is intentionally not
/// represented here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationState {
    pub languages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

/// Frontend-visible optimization controls. The optimizer itself is outside
/// this repository and remains lowering-dependent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptimizationState {
    pub enabled: bool,
    pub for_size: bool,
    pub for_size_aggressive: bool,
    pub strict: bool,
}

impl Default for OptimizationState {
    fn default() -> Self {
        Self {
            enabled: true,
            for_size: false,
            for_size_aggressive: false,
            strict: false,
        }
    }
}

/// One preprocessing event, retained so block-scoped state transitions remain
/// inspectable without executing a backend optimizer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectiveRecord {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub scope_col: u32,
    #[serde(default)]
    pub scope_depth: u32,
    #[serde(default)]
    pub state: PreprocessingSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PreprocessingSnapshot {
    pub allow_macro_redeclaration: bool,
    pub optimization: OptimizationState,
    #[serde(default)]
    pub rule_prefix: Option<String>,
    #[serde(default)]
    pub rule_prefix_template: Option<String>,
    #[serde(default)]
    pub translations: Option<Vec<String>>,
    #[serde(default)]
    pub replacements: Vec<String>,
}

/// A custom-game-settings block (`settings { ... }`, #86).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    #[serde(default)]
    pub children: Vec<SettingsNode>,
}

/// One member of a settings group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SettingsNode {
    Group {
        name: String,
        #[serde(default)]
        children: Vec<SettingsNode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Number {
        name: String,
        value: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Bool {
        name: String,
        value: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    String {
        name: String,
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    List {
        name: String,
        #[serde(default)]
        elements: Vec<SettingsListElement>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
}

impl SettingsNode {
    /// The source span of this node, if any.
    pub fn span(&self) -> Option<&Span> {
        match self {
            SettingsNode::Group { span, .. }
            | SettingsNode::Number { span, .. }
            | SettingsNode::Bool { span, .. }
            | SettingsNode::String { span, .. }
            | SettingsNode::List { span, .. } => span.as_ref(),
        }
    }
}

/// One element of a settings list (corpus lists are all strings).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsListElement {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

/// A program-scope symbol declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Declaration {
    GlobalVariable {
        name: String,
        #[serde(default)]
        index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
        /// The exact span of the declared identifier token.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name_span: Option<Span>,
        #[serde(default)]
        initializer: Option<Box<Expr>>,
    },
    PlayerVariable {
        name: String,
        #[serde(default)]
        index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
        /// The exact span of the declared identifier token.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name_span: Option<Span>,
        #[serde(default)]
        initializer: Option<Box<Expr>>,
    },
    Subroutine {
        name: String,
        #[serde(default)]
        index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
        /// The exact span of the declared identifier token.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name_span: Option<Span>,
    },
    Constant {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
        value: Box<Expr>,
    },
    Macro {
        name: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
        #[serde(default)]
        body: Vec<Stmt>,
    },
}

/// An entry in `rules`: a rule or a subroutine definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuleEntry {
    /// A rule: an object without a `kind` tag.
    Rule(Rule),
    /// A subroutine definition: `{ "kind": "subroutineDef", ... }`.
    SubroutineDef {
        #[serde(rename = "kind")]
        kind: String,
        name: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        source_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
        /// The exact span of the defined identifier token in `def name():`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name_span: Option<Span>,
        #[serde(default)]
        body: Vec<Stmt>,
        #[serde(default)]
        annotations: Vec<Annotation>,
    },
}

/// A rule with its event, conditions, and actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// The exact span of the rule name inside its string literal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_span: Option<Span>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub delimiter: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_page: Option<String>,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    pub event: Event,
    #[serde(default)]
    pub conditions: Vec<Expr>,
    #[serde(default)]
    pub actions: Vec<Stmt>,
}

/// A source annotation retained on a rule or subroutine definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub name: String,
    #[serde(default)]
    pub args: Vec<AnnotationArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationArg {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

/// A rule event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    #[serde(default)]
    pub args: Vec<Expr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

/// A statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Stmt {
    Expr {
        expr: Box<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    If {
        branches: Vec<IfBranch>,
        #[serde(default)]
        r#else: Option<Vec<Stmt>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    For {
        variable: Box<Expr>,
        iterable: Box<Expr>,
        #[serde(default)]
        body: Vec<Stmt>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    While {
        condition: Box<Expr>,
        #[serde(default)]
        body: Vec<Stmt>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    DoWhile {
        condition: Box<Expr>,
        #[serde(default)]
        body: Vec<Stmt>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Switch {
        value: Box<Expr>,
        #[serde(default)]
        cases: Vec<SwitchCase>,
        #[serde(default, rename = "default")]
        r#default: Option<Vec<Stmt>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Break {
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    CallSubroutine {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Pass {
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
}

/// One condition/body pair of an `if` statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IfBranch {
    pub condition: Box<Expr>,
    #[serde(default)]
    pub body: Vec<Stmt>,
}

impl Stmt {
    /// The source span of this statement, if any.
    pub fn span(&self) -> Option<&Span> {
        match self {
            Stmt::Expr { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::If { span, .. }
            | Stmt::For { span, .. }
            | Stmt::While { span, .. }
            | Stmt::DoWhile { span, .. }
            | Stmt::Switch { span, .. }
            | Stmt::Break { span }
            | Stmt::CallSubroutine { span, .. }
            | Stmt::Pass { span } => span.as_ref(),
        }
    }
}

/// One switch case in the OPY HIR. Cases execute in source order and fall
/// through to subsequent cases until a `break` statement is encountered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwitchCase {
    pub value: Box<Expr>,
    #[serde(default)]
    pub body: Vec<Stmt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

/// An expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Expr {
    Number {
        value: f64,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    String {
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Bool {
        value: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Null {
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Array {
        #[serde(default)]
        elements: Vec<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Dict {
        #[serde(default)]
        entries: Vec<DictEntry>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Comprehension {
        element: Box<Expr>,
        variable: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        variable_span: Option<Span>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index_span: Option<Span>,
        iterable: Box<Expr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<Box<Expr>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Lambda {
        #[serde(default)]
        params: Vec<String>,
        #[serde(default)]
        param_spans: Vec<Option<Span>>,
        body: Box<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    StringModifier {
        modifier: String,
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Local {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Vector {
        x: Box<Expr>,
        y: Box<Expr>,
        z: Box<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Enum {
        #[serde(rename = "type")]
        value_type: String,
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    GlobalVar {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    PlayerVar {
        player: Box<Expr>,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    /// An OPY member expression whose canonical Workshop meaning is deferred
    /// to the integration catalog. The receiver and source member identity
    /// remain available to tooling and lowering.
    Member {
        receiver: Box<Expr>,
        member: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        member_span: Option<Span>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    EventPlayer {
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Constant {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Call {
        name: String,
        #[serde(default)]
        args: Vec<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    ReceiverCall {
        receiver: Box<Expr>,
        name: String,
        #[serde(default)]
        args: Vec<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    MacroCall {
        name: String,
        #[serde(default)]
        args: Vec<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    MacroParam {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Unary {
        op: String,
        operand: Box<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
    Format {
        text: String,
        #[serde(default)]
        args: Vec<Expr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<Span>,
    },
}

/// One key/value pair in an OPY dictionary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DictEntry {
    pub key: Box<Expr>,
    pub value: Box<Expr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

impl Expr {
    /// The source span of this expression, if any.
    pub fn span(&self) -> Option<&Span> {
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
            | Expr::Local { span, .. }
            | Expr::Vector { span, .. }
            | Expr::Enum { span, .. }
            | Expr::GlobalVar { span, .. }
            | Expr::PlayerVar { span, .. }
            | Expr::Member { span, .. }
            | Expr::EventPlayer { span }
            | Expr::Constant { span, .. }
            | Expr::Call { span, .. }
            | Expr::ReceiverCall { span, .. }
            | Expr::MacroCall { span, .. }
            | Expr::MacroParam { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Index { span, .. }
            | Expr::Format { span, .. } => span.as_ref(),
        }
    }

    /// The protocol `kind` of this expression.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Expr::Number { .. } => "number",
            Expr::String { .. } => "string",
            Expr::Bool { .. } => "bool",
            Expr::Null { .. } => "null",
            Expr::Array { .. } => "array",
            Expr::Dict { .. } => "dict",
            Expr::Comprehension { .. } => "comprehension",
            Expr::Lambda { .. } => "lambda",
            Expr::StringModifier { .. } => "stringModifier",
            Expr::Local { .. } => "local",
            Expr::Vector { .. } => "vector",
            Expr::Enum { .. } => "enum",
            Expr::GlobalVar { .. } => "globalVar",
            Expr::PlayerVar { .. } => "playerVar",
            Expr::Member { .. } => "member",
            Expr::EventPlayer { .. } => "eventPlayer",
            Expr::Constant { .. } => "constant",
            Expr::Call { .. } => "call",
            Expr::ReceiverCall { .. } => "receiverCall",
            Expr::MacroCall { .. } => "macroCall",
            Expr::MacroParam { .. } => "macroParam",
            Expr::Binary { .. } => "binary",
            Expr::Unary { .. } => "unary",
            Expr::Index { .. } => "index",
            Expr::Format { .. } => "format",
        }
    }
}
