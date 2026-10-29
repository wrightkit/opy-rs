use super::*;

mod arithmetic;

impl Parser<'_> {
    pub(super) fn parse_expr(&mut self) -> Result<Expr, ()> {
        self.parse_expr_inner(false)
    }

    fn parse_expr_inner(&mut self, allow_multiline_conditional: bool) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let then_value = self.parse_or()?;
        if self.is_ident("if") && self.if_is_past_then_value_layout_break() {
            return Ok(then_value);
        }
        if allow_multiline_conditional && self.inside_delimiter_group() {
            self.skip_newlines();
        }
        if !self.is_ident("if") {
            return Ok(then_value);
        }

        let conditional_start = self.pos;
        self.bump();
        self.skip_expression_newlines();
        let condition = self.parse_or()?;
        self.skip_expression_newlines();
        if !self.is_ident("else") {
            if self.peek_kind() == TokenKind::Colon {
                self.pos = conditional_start;
                return Ok(then_value);
            }
            return self.fail("expected `else` in conditional expression".to_string());
        }
        self.bump();
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

    fn parse_or(&mut self) -> Result<Expr, ()> {
        self.parse_ident_chain("or", |parser| parser.parse_and())
    }

    fn parse_and(&mut self) -> Result<Expr, ()> {
        self.parse_ident_chain("and", |parser| parser.parse_not())
    }

    fn parse_ident_chain(
        &mut self,
        operator: &str,
        mut parse_next: impl FnMut(&mut Self) -> Result<Expr, ()>,
    ) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let mut left = parse_next(self)?;
        loop {
            self.skip_expression_newlines();
            if !self.is_ident(operator) {
                break;
            }
            self.bump();
            self.skip_expression_newlines();
            let right = parse_next(self)?;
            left = binary(operator, left, right);
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        if self.is_ident("not") {
            let start = self.advance();
            self.skip_expression_newlines();
            let operand = self.parse_not()?;
            return Ok(unary_op("not", start.span, operand));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, ()> {
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
            self.bump();
            if op == "not in" {
                self.bump();
            }
            self.skip_expression_newlines();
            let right = self.parse_additive()?;
            left = binary(op, left, right);
        }
        Ok(left)
    }

    pub(super) fn parse_postfix(&mut self) -> Result<Expr, ()> {
        let mut base = self.parse_primary()?;
        loop {
            self.skip_expression_newlines();
            match self.peek_kind() {
                TokenKind::LParen => {
                    let args_start = self.pos + 1;
                    let mut args = Vec::new();
                    self.parse_call_args(&mut args)?;
                    let end = self.tokens[self.pos.saturating_sub(1)].span.end;
                    base = match base {
                        Expr::Name { name, span } => Expr::Call {
                            debug_source: (name == "debug").then(|| {
                                display_tokens(&self.tokens[args_start..self.pos.saturating_sub(1)])
                            }),
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
                            return self.fail("cannot call this expression".to_string());
                        }
                    };
                }
                TokenKind::LBracket => {
                    self.bump();
                    let index = self.parse_expr()?;
                    if self.peek_kind() == TokenKind::Colon {
                        self.bump();
                        let maximum = self.parse_expr()?;
                        let end = self.expect(TokenKind::RBracket, "']'")?.span.end;
                        let Expr::Name { name, span } = &base else {
                            return self
                                .fail("a range type must start with a type name".to_string());
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
                    self.bump();
                    let member = match self.peek_kind() {
                        TokenKind::Ident | TokenKind::Number => self.advance(),
                        _ => {
                            return self.fail("expected a member name after '.'".to_string());
                        }
                    };
                    let member_span = member.span;
                    let end = member_span.end;
                    let span = Span::new(base.span().file, base.span().start, end);
                    base = Expr::Member {
                        receiver: Box::new(base),
                        member: member.text,
                        member_span,
                        span,
                    };
                }
                _ => break,
            }
        }
        Ok(base)
    }

    fn has_more_delimited_items(
        &mut self,
        closing: TokenKind,
        allow_trailing_comma: bool,
    ) -> Result<bool, ()> {
        self.skip_newlines();
        if self.peek_kind() != TokenKind::Comma {
            return Ok(false);
        }
        self.bump();
        self.skip_newlines();
        if self.peek_kind() == closing && !allow_trailing_comma {
            // The reference keeps the empty segment after a trailing comma
            // and rejects it (issue #444); array literals allow it.
            return self.fail("expected an item after ','".to_string());
        }
        Ok(self.peek_kind() != closing)
    }

    fn parse_delimited_items<T>(
        &mut self,
        items: &mut Vec<T>,
        first_item_parsed: bool,
        closing: TokenKind,
        closing_text: &str,
        mut parse_item: impl FnMut(&mut Self) -> Result<T, ()>,
        allow_trailing_comma: bool,
    ) -> Result<Token, ()> {
        self.skip_newlines();
        if !first_item_parsed && self.peek_kind() == closing {
            return Ok(self.advance());
        }
        if !first_item_parsed {
            items.push(parse_item(self)?);
        }
        while self.has_more_delimited_items(closing, allow_trailing_comma)? {
            items.push(parse_item(self)?);
        }
        self.expect(closing, closing_text)
    }

    pub(super) fn parse_event_args(&mut self, args: &mut Vec<Expr>) -> Result<(), ()> {
        self.expect(TokenKind::LParen, "'('")?;
        self.parse_delimited_items(
            args,
            false,
            TokenKind::RParen,
            "')'",
            |parser| {
                let expr = parser.parse_expr()?;
                if parser.peek_kind() == TokenKind::Assign {
                    return parser.fail("keyword arguments are not valid in @Event".to_string());
                }
                Ok(expr)
            },
            true,
        )?;
        Ok(())
    }

    fn parse_call_args(&mut self, args: &mut Vec<CallArg>) -> Result<(), ()> {
        self.expect(TokenKind::LParen, "'('")?;
        self.parse_delimited_items(
            args,
            false,
            TokenKind::RParen,
            "')'",
            |parser| {
                // A keyword argument is `name = expr` (issue #110): a bare
                // identifier immediately followed by `=`. The reference detects
                // keyword arguments at the token level, so the name must be the
                // argument's first token; a parenthesized `(name) = expr`
                // parses positionally and rejects at the `=` (issue #443).
                // Anything else is rejected like the pinned reference rejects
                // it.
                let bare_name = parser.peek_kind() == TokenKind::Ident;
                let expr = parser.parse_expr()?;
                if parser.peek_kind() == TokenKind::Assign {
                    let (true, Expr::Name { name, span }) = (bare_name, expr) else {
                        return parser
                            .fail("expected a keyword name before '=' in this call".to_string());
                    };
                    parser.advance();
                    let value = parser.parse_expr()?;
                    Ok(CallArg {
                        keyword: Some((name, span)),
                        value,
                    })
                } else {
                    Ok(CallArg {
                        keyword: None,
                        value: expr,
                    })
                }
            },
            false,
        )?;
        Ok(())
    }

    fn parse_primary(&mut self) -> Result<Expr, ()> {
        let token = self.peek();
        match token.kind {
            TokenKind::Number => {
                let mut token = self.advance();
                // `0x` is the one lenient prefix upstream: the reference
                // rewrites it through `parseInt(name, 16)`, which reads a
                // hex *prefix* — `0x1FZ` is 31, `0x`/`0xZZ` are `NaN`. The
                // other radixes go through `Number` and need the whole
                // tail (`0o17` is 15, `0o18`/`0X1FZ` are `NaN`; #512).
                let value = if let Some(digits) = token.text.strip_prefix("0x") {
                    let mut seen = false;
                    let value =
                        digits
                            .chars()
                            .take_while(|c| c.is_ascii_hexdigit())
                            .fold(0f64, |v, c| {
                                seen = true;
                                v * 16.0 + c.to_digit(16).unwrap_or_default() as f64
                            });
                    if seen { value } else { f64::NAN }
                } else {
                    let radix = token.text.strip_prefix('0').and_then(|rest| {
                        match rest.as_bytes().first()? {
                            b'X' => Some((&rest[1..], 16)),
                            b'b' | b'B' => Some((&rest[1..], 2)),
                            b'o' | b'O' => Some((&rest[1..], 8)),
                            _ => None,
                        }
                    });
                    radix
                        .and_then(|(digits, radix)| {
                            // `Number("0x")` is `NaN`, not `0` (#512).
                            (!digits.is_empty()).then(|| {
                                digits.chars().try_fold(0f64, |v, c| {
                                    c.to_digit(radix).map(|d| v * radix as f64 + d as f64)
                                })
                            })?
                        })
                        .unwrap_or_else(|| token.text.parse().unwrap_or(f64::NAN))
                };
                Ok(Expr::Number {
                    value,
                    text: std::mem::take(&mut token.text),
                    span: token.span,
                })
            }
            TokenKind::String => Ok(self.parse_string_literal()),
            TokenKind::Ident => {
                let mut token = self.advance();
                if token.text == "lambda" {
                    return self.parse_lambda(token.span);
                }
                if is_string_modifier(&token.text) && self.peek_kind() == TokenKind::String {
                    let string = self.advance();
                    let (format_text, interpolations) = if token.text.contains('f') {
                        let raw = string.raw.as_deref().unwrap_or(&string.text);
                        let (format_text, interpolations) =
                            self.parse_f_string(raw, string.span)?;
                        (Some(format_text), interpolations)
                    } else {
                        (None, Vec::new())
                    };
                    return Ok(Expr::StringModifier {
                        modifier: token.text,
                        value: string.text,
                        format_text,
                        interpolations,
                        span: Span::new(token.span.file, token.span.start, string.span.end),
                    });
                }
                // `true()`, `false()`, `null()` are accepted by the
                // reference: `funcKw` `args === null` names produce the
                // same AST bare or called with an empty argument list
                // (#529). Keep the literal's spelling as the callee so
                // the lowerer resolves the zero-argument form like the
                // bare literal and rejects arguments like any other
                // bare-spelled value.
                if matches!(token.text.as_str(), "true" | "false" | "null")
                    && self.peek_kind() == TokenKind::LParen
                {
                    return Ok(Expr::Name {
                        name: std::mem::take(&mut token.text),
                        span: token.span,
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
                    "null" => Ok(Expr::Null { span: token.span }),
                    _ => Ok(Expr::Name {
                        name: std::mem::take(&mut token.text),
                        span: token.span,
                    }),
                }
            }
            TokenKind::LParen => {
                self.bump();
                self.skip_expression_newlines();
                let mut expr = self.parse_expr()?;
                self.skip_expression_newlines();
                self.expect(TokenKind::RParen, "')'")?;
                // The reference detects binder lambdas at the token level, so
                // a parenthesized lambda never satisfies a lambda-argument
                // position (issue #445); record the surface on the CST.
                if let Expr::Lambda { parenthesized, .. } = &mut expr {
                    *parenthesized = true;
                }
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
                    self.bump();
                    let variable_token =
                        self.expect(TokenKind::Ident, "a comprehension variable")?;
                    self.skip_newlines();
                    let index = if self.peek_kind() == TokenKind::Comma {
                        self.bump();
                        self.skip_newlines();
                        let index = self.expect(TokenKind::Ident, "a comprehension index")?;
                        Some((index.text, index.span))
                    } else {
                        None
                    };
                    self.skip_newlines();
                    if !self.is_ident("in") {
                        return self.fail("expected `in` in list comprehension".to_string());
                    }
                    self.bump();
                    self.skip_newlines();
                    let iterable = self.parse_or()?;
                    self.skip_newlines();
                    let condition = if self.is_ident("if") {
                        self.bump();
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
                let end = self
                    .parse_delimited_items(
                        &mut elements,
                        true,
                        TokenKind::RBracket,
                        "']'",
                        Self::parse_expr,
                        true,
                    )?
                    .span
                    .end;
                Ok(Expr::Array {
                    elements,
                    span: Span::new(open.span.file, open.span.start, end),
                })
            }
            TokenKind::LBrace => self.parse_dict(),
            _ => self.fail(format!("expected an expression but found '{}'", token.text)),
        }
    }

    fn parse_string_literal(&mut self) -> Expr {
        let mut first = self.advance();
        let mut value = std::mem::take(&mut first.text);
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
        Expr::String {
            value,
            span: Span::new(first.span.file, first.span.start, end),
        }
    }

    pub(super) fn inside_delimiter_group(&self) -> bool {
        self.delimiter_depth[self.pos] != 0
    }

    fn if_is_past_then_value_layout_break(&self) -> bool {
        let then = &self.tokens[self.pos - 1];
        !self.inside_delimiter_group()
            && self.peek().layout.start.line != then.layout.end.line
            && self.newline_after_line(then.layout.file, then.layout.end.line)
    }

    fn parse_dict(&mut self) -> Result<Expr, ()> {
        let open = self.advance();
        let mut entries = Vec::new();
        let end = self
            .parse_delimited_items(
                &mut entries,
                false,
                TokenKind::RBrace,
                "'}'",
                |parser| {
                    let key = parser.parse_expr()?;
                    parser.expect(TokenKind::Colon, "':' in a dictionary entry")?;
                    let value = parser.parse_expr()?;
                    let span = Span::new(key.span().file, key.span().start, value.span().end);
                    Ok(DictEntry { key, value, span })
                },
                true,
            )?
            .span
            .end;
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
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokenKind::Colon, "':' after lambda parameters")?;
        let body = self.parse_expr()?;
        let end = body.span().end;
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
            parenthesized: false,
            span: Span::new(start.file, start.start, end),
        })
    }

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
                        return self.fail_at(
                            "parse-error",
                            "unterminated f-string interpolation".to_string(),
                            string_span,
                        );
                    };
                    let expression: String = chars[index + 1..end].iter().collect();
                    if expression.trim().is_empty() {
                        return self.fail_at(
                            "parse-error",
                            "f-string interpolation cannot be empty".to_string(),
                            crate::parser::bounded_span(
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
                                string_span,
                            ),
                        );
                    }
                    let origin = Position::new(
                        string_span.start.line,
                        string_span.start.col + index as u32 + 1,
                    );
                    let parsed = parse_expression_fragment(
                        &expression,
                        string_span.file,
                        origin,
                        Some(string_span),
                        self.defines,
                    )
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
                    return self.fail_at(
                        "parse-error",
                        "single '}' is not valid in an f-string".to_string(),
                        string_span,
                    );
                }
                '\\' if index + 1 < chars.len() => {
                    if chars[index + 1] == '&' {
                        let name_start = index + 2;
                        let mut name_end = name_start;
                        while name_end < chars.len()
                            && (chars[name_end].is_ascii_alphanumeric() || chars[name_end] == '_')
                        {
                            name_end += 1;
                        }
                        if name_end == name_start || chars.get(name_end) != Some(&';') {
                            return self.fail_at(
                                "invalid-string-entity",
                                "string entity must have the form \\&name;".to_string(),
                                string_span,
                            );
                        }
                        let name: String = chars[name_start..name_end].iter().collect();
                        let Some(decoded) = crate::string_entities::codepoint(&name) else {
                            return self.fail_at(
                                "unknown-string-entity",
                                format!("unknown string entity '&{name};'"),
                                string_span,
                            );
                        };
                        text.push(decoded);
                        index = name_end + 1;
                        continue;
                    }
                    if matches!(chars[index + 1], 'x' | 'u') {
                        let escaped = chars[index + 1];
                        let digits = if escaped == 'x' { 2 } else { 4 };
                        let digits_start = index + 2;
                        if digits_start + digits > chars.len()
                            || !chars[digits_start..digits_start + digits]
                                .iter()
                                .all(|character| character.is_ascii_hexdigit())
                        {
                            return self.fail_at(
                                "lex-error",
                                format!("expected {digits} hexadecimal digits after '\\{escaped}'"),
                                string_span,
                            );
                        }
                        let codepoint = chars[digits_start..digits_start + digits]
                            .iter()
                            .filter_map(|character| character.to_digit(16))
                            .fold(0_u32, |value, digit| value * 16 + digit);
                        text.push(char::from_u32(codepoint).unwrap_or('\u{FFFD}'));
                        index = digits_start + digits;
                        continue;
                    }
                    let Some(decoded) = decode_string_escape(chars[index + 1]) else {
                        return self.fail_at(
                            "lex-error",
                            format!("unknown escape sequence '\\{}'", chars[index + 1]),
                            string_span,
                        );
                    };
                    text.push(decoded);
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

    fn fail<T>(&mut self, message: String) -> Result<T, ()> {
        self.error_at_current(message);
        Err(())
    }

    fn fail_at<T>(&mut self, code: &str, message: String, span: Span) -> Result<T, ()> {
        self.errors.push(OpyError::at(code, message, span));
        Err(())
    }
}

fn unary_op(op: &str, start: Span, operand: Expr) -> Expr {
    let span = Span::new(start.file, start.start, operand.span().end);
    Expr::Unary {
        op: op.to_string(),
        operand: Box::new(operand),
        span,
    }
}

fn binary(op: &str, left: Expr, right: Expr) -> Expr {
    let span = Span::new(left.span().file, left.span().start, right.span().end);
    Expr::Binary {
        op: op.to_string(),
        left: Box::new(left),
        right: Box::new(right),
        span,
    }
}

fn display_tokens(tokens: &[Token]) -> String {
    let mut output = String::new();
    for (index, token) in tokens.iter().enumerate() {
        let text = if token.kind == TokenKind::String {
            let raw = token.raw.as_deref().unwrap_or(&token.text);
            format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
        } else {
            token.text.clone()
        };
        let previous = tokens.get(index.wrapping_sub(1));
        let unary_sign = is_unary_sign(tokens, index);
        let previous_is_unary_sign = index > 0 && is_unary_sign(tokens, index - 1);
        let no_space_before = unary_sign
            || previous_is_unary_sign
            || matches!(
                token.kind,
                TokenKind::LParen
                    | TokenKind::LBracket
                    | TokenKind::LBrace
                    | TokenKind::RParen
                    | TokenKind::RBracket
                    | TokenKind::RBrace
                    | TokenKind::Comma
                    | TokenKind::Colon
                    | TokenKind::Dot
            );
        let no_space_after = previous.is_some_and(|previous| {
            matches!(
                previous.kind,
                TokenKind::LParen
                    | TokenKind::LBracket
                    | TokenKind::LBrace
                    | TokenKind::Dot
                    | TokenKind::Comma
                    | TokenKind::Colon
            )
        });
        if !output.is_empty() && !no_space_before && !no_space_after {
            output.push(' ');
        }
        output.push_str(&text);
        if matches!(token.kind, TokenKind::Comma | TokenKind::Colon) {
            output.push(' ');
        }
    }
    output.trim_end().to_string()
}

fn is_unary_sign(tokens: &[Token], index: usize) -> bool {
    let Some(token) = tokens.get(index) else {
        return false;
    };
    if !matches!(token.kind, TokenKind::Plus | TokenKind::Minus) {
        return false;
    }
    index == 0
        || tokens.get(index - 1).is_some_and(|previous| {
            matches!(
                previous.kind,
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace | TokenKind::Comma
            ) || matches!(
                previous.text.as_str(),
                "else"
                    | "="
                    | "+="
                    | "-="
                    | "*="
                    | "/="
                    | "%="
                    | "**="
                    | "min="
                    | "max="
                    | "if"
                    | "or"
                    | "and"
                    | "not"
                    | "in"
                    | "=="
                    | "!="
                    | "<="
                    | ">="
                    | ">"
                    | "<"
                    | "+"
                    | "-"
                    | "*"
                    | "/"
                    | "%"
                    | "**"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{LexInput, lex};

    #[test]
    fn display_tokens_keeps_pinned_unary_signs_attached() {
        // Pinned OverPy 9.7.10 `dispTokens(..., true)` renders these signs
        // without inserting a space before their operands.
        for (source, expected) in [("-value", "-value"), ("+value", "+value")] {
            let mut tokens = lex(LexInput {
                file_id: 0,
                text: source,
            })
            .unwrap();
            tokens.retain(|token| token.kind != TokenKind::Eof);
            assert_eq!(display_tokens(&tokens), expected, "source: {source}");
        }
    }
}
