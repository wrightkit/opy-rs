//! The indentation-aware `.opy` CST parser.
//!
//! Consumes the expanded token stream from [`crate::preprocess`] and builds a
//! [`cst::Program`]. Parsing is deterministic and corpus-backed; malformed
//! input produces structured [`FrontendError`]s rather than panics, and the
//! parser recovers at statement/line boundaries so multiple useful errors are
//! reported. The returned [`ParseOutput`] carries either a complete program
//! or the collected errors (never both).

use crate::cst::{
    Annotation, AnnotationArg, CallArg, Decl, Event, Expr, IfBranch, Program, Rule, RuleEntry, Stmt,
};
use crate::diag::{FrontendError, Position, Span};
use crate::lexer::{Token, TokenKind};

/// The outcome of a parse.
#[derive(Debug, Default)]
pub struct ParseOutput {
    /// The parsed program, present only when no errors were collected.
    pub program: Option<Program>,
    /// Every structured error collected during the parse.
    pub errors: Vec<FrontendError>,
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
    errors: Vec<FrontendError>,
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
        self.errors
            .push(FrontendError::at("parse-error", message, span));
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
                self.errors.push(FrontendError::at(
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
                    self.errors.push(FrontendError::at(
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
            self.errors.push(FrontendError::at(
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
            | TokenKind::DoubleSlashAssign
            | TokenKind::PercentAssign => {
                let op = match self.peek_kind() {
                    TokenKind::PlusAssign => "+",
                    TokenKind::MinusAssign => "-",
                    TokenKind::StarAssign => "*",
                    TokenKind::SlashAssign => "/",
                    TokenKind::DoubleSlashAssign => "//",
                    TokenKind::PercentAssign => "%",
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
        let variable = self.parse_expr()?;
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

    // ---- expressions ----

    fn parse_expr(&mut self) -> Result<Expr, ()> {
        self.parse_or()
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
                _ => break,
            };
            self.advance();
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
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => "*",
                TokenKind::Slash => "/",
                TokenKind::DoubleSlash => "//",
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
        if self.peek_kind() == TokenKind::Minus {
            let start = self.advance();
            let operand = self.parse_unary()?;
            let end = operand.span().end;
            return Ok(Expr::Unary {
                op: "-".to_string(),
                operand: Box::new(operand),
                span: Span::new(start.span.file, start.span.start, end),
            });
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
                            self.errors.push(FrontendError::at(
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
                    let member = match self.expect_ident("a member name after '.'") {
                        Ok(member) => member,
                        Err(()) => return Err(()),
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
                let value: f64 = token.text.parse().unwrap_or(f64::NAN);
                Ok(Expr::Number {
                    value,
                    text: token.text.clone(),
                    span: token.span,
                })
            }
            TokenKind::String => {
                let token = self.advance();
                Ok(Expr::String {
                    value: token.text.clone(),
                    span: token.span,
                })
            }
            TokenKind::Ident => {
                let token = self.advance();
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
                loop {
                    match self.parse_expr() {
                        Ok(expr) => elements.push(expr),
                        Err(()) => return Err(()),
                    }
                    self.skip_newlines();
                    if self.peek_kind() == TokenKind::Comma {
                        self.advance();
                        self.skip_newlines();
                        if self.peek_kind() == TokenKind::RBracket {
                            break;
                        }
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
            _ => {
                self.error_at_current(format!("expected an expression but found '{}'", token.text));
                Err(())
            }
        }
    }
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

    fn parse_err(text: &str) -> Vec<FrontendError> {
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
