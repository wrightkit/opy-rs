//! Rule, annotation, and definition grammar.

use super::*;

impl Parser<'_> {
    // ---- rules and definitions ----

    pub(super) fn parse_rule(
        &mut self,
        rules: &mut Vec<RuleEntry>,
        rule_prefix: Option<String>,
    ) -> bool {
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
        if self.block_indent(line_indent).is_none() {
            return false;
        }
        // OverPy accepts a small amount of indentation drift between rule
        // directives and actions. The rule itself is still top-level, so any
        // indentation greater than its column belongs to this rule.
        let body_indent = line_indent + 1;
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
    pub(super) fn parse_directive(
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
                let end = args
                    .last()
                    .map_or(event_annotation_arg.span.end, |arg| arg.span().end);
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
                        let end = expr.span().end;
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
                let end = args.last().map_or(at.span.end, |arg| arg.span.end);
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
                let end = args.last().map_or(at.span.end, |arg| arg.span.end);
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
                let end = args.last().map_or(at.span.end, |arg| arg.span.end);
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
                let end = args.last().map_or(at.span.end, |arg| arg.span.end);
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

    pub(super) fn consume_annotation_args(&mut self) -> Vec<AnnotationArg> {
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

    pub(super) fn raw_annotation_arg(&self, start: usize, end: usize) -> AnnotationArg {
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

    pub(super) fn annotation_arg_is_string(&self, arg: &AnnotationArg) -> bool {
        arg.text.starts_with('"') && arg.text.ends_with('"')
    }

    pub(super) fn expect_annotation_end(&mut self, name: &str) -> bool {
        if self.peek_kind() == TokenKind::Newline || self.peek_kind() == TokenKind::Eof {
            true
        } else {
            self.error_at_current(format!("{name} takes no arguments"));
            false
        }
    }

    pub(super) fn parse_def(
        &mut self,
        rules: &mut Vec<RuleEntry>,
        rule_prefix: Option<String>,
    ) -> bool {
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
                "subroutine parameters are outside the declared language support surface"
                    .to_string(),
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
}
