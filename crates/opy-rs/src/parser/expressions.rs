//! Expression and postfix grammar.

use super::*;

impl Parser<'_> {
    // ---- expressions ----

    pub(super) fn parse_expr(&mut self) -> Result<Expr, ()> {
        self.parse_expr_inner(false)
    }

    pub(super) fn parse_expr_inner(
        &mut self,
        allow_multiline_conditional: bool,
    ) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let then_value = self.parse_or()?;
        let has_newline_before_conditional = self.tokens[..self.pos].iter().any(|token| {
            token.kind == TokenKind::Newline
                && token.span.file == then_value.span().file
                && token.span.start.line > then_value.span().end.line
        });
        if self.is_ident("if")
            && !self.inside_delimiter_group()
            && has_newline_before_conditional
            && self.peek().span.start.line != then_value.span().end.line
        {
            return Ok(then_value);
        }
        if allow_multiline_conditional && self.inside_delimiter_group() {
            self.skip_newlines();
        }
        if !self.is_ident("if") {
            return Ok(then_value);
        }

        let conditional_start = self.pos;
        self.advance();
        self.skip_expression_newlines();
        let condition = self.parse_or()?;
        self.skip_expression_newlines();
        if !self.is_ident("else") {
            if self.peek_kind() == TokenKind::Colon {
                self.pos = conditional_start;
                return Ok(then_value);
            }
            self.error_at_current("expected `else` in conditional expression".to_string());
            return Err(());
        }
        self.advance();
        // Conditional expressions are right-associative, so a chained form
        // such as `a if c else b if d else e` groups at the else branch.
        let else_value = self.parse_expr_inner(true)?;
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

    pub(super) fn parse_or(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let mut left = self.parse_and()?;
        loop {
            self.skip_expression_newlines();
            if !self.is_ident("or") {
                break;
            }
            self.advance();
            self.skip_expression_newlines();
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

    pub(super) fn parse_and(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let mut left = self.parse_not()?;
        loop {
            self.skip_expression_newlines();
            if !self.is_ident("and") {
                break;
            }
            self.advance();
            self.skip_expression_newlines();
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

    pub(super) fn parse_not(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        if self.is_ident("not") {
            let start = self.advance();
            self.skip_expression_newlines();
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

    pub(super) fn parse_comparison(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let mut left = self.parse_additive()?;
        loop {
            self.skip_expression_newlines();
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
            self.skip_expression_newlines();
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

    pub(super) fn parse_additive(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let mut left = self.parse_multiplicative()?;
        loop {
            self.skip_expression_newlines();
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
            self.skip_expression_newlines();
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

    pub(super) fn parse_multiplicative(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let left = self.parse_unary()?;
        self.parse_multiplicative_tail(left)
    }

    pub(super) fn parse_multiplicative_tail(&mut self, mut left: Expr) -> Result<Expr, ()> {
        loop {
            self.skip_expression_newlines();
            let op = match self.peek_kind() {
                TokenKind::Star => "*",
                TokenKind::Slash => "/",
                TokenKind::Percent => "%",
                _ => break,
            };
            self.advance();
            self.skip_expression_newlines();
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

    pub(super) fn parse_unary(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        if matches!(self.peek_kind(), TokenKind::Minus | TokenKind::Decrement) {
            let start = self.advance();
            self.skip_expression_newlines();
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

    pub(super) fn parse_power(&mut self) -> Result<Expr, ()> {
        let base = self.parse_postfix()?;
        self.skip_expression_newlines();
        if self.peek_kind() == TokenKind::DoubleStar {
            self.advance();
            // Right-associative.
            self.skip_expression_newlines();
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

    pub(super) fn parse_postfix(&mut self) -> Result<Expr, ()> {
        let mut base = self.parse_primary()?;
        loop {
            self.skip_expression_newlines();
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
    pub(super) fn parse_event_args(&mut self, args: &mut Vec<Expr>) -> Result<(), ()> {
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

    pub(super) fn parse_call_args(&mut self, args: &mut Vec<CallArg>) -> Result<(), ()> {
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

    pub(super) fn parse_primary(&mut self) -> Result<Expr, ()> {
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
                self.skip_expression_newlines();
                let expr = self.parse_expr()?;
                self.skip_expression_newlines();
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
                    self.skip_newlines();
                    let index = if self.peek_kind() == TokenKind::Comma {
                        self.advance();
                        self.skip_newlines();
                        let index = self.expect(TokenKind::Ident, "a comprehension index")?;
                        Some((index.text, index.span))
                    } else {
                        None
                    };
                    self.skip_newlines();
                    if !self.is_ident("in") {
                        self.error_at_current("expected `in` in list comprehension".to_string());
                        return Err(());
                    }
                    self.advance();
                    self.skip_newlines();
                    let iterable = self.parse_or()?;
                    self.skip_newlines();
                    let condition = if self.is_ident("if") {
                        self.advance();
                        self.skip_newlines();
                        Some(Box::new(self.parse_or()?))
                    } else {
                        None
                    };
                    self.skip_newlines();
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
    pub(super) fn parse_string_literal(&mut self) -> Result<Expr, ()> {
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
    pub(super) fn inside_delimiter_group(&self) -> bool {
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

    pub(super) fn parse_dict(&mut self) -> Result<Expr, ()> {
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

    pub(super) fn parse_lambda(&mut self, start: Span) -> Result<Expr, ()> {
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
    pub(super) fn parse_f_string(
        &mut self,
        raw: &str,
        string_span: Span,
    ) -> Result<(String, Vec<Expr>), ()> {
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

    pub(super) fn find_f_string_end(&self, chars: &[char], start: usize) -> Option<usize> {
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
