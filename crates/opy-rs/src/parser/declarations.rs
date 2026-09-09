use super::*;

impl Parser<'_> {
    pub(super) fn parse_variable(&mut self, declarations: &mut Vec<Decl>, global: bool) -> bool {
        let start = self.advance();
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
        let end = self
            .tokens
            .get(self.pos.saturating_sub(1))
            .map_or(start.span.end, |token| token.span.end);
        declarations.push(Decl::Subroutine {
            name,
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
                if self.peek_kind() == TokenKind::Assign {
                    self.advance();
                    if self.parse_expr().is_err() {
                        self.recover_line();
                        continue;
                    }
                }
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

    pub(super) fn parse_macro(&mut self, declarations: &mut Vec<Decl>) -> bool {
        let start = self.advance();
        let name_token = self.peek().clone();
        let mut name = match self.expect_ident("a macro name") {
            Ok(name) => name,
            Err(()) => return false,
        };
        let qualified = if self.peek_kind() == TokenKind::Dot {
            self.advance();
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
        if self.peek_kind() == TokenKind::Assign {
            self.advance();
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

    pub(super) fn parse_param_list(&mut self) -> Option<Vec<String>> {
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
}
