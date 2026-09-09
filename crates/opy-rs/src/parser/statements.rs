use super::*;

impl Parser<'_> {
    pub(super) fn parse_block(&mut self, block_indent: u32) -> Vec<Stmt> {
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

    pub(super) fn parse_statement(&mut self) -> Result<Stmt, ()> {
        let token = self.peek();
        if token.kind == TokenKind::Ident {
            match token.text.as_str() {
                "if" => return self.parse_if(),
                "for" => return self.parse_for(),
                "while" => return self.parse_while(),
                "do" => return self.parse_do_while(),
                "switch" => return self.parse_switch(),
                "del" => return self.parse_delete(),
                "continue" => {
                    let token = self.advance();
                    self.expect_statement_end("the continue statement")?;
                    return Ok(Stmt::Continue { span: token.span });
                }
                "goto" => return self.parse_goto(),
                "break" => {
                    let token = self.advance();
                    return Ok(Stmt::Break { span: token.span });
                }
                "return" => {
                    let token = self.advance();
                    self.expect_statement_end("the return statement")?;
                    return Ok(Stmt::Return { span: token.span });
                }
                "pass" => {
                    let start = self.advance();
                    return Ok(Stmt::Pass { span: start.span });
                }
                _ => {}
            }
            if self.peek_at(1).kind == TokenKind::Colon {
                return self.parse_label();
            }
        }
        self.parse_expr_statement()
    }

    pub(super) fn parse_delete(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let target = self.parse_postfix()?;
        if !matches!(target, Expr::Index { .. }) {
            self.errors.push(OpyError::at(
                "parse-error",
                "the del statement requires an array index target".to_string(),
                target.span(),
            ));
            return Err(());
        }
        self.expect_statement_end("the del statement")?;
        Ok(Stmt::Delete {
            span: Span::new(start.span.file, start.span.start, target.span().end),
            target,
        })
    }

    pub(super) fn parse_goto(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        if self.is_ident("loc") {
            self.advance();
            self.expect(TokenKind::Plus, "'+' after 'goto loc'")?;
            let offset = self.parse_expr()?;
            self.expect_statement_end("the goto target")?;
            return Ok(Stmt::Goto {
                label: None,
                span: Span::new(start.span.file, start.span.start, offset.span().end),
                offset: Some(offset),
                rule_start: false,
            });
        }

        let label = self.expect_ident("a label or 'loc+...' after 'goto'")?;
        self.expect_statement_end("the goto target")?;
        let rule_start = label == "RULE_START";
        Ok(Stmt::Goto {
            label: (!rule_start).then_some(label),
            offset: None,
            rule_start,
            span: Span::new(
                start.span.file,
                start.span.start,
                self.tokens[self.pos - 1].span.end,
            ),
        })
    }

    pub(super) fn parse_label(&mut self) -> Result<Stmt, ()> {
        let name = self.advance();
        let colon = self.expect(TokenKind::Colon, "':' after a label")?;
        self.expect_statement_end("the label")?;
        Ok(Stmt::Label {
            name: name.text,
            span: Span::new(name.span.file, name.span.start, colon.span.end),
        })
    }

    pub(super) fn parse_expr_statement(&mut self) -> Result<Stmt, ()> {
        if self.peek_kind() == TokenKind::LParen {
            let save_pos = self.pos;
            let save_errors = self.errors.len();
            let start = self.advance().span;
            if let Ok(target) = self.parse_postfix()
                && self.peek_kind() == TokenKind::Assign
            {
                self.advance();
                let value = self.parse_expr()?;
                let end = self.expect(TokenKind::RParen, "')'")?.span.end;
                return Ok(Stmt::Assign {
                    target,
                    value,
                    span: Span::new(start.file, start.start, end),
                });
            }
            self.pos = save_pos;
            self.errors.truncate(save_errors);
        }
        let start = self.peek().span;
        let expr = self.parse_expr()?;
        match self.peek_kind() {
            TokenKind::Assign => {
                self.advance();
                let value = self.parse_expr()?;
                let end = value.span().end;
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
                let end = rhs.span().end;
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
            TokenKind::Ident
                if matches!(self.peek().text.as_str(), "min" | "max")
                    && self.peek_at(1).kind == TokenKind::Assign =>
            {
                let op = self.advance().text;
                self.advance();
                let rhs = self.parse_expr()?;
                let end = rhs.span().end;
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
                let end = expr.span().end;
                Ok(Stmt::Expr {
                    expr,
                    span: Span::new(start.file, start.start, end),
                })
            }
        }
    }

    pub(super) fn parse_if(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let line_indent = start.span.start.col;
        let condition = self.parse_expr()?;
        if self
            .expect(TokenKind::Colon, "':' after the if condition")
            .is_err()
        {
            return Err(());
        }
        let body = self.parse_colon_body(line_indent)?;
        let mut branches = vec![IfBranch { condition, body }];
        let mut r#else = None;
        loop {
            let save = self.pos;
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof || self.peek().span.start.col > line_indent {
                self.pos = save;
                break;
            }
            if self.is_ident("elif") {
                let branch_start = self.advance();
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
                let body = self.parse_colon_body(branch_start.span.start.col)?;
                branches.push(IfBranch { condition, body });
            } else if self.is_ident("else") {
                let branch_start = self.advance();
                if self.is_ident("if") {
                    self.advance();
                    let condition = self.parse_expr()?;
                    if self
                        .expect(TokenKind::Colon, "':' after the else-if condition")
                        .is_err()
                    {
                        return Err(());
                    }
                    let body = self.parse_colon_body(branch_start.span.start.col)?;
                    branches.push(IfBranch { condition, body });
                    continue;
                }
                if self.expect(TokenKind::Colon, "':' after `else`").is_err() {
                    return Err(());
                }
                let body = self.parse_colon_body(branch_start.span.start.col)?;
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

    pub(super) fn parse_colon_body(&mut self, line_indent: u32) -> Result<Vec<Stmt>, ()> {
        if matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) {
            let save = self.pos;
            self.skip_newlines();
            if self.peek_kind() != TokenKind::Eof && self.peek().span.start.col == line_indent {
                let statement = self.parse_statement()?;
                self.expect_statement_end("the inline statement")?;
                return Ok(vec![statement]);
            }
            self.pos = save;
            let body_indent = self.block_indent(line_indent).ok_or(())?;
            Ok(self.parse_block(body_indent))
        } else {
            let statement = self.parse_statement()?;
            self.expect_statement_end("the inline statement")?;
            Ok(vec![statement])
        }
    }

    pub(super) fn parse_for(&mut self) -> Result<Stmt, ()> {
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

    pub(super) fn parse_while(&mut self) -> Result<Stmt, ()> {
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

    pub(super) fn parse_do_while(&mut self) -> Result<Stmt, ()> {
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

    pub(super) fn parse_switch(&mut self) -> Result<Stmt, ()> {
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
}
