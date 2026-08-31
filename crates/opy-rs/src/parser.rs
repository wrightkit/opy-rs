//! The indentation-aware `.opy` CST parser.
//!
//! Consumes the expanded token stream from [`crate::preprocess`] and builds a
//! [`cst::Program`]. Parsing is deterministic and corpus-backed; malformed
//! input produces structured [`OpyError`]s rather than panics, and the
//! parser recovers at statement/line boundaries so multiple useful errors are
//! reported. The returned [`ParseOutput`] carries either a complete program
//! or the collected errors (never both).

use crate::cst::{
    Annotation, AnnotationArg, CallArg, Decl, DictEntry, Event, Expr, IfBranch, Program, Rule,
    RuleEntry, Stmt, SwitchArm,
};
use crate::diag::{OpyError, Position, Span};
use crate::lexer::{Token, TokenKind};

/// The outcome of a parse.
#[derive(Debug, Default)]
pub struct ParseOutput {
    /// The parsed program, present only when no errors were collected.
    pub program: Option<Program>,
    /// Every structured error collected during the parse.
    pub errors: Vec<OpyError>,
}

/// Parse an expanded token stream into a CST program.
pub fn parse(tokens: &[Token]) -> ParseOutput {
    parse_with_options(tokens, false)
}

/// Parse with the global redeclaration policy observed by the pinned oracle.
pub fn parse_with_options(tokens: &[Token], allow_macro_redeclaration: bool) -> ParseOutput {
    let mut parser = Parser {
        tokens,
        pos: 0,
        errors: Vec::new(),
        allow_macro_redeclaration,
    };
    let program = parser.parse_program();
    if parser.errors.is_empty() {
        ParseOutput {
            program: Some(program),
            errors: Vec::new(),
        }
    } else {
        ParseOutput {
            program: None,
            errors: parser.errors,
        }
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    errors: Vec<OpyError>,
    allow_macro_redeclaration: bool,
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.chars().enumerate().all(|(index, ch)| {
            if index == 0 {
                ch.is_ascii_alphabetic() || ch == '_'
            } else {
                ch.is_ascii_alphanumeric() || ch == '_'
            }
        })
}

fn unquote_annotation_arg(text: &str) -> String {
    text.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(text)
        .to_string()
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> TokenKind {
        self.peek().kind
    }

    fn peek_at(&self, offset: usize) -> &Token {
        &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn skip_newlines(&mut self) {
        while self.peek_kind() == TokenKind::Newline {
            self.advance();
        }
    }

    fn is_ident(&self, text: &str) -> bool {
        self.peek_kind() == TokenKind::Ident && self.peek().text == text
    }

    fn expect_ident(&mut self, what: &str) -> Result<String, ()> {
        if self.peek_kind() == TokenKind::Ident {
            Ok(self.advance().text)
        } else {
            self.error_at_current(format!("expected {what}"));
            Err(())
        }
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> Result<Token, ()> {
        if self.peek_kind() == kind {
            Ok(self.advance())
        } else {
            self.error_at_current(format!("expected {what}"));
            Err(())
        }
    }

    fn error_at_current(&mut self, message: String) {
        let span = self.peek().span;
        self.errors.push(OpyError::at("parse-error", message, span));
    }

    // ---- program ----

    fn parse_program(&mut self) -> Program {
        let mut declarations = Vec::new();
        let mut rules = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof {
                break;
            }
            let rule_prefix = if self.peek_kind() == TokenKind::RulePrefixMarker {
                Some(self.advance().text)
            } else {
                None
            };
            let ok = self.parse_top_level(&mut declarations, &mut rules, rule_prefix);
            if !ok {
                self.recover_line();
            }
        }
        Program {
            declarations,
            rules,
            settings: None,
        }
    }

    fn parse_top_level(
        &mut self,
        declarations: &mut Vec<Decl>,
        rules: &mut Vec<RuleEntry>,
        rule_prefix: Option<String>,
    ) -> bool {
        let token = self.peek();
        if token.kind == TokenKind::Ident {
            match token.text.as_str() {
                "rule" => return self.parse_rule(rules, rule_prefix),
                "def" => return self.parse_def(rules, rule_prefix),
                "globalvar" => return self.parse_variable(declarations, true),
                "playervar" => return self.parse_variable(declarations, false),
                "subroutine" => return self.parse_subroutine(declarations),
                "enum" => return self.parse_enum(declarations),
                "macro" => return self.parse_macro(declarations),
                _ => {}
            }
        }
        self.error_at_current(format!(
            "expected a top-level declaration (rule/def/globalvar/playervar/subroutine/enum/macro) but found '{}'",
            token.text
        ));
        false
    }

    /// Skip to the end of the current line (error recovery).
    fn recover_line(&mut self) {
        while self.peek_kind() != TokenKind::Newline && self.peek_kind() != TokenKind::Eof {
            self.advance();
        }
    }

    // ---- declarations ----

    fn parse_variable(&mut self, declarations: &mut Vec<Decl>, global: bool) -> bool {
        let start = self.advance(); // `globalvar`/`playervar`
        // The name token follows the keyword; its span is the exact declared
        // identifier occurrence (rename targets, not the keyword/statement).
        let name_token = self.peek().clone();
        let name = match self.expect_ident("a variable name after the keyword") {
            Ok(name) => name,
            Err(()) => return false,
        };
        let name_span = if name_token.kind == TokenKind::Ident {
            name_token.span
        } else {
            start.span
        };
        let mut index = None;
        let mut initializer = None;
        if self.peek_kind() == TokenKind::Assign {
            self.advance();
            match self.parse_expr() {
                Ok(expr) => initializer = Some(expr),
                Err(()) => return false,
            }
        } else if self.peek_kind() == TokenKind::Number {
            // `globalvar cakePos 100`: an explicit Workshop variable index.
            let token = self.advance();
            index = token.text.parse::<u32>().ok();
            if index.is_none() {
                self.errors.push(OpyError::at(
                    "parse-error",
                    format!(
                        "invalid variable index '{}' (expected an integer)",
                        token.text
                    ),
                    token.span,
                ));
                return false;
            }
        } else if self.peek_kind() != TokenKind::Newline && self.peek_kind() != TokenKind::Eof {
            self.error_at_current(
                "expected '=', an integer index, or end of line after the variable name"
                    .to_string(),
            );
            return false;
        }
        let end = self.peek().span.start;
        let span = Span::new(start.span.file, start.span.start, end);
        let decl = if global {
            Decl::GlobalVariable {
                name,
                index,
                span,
                name_span,
                initializer,
            }
        } else {
            Decl::PlayerVariable {
                name,
                index,
                span,
                name_span,
                initializer,
            }
        };
        declarations.push(decl);
        true
    }

    fn parse_subroutine(&mut self, declarations: &mut Vec<Decl>) -> bool {
        let start = self.advance();
        // The name token follows the `subroutine` keyword; its span is the
        // exact declared identifier occurrence.
        let name_token = self.peek().clone();
        let name = match self.expect_ident("a subroutine name") {
            Ok(name) => name,
            Err(()) => return false,
        };
        let name_span = if name_token.kind == TokenKind::Ident {
            name_token.span
        } else {
            start.span
        };
        let end = self.peek().span.start;
        declarations.push(Decl::Subroutine {
            name,
            span: Span::new(start.span.file, start.span.start, end),
            name_span,
        });
        true
    }

    fn parse_enum(&mut self, declarations: &mut Vec<Decl>) -> bool {
        let start = self.advance();
        let name = match self.expect_ident("an enum name") {
            Ok(name) => name,
            Err(()) => return false,
        };
        if self
            .expect(TokenKind::Colon, "':' after the enum name")
            .is_err()
        {
            return false;
        }
        let line_indent = start.span.start.col;
        let body_indent = match self.block_indent(line_indent) {
            Some(indent) => indent,
            None => return false,
        };
        let mut members = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof || self.peek().span.start.col < body_indent {
                break;
            }
            if self.peek_kind() == TokenKind::Ident {
                let member = self.advance();
                let member_span = member.span;
                if !self.allow_macro_redeclaration
                    && members.iter().any(|(name, _)| name == &member.text)
                {
                    self.errors.push(OpyError::at(
                        "macro-redeclaration",
                        format!("enum member '{name}.{}' is already defined", member.text),
                        member_span,
                    ));
                }
                members.push((member.text, member_span));
            } else {
                self.error_at_current("expected an enum member name".to_string());
                self.recover_line();
                continue;
            }
            if self.peek_kind() == TokenKind::Comma {
                self.advance();
            } else {
                // A member must end the line (or be comma-separated).
                if self.peek_kind() != TokenKind::Newline && self.peek_kind() != TokenKind::Eof {
                    self.error_at_current("expected ',' after the enum member".to_string());
                    self.recover_line();
                    continue;
                }
            }
        }
        declarations.push(Decl::Enum {
            name,
            members,
            span: start.span,
        });
        true
    }

    fn parse_macro(&mut self, declarations: &mut Vec<Decl>) -> bool {
        let start = self.advance();
        let name_token = self.peek().clone();
        let name = match self.expect_ident("a macro name") {
            Ok(name) => name,
            Err(()) => return false,
        };
        let args = match self.parse_param_list() {
            Some(args) => args,
            None => return false,
        };
        if self
            .expect(TokenKind::Colon, "':' after the macro signature")
            .is_err()
        {
            return false;
        }
        let line_indent = start.span.start.col;
        let body_indent = match self.block_indent(line_indent) {
            Some(indent) => indent,
            None => return false,
        };
        let body = self.parse_block(body_indent);
        if !self.allow_macro_redeclaration
            && declarations.iter().any(|declaration| {
                matches!(declaration, Decl::Macro { name: existing, .. } if existing == &name)
            })
        {
            self.errors.push(OpyError::at(
                "macro-redeclaration",
                format!("macro '{name}' is already defined"),
                name_token.span,
            ));
        }
        declarations.push(Decl::Macro {
            name,
            args,
            body,
            span: start.span,
        });
        true
    }

    fn parse_param_list(&mut self) -> Option<Vec<String>> {
        if self.expect(TokenKind::LParen, "'('").is_err() {
            return None;
        }
        let mut params = Vec::new();
        self.skip_newlines();
        if self.peek_kind() == TokenKind::RParen {
            self.advance();
            return Some(params);
        }
        loop {
            match self.expect_ident("a parameter name") {
                Ok(name) => params.push(name),
                Err(()) => return None,
            }
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Comma {
                self.advance();
                self.skip_newlines();
            } else {
                break;
            }
        }
        if self.expect(TokenKind::RParen, "')'").is_err() {
            return None;
        }
        Some(params)
    }

    // ---- rules and definitions ----

    fn parse_rule(&mut self, rules: &mut Vec<RuleEntry>, rule_prefix: Option<String>) -> bool {
        let start = self.advance();
        let name = match self.peek_kind() {
            TokenKind::String => self.advance().text,
            _ => {
                self.error_at_current("expected a rule name string after `rule`".to_string());
                return false;
            }
        };
        let name_token_span = self.tokens[self.pos.saturating_sub(1)].span;
        // The exact rule-name occurrence is the string content between the
        // quotes (the `"name"` token itself spans the quotes).
        let name_span = Span::new(
            name_token_span.file,
            Position::new(name_token_span.start.line, name_token_span.start.col + 1),
            Position::new(
                name_token_span.end.line,
                name_token_span
                    .end
                    .col
                    .saturating_sub(1)
                    .max(name_token_span.start.col + 1),
            ),
        );
        if self
            .expect(TokenKind::Colon, "':' after the rule name")
            .is_err()
        {
            return false;
        }
        let line_indent = start.span.start.col;
        let body_indent = match self.block_indent(line_indent) {
            Some(indent) => indent,
            None => return false,
        };
        let mut event = None;
        let mut conditions = Vec::new();
        let mut annotations = Vec::new();
        let mut disabled = false;
        let mut delimiter = false;
        let mut new_page = None;
        let mut actions = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof || self.peek().span.start.col < body_indent {
                break;
            }
            if self.peek_kind() == TokenKind::At {
                if !self.parse_directive(
                    &mut event,
                    &mut conditions,
                    &mut annotations,
                    &mut disabled,
                    &mut delimiter,
                    &mut new_page,
                    false,
                ) {
                    self.recover_line();
                }
                continue;
            }
            match self.parse_statement() {
                Ok(stmt) => actions.push(stmt),
                Err(()) => self.recover_line(),
            }
        }
        rules.push(RuleEntry::Rule(Rule {
            name,
            span: Span::new(start.span.file, start.span.start, name_token_span.end),
            name_span,
            disabled,
            delimiter,
            new_page,
            annotations,
            rule_prefix,
            event: event.unwrap_or_else(|| Event {
                name: "global".to_string(),
                args: Vec::new(),
                span: start.span,
            }),
            conditions,
            actions,
        }));
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_directive(
        &mut self,
        event: &mut Option<Event>,
        conditions: &mut Vec<Expr>,
        annotations: &mut Vec<Annotation>,
        disabled: &mut bool,
        delimiter: &mut bool,
        new_page: &mut Option<String>,
        subroutine: bool,
    ) -> bool {
        let at = self.advance();
        let name = match self.expect_ident("a directive name after '@'") {
            Ok(name) => name,
            Err(()) => return false,
        };
        if matches!(
            name.as_str(),
            "Event" | "Team" | "Slot" | "Hero" | "Name" | "Disabled" | "Delimiter" | "NewPage"
        ) && annotations.iter().any(|annotation| annotation.name == name)
        {
            self.error_at_current(format!("annotation '@{name}' was already declared"));
            return false;
        }
        match name.as_str() {
            "Event" => {
                let event_name = match self.expect_ident("an event name after @Event") {
                    Ok(name) => name,
                    Err(()) => return false,
                };
                let event_annotation_arg = AnnotationArg {
                    text: event_name.clone(),
                    span: self.tokens[self.pos.saturating_sub(1)].span,
                };
                let mut args = Vec::new();
                if self.peek_kind() == TokenKind::LParen
                    && self.parse_event_args(&mut args).is_err()
                {
                    return false;
                }
                let end = self.peek().span.start;
                *event = Some(Event {
                    name: event_name,
                    args,
                    span: Span::new(at.span.file, at.span.start, end),
                });
                annotations.push(Annotation {
                    name,
                    args: vec![event_annotation_arg],
                    span: Span::new(at.span.file, at.span.start, end),
                });
                true
            }
            "Condition" => {
                let start = self.pos;
                match self.parse_expr() {
                    Ok(expr) => {
                        let end = self.peek().span.start;
                        conditions.push(expr);
                        annotations.push(Annotation {
                            name,
                            args: vec![self.raw_annotation_arg(start, self.pos)],
                            span: Span::new(at.span.file, at.span.start, end),
                        });
                        true
                    }
                    Err(()) => false,
                }
            }
            "Team" | "Slot" | "Hero" => {
                let args = self.consume_annotation_args();
                if args.len() != 1 {
                    self.error_at_current(format!("@{name} expects exactly one argument"));
                    return false;
                }
                if subroutine {
                    self.error_at_current(format!("@{name} is not valid on a subroutine"));
                    return false;
                }
                if (name == "Slot"
                    && annotations
                        .iter()
                        .any(|annotation| annotation.name == "Hero"))
                    || (name == "Hero"
                        && annotations
                            .iter()
                            .any(|annotation| annotation.name == "Slot"))
                {
                    self.error_at_current("@Slot and @Hero cannot be used together".to_string());
                    return false;
                }
                let end = self.peek().span.start;
                annotations.push(Annotation {
                    name,
                    args,
                    span: Span::new(at.span.file, at.span.start, end),
                });
                true
            }
            "Name" => {
                let args = self.consume_annotation_args();
                if args.len() != 1 || !self.annotation_arg_is_string(&args[0]) {
                    self.error_at_current(
                        "@Name expects exactly one plain string literal".to_string(),
                    );
                    return false;
                }
                if !subroutine {
                    self.error_at_current(
                        "@Name is only supported on subroutine definitions".to_string(),
                    );
                    return false;
                }
                let end = self.peek().span.start;
                annotations.push(Annotation {
                    name,
                    args,
                    span: Span::new(at.span.file, at.span.start, end),
                });
                true
            }
            "SuppressWarnings" => {
                let args = self.consume_annotation_args();
                if args.is_empty() || args.iter().any(|arg| !is_identifier(&arg.text)) {
                    self.error_at_current(
                        "@SuppressWarnings expects one or more warning identifiers".to_string(),
                    );
                    return false;
                }
                let end = self.peek().span.start;
                annotations.push(Annotation {
                    name,
                    args,
                    span: Span::new(at.span.file, at.span.start, end),
                });
                true
            }
            "Disabled" => {
                if !self.expect_annotation_end("@Disabled") {
                    return false;
                }
                *disabled = true;
                annotations.push(Annotation {
                    name,
                    args: Vec::new(),
                    span: at.span,
                });
                true
            }
            "Delimiter" => {
                if !self.expect_annotation_end("@Delimiter") {
                    return false;
                }
                *delimiter = true;
                annotations.push(Annotation {
                    name,
                    args: Vec::new(),
                    span: at.span,
                });
                true
            }
            "NewPage" => {
                let args = self.consume_annotation_args();
                if args.len() > 1
                    || args
                        .first()
                        .is_some_and(|arg| !self.annotation_arg_is_string(arg))
                {
                    self.error_at_current(
                        "@NewPage expects at most one plain string literal".to_string(),
                    );
                    return false;
                }
                let end = self.peek().span.start;
                *new_page = args.first().map(|arg| unquote_annotation_arg(&arg.text));
                annotations.push(Annotation {
                    name,
                    args,
                    span: Span::new(at.span.file, at.span.start, end),
                });
                true
            }
            other => {
                self.error_at_current(format!("unsupported directive '@{other}'"));
                false
            }
        }
    }

    fn consume_annotation_args(&mut self) -> Vec<AnnotationArg> {
        let start = self.pos;
        while self.peek_kind() != TokenKind::Newline && self.peek_kind() != TokenKind::Eof {
            self.advance();
        }
        if self.pos == start {
            return Vec::new();
        }
        let tokens = &self.tokens[start..self.pos];
        if tokens.len() == 3
            && tokens[1].kind == TokenKind::Dot
            && tokens[0].kind == TokenKind::Ident
        {
            return vec![AnnotationArg {
                text: tokens.iter().map(|token| token.text.as_str()).collect(),
                span: Span::new(
                    tokens[0].span.file,
                    tokens[0].span.start,
                    tokens[2].span.end,
                ),
            }];
        }
        tokens
            .iter()
            .map(|token| AnnotationArg {
                text: if token.kind == TokenKind::String {
                    format!("\"{}\"", token.text)
                } else {
                    token.text.clone()
                },
                span: token.span,
            })
            .collect()
    }

    fn raw_annotation_arg(&self, start: usize, end: usize) -> AnnotationArg {
        let tokens = &self.tokens[start..end];
        let first = tokens
            .first()
            .map(|token| token.span)
            .unwrap_or(self.peek().span);
        let last = tokens.last().map(|token| token.span).unwrap_or(first);
        AnnotationArg {
            text: tokens.iter().map(|token| token.text.as_str()).collect(),
            span: Span::new(first.file, first.start, last.end),
        }
    }

    fn annotation_arg_is_string(&self, arg: &AnnotationArg) -> bool {
        arg.text.starts_with('"') && arg.text.ends_with('"')
    }

    fn expect_annotation_end(&mut self, name: &str) -> bool {
        if self.peek_kind() == TokenKind::Newline || self.peek_kind() == TokenKind::Eof {
            true
        } else {
            self.error_at_current(format!("{name} takes no arguments"));
            false
        }
    }

    fn parse_def(&mut self, rules: &mut Vec<RuleEntry>, rule_prefix: Option<String>) -> bool {
        let start = self.advance();
        // The name token follows the `def` keyword. `span` covers the
        // definition (`def name`), and `name_span` is the exact identifier
        // occurrence (rename targets, not the keyword).
        let name_token = self.peek().clone();
        let name = match self.expect_ident("a subroutine name after `def`") {
            Ok(name) => name,
            Err(()) => return false,
        };
        let name_span = if name_token.kind == TokenKind::Ident {
            name_token.span
        } else {
            start.span
        };
        let params = match self.parse_param_list() {
            Some(params) => params,
            None => return false,
        };
        if !params.is_empty() {
            self.error_at_current(
                "subroutine parameters are outside the declared support matrix".to_string(),
            );
            return false;
        }
        if self
            .expect(TokenKind::Colon, "':' after the subroutine signature")
            .is_err()
        {
            return false;
        }
        let line_indent = start.span.start.col;
        let body_indent = match self.block_indent(line_indent) {
            Some(indent) => indent,
            None => return false,
        };
        let mut annotations = Vec::new();
        let mut event = None;
        let mut conditions = Vec::new();
        let mut disabled = false;
        let mut delimiter = false;
        let mut new_page = None;
        loop {
            self.skip_newlines();
            if self.peek_kind() != TokenKind::At {
                break;
            }
            if !self.parse_directive(
                &mut event,
                &mut conditions,
                &mut annotations,
                &mut disabled,
                &mut delimiter,
                &mut new_page,
                true,
            ) {
                self.recover_line();
                return false;
            }
        }
        if event.is_some() || !conditions.is_empty() {
            self.error_at_current("subroutines cannot have events or conditions".to_string());
            return false;
        }
        let _ = (disabled, delimiter, new_page);
        let presentation_name = annotations
            .iter()
            .find(|annotation| annotation.name == "Name")
            .and_then(|annotation| annotation.args.first())
            .map(|arg| unquote_annotation_arg(&arg.text));
        let body = self.parse_block(body_indent);
        let span = if name_token.kind == TokenKind::Ident {
            Span::new(start.span.file, start.span.start, name_token.span.end)
        } else {
            start.span
        };
        rules.push(RuleEntry::SubroutineDef {
            name,
            presentation_name,
            span,
            name_span,
            body,
            annotations,
            rule_prefix,
        });
        true
    }

    /// The indentation of the next non-empty line, which must exceed
    /// `line_indent` (an indented block follows the colon).
    fn block_indent(&mut self, line_indent: u32) -> Option<u32> {
        self.skip_newlines();
        if self.peek_kind() == TokenKind::Eof {
            self.error_at_current("expected an indented block".to_string());
            return None;
        }
        let indent = self.peek().span.start.col;
        if indent <= line_indent {
            self.error_at_current("expected an indented block after ':'".to_string());
            return None;
        }
        Some(indent)
    }

    // ---- statements ----

    fn parse_block(&mut self, block_indent: u32) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof {
                break;
            }
            if self.peek().span.start.col < block_indent {
                break;
            }
            if self.peek().span.start.col > block_indent {
                // A deeper indent without an introducer: recover by line.
                self.error_at_current("unexpected indentation".to_string());
                self.recover_line();
                continue;
            }
            match self.parse_statement() {
                Ok(stmt) => stmts.push(stmt),
                Err(()) => self.recover_line(),
            }
        }
        stmts
    }

    fn parse_statement(&mut self) -> Result<Stmt, ()> {
        let token = self.peek();
        if token.kind == TokenKind::Ident {
            match token.text.as_str() {
                "if" => return self.parse_if(),
                "for" => return self.parse_for(),
                "while" => return self.parse_while(),
                "do" => return self.parse_do_while(),
                "switch" => return self.parse_switch(),
                "break" => {
                    let token = self.advance();
                    return Ok(Stmt::Break { span: token.span });
                }
                "pass" => {
                    let start = self.advance();
                    return Ok(Stmt::Pass { span: start.span });
                }
                _ => {}
            }
        }
        self.parse_expr_statement()
    }

    fn parse_expr_statement(&mut self) -> Result<Stmt, ()> {
        let start = self.peek().span;
        let expr = self.parse_expr()?;
        match self.peek_kind() {
            TokenKind::Assign => {
                self.advance();
                let value = self.parse_expr()?;
                let end = self.peek().span.start;
                Ok(Stmt::Assign {
                    target: expr,
                    value,
                    span: Span::new(start.file, start.start, end),
                })
            }
            TokenKind::PlusAssign
            | TokenKind::MinusAssign
            | TokenKind::StarAssign
            | TokenKind::SlashAssign
            | TokenKind::PercentAssign
            | TokenKind::DoubleStarAssign => {
                let op = match self.peek_kind() {
                    TokenKind::PlusAssign => "+",
                    TokenKind::MinusAssign => "-",
                    TokenKind::StarAssign => "*",
                    TokenKind::SlashAssign => "/",
                    TokenKind::PercentAssign => "%",
                    TokenKind::DoubleStarAssign => "**",
                    _ => unreachable!(),
                }
                .to_string();
                self.advance();
                let rhs = self.parse_expr()?;
                let end = self.peek().span.start;
                let value = Expr::Binary {
                    op,
                    left: Box::new(expr.clone()),
                    right: Box::new(rhs),
                    span: Span::new(start.file, start.start, end),
                };
                Ok(Stmt::Assign {
                    target: expr,
                    value,
                    span: Span::new(start.file, start.start, end),
                })
            }
            TokenKind::Increment | TokenKind::Decrement => {
                let operator = self.advance();
                if !matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) {
                    self.error_at_current(
                        "postfix increment/decrement must be a standalone assignment".to_string(),
                    );
                    return Err(());
                }
                let operation = if operator.kind == TokenKind::Increment {
                    "+"
                } else {
                    "-"
                };
                let span = Span::new(start.file, start.start, operator.span.end);
                let value = Expr::Binary {
                    op: operation.to_string(),
                    left: Box::new(expr.clone()),
                    right: Box::new(Expr::Number {
                        value: 1.0,
                        text: "1".to_string(),
                        span: operator.span,
                    }),
                    span,
                };
                Ok(Stmt::Assign {
                    target: expr,
                    value,
                    span,
                })
            }
            _ => {
                let end = self.peek().span.start;
                Ok(Stmt::Expr {
                    expr,
                    span: Span::new(start.file, start.start, end),
                })
            }
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let line_indent = start.span.start.col;
        let condition = self.parse_expr()?;
        if self
            .expect(TokenKind::Colon, "':' after the if condition")
            .is_err()
        {
            return Err(());
        }
        let body_indent = self.block_indent(line_indent).ok_or(())?;
        let body = self.parse_block(body_indent);
        let mut branches = vec![IfBranch { condition, body }];
        let mut r#else = None;
        loop {
            let save = self.pos;
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof || self.peek().span.start.col != line_indent {
                self.pos = save;
                break;
            }
            if self.is_ident("elif") {
                self.advance();
                let condition = match self.parse_expr() {
                    Ok(expr) => expr,
                    Err(()) => return Err(()),
                };
                if self
                    .expect(TokenKind::Colon, "':' after the elif condition")
                    .is_err()
                {
                    return Err(());
                }
                let body_indent = self.block_indent(line_indent).ok_or(())?;
                let body = self.parse_block(body_indent);
                branches.push(IfBranch { condition, body });
            } else if self.is_ident("else") {
                self.advance();
                if self.expect(TokenKind::Colon, "':' after `else`").is_err() {
                    return Err(());
                }
                let body_indent = self.block_indent(line_indent).ok_or(())?;
                let body = self.parse_block(body_indent);
                r#else = Some(body);
                break;
            } else {
                self.pos = save;
                break;
            }
        }
        Ok(Stmt::If {
            branches,
            r#else,
            span: start.span,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let variable = self.parse_postfix()?;
        if !self.is_ident("in") {
            self.error_at_current("expected `in` in the for statement".to_string());
            return Err(());
        }
        self.advance();
        let iterable = self.parse_expr()?;
        if self
            .expect(TokenKind::Colon, "':' after the for header")
            .is_err()
        {
            return Err(());
        }
        let line_indent = start.span.start.col;
        let body_indent = self.block_indent(line_indent).ok_or(())?;
        let body = self.parse_block(body_indent);
        Ok(Stmt::For {
            variable,
            iterable,
            body,
            span: start.span,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let condition = self.parse_expr()?;
        if self
            .expect(TokenKind::Colon, "':' after the while condition")
            .is_err()
        {
            return Err(());
        }
        let line_indent = start.span.start.col;
        let body_indent = self.block_indent(line_indent).ok_or(())?;
        let body = self.parse_block(body_indent);
        Ok(Stmt::While {
            condition,
            body,
            span: start.span,
        })
    }

    fn parse_do_while(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        if self.expect(TokenKind::Colon, "':' after `do`").is_err() {
            return Err(());
        }
        let body_indent = self.block_indent(start.span.start.col).ok_or(())?;
        let body = self.parse_block(body_indent);
        if !self.is_ident("while") {
            self.error_at_current("expected `while` after the do block".to_string());
            return Err(());
        }
        self.advance();
        let condition = self.parse_expr()?;
        if self.peek_kind() != TokenKind::Newline && self.peek_kind() != TokenKind::Eof {
            self.error_at_current("expected the end of the do-while condition".to_string());
            return Err(());
        }
        Ok(Stmt::DoWhile {
            condition,
            body,
            span: start.span,
        })
    }

    fn parse_switch(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let value = self.parse_expr()?;
        if self
            .expect(TokenKind::Colon, "':' after the switch value")
            .is_err()
        {
            return Err(());
        }
        let body_indent = self.block_indent(start.span.start.col).ok_or(())?;
        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof || self.peek().span.start.col < body_indent {
                break;
            }
            if self.peek().span.start.col != body_indent {
                self.error_at_current("unexpected indentation in switch".to_string());
                self.recover_line();
                continue;
            }
            if self.is_ident("case") {
                let case_start = self.advance();
                let case_value = self.parse_expr()?;
                if self
                    .expect(TokenKind::Colon, "':' after the case value")
                    .is_err()
                {
                    return Err(());
                }
                let case_body_indent = self.block_indent(body_indent).ok_or(())?;
                let body = self.parse_block(case_body_indent);
                arms.push(SwitchArm::Case {
                    value: case_value,
                    body,
                    span: case_start.span,
                });
            } else if self.is_ident("default") {
                let default_start = self.advance();
                if self
                    .expect(TokenKind::Colon, "':' after `default`")
                    .is_err()
                {
                    return Err(());
                }
                let default_body_indent = self.block_indent(body_indent).ok_or(())?;
                arms.push(SwitchArm::Default {
                    body: self.parse_block(default_body_indent),
                    span: default_start.span,
                });
                if default_start.span.start.col != body_indent {
                    self.error_at_current("invalid default indentation".to_string());
                    return Err(());
                }
            } else {
                self.error_at_current("expected `case` or `default` in switch".to_string());
                self.recover_line();
            }
        }
        if arms.is_empty() {
            self.errors.push(OpyError::at(
                "parse-error",
                "switch must contain at least one case or default arm".to_string(),
                start.span,
            ));
            return Err(());
        }
        Ok(Stmt::Switch {
            value,
            arms,
            span: start.span,
        })
    }

    // ---- expressions ----

    fn parse_expr(&mut self) -> Result<Expr, ()> {
        let then_value = self.parse_or()?;
        if !self.is_ident("if") {
            return Ok(then_value);
        }

        self.advance();
        let condition = self.parse_or()?;
        if !self.is_ident("else") {
            self.error_at_current("expected `else` in conditional expression".to_string());
            return Err(());
        }
        self.advance();
        // Conditional expressions are right-associative, so a chained form
        // such as `a if c else b if d else e` groups at the else branch.
        let else_value = self.parse_expr()?;
        let span = Span::new(
            then_value.span().file,
            then_value.span().start,
            else_value.span().end,
        );
        Ok(Expr::Conditional {
            then_value: Box::new(then_value),
            condition: Box::new(condition),
            else_value: Box::new(else_value),
            span,
        })
    }

    fn parse_or(&mut self) -> Result<Expr, ()> {
        let mut left = self.parse_and()?;
        while self.is_ident("or") {
            self.advance();
            let right = self.parse_and()?;
            let span = Span::new(left.span().file, left.span().start, right.span().end);
            left = Expr::Binary {
                op: "or".to_string(),
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ()> {
        let mut left = self.parse_not()?;
        while self.is_ident("and") {
            self.advance();
            let right = self.parse_not()?;
            let span = Span::new(left.span().file, left.span().start, right.span().end);
            left = Expr::Binary {
                op: "and".to_string(),
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, ()> {
        if self.is_ident("not") {
            let start = self.advance();
            let operand = self.parse_not()?;
            let end = operand.span().end;
            return Ok(Expr::Unary {
                op: "not".to_string(),
                operand: Box::new(operand),
                span: Span::new(start.span.file, start.span.start, end),
            });
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, ()> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Eq => "==",
                TokenKind::Ne => "!=",
                TokenKind::Lt => "<",
                TokenKind::Le => "<=",
                TokenKind::Gt => ">",
                TokenKind::Ge => ">=",
                _ if self.is_ident("in") => "in",
                _ if self.is_ident("not") && self.peek_at(1).text == "in" => "not in",
                _ => break,
            };
            self.advance();
            if op == "not in" {
                self.advance();
            }
            let right = self.parse_additive()?;
            let span = Span::new(left.span().file, left.span().start, right.span().end);
            left = Expr::Binary {
                op: op.to_string(),
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, ()> {
        let mut left = self.parse_multiplicative()?;
        loop {
            if self.peek_kind() == TokenKind::Decrement
                && !matches!(self.peek_at(1).kind, TokenKind::Newline | TokenKind::Eof)
            {
                let operator = self.advance();
                let operand = self.parse_unary()?;
                let unary = Expr::Unary {
                    op: "-".to_string(),
                    span: Span::new(operator.span.file, operator.span.start, operand.span().end),
                    operand: Box::new(operand),
                };
                let right = self.parse_multiplicative_tail(unary)?;
                let span = Span::new(left.span().file, left.span().start, right.span().end);
                left = Expr::Binary {
                    op: "-".to_string(),
                    left: Box::new(left),
                    right: Box::new(right),
                    span,
                };
                continue;
            }
            let op = match self.peek_kind() {
                TokenKind::Plus => "+",
                TokenKind::Minus => "-",
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            let span = Span::new(left.span().file, left.span().start, right.span().end);
            left = Expr::Binary {
                op: op.to_string(),
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, ()> {
        let left = self.parse_unary()?;
        self.parse_multiplicative_tail(left)
    }

    fn parse_multiplicative_tail(&mut self, mut left: Expr) -> Result<Expr, ()> {
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => "*",
                TokenKind::Slash => "/",
                TokenKind::Percent => "%",
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            let span = Span::new(left.span().file, left.span().start, right.span().end);
            left = Expr::Binary {
                op: op.to_string(),
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ()> {
        if matches!(self.peek_kind(), TokenKind::Minus | TokenKind::Decrement) {
            let start = self.advance();
            let operand = self.parse_unary()?;
            let end = operand.span().end;
            let unary = Expr::Unary {
                op: "-".to_string(),
                operand: Box::new(operand),
                span: Span::new(start.span.file, start.span.start, end),
            };
            if start.kind == TokenKind::Decrement {
                return Ok(Expr::Unary {
                    op: "-".to_string(),
                    operand: Box::new(unary),
                    span: Span::new(start.span.file, start.span.start, end),
                });
            }
            return Ok(unary);
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<Expr, ()> {
        let base = self.parse_postfix()?;
        if self.peek_kind() == TokenKind::DoubleStar {
            self.advance();
            // Right-associative.
            let exponent = self.parse_unary()?;
            let span = Span::new(base.span().file, base.span().start, exponent.span().end);
            return Ok(Expr::Binary {
                op: "**".to_string(),
                left: Box::new(base),
                right: Box::new(exponent),
                span,
            });
        }
        Ok(base)
    }

    fn parse_postfix(&mut self) -> Result<Expr, ()> {
        let mut base = self.parse_primary()?;
        loop {
            match self.peek_kind() {
                TokenKind::LParen => {
                    let mut args = Vec::new();
                    self.parse_call_args(&mut args)?;
                    let end = self.tokens[self.pos.saturating_sub(1)].span.end;
                    base = match base {
                        Expr::Name { name, span } => Expr::Call {
                            name,
                            args,
                            span: Span::new(span.file, span.start, end),
                        },
                        Expr::Member {
                            receiver,
                            member,
                            span,
                            ..
                        } => Expr::ReceiverCall {
                            receiver,
                            name: member,
                            args,
                            span: Span::new(span.file, span.start, end),
                        },
                        _other => {
                            self.errors.push(OpyError::at(
                                "parse-error",
                                "cannot call this expression".to_string(),
                                self.peek().span,
                            ));
                            return Err(());
                        }
                    };
                }
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    if self.peek_kind() == TokenKind::Colon {
                        self.advance();
                        let maximum = self.parse_expr()?;
                        let end = self.expect(TokenKind::RBracket, "']'")?.span.end;
                        let Expr::Name { name, span } = &base else {
                            self.error_at_current(
                                "a range type must start with a type name".to_string(),
                            );
                            return Err(());
                        };
                        base = Expr::Type {
                            name: name.clone(),
                            args: vec![index, maximum],
                            span: Span::new(span.file, span.start, end),
                        };
                        continue;
                    }
                    let end = match self.expect(TokenKind::RBracket, "']'") {
                        Ok(token) => token.span.end,
                        Err(()) => return Err(()),
                    };
                    let span = Span::new(base.span().file, base.span().start, end);
                    base = Expr::Index {
                        array: Box::new(base),
                        index: Box::new(index),
                        span,
                    };
                }
                TokenKind::Dot => {
                    self.advance();
                    let member_token = self.peek().clone();
                    let member = match self.peek_kind() {
                        TokenKind::Ident | TokenKind::Number => self.advance().text,
                        _ => {
                            self.error_at_current("expected a member name after '.'".to_string());
                            return Err(());
                        }
                    };
                    let member_span = member_token.span;
                    let end = member_span.end;
                    let span = Span::new(base.span().file, base.span().start, end);
                    base = Expr::Member {
                        receiver: Box::new(base),
                        member,
                        member_span,
                        span,
                    };
                }
                _ => break,
            }
        }
        Ok(base)
    }

    /// `@Event name(args)`: positional expressions only (keyword arguments
    /// are a call-argument form, not an event form).
    fn parse_event_args(&mut self, args: &mut Vec<Expr>) -> Result<(), ()> {
        self.expect(TokenKind::LParen, "'('")?;
        self.skip_newlines();
        if self.peek_kind() == TokenKind::RParen {
            self.advance();
            return Ok(());
        }
        loop {
            let expr = self.parse_expr()?;
            if self.peek_kind() == TokenKind::Assign {
                self.error_at_current("keyword arguments are not valid in @Event".to_string());
                return Err(());
            }
            args.push(expr);
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Comma {
                self.advance();
                self.skip_newlines();
                if self.peek_kind() == TokenKind::RParen {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen, "')'")?;
        Ok(())
    }

    fn parse_call_args(&mut self, args: &mut Vec<CallArg>) -> Result<(), ()> {
        self.expect(TokenKind::LParen, "'('")?;
        self.skip_newlines();
        if self.peek_kind() == TokenKind::RParen {
            self.advance();
            return Ok(());
        }
        loop {
            match self.parse_expr() {
                Ok(expr) => {
                    // A keyword argument is `name = expr` (issue #110): a
                    // bare identifier immediately followed by `=`. Anything
                    // else (`expr = ...`) is not a call argument form and is
                    // rejected like the pinned reference rejects it.
                    if self.peek_kind() == TokenKind::Assign {
                        let Expr::Name { name, span } = expr else {
                            self.error_at_current(
                                "expected a keyword name before '=' in this call".to_string(),
                            );
                            return Err(());
                        };
                        self.advance();
                        let value = match self.parse_expr() {
                            Ok(value) => value,
                            Err(()) => return Err(()),
                        };
                        args.push(CallArg {
                            keyword: Some((name, span)),
                            value,
                        });
                    } else {
                        args.push(CallArg {
                            keyword: None,
                            value: expr,
                        });
                    }
                }
                Err(()) => return Err(()),
            }
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Comma {
                self.advance();
                self.skip_newlines();
                if self.peek_kind() == TokenKind::RParen {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen, "')'")?;
        Ok(())
    }

    fn parse_primary(&mut self) -> Result<Expr, ()> {
        let token = self.peek();
        match token.kind {
            TokenKind::Number => {
                let token = self.advance();
                let value = if let Some(hex) = token
                    .text
                    .strip_prefix("0x")
                    .or_else(|| token.text.strip_prefix("0X"))
                {
                    u64::from_str_radix(hex, 16).map_or(f64::NAN, |value| value as f64)
                } else {
                    token.text.parse().unwrap_or(f64::NAN)
                };
                Ok(Expr::Number {
                    value,
                    text: token.text.clone(),
                    span: token.span,
                })
            }
            TokenKind::String => self.parse_string_literal(),
            TokenKind::Ident => {
                let token = self.advance();
                if token.text == "lambda" {
                    return self.parse_lambda(token.span);
                }
                if is_string_modifier(&token.text) && self.peek_kind() == TokenKind::String {
                    let string = self.advance();
                    let (format_text, interpolations) = if token.text == "f" {
                        let raw = string.raw.as_deref().unwrap_or(&string.text);
                        let (format_text, interpolations) =
                            self.parse_f_string(raw, string.span)?;
                        (Some(format_text), interpolations)
                    } else {
                        (None, Vec::new())
                    };
                    return Ok(Expr::StringModifier {
                        modifier: token.text.chars().next().unwrap_or_default(),
                        value: string.text,
                        format_text,
                        interpolations,
                        span: Span::new(token.span.file, token.span.start, string.span.end),
                    });
                }
                match token.text.as_str() {
                    "true" => Ok(Expr::Bool {
                        value: true,
                        span: token.span,
                    }),
                    "false" => Ok(Expr::Bool {
                        value: false,
                        span: token.span,
                    }),
                    "None" | "null" => Ok(Expr::Null { span: token.span }),
                    _ => Ok(Expr::Name {
                        name: token.text.clone(),
                        span: token.span,
                    }),
                }
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen, "')'")?;
                Ok(expr)
            }
            TokenKind::LBracket => {
                let open = self.advance();
                let mut elements = Vec::new();
                self.skip_newlines();
                if self.peek_kind() == TokenKind::RBracket {
                    let end = self.advance().span.end;
                    return Ok(Expr::Array {
                        elements,
                        span: Span::new(open.span.file, open.span.start, end),
                    });
                }
                let first = self.parse_expr()?;
                if self.is_ident("for") {
                    self.advance();
                    let variable_token =
                        self.expect(TokenKind::Ident, "a comprehension variable")?;
                    let index = if self.peek_kind() == TokenKind::Comma {
                        self.advance();
                        let index = self.expect(TokenKind::Ident, "a comprehension index")?;
                        Some((index.text, index.span))
                    } else {
                        None
                    };
                    if !self.is_ident("in") {
                        self.error_at_current("expected `in` in list comprehension".to_string());
                        return Err(());
                    }
                    self.advance();
                    let iterable = self.parse_or()?;
                    let condition = if self.is_ident("if") {
                        self.advance();
                        Some(Box::new(self.parse_or()?))
                    } else {
                        None
                    };
                    let end = self.expect(TokenKind::RBracket, "']'")?.span.end;
                    return Ok(Expr::Comprehension {
                        element: Box::new(first),
                        variable: variable_token.text,
                        variable_span: variable_token.span,
                        index,
                        iterable: Box::new(iterable),
                        condition,
                        span: Span::new(open.span.file, open.span.start, end),
                    });
                }
                elements.push(first);
                loop {
                    self.skip_newlines();
                    if self.peek_kind() == TokenKind::Comma {
                        self.advance();
                        self.skip_newlines();
                        if self.peek_kind() == TokenKind::RBracket {
                            break;
                        }
                        elements.push(self.parse_expr()?);
                    } else {
                        break;
                    }
                }
                let end = match self.expect(TokenKind::RBracket, "']'") {
                    Ok(token) => token.span.end,
                    Err(()) => return Err(()),
                };
                Ok(Expr::Array {
                    elements,
                    span: Span::new(open.span.file, open.span.start, end),
                })
            }
            TokenKind::LBrace => self.parse_dict(),
            _ => {
                self.error_at_current(format!("expected an expression but found '{}'", token.text));
                Err(())
            }
        }
    }

    /// Parse adjacent string literals as one source-language string value.
    ///
    /// Newlines are only ignored while looking for another literal inside a
    /// delimiter group. Outside a group, a newline remains a statement
    /// boundary, matching the bounded implicit-concatenation surface used by
    /// the OverPy examples.
    fn parse_string_literal(&mut self) -> Result<Expr, ()> {
        let first = self.advance();
        let mut value = first.text.clone();
        let mut end = first.span.end;
        loop {
            let saved = self.pos;
            if self.inside_delimiter_group() {
                self.skip_newlines();
            }
            if self.peek_kind() != TokenKind::String || self.peek().span.file != first.span.file {
                self.pos = saved;
                break;
            }
            let next = self.advance();
            value.push_str(&next.text);
            end = next.span.end;
        }
        Ok(Expr::String {
            value,
            span: Span::new(first.span.file, first.span.start, end),
        })
    }

    /// Return whether the current parser position is inside `()`, `[]`, or
    /// `{}`. The token stream retains newlines, so this keeps multiline
    /// implicit concatenation scoped to syntactic grouping without adding
    /// parser state to every delimiter path.
    fn inside_delimiter_group(&self) -> bool {
        let mut depth = 0usize;
        for token in &self.tokens[..self.pos] {
            match token.kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    depth = depth.saturating_sub(1)
                }
                _ => {}
            }
        }
        depth != 0
    }

    fn parse_dict(&mut self) -> Result<Expr, ()> {
        let open = self.advance();
        let mut entries = Vec::new();
        self.skip_newlines();
        if self.peek_kind() == TokenKind::RBrace {
            let end = self.advance().span.end;
            return Ok(Expr::Dict {
                entries,
                span: Span::new(open.span.file, open.span.start, end),
            });
        }
        loop {
            let key = self.parse_expr()?;
            self.expect(TokenKind::Colon, "':' in a dictionary entry")?;
            let value = self.parse_expr()?;
            let span = Span::new(key.span().file, key.span().start, value.span().end);
            entries.push(DictEntry { key, value, span });
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Comma {
                self.advance();
                self.skip_newlines();
                if self.peek_kind() == TokenKind::RBrace {
                    break;
                }
            } else {
                break;
            }
        }
        let end = self.expect(TokenKind::RBrace, "'}'")?.span.end;
        Ok(Expr::Dict {
            entries,
            span: Span::new(open.span.file, open.span.start, end),
        })
    }

    fn parse_lambda(&mut self, start: Span) -> Result<Expr, ()> {
        let mut params = Vec::new();
        loop {
            let param = self.expect(TokenKind::Ident, "a lambda parameter")?;
            params.push((param.text, param.span));
            if self.peek_kind() == TokenKind::Comma {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(TokenKind::Colon, "':' after lambda parameters")?;
        let body = self.parse_expr()?;
        Ok(Expr::Lambda {
            params,
            body: Box::new(body.clone()),
            span: Span::new(start.file, start.start, body.span().end),
        })
    }

    /// Parse the expression regions of a pinned-OverPy f-string. Double
    /// braces are literal braces; a single brace introduces one expression.
    /// The resulting expression tokens are shifted back into the source
    /// string so HIR and tooling retain source provenance.
    fn parse_f_string(&mut self, raw: &str, string_span: Span) -> Result<(String, Vec<Expr>), ()> {
        let chars: Vec<char> = raw.chars().collect();
        let mut text = String::new();
        let mut interpolations = Vec::new();
        let mut index = 0;
        while index < chars.len() {
            match chars[index] {
                '{' if chars.get(index + 1) == Some(&'{') => {
                    text.push_str("{{");
                    index += 2;
                }
                '}' if chars.get(index + 1) == Some(&'}') => {
                    text.push_str("}}");
                    index += 2;
                }
                '{' => {
                    let end = self.find_f_string_end(&chars, index + 1);
                    let Some(end) = end else {
                        self.errors.push(OpyError::at(
                            "parse-error",
                            "unterminated f-string interpolation".to_string(),
                            string_span,
                        ));
                        return Err(());
                    };
                    let expression: String = chars[index + 1..end].iter().collect();
                    if expression.trim().is_empty() {
                        self.errors.push(OpyError::at(
                            "parse-error",
                            "f-string interpolation cannot be empty".to_string(),
                            Span::new(
                                string_span.file,
                                Position::new(
                                    string_span.start.line,
                                    string_span.start.col + index as u32 + 1,
                                ),
                                Position::new(
                                    string_span.start.line,
                                    string_span.start.col + end as u32 + 1,
                                ),
                            ),
                        ));
                        return Err(());
                    }
                    let origin = Position::new(
                        string_span.start.line,
                        string_span.start.col + index as u32 + 1,
                    );
                    let parsed = parse_expression_fragment(&expression, string_span.file, origin)
                        .map_err(|error| {
                            self.errors.push(error);
                        });
                    let Ok(parsed) = parsed else {
                        return Err(());
                    };
                    text.push_str(&format!("{{{}}}", interpolations.len()));
                    interpolations.push(parsed);
                    index = end + 1;
                }
                '}' => {
                    self.errors.push(OpyError::at(
                        "parse-error",
                        "single '}' is not valid in an f-string".to_string(),
                        string_span,
                    ));
                    return Err(());
                }
                '\\' if index + 1 < chars.len() => {
                    text.push(decode_string_escape(chars[index + 1]));
                    index += 2;
                }
                character => {
                    text.push(character);
                    index += 1;
                }
            }
        }
        Ok((text, interpolations))
    }

    fn find_f_string_end(&self, chars: &[char], start: usize) -> Option<usize> {
        let mut nested_braces = 0;
        let mut quote = None;
        let mut escaped = false;
        for (index, character) in chars.iter().enumerate().skip(start) {
            if escaped {
                escaped = false;
                continue;
            }
            if *character == '\\' && quote.is_some() {
                escaped = true;
                continue;
            }
            if let Some(active_quote) = quote {
                if *character == active_quote {
                    quote = None;
                }
                continue;
            }
            match character {
                '"' | '\'' => quote = Some(*character),
                '{' => nested_braces += 1,
                '}' if nested_braces == 0 => return Some(index),
                '}' => nested_braces -= 1,
                _ => {}
            }
        }
        None
    }
}

/// Parse one f-string expression fragment and shift its local token spans
/// into the original source file.
fn parse_expression_fragment(text: &str, file: u32, origin: Position) -> Result<Expr, OpyError> {
    let mut tokens = crate::lexer::lex(crate::lexer::LexInput {
        file_id: file,
        text,
    })?;
    for token in &mut tokens {
        token.span = shift_span(token.span, origin);
    }
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        allow_macro_redeclaration: false,
        errors: Vec::new(),
    };
    let expression = parser.parse_expr().map_err(|()| {
        parser.errors.first().cloned().unwrap_or_else(|| {
            OpyError::at(
                "parse-error",
                "invalid f-string expression",
                Span::new(file, origin, origin),
            )
        })
    })?;
    if parser.peek_kind() != TokenKind::Eof {
        parser.error_at_current("unexpected tokens in f-string interpolation".to_string());
    }
    parser.errors.into_iter().next().map_or(Ok(expression), Err)
}

fn shift_span(span: Span, origin: Position) -> Span {
    fn shift(position: Position, origin: Position) -> Position {
        Position::new(
            origin.line + position.line.saturating_sub(1),
            if position.line == 1 {
                origin.col + position.col.saturating_sub(1)
            } else {
                position.col
            },
        )
    }
    Span::new(
        span.file,
        shift(span.start, origin),
        shift(span.end, origin),
    )
}

fn decode_string_escape(character: char) -> char {
    match character {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '\\' => '\\',
        '"' => '"',
        '\'' => '\'',
        other => other,
    }
}

fn is_string_modifier(text: &str) -> bool {
    matches!(text, "f" | "w" | "l" | "b" | "c" | "t")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{LexInput, lex};

    fn parse_ok(text: &str) -> Program {
        let tokens = lex(LexInput { file_id: 0, text }).unwrap();
        let output = parse(&tokens);
        assert!(
            output.errors.is_empty(),
            "unexpected errors: {:?}",
            output.errors
        );
        output.program.unwrap()
    }

    fn parse_err(text: &str) -> Vec<OpyError> {
        let tokens = lex(LexInput { file_id: 0, text }).unwrap();
        parse(&tokens).errors
    }

    #[test]
    fn parses_basic_rule() {
        let program = parse_ok("rule \"setup\":\n    @Event global\n    disableInspector()\n");
        assert_eq!(program.rules.len(), 1);
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        assert_eq!(rule.name, "setup");
        assert_eq!(rule.event.name, "global");
        assert_eq!(rule.actions.len(), 1);
    }

    #[test]
    fn parses_power_augmented_assignment() {
        // The pinned OverPy 9.7.10 reference accepts `**=` as the power
        // augmented assignment (`a **= b` ⇔ `a = a ** b`).
        let program =
            parse_ok("globalvar a\nrule \"r\":\n    @Event global\n    a = 2\n    a **= 3\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Assign {
            value,
            target: assigned_target,
            ..
        } = &rule.actions[1]
        else {
            panic!("expected an assignment");
        };
        let Expr::Binary {
            op, left, right, ..
        } = value
        else {
            panic!("expected a binary modification, got {value:?}");
        };
        assert_eq!(op, "**");
        assert!(matches!(&**left, Expr::Name { .. }));
        assert!(matches!(
            assigned_target,
            Expr::Name { name, .. } if name == "a"
        ));
        assert!(matches!(right.as_ref(), Expr::Number { .. }));
    }

    #[test]
    fn parses_postfix_increment_and_decrement_as_modifications() {
        let program =
            parse_ok("globalvar value\nrule \"r\":\n    @Event global\n    value++\n    value--\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        for (statement, expected_op) in [(&rule.actions[0], "+"), (&rule.actions[1], "-")] {
            let Stmt::Assign { target, value, .. } = statement else {
                panic!("expected a postfix assignment");
            };
            let Expr::Binary {
                op, left, right, ..
            } = value
            else {
                panic!("expected a synthetic modification value");
            };
            assert_eq!(op, expected_op);
            let Expr::Name {
                name: left_name, ..
            } = left.as_ref()
            else {
                panic!("expected the target to be the modification's left operand");
            };
            let Expr::Name {
                name: target_name, ..
            } = target
            else {
                panic!("expected a name target");
            };
            assert_eq!(left_name, target_name);
            assert!(
                matches!(right.as_ref(), Expr::Number { value, text, .. } if *value == 1.0 && text == "1")
            );
        }
    }

    #[test]
    fn rejects_prefix_increment_and_embedded_postfix_forms() {
        for source in [
            "globalvar value\nrule \"r\":\n    @Event global\n    ++value\n",
            "globalvar value\nrule \"r\":\n    @Event global\n    value++++\n",
        ] {
            let errors = parse_err(source);
            assert!(!errors.is_empty());
            assert!(errors.iter().all(|error| error.code == "parse-error"));
            assert!(errors.iter().all(|error| error.span.is_some()));
        }
    }

    #[test]
    fn preserves_consecutive_unary_minus_expressions() {
        let source = concat!(
            "globalvar value = 0\n",
            "globalvar B = 1\n",
            "rule \"r\":\n",
            "    @Event global\n",
            "    value = --1\n",
            "    value = --B\n",
            "    value = B--1\n",
        );
        parse_ok(source);
    }

    #[test]
    fn parses_control_flow() {
        let program = parse_ok(
            "globalvar index = 0\n\nrule \"r\":\n    @Event global\n    for index in range(3):\n        if index == 0:\n            debug(index)\n        elif index == 1:\n            debug(index)\n        else:\n            debug(index)\n    while index < 3:\n        index += 1\n        wait()\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!();
        };
        assert!(matches!(rule.actions[0], Stmt::For { .. }));
        let Stmt::For { body, .. } = &rule.actions[0] else {
            panic!();
        };
        let Stmt::If {
            branches, r#else, ..
        } = &body[0]
        else {
            panic!();
        };
        assert_eq!(branches.len(), 2);
        assert!(r#else.is_some());
        let Stmt::While { body, .. } = &rule.actions[1] else {
            panic!();
        };
        assert_eq!(body.len(), 2);
    }

    #[test]
    fn parses_issue_28_constructs() {
        let program = parse_ok(
            "globalvar x\nrule \"r\":\n    @Event global\n    switch x:\n        case 0x10:\n            x = 1 in [1, 2]\n        default:\n            do:\n                x = {\"x\": 1}[\"x\"]\n            while x not in [2, 3]\n    x = [value * 2 for value, index in [1, 2] if value > index]\n    x = sorted([1, 2], key=lambda value: value)\n    x = w\"wide\"\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        assert!(matches!(rule.actions[0], Stmt::Switch { .. }));
        assert!(matches!(rule.actions[1], Stmt::Assign { .. }));
    }

    #[test]
    fn rejects_incomplete_do_while_and_dictionary_entries() {
        let errors = parse_err(
            "rule \"r\":\n    @Event global\n    do:\n        pass\n    while\n    x = {\"x\"}\n",
        );
        assert!(!errors.is_empty());
        assert!(errors.iter().all(|error| error.code == "parse-error"));
    }

    #[test]
    fn parses_multi_line_array() {
        let program = parse_ok(
            "globalvar p\nrule \"r\":\n    @Event global\n    p = [\n        vect(1, 0, 0),\n        vect(2, 0, 0),\n    ]\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!();
        };
        let Stmt::Assign { value, .. } = &rule.actions[0] else {
            panic!();
        };
        let Expr::Array { elements, .. } = value else {
            panic!("expected array, got {value:?}");
        };
        assert_eq!(elements.len(), 2);
    }

    #[test]
    fn missing_colon_is_a_structured_error() {
        let errors = parse_err("rule \"x\"\n    @Event global\n");
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "parse-error");
        assert!(errors[0].span.is_some());
    }

    #[test]
    fn def_and_macro_parse() {
        let program = parse_ok(
            "subroutine showStatus\n\ndef showStatus():\n    print(\"hi\")\n\nmacro double(value):\n    value + value\n",
        );
        assert_eq!(program.declarations.len(), 2);
        assert!(matches!(program.declarations[1], Decl::Macro { .. }));
        let Decl::Macro { args, body, .. } = &program.declarations[1] else {
            panic!();
        };
        assert_eq!(args, &vec!["value".to_string()]);
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn macro_and_enum_redeclarations_are_checked_at_ast_surfaces() {
        let text = "enum Kind:\n    First\n    First\nmacro helper():\n    pass\nmacro helper():\n    pass\n";
        let errors = parse_err(text);
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.code == "macro-redeclaration")
                .count(),
            2
        );

        let tokens = lex(LexInput { file_id: 0, text }).unwrap();
        let output = parse_with_options(&tokens, true);
        assert!(
            output.errors.is_empty(),
            "unexpected errors: {:?}",
            output.errors
        );
        assert!(output.program.is_some());
    }

    #[test]
    fn multiple_errors_are_reported() {
        let errors =
            parse_err("rule \"a\"\n    bad statement here\nrule \"b\"\n    @Event global\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn precedence_parses_python_like() {
        let program = parse_ok("globalvar x\nrule \"r\":\n    @Event global\n    x = 1 + 2 * 3\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!();
        };
        let Stmt::Assign { value, .. } = &rule.actions[0] else {
            panic!();
        };
        let Expr::Binary {
            op, left, right, ..
        } = value
        else {
            panic!();
        };
        assert_eq!(op, "+");
        let Expr::Binary { op: inner, .. } = right.as_ref() else {
            panic!();
        };
        assert_eq!(inner, "*");
        assert!(matches!(left.as_ref(), Expr::Number { .. }));
    }

    #[test]
    fn parses_right_associative_conditional_expressions() {
        let program = parse_ok(
            "rule \"r\":\n    @Event global\n    debug(1 if true else 2 if false else 3)\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected an expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected a call");
        };
        let Expr::Conditional {
            then_value,
            condition,
            else_value,
            span,
        } = &args[0].value
        else {
            panic!("expected a conditional expression");
        };
        assert!(matches!(then_value.as_ref(), Expr::Number { value, .. } if *value == 1.0));
        assert!(matches!(condition.as_ref(), Expr::Bool { value: true, .. }));
        assert!(matches!(
            else_value.as_ref(),
            Expr::Conditional { then_value, condition, else_value, .. }
                if matches!(then_value.as_ref(), Expr::Number { value, .. } if *value == 2.0)
                    && matches!(condition.as_ref(), Expr::Bool { value: false, .. })
                    && matches!(else_value.as_ref(), Expr::Number { value, .. } if *value == 3.0)
        ));
        assert_eq!(span.start.line, 3);
        assert_eq!(span.start.col, 11);
    }

    #[test]
    fn parses_parenthesized_nested_conditional_and_rejects_missing_else() {
        let program = parse_ok(
            "rule \"r\":\n    @Event global\n    debug((1 if true else 2) if false else 3)\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected an expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected a call");
        };
        assert!(matches!(
            &args[0].value,
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                ..
            } if matches!(then_value.as_ref(), Expr::Conditional { .. })
                && matches!(condition.as_ref(), Expr::Bool { value: false, .. })
                && matches!(else_value.as_ref(), Expr::Number { value, .. } if *value == 3.0)
        ));

        let errors = parse_err("rule \"r\":\n    @Event global\n    debug(1 if true)\n");
        assert_eq!(errors[0].code, "parse-error");
        assert!(errors[0].message.contains("expected `else`"));
    }

    #[test]
    fn parses_receiver_calls() {
        // `eventPlayer.setMoveSpeed(100)` is a receiver call: postfix `.`
        // member access followed by call arguments (#104).
        let program =
            parse_ok("rule \"r\":\n    @Event eachPlayer\n    eventPlayer.setMoveSpeed(100)\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement, got {:?}", rule.actions[0]);
        };
        let Expr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } = &expr
        else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "setMoveSpeed");
        assert!(
            matches!(receiver.as_ref(), Expr::Name { name, .. } if name == "eventPlayer"),
            "receiver must be the eventPlayer name"
        );
        assert_eq!(args.len(), 1);
        assert!(args[0].keyword.is_none(), "positional argument");
        assert!(matches!(&args[0].value, Expr::Number { .. }));
    }

    #[test]
    fn parses_keyword_arguments_with_name_spans() {
        // `name = expr` call arguments are keyword arguments carrying the
        // name token's exact span (issue #110); comparisons stay positional.
        let program =
            parse_ok("rule \"r\":\n    @Event global\n    wait(time=1)\n    debug(g == 1)\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected a call, got {expr:?}");
        };
        let (keyword, span) = args[0].keyword.as_ref().expect("keyword argument");
        assert_eq!(keyword, "time");
        assert_eq!(span.start.line, 3);
        assert!(matches!(&args[0].value, Expr::Number { .. }));

        let Stmt::Expr { expr, .. } = &rule.actions[1] else {
            panic!("expected expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert!(args[0].keyword.is_none(), "comparisons are not keywords");
        assert!(matches!(&args[0].value, Expr::Binary { .. }));
    }

    #[test]
    fn adjacent_string_literals_concatenate_and_preserve_span() {
        let program = parse_ok("rule \"r\":\n    @Event global\n    debug(\"one\" \"two\")\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected call");
        };
        let Expr::String { value, span } = &args[0].value else {
            panic!("expected concatenated string");
        };
        assert_eq!(value, "onetwo");
        assert_eq!(span.start.line, 3);
        assert_eq!(span.start.col, 11);
        assert_eq!(span.end.col, 22);
    }

    #[test]
    fn multiline_adjacent_string_literals_concatenate_inside_group() {
        let program =
            parse_ok("rule \"r\":\n    @Event global\n    debug(\"one\"\n        \"two\")\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected call");
        };
        assert!(matches!(
            &args[0].value,
            Expr::String { value, .. } if value == "onetwo"
        ));
    }

    #[test]
    fn newline_outside_group_keeps_adjacent_literals_as_statements() {
        let program = parse_ok("rule \"r\":\n    @Event global\n    \"one\"\n    \"two\"\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        assert_eq!(rule.actions.len(), 2);
    }

    #[test]
    fn non_name_keyword_lhs_is_a_parse_error() {
        // `f(1 = 2)` is not a call argument form; rejected explicitly.
        let errors = parse_err("rule \"r\":\n    @Event global\n    debug(1 = 2)\n");
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "parse-error");
    }

    #[test]
    fn parses_member_call_on_call_result() {
        // `getPlayersInRadius(...).setStatusEffect(...)`: member access
        // followed by call arguments on a call result stays a receiver call.
        let program = parse_ok(
            "rule \"r\":\n    @Event eachPlayer\n    getPlayersInRadius(eventPlayer, 10).setStatusEffect(eventPlayer, 30)\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let Expr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } = &expr
        else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "setStatusEffect");
        assert!(
            matches!(receiver.as_ref(), Expr::Call { name, .. } if name == "getPlayersInRadius"),
            "receiver must be the preceding call"
        );
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn member_without_call_is_not_a_call() {
        // `eventPlayer.moveSpeed` alone (no parentheses) stays a member
        // access; only a following `(` turns it into a receiver call.
        let program =
            parse_ok("rule \"r\":\n    @Event eachPlayer\n    x = eventPlayer.moveSpeed\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Assign { value, .. } = &rule.actions[0] else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value,
            Expr::Member { member, .. } if member == "moveSpeed"
        ));
    }

    #[test]
    fn parses_advanced_rule_annotations_with_source_arguments() {
        let program = parse_ok(
            "subroutine helper\ndef helper():\n    @Name \"renamed\"\n    @SuppressWarnings unusedVariable\n    pass\nrule \"r\":\n    @Event eachPlayer\n    @Team 1\n    @Hero dmon\n    @Disabled\n    @Delimiter\n    @NewPage \"Page\"\n    @SuppressWarnings unusedVariable\n    pass\n",
        );
        let RuleEntry::SubroutineDef { annotations, .. } = &program.rules[0] else {
            panic!("expected subroutine");
        };
        assert_eq!(annotations.len(), 2);
        let RuleEntry::Rule(rule) = &program.rules[1] else {
            panic!("expected rule");
        };
        assert!(rule.disabled);
        assert!(rule.delimiter);
        assert_eq!(rule.new_page.as_deref(), Some("Page"));
        assert_eq!(rule.annotations.len(), 7);
        assert_eq!(rule.annotations[1].args[0].text, "1");
        assert_eq!(rule.annotations[2].args[0].text, "dmon");
    }
}
