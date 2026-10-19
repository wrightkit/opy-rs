use super::*;

impl Parser<'_> {
    pub(super) fn parse_variable(&mut self, declarations: &mut Vec<Decl>, global: bool) -> bool {
        let start = self.advance();
        // The name token follows the keyword; its span is the exact declared
        // identifier occurrence (rename targets, not the keyword/statement).
        let name_span = self.peek().span;
        let name = match self.expect_ident("a variable name after the keyword") {
            Ok(name) => name,
            Err(()) => return false,
        };
        let mut index = None;
        let mut initializer = None;
        if self.peek_kind() == TokenKind::Number {
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
        }
        if self.peek_kind() == TokenKind::Assign {
            self.bump();
            match self.parse_expr() {
                Ok(expr) => initializer = Some(expr),
                Err(()) => return false,
            }
        } else if self.peek_kind() == TokenKind::LBrace {
            self.report_workshop_source(self.peek().span, BRACE_BLOCKS);
            self.skip_workshop_construct();
            return false;
        } else if self.peek_kind() != TokenKind::Newline && self.peek_kind() != TokenKind::Eof {
            self.error_at_current(
                "expected '=', an integer index, or end of line after the variable name"
                    .to_string(),
            );
            return false;
        }
        let end = self
            .tokens
            .get(self.pos.saturating_sub(1))
            .map_or(start.span.end, |token| token.span.end);
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

    pub(super) fn parse_subroutine(&mut self, declarations: &mut Vec<Decl>) -> bool {
        let start = self.advance();
        // The name token follows the `subroutine` keyword; its span is the
        // exact declared identifier occurrence.
        let name_span = self.peek().span;
        let name = match self.expect_ident("a subroutine name") {
            Ok(name) => name,
            Err(()) => return false,
        };
        let mut index = None;
        if self.peek_kind() == TokenKind::Number {
            let token = self.advance();
            index = token.text.parse::<u32>().ok();
            if index.is_none() {
                self.errors.push(OpyError::at(
                    "parse-error",
                    format!(
                        "invalid subroutine index '{}' (expected an integer)",
                        token.text
                    ),
                    token.span,
                ));
                return false;
            }
        }
        // The pinned reference splits every globalvar/playervar/subroutine
        // declaration line at its first '='; for `subroutine` the rest of
        // the logical line is dropped without being parsed, including
        // bracket-continued tokens on following physical lines.
        if self.peek_kind() == TokenKind::Assign {
            self.bump();
            let mut bracket = 0usize;
            loop {
                match self.peek_kind() {
                    TokenKind::Eof => break,
                    TokenKind::Newline if bracket == 0 => break,
                    TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => bracket += 1,
                    TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                        bracket = bracket.saturating_sub(1)
                    }
                    _ => {}
                }
                self.bump();
            }
        }
        let end = self
            .tokens
            .get(self.pos.saturating_sub(1))
            .map_or(start.span.end, |token| token.span.end);
        declarations.push(Decl::Subroutine {
            name,
            index,
            span: Span::new(start.span.file, start.span.start, end),
            name_span,
        });
        true
    }

    pub(super) fn parse_enum(&mut self, declarations: &mut Vec<Decl>) -> bool {
        let start = self.advance();
        let name = match self.expect_ident("an enum name") {
            Ok(name) => name,
            Err(()) => return false,
        };
        let body_indent =
            match self.expect_block_indent(start.layout.start.col, "':' after the enum name") {
                Ok(body_indent) => body_indent,
                Err(()) => return false,
            };
        let mut members = Vec::new();
        let Some(body_indent) = body_indent else {
            declarations.push(Decl::Enum {
                name,
                members,
                span: start.span,
            });
            return true;
        };
        loop {
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof || self.peek().layout.start.col < body_indent {
                break;
            }
            if self.peek_kind() == TokenKind::Ident {
                let member = self.advance();
                let member_span = member.span;
                if !self.allow_macro_redeclaration
                    && members.iter().any(|(name, _, _)| name == &member.text)
                {
                    self.errors.push(OpyError::at(
                        "macro-redeclaration",
                        format!("enum member '{name}.{}' is already defined", member.text),
                        member_span,
                    ));
                }
                members.push((member.text, None, member_span));
                // The reference names the member from the line's first token
                // and takes the value from the first `=` on the line; the
                // rest of the member line is ignored, so `settings { ... }`
                // is a member named `settings`, not a settings block (#517).
                let mut bracket = 0usize;
                loop {
                    match self.peek_kind() {
                        TokenKind::Eof => break,
                        TokenKind::Newline | TokenKind::Assign if bracket == 0 => break,
                        TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => bracket += 1,
                        TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                            bracket = bracket.saturating_sub(1)
                        }
                        _ => {}
                    }
                    self.bump();
                }
                if bracket != 0 {
                    self.error_at_current("unclosed bracket in the enum member line".to_string());
                    self.recover_line();
                    continue;
                }
                if self.peek_kind() == TokenKind::Assign {
                    self.bump();
                    match self.parse_expr() {
                        Ok(value) => {
                            members
                                .last_mut()
                                .expect("the enum member was just pushed")
                                .1 = Some(value);
                        }
                        Err(()) => {
                            self.recover_line();
                            continue;
                        }
                    }
                    if self.peek_kind() == TokenKind::Comma {
                        self.bump();
                    }
                    if !matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) {
                        self.error_at_current("expected ',' after the enum member".to_string());
                        self.recover_line();
                        continue;
                    }
                }
                continue;
            }
            self.error_at_current("expected an enum member name".to_string());
            self.recover_line();
            continue;
        }
        declarations.push(Decl::Enum {
            name,
            members,
            span: start.span,
        });
        true
    }

    pub(super) fn parse_macro(&mut self, declarations: &mut Vec<Decl>) -> bool {
        let start = self.advance();
        let name_span = self.peek().span;
        let mut name = match self.expect_ident("a macro name") {
            Ok(name) => name,
            Err(()) => return false,
        };
        let qualified = if self.peek_kind() == TokenKind::Dot {
            self.bump();
            let member = match self.expect_ident("a macro member name") {
                Ok(member) => member,
                Err(()) => return false,
            };
            name.push('.');
            name.push_str(&member);
            true
        } else {
            false
        };
        if self.peek_kind() == TokenKind::LBrace {
            self.report_workshop_source(self.peek().span, BRACE_BLOCKS);
            self.skip_workshop_construct();
            return false;
        }
        if self.peek_kind() == TokenKind::Assign {
            self.bump();
            let value = match self.parse_expr() {
                Ok(value) => value,
                Err(()) => return false,
            };
            let span = Span::new(start.span.file, start.span.start, value.span().end);
            if qualified {
                declarations.push(Decl::Macro {
                    name,
                    args: vec!["self".to_string()],
                    body: vec![Stmt::Expr { expr: value, span }],
                    span,
                });
            } else {
                declarations.push(Decl::Constant { name, value, span });
            }
            return true;
        }
        let mut args = match self.parse_param_list() {
            Some(args) => args,
            None => return false,
        };
        if qualified {
            args.insert(0, "self".to_string());
        }
        let body_indent = match self
            .expect_block_indent(start.layout.start.col, "':' after the macro signature")
        {
            Ok(Some(body_indent)) => body_indent,
            // The pinned OverPy requires a non-empty macro body even though
            // other `:`-headed bodies may be empty (#516).
            Ok(None) => {
                self.error_at_current(format!(
                    "macro '{name}' cannot be empty (use 'pass' for a no-op macro)"
                ));
                return false;
            }
            Err(()) => return false,
        };
        let body = self.parse_child_block(start.layout.start.col, body_indent);
        if !self.allow_macro_redeclaration
            && declarations.iter().any(|declaration| {
                matches!(declaration, Decl::Macro { name: existing, .. } if existing == &name)
            })
        {
            self.errors.push(OpyError::at(
                "macro-redeclaration",
                format!("macro '{name}' is already defined"),
                name_span,
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

    pub(super) fn parse_param_list(&mut self) -> Option<Vec<String>> {
        if self.expect(TokenKind::LParen, "'('").is_err() {
            return None;
        }
        let mut params = Vec::new();
        self.skip_newlines();
        if self.peek_kind() == TokenKind::RParen {
            self.bump();
            return Some(params);
        }
        loop {
            match self.expect_ident("a parameter name") {
                Ok(name) => params.push(name),
                Err(()) => return None,
            }
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Comma {
                self.bump();
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
}
