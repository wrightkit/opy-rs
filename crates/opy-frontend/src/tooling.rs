//! Workshop-independent tooling APIs: check a project and query the resolved
//! semantic model.
//!
//! This module is the public tooling surface for Wright and other consumers
//! that want to parse, check, inspect, and reason about OPY projects before
//! any Workshop backend is connected (issue #7):
//!
//! * [`check`] / [`check_with_overlay`] run the full frontend pipeline
//!   (preprocess → parse → resolve) on a main file plus its includes and
//!   return every structured diagnostic together with the file registry,
//!   without requiring lowering to any Workshop backend. Resolution stops at
//!   the Opy HIR semantic model ([`hir::Program`]); Workshop emission,
//!   decompilation, and catalog behavior are deliberately out of scope here.
//! * [`SemanticModel`] wraps the resolved program and answers semantic
//!   queries: declarations, rule listing, symbol/reference lookup by name or
//!   span, custom-enum declarations, macro defines, and source provenance
//!   (span → file id, path, line/col).
//!
//! Diagnostics contract: every [`Diagnostic`] carries a stable machine code,
//! a severity, a human message, and — when known — a resolved source location
//! (`path:line:col` through the file registry). Codes are the same ones the
//! compile pipeline emits (`lex-error`, `parse-error`, `unknown-identifier`,
//! `unknown-action`, `include-not-found`, …); see
//! `docs/opy/tooling-api.md` for the full table.
//!
//! Parse diagnostics are collected in full (the parser recovers at statement
//! boundaries); semantic-resolution diagnostics follow the compile contract
//! and report the first error, so `check` never disagrees with `compile`
//! about whether a project is clean.

use std::path::Path;

use serde::Serialize;

use crate::cst;
use crate::diag::{FrontendError, Position, Span};
use crate::hir;
use crate::hir::types::{
    Declaration, Define, Expr as HirExpr, RuleEntry, SourceFile, Stmt as HirStmt,
};
use crate::preprocess::{FileRecord, PreprocessOutcome};

/// The outcome of [`check`]: structured diagnostics plus the resolved model.
///
/// `model` is present exactly when `diagnostics` is empty (a clean project);
/// `files` is the frontend file registry (main file id 0, then one entry per
/// include) and is retained even on failure so diagnostics map to real
/// sources.
#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub diagnostics: Vec<Diagnostic>,
    pub model: Option<SemanticModel>,
    pub files: Vec<FileRecord>,
    /// The declared `#!postCompileHook` script, when the source declared one
    /// and the project checked clean.
    ///
    /// This is the declaration record, not an execution result: the frontend
    /// recognizes, parses, validates, and records the directive, but never
    /// executes the hook. Execution against the final Workshop text is
    /// lowering-dependent (workshop-rs emission, issue #8); the frontend
    /// never fabricates a Workshop payload.
    pub post_compile_hook: Option<crate::preprocess::PostCompileHook>,
}

impl CheckOutcome {
    /// Whether the project checked clean.
    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Check one `.opy` project: preprocess (includes/defines) → parse (CST) →
/// resolve (Opy HIR). `main_path` is the display path recorded in the file
/// registry; `root` is the include base. No Workshop backend is required.
pub fn check(source: &str, main_path: &str, root: &Path) -> CheckOutcome {
    check_with_overlay(source, main_path, root, &std::collections::BTreeMap::new())
}

/// [`check`] with open-document overlays (unsaved editor buffers participate
/// in include resolution, see [`crate::preprocess::preprocess_with_overlay`]).
pub fn check_with_overlay(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> CheckOutcome {
    let PreprocessOutcome { result, files } =
        crate::preprocess::preprocess_with_overlay_outcome(source, main_path, root, overlay);
    let preprocessed = match result {
        Ok((preprocessed, _)) => preprocessed,
        Err(error) => {
            return CheckOutcome {
                diagnostics: vec![Diagnostic::from_error(error, &files)],
                model: None,
                files,
                post_compile_hook: None,
            };
        }
    };
    let parsed = crate::parser::parse(&preprocessed.tokens);
    let Some(mut program) = parsed.program else {
        // The parser recovers at statement boundaries; every collected error
        // is reported (the compile pipeline reads only the first).
        return CheckOutcome {
            diagnostics: parsed
                .errors
                .iter()
                .map(|error| Diagnostic::from_error(error.clone(), &files))
                .collect(),
            model: None,
            files,
            post_compile_hook: None,
        };
    };
    // Parse the extracted settings block into the CST; errors flow through
    // the same diagnostic path (#86).
    if let Some(block) = &preprocessed.settings {
        match crate::settings::parse_block(block) {
            Ok(parsed_settings) => program.settings = Some(parsed_settings),
            Err(error) => {
                return CheckOutcome {
                    diagnostics: vec![Diagnostic::from_error(error, &files)],
                    model: None,
                    files,
                    post_compile_hook: None,
                };
            }
        }
    }
    let defines = preprocessed
        .defines
        .iter()
        .map(|define| Define {
            name: define.name.clone(),
            is_function: define.is_function,
            span: define.span.map(Into::into),
        })
        .collect();
    let hir_files = files
        .iter()
        .map(|file| hir::types::SourceFile {
            id: file.id,
            path: file.path.clone(),
        })
        .collect();
    match crate::lower::lower(&program, hir_files, defines) {
        Ok(hir) => CheckOutcome {
            diagnostics: Vec::new(),
            model: Some(SemanticModel::build(hir, &program)),
            files,
            // The directive was parsed, validated, and recorded by
            // preprocessing; the frontend never executes the hook (real hook
            // execution receives the final Workshop text and is
            // lowering-dependent, issue #8).
            post_compile_hook: preprocessed.post_compile_hook,
        },
        Err(error) => CheckOutcome {
            diagnostics: vec![Diagnostic::from_error(error, &files)],
            model: None,
            files,
            post_compile_hook: None,
        },
    }
}

/// A structured, source-attributed diagnostic.
///
/// `code` is the stable machine contract (see the module docs and
/// `docs/opy/tooling-api.md`); `message` is human wording and not part of the
/// contract; `span` resolves through the file registry when known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub span: Option<SourceLocation>,
}

impl Diagnostic {
    fn from_error(error: FrontendError, files: &[FileRecord]) -> Diagnostic {
        Diagnostic {
            severity: DiagnosticSeverity::Error,
            code: error.code,
            message: error.message,
            span: error.span.and_then(|span| resolve_record_span(span, files)),
        }
    }
}

/// The severity of a diagnostic. All frontend diagnostics are errors today;
/// the enum is the machine contract for future warning/note severities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Error,
}

impl DiagnosticSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticSeverity::Error => "error",
        }
    }
}

/// A resolved source location: a span's file id and path (through the file
/// registry) plus its 1-based line/column interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    pub file_id: u32,
    pub path: String,
    pub start: Position,
    pub end: Position,
}

impl SourceLocation {
    /// Recover the frontend span (file id + positions) of this location.
    pub fn to_span(&self) -> Span {
        Span::new(self.file_id, self.start, self.end)
    }
}

fn resolve_span(span: Span, files: &[SourceFile]) -> Option<SourceLocation> {
    let path = files.iter().find(|file| file.id == span.file)?.path.clone();
    Some(SourceLocation {
        file_id: span.file,
        path,
        start: span.start,
        end: span.end,
    })
}

/// Resolve a span through the preprocess file registry (used for
/// diagnostics, where the model may not exist).
fn resolve_record_span(span: Span, files: &[FileRecord]) -> Option<SourceLocation> {
    let path = files.iter().find(|file| file.id == span.file)?.path.clone();
    Some(SourceLocation {
        file_id: span.file,
        path,
        start: span.start,
        end: span.end,
    })
}

/// Map an Opy HIR span (the protocol type) back to the frontend span type;
/// positions are the same 1-based source coordinates carried through
/// lowering.
fn to_frontend_span(span: hir::types::Span) -> Span {
    Span::new(
        span.file,
        Position::new(span.start.line, span.start.col),
        Position::new(span.end.line, span.end.col),
    )
}

/// The resolved program model: the Opy HIR semantic program plus the
/// queryable symbol index and custom-enum declarations.
///
/// Custom enums are not retained in the Opy HIR (they fold to numeric
/// constants at use sites, reference behavior), so they are carried here from
/// the CST to keep declarations queryable.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticModel {
    pub hir: hir::Program,
    pub enums: Vec<EnumDecl>,
    pub symbols: Vec<Symbol>,
}

impl SemanticModel {
    /// Build the queryable model from a resolved HIR program and its parsed
    /// CST (required for custom-enum declarations).
    pub fn build(hir: hir::Program, cst: &cst::Program) -> SemanticModel {
        let enums = cst
            .declarations
            .iter()
            .filter_map(|decl| match decl {
                cst::Decl::Enum { name, members, .. } => Some(EnumDecl {
                    name: name.clone(),
                    members: members
                        .iter()
                        .map(|(member, span)| EnumMember {
                            name: member.clone(),
                            span: resolve_span(*span, &hir.files)
                                .expect("every token span resolves through the file registry"),
                        })
                        .collect(),
                }),
                _ => None,
            })
            .collect();
        let mut model = SemanticModel {
            hir,
            enums,
            symbols: Vec::new(),
        };
        model.index_symbols();
        model
    }

    /// The HIR declarations (globals, players, subroutines, constants,
    /// macros). Custom enums are queried through [`SemanticModel::enums`].
    pub fn declarations(&self) -> &[Declaration] {
        &self.hir.declarations
    }

    /// The rule listing: rules and subroutine definitions.
    pub fn rules(&self) -> &[RuleEntry] {
        &self.hir.rules
    }

    /// The recorded preprocessing defines (macro-expansion provenance).
    pub fn defines(&self) -> &[Define] {
        &self.hir.defines
    }

    /// The custom-enum declarations of the project.
    pub fn enums(&self) -> &[EnumDecl] {
        &self.enums
    }

    /// Every indexed program-scope symbol with its declaration site and
    /// reference sites.
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// The first symbol bound under `name` (a `subroutine` declaration and a
    /// `def` definition of the same name index as separate symbols).
    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| symbol.name == name)
    }

    /// The symbol whose declaration site contains `span`, or — failing that —
    /// the symbol owning a reference site containing `span`.
    pub fn symbol_at(&self, span: Span) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| {
            span_contains(symbol.declaration.to_span(), span)
                || symbol
                    .references
                    .iter()
                    .any(|reference| span_contains(reference.to_span(), span))
        })
    }

    /// Resolve a span to its file id, path, and line/column through the file
    /// registry.
    pub fn provenance(&self, span: Span) -> Option<SourceLocation> {
        resolve_span(span, &self.hir.files)
    }

    /// The registry path of a file id.
    pub fn file(&self, id: u32) -> Option<&str> {
        self.hir
            .files
            .iter()
            .find(|file| file.id == id)
            .map(|file| file.path.as_str())
    }

    /// Index every program-scope binding, then attach resolved reference
    /// sites by name/kind.
    fn index_symbols(&mut self) {
        for decl in &self.hir.declarations {
            let (kind, name, span) = match decl {
                Declaration::GlobalVariable {
                    name,
                    name_span,
                    span,
                    ..
                } => (SymbolKind::Global, name, name_span.or(*span)),
                Declaration::PlayerVariable {
                    name,
                    name_span,
                    span,
                    ..
                } => (SymbolKind::Player, name, name_span.or(*span)),
                Declaration::Subroutine {
                    name,
                    name_span,
                    span,
                    ..
                } => (SymbolKind::Subroutine, name, name_span.or(*span)),
                Declaration::Constant { name, span, .. } => (SymbolKind::Constant, name, *span),
                Declaration::Macro { name, span, .. } => (SymbolKind::Macro, name, *span),
            };
            let Some(span) = span.map(to_frontend_span) else {
                // Foreign payloads may omit spans; such declarations are not
                // addressable and stay out of the index.
                continue;
            };
            let Some(declaration) = resolve_span(span, &self.hir.files) else {
                continue;
            };
            self.symbols.push(Symbol {
                name: name.clone(),
                kind,
                declaration,
                references: Vec::new(),
            });
        }
        for entry in &self.hir.rules {
            let RuleEntry::SubroutineDef {
                name,
                name_span,
                span,
                ..
            } = entry
            else {
                continue;
            };
            let Some(span) = name_span.or(*span).map(to_frontend_span) else {
                continue;
            };
            let Some(declaration) = resolve_span(span, &self.hir.files) else {
                continue;
            };
            self.symbols.push(Symbol {
                name: name.clone(),
                kind: SymbolKind::Def,
                declaration,
                references: Vec::new(),
            });
        }

        let mut sites: Vec<(SymbolKind, String, Span)> = Vec::new();
        for decl in &self.hir.declarations {
            match decl {
                Declaration::GlobalVariable {
                    initializer: Some(initializer),
                    ..
                }
                | Declaration::PlayerVariable {
                    initializer: Some(initializer),
                    ..
                } => Self::collect_expr(initializer, &mut sites),
                Declaration::Constant { value, .. } => Self::collect_expr(value, &mut sites),
                Declaration::Macro { body, .. } => {
                    for stmt in body {
                        Self::collect_stmt(stmt, &mut sites);
                    }
                }
                _ => {}
            }
        }
        for entry in &self.hir.rules {
            match entry {
                RuleEntry::Rule(rule) => {
                    for arg in &rule.event.args {
                        Self::collect_expr(arg, &mut sites);
                    }
                    for condition in &rule.conditions {
                        Self::collect_expr(condition, &mut sites);
                    }
                    for stmt in &rule.actions {
                        Self::collect_stmt(stmt, &mut sites);
                    }
                }
                RuleEntry::SubroutineDef { body, .. } => {
                    for stmt in body {
                        Self::collect_stmt(stmt, &mut sites);
                    }
                }
            }
        }
        for (kind, name, span) in sites {
            self.attach_reference(kind, &name, span);
        }
    }

    /// Record a reference site for the first symbol of `kind` named `name`.
    /// A call site is offered to both the `subroutine` and the `def` binding
    /// kinds so both bindings of a defined subroutine collect their uses.
    fn attach_reference(&mut self, kind: SymbolKind, name: &str, span: Span) {
        let Some(location) = resolve_span(span, &self.hir.files) else {
            return;
        };
        if let Some(index) = self
            .symbols
            .iter()
            .position(|symbol| symbol.kind == kind && symbol.name == name)
        {
            self.symbols[index].references.push(location);
        }
    }

    fn collect_expr(expr: &HirExpr, sites: &mut Vec<(SymbolKind, String, Span)>) {
        match expr {
            HirExpr::Number { .. }
            | HirExpr::String { .. }
            | HirExpr::Bool { .. }
            | HirExpr::Null { .. }
            | HirExpr::Enum { .. }
            | HirExpr::EventPlayer { .. }
            | HirExpr::MacroParam { .. } => {}
            HirExpr::GlobalVar { name, span } | HirExpr::Constant { name, span } => {
                let kind = if matches!(expr, HirExpr::GlobalVar { .. }) {
                    SymbolKind::Global
                } else {
                    SymbolKind::Constant
                };
                if let Some(span) = span {
                    sites.push((kind, name.clone(), to_frontend_span(*span)));
                }
            }
            HirExpr::PlayerVar { name, span, .. } => {
                if let Some(span) = span {
                    sites.push((SymbolKind::Player, name.clone(), to_frontend_span(*span)));
                }
            }
            HirExpr::Member { receiver, .. } => Self::collect_expr(receiver, sites),
            HirExpr::Array { elements, .. } => {
                for element in elements {
                    Self::collect_expr(element, sites);
                }
            }
            HirExpr::Vector { x, y, z, .. } => {
                Self::collect_expr(x, sites);
                Self::collect_expr(y, sites);
                Self::collect_expr(z, sites);
            }
            HirExpr::Call { name, span, args } => {
                // A call may name a declared subroutine (with arguments) or
                // nothing user-declared (a builtin); unresolved names never
                // reach the model. Offer both subroutine binding kinds.
                if let Some(span) = span {
                    sites.push((
                        SymbolKind::Subroutine,
                        name.clone(),
                        to_frontend_span(*span),
                    ));
                    sites.push((SymbolKind::Def, name.clone(), to_frontend_span(*span)));
                }
                for arg in args {
                    Self::collect_expr(arg, sites);
                }
            }
            HirExpr::MacroCall { name, span, args } => {
                if let Some(span) = span {
                    sites.push((SymbolKind::Macro, name.clone(), to_frontend_span(*span)));
                }
                for arg in args {
                    Self::collect_expr(arg, sites);
                }
            }
            HirExpr::ReceiverCall { receiver, args, .. } => {
                // The receiver may be a call (e.g. getPlayersInRadius(...).x)
                // whose name binds a symbol; the call span of the outer node
                // is attributed to the member name, not the receiver.
                Self::collect_expr(receiver, sites);
                for arg in args {
                    Self::collect_expr(arg, sites);
                }
            }
            HirExpr::Binary { left, right, .. } => {
                Self::collect_expr(left, sites);
                Self::collect_expr(right, sites);
            }
            HirExpr::Unary { operand, .. } => Self::collect_expr(operand, sites),
            HirExpr::Index { array, index, .. } => {
                Self::collect_expr(array, sites);
                Self::collect_expr(index, sites);
            }
            HirExpr::Format { args, .. } => {
                for arg in args {
                    Self::collect_expr(arg, sites);
                }
            }
        }
    }

    fn collect_stmt(stmt: &HirStmt, sites: &mut Vec<(SymbolKind, String, Span)>) {
        match stmt {
            HirStmt::Expr { expr, .. } => Self::collect_expr(expr, sites),
            HirStmt::Assign { target, value, .. } => {
                Self::collect_expr(target, sites);
                Self::collect_expr(value, sites);
            }
            HirStmt::If {
                branches, r#else, ..
            } => {
                for branch in branches {
                    Self::collect_expr(&branch.condition, sites);
                    for stmt in &branch.body {
                        Self::collect_stmt(stmt, sites);
                    }
                }
                if let Some(r#else) = r#else {
                    for stmt in r#else {
                        Self::collect_stmt(stmt, sites);
                    }
                }
            }
            HirStmt::For {
                variable,
                iterable,
                body,
                ..
            } => {
                Self::collect_expr(variable, sites);
                Self::collect_expr(iterable, sites);
                for stmt in body {
                    Self::collect_stmt(stmt, sites);
                }
            }
            HirStmt::While {
                condition, body, ..
            } => {
                Self::collect_expr(condition, sites);
                for stmt in body {
                    Self::collect_stmt(stmt, sites);
                }
            }
            HirStmt::CallSubroutine { name, span } => {
                if let Some(span) = span {
                    sites.push((
                        SymbolKind::Subroutine,
                        name.clone(),
                        to_frontend_span(*span),
                    ));
                    sites.push((SymbolKind::Def, name.clone(), to_frontend_span(*span)));
                }
            }
            HirStmt::Pass { .. } => {}
        }
    }
}

/// A custom `enum` declaration (CST-retained; enums fold to constants in the
/// HIR).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumDecl {
    pub name: String,
    pub members: Vec<EnumMember>,
}

/// One custom-enum member with its declaration site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumMember {
    pub name: String,
    pub span: SourceLocation,
}

/// The kind of a program-scope symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SymbolKind {
    Global,
    Player,
    Subroutine,
    Def,
    Constant,
    Macro,
}

/// A program-scope symbol with its declaration site and resolved reference
/// sites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub declaration: SourceLocation,
    pub references: Vec<SourceLocation>,
}

/// Whether the interval `outer` contains the interval `inner` (half-open
/// end positions, so a 1:1 zero-width span is contained by itself).
fn span_contains(outer: Span, inner: Span) -> bool {
    position_leq(outer.start, inner.start) && position_leq(inner.end, outer.end)
}

fn position_leq(a: Position, b: Position) -> bool {
    a.line < b.line || (a.line == b.line && a.col <= b.col)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_source(source: &str) -> CheckOutcome {
        check(source, "main.opy", Path::new(""))
    }

    #[test]
    fn clean_project_has_no_diagnostics_and_a_model() {
        let outcome = check_source(
            "globalvar total = 0\nrule \"r\":\n    @Event global\n    total += 1\n    debug(total)\n",
        );
        assert!(
            outcome.is_clean(),
            "unexpected diagnostics: {:?}",
            outcome.diagnostics
        );
        let model = outcome.model.expect("a clean project resolves");
        assert_eq!(outcome.files.len(), 1);
        assert_eq!(model.declarations().len(), 1);
        assert_eq!(model.rules().len(), 1);
    }

    #[test]
    fn symbols_index_declarations_and_references() {
        let outcome = check_source(
            "globalvar total\nplayervar P\nsubroutine reset\nmacro double(x):\n    x + x\nrule \"r\":\n    @Event eachPlayer\n    total = 1\n    eventPlayer.P = total\n    reset()\n    double(2)\n",
        );
        let model = outcome.model.expect("clean project");
        let names: Vec<(&str, SymbolKind)> = model
            .symbols()
            .iter()
            .map(|symbol| (symbol.name.as_str(), symbol.kind))
            .collect();
        assert_eq!(
            names,
            vec![
                ("total", SymbolKind::Global),
                ("P", SymbolKind::Player),
                ("reset", SymbolKind::Subroutine),
                ("double", SymbolKind::Macro),
            ]
        );
        assert_eq!(model.symbol("total").expect("symbol").references.len(), 2);
        assert_eq!(model.symbol("P").expect("symbol").references.len(), 1);
        let reset = model.symbol("reset").expect("symbol");
        assert_eq!(reset.references.len(), 1);
        assert_eq!(reset.references[0].path, "main.opy");
        assert_eq!(model.symbol("double").expect("symbol").references.len(), 1);
    }

    #[test]
    fn symbol_lookup_by_name_and_span() {
        let outcome =
            check_source("globalvar total\nrule \"r\":\n    @Event global\n    total = 1\n");
        let model = outcome.model.expect("clean project");
        let total = model.symbol("total").expect("symbol by name");
        assert_eq!(total.kind, SymbolKind::Global);
        // The declaration site answers span lookup…
        let at_decl = model
            .symbol_at(total.declaration.to_span())
            .expect("symbol at declaration span");
        assert_eq!(at_decl.name, "total");
        // …and so does a reference site.
        let at_ref = model
            .symbol_at(total.references[0].to_span())
            .expect("symbol at reference span");
        assert_eq!(at_ref.name, "total");
        assert!(
            model
                .symbol_at(Span::new(99, Position::new(1, 1), Position::new(1, 1)))
                .is_none()
        );
    }

    #[test]
    fn provenance_resolves_through_the_file_registry() {
        let outcome =
            check_source("globalvar total\nrule \"r\":\n    @Event global\n    total = 1\n");
        let model = outcome.model.expect("clean project");
        let total = model.symbol("total").expect("symbol");
        let provenance = model
            .provenance(total.references[0].to_span())
            .expect("provenance");
        assert_eq!(provenance.file_id, 0);
        assert_eq!(provenance.path, "main.opy");
        assert_eq!(provenance.start.line, 4);
        assert_eq!(model.file(0), Some("main.opy"));
        assert_eq!(model.file(1), None);
    }

    #[test]
    fn custom_enums_are_queried_from_the_model() {
        let outcome = check_source(
            "globalvar x\nenum Direction:\n    NORTH\n    SOUTH\nrule \"r\":\n    @Event global\n    x = Direction.SOUTH\n",
        );
        let model = outcome.model.expect("clean project");
        assert_eq!(model.enums().len(), 1);
        let direction = &model.enums()[0];
        assert_eq!(direction.name, "Direction");
        let members: Vec<&str> = direction
            .members
            .iter()
            .map(|member| member.name.as_str())
            .collect();
        assert_eq!(members, vec!["NORTH", "SOUTH"]);
        assert!(direction.members[0].span.path.ends_with("main.opy"));
    }

    #[test]
    fn check_reports_every_parse_error() {
        // The parser recovers at statement boundaries; check collects all
        // parse diagnostics (compile reads only the first). The two rules
        // missing their colon and the stray directive line yield three
        // parse-error diagnostics.
        let outcome = check_source("rule \"a\"\n    @Event global\nrule \"b\"\n");
        assert!(!outcome.is_clean());
        assert!(outcome.model.is_none());
        assert_eq!(outcome.diagnostics.len(), 3);
        assert!(
            outcome
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == "parse-error")
        );
    }

    #[test]
    fn diagnostics_carry_severity_code_and_span() {
        let outcome = check_source("rule \"r\":\n    @Event global\n    frobnicate()\n");
        let diagnostic = &outcome.diagnostics[0];
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostic.code, "unknown-action");
        let span = diagnostic.span.as_ref().expect("source-located");
        assert_eq!(span.path, "main.opy");
        assert_eq!(span.start.line, 3);
    }
}
