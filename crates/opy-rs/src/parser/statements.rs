use super::*;

impl Parser<'_> {
    /// The column of the header that opened the block containing the
    /// statement being parsed, when any — the floor an `elif`/`else`/`while`
    /// tail must stay deeper than to still belong inside (#516).
    fn enclosing_floor(&self) -> Option<u32> {
        self.block_floors.last().copied()
    }

    /// Parse a `:`-opened block at `body_indent`, recording `header_col` — the
    /// column of the construct that opened it — as the enclosing floor for the
    /// statements inside.
    pub(super) fn parse_child_block(&mut self, header_col: u32, body_indent: u32) -> Vec<Stmt> {
        self.block_floors.push(header_col);
        let block = self.parse_block(body_indent);
        self.block_floors.pop();
        block
    }

    pub(super) fn parse_block(&mut self, block_indent: u32) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof {
                break;
            }
            if self.peek().layout.start.col < block_indent {
                break;
            }
            if self.peek().layout.start.col > block_indent {
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
        if let Some((span, detail)) = self.workshop_construct(false) {
            self.report_workshop_source(span, &detail);
            self.skip_workshop_construct();
            return Err(());
        }
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
                // An `elif`/`else` without a preceding `if` still emits its
                // `Else If`/`Else` marker in the pinned reference (warned as
                // `w_lone_else`) — it must never reach the label path
                // (`else:` is a keyword, not a label; #516).
                "elif" | "else" => return self.parse_orphan_else(),
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
            self.bump();
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
                self.bump();
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
                self.bump();
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
                self.bump();
                self.finish_augmented_assignment(expr, start, op)
            }
            TokenKind::Ident
                if matches!(self.peek().text.as_str(), "min" | "max")
                    && self.peek_at(1).kind == TokenKind::Assign =>
            {
                let op = self.advance().text;
                self.bump();
                self.finish_augmented_assignment(expr, start, op)
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

    fn finish_augmented_assignment(
        &mut self,
        target: Expr,
        start: Span,
        op: String,
    ) -> Result<Stmt, ()> {
        let rhs = self.parse_expr()?;
        let end = rhs.span().end;
        let span = Span::new(start.file, start.start, end);
        Ok(Stmt::Assign {
            target: target.clone(),
            value: Expr::Binary {
                op,
                left: Box::new(target),
                right: Box::new(rhs),
                span,
            },
            span,
        })
    }

    pub(super) fn parse_if(&mut self) -> Result<Stmt, ()> {
        let indent = self.peek().layout.start.col;
        self.open_if_indents.push(indent);
        let stmt = self.parse_if_chain();
        self.open_if_indents.pop();
        stmt
    }

    fn parse_if_chain(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let line_indent = start.layout.start.col;
        let condition = self.parse_expr()?;
        let body = self.expect_colon_body(line_indent, "':' after the if condition")?;
        let continued_inline_body = self.last_colon_body_continued;
        if continued_inline_body {
            self.errors.push(OpyError::at(
                "parse-error",
                "Found 'if', but no 'else'".to_string(),
                start.span,
            ));
            return Err(());
        }
        let branches = vec![IfBranch {
            condition,
            body,
            marker: start.span,
        }];
        self.finish_if_chain(start.span, line_indent, branches, false)
    }

    /// Collect `elif`/`else` continuations at the chain's column (or at a
    /// mid-dedent column still inside the enclosing block) and build the
    /// `Stmt::If`. `orphan` marks a chain that began with an `elif`/`else`
    /// rather than `if`: the reference emits `Else If`/`Else` markers instead
    /// of `If` (#516).
    fn finish_if_chain(
        &mut self,
        start_span: Span,
        line_indent: u32,
        mut branches: Vec<IfBranch>,
        orphan: bool,
    ) -> Result<Stmt, ()> {
        let mut r#else = None;
        let mut else_span = None;
        loop {
            let save = self.pos;
            self.skip_newlines();
            let column = self.peek().layout.start.col;
            if self.peek_kind() == TokenKind::Eof
                || column > line_indent
                || (column != line_indent && self.open_if_indents.contains(&column))
                // A candidate at or shallower than the enclosing block's
                // header column belongs outside the block: the chain ends and
                // the line surfaces at its real level (#516).
                || self
                    .enclosing_floor()
                    .is_some_and(|floor| column <= floor)
            {
                self.pos = save;
                break;
            }
            if self.is_ident("elif") {
                let branch_start = self.advance();
                let condition = match self.parse_expr() {
                    Ok(expr) => expr,
                    Err(()) => return Err(()),
                };
                let body = self.expect_colon_body(
                    branch_start.layout.start.col,
                    "':' after the elif condition",
                )?;
                branches.push(IfBranch {
                    condition,
                    body,
                    marker: branch_start.span,
                });
            } else if self.is_ident("else") {
                let branch_start = self.advance();
                if self.is_ident("if") {
                    self.bump();
                    let condition = self.parse_expr()?;
                    let body = self.expect_colon_body(
                        branch_start.layout.start.col,
                        "':' after the else-if condition",
                    )?;
                    branches.push(IfBranch {
                        condition,
                        body,
                        marker: branch_start.span,
                    });
                    continue;
                }
                let body =
                    self.expect_colon_body(branch_start.layout.start.col, "':' after `else`")?;
                else_span = Some(branch_start.span);
                r#else = Some(body);
                break;
            } else {
                self.pos = save;
                break;
            }
        }
        // The chain ends where the next statement at a shallower indent (or
        // EOF) begins; that position stands in for Workshop's explicit `End`.
        let end_span = self.boundary_span();
        Ok(Stmt::If {
            branches,
            r#else,
            else_span,
            end_span,
            orphan,
            span: start_span,
        })
    }

    /// A zero-width span at the next statement boundary after skipped
    /// newlines: the position a Workshop `End` marker would occupy.
    fn boundary_span(&mut self) -> Span {
        let save = self.pos;
        self.skip_newlines();
        let boundary = self.peek().span;
        self.pos = save;
        Span::new(boundary.file, boundary.start, boundary.start)
    }

    /// An `elif`/`else` reached in statement position — no `if` precedes it in
    /// this block. The pinned reference emits the marker anyway (`Else
    /// If`/`Else`, an orphan `else if` emits `Else`) and warns `w_lone_else`;
    /// a following `elif`/`else` at the same column chains to it (#516).
    fn parse_orphan_else(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let line_indent = start.layout.start.col;
        let is_elif = start.text == "elif";
        self.warnings.push(crate::preprocess::PreprocessWarning {
            code: "w_lone_else".to_string(),
            message: format!(
                "Found '{}', but no 'if' or 'elif' before it",
                if is_elif { "elif" } else { "else" }
            ),
            span: start.span,
        });
        if is_elif {
            let condition = self.parse_expr()?;
            let body = self.expect_colon_body(line_indent, "':' after the elif condition")?;
            let branches = vec![IfBranch {
                condition,
                body,
                marker: start.span,
            }];
            self.open_if_indents.push(line_indent);
            let stmt = self.finish_if_chain(start.span, line_indent, branches, true);
            self.open_if_indents.pop();
            stmt
        } else {
            // `else if` keeps only the `else` in the reference: the condition
            // is parsed (its own diagnostics still surface) then dropped.
            if self.is_ident("if") {
                self.bump();
                self.parse_expr()?;
            }
            let body = self.expect_colon_body(line_indent, "':' after `else`")?;
            let end_span = self.boundary_span();
            Ok(Stmt::If {
                branches: Vec::new(),
                r#else: Some(body),
                else_span: Some(start.span),
                end_span,
                orphan: true,
                span: start.span,
            })
        }
    }

    pub(super) fn parse_colon_body(&mut self, line_indent: u32) -> Result<Vec<Stmt>, ()> {
        self.last_colon_body_continued = false;
        if matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) {
            // A `:`-headed body may be empty: a next line at `line_indent` or
            // shallower belongs to the enclosing block, not to this body
            // (#516).
            Ok(self
                .block_indent(line_indent)
                .map(|body_indent| self.parse_child_block(line_indent, body_indent))
                .unwrap_or_default())
        } else {
            let statement = self.parse_statement()?;
            self.expect_statement_end("the inline statement")?;
            self.last_colon_body_continued = self.last_statement_continued;
            Ok(vec![statement])
        }
    }

    fn expect_colon_body(
        &mut self,
        line_indent: u32,
        colon_context: &str,
    ) -> Result<Vec<Stmt>, ()> {
        self.expect_block_colon(colon_context)?;
        self.parse_colon_body(line_indent)
    }

    pub(super) fn parse_for(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let variable = self.parse_postfix()?;
        if !self.is_ident("in") {
            self.error_at_current("expected `in` in the for statement".to_string());
            return Err(());
        }
        self.bump();
        let iterable = self.parse_expr()?;
        let body = self
            .expect_block_indent(start.layout.start.col, "':' after the for header")?
            .map(|body_indent| self.parse_child_block(start.layout.start.col, body_indent))
            .unwrap_or_default();
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
        let body = self
            .expect_block_indent(start.layout.start.col, "':' after the while condition")?
            .map(|body_indent| self.parse_child_block(start.layout.start.col, body_indent))
            .unwrap_or_default();
        Ok(Stmt::While {
            condition,
            body,
            span: start.span,
        })
    }

    pub(super) fn parse_do_while(&mut self) -> Result<Stmt, ()> {
        let start = self.advance();
        let body = self
            .expect_block_indent(start.layout.start.col, "':' after `do`")?
            .map(|body_indent| self.parse_child_block(start.layout.start.col, body_indent))
            .unwrap_or_default();
        // The `while` tail belongs to this `do` only while it stays inside
        // the enclosing block (deeper than the enclosing header's column); a
        // `while` dedented out leaves the `do` unmatched like the reference's
        // "no matching 'while'" (#516).
        let tail_inside = self
            .enclosing_floor()
            .is_none_or(|floor| self.peek().layout.start.col > floor);
        if !tail_inside || !self.is_ident("while") {
            self.error_at_current("expected `while` after the do block".to_string());
            return Err(());
        }
        self.bump();
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
        let Some(body_indent) =
            self.expect_block_indent(start.layout.start.col, "':' after the switch value")?
        else {
            return Ok(Stmt::Switch {
                value,
                arms: Vec::new(),
                span: start.span,
            });
        };
        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof || self.peek().layout.start.col < body_indent {
                break;
            }
            if self.peek().layout.start.col != body_indent {
                self.error_at_current("unexpected indentation in switch".to_string());
                self.recover_line();
                continue;
            }
            if self.is_ident("case") {
                let case_start = self.advance();
                let case_value = self.parse_expr()?;
                let body = self
                    .expect_block_indent(body_indent, "':' after the case value")?
                    .map(|case_body_indent| {
                        self.parse_child_block(case_start.layout.start.col, case_body_indent)
                    })
                    .unwrap_or_default();
                arms.push(SwitchArm::Case {
                    value: case_value,
                    body,
                    span: case_start.span,
                });
            } else if self.is_ident("default") {
                let default_start = self.advance();
                let body = self
                    .expect_block_indent(body_indent, "':' after `default`")?
                    .map(|default_body_indent| {
                        self.parse_child_block(default_start.layout.start.col, default_body_indent)
                    })
                    .unwrap_or_default();
                arms.push(SwitchArm::Default {
                    body,
                    span: default_start.span,
                });
                if default_start.layout.start.col != body_indent {
                    self.error_at_current("invalid default indentation".to_string());
                    return Err(());
                }
            } else {
                self.error_at_current("expected `case` or `default` in switch".to_string());
                self.recover_line();
            }
        }
        Ok(Stmt::Switch {
            value,
            arms,
            span: start.span,
        })
    }
}
