use super::*;

impl Parser<'_> {
    pub(super) fn parse_additive(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let mut left = self.parse_multiplicative()?;
        loop {
            self.skip_expression_newlines();
            if matches!(
                self.peek_kind(),
                TokenKind::Increment | TokenKind::Decrement
            ) && !matches!(self.peek_at(1).kind, TokenKind::Newline | TokenKind::Eof)
            {
                let operator = self.advance();
                let operand = self.parse_unary()?;
                let (op, right) = if operator.kind == TokenKind::Decrement {
                    ("-", unary_op("-", operator.span, operand))
                } else {
                    ("+", operand)
                };
                let right = self.parse_multiplicative_tail(right)?;
                left = binary(op, left, right);
                continue;
            }
            let op = match self.peek_kind() {
                TokenKind::Plus => "+",
                TokenKind::Minus => "-",
                _ => break,
            };
            self.bump();
            self.skip_expression_newlines();
            let right = self.parse_multiplicative()?;
            left = binary(op, left, right);
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        let left = self.parse_unary()?;
        self.parse_multiplicative_tail(left)
    }

    fn parse_multiplicative_tail(&mut self, mut left: Expr) -> Result<Expr, ()> {
        loop {
            self.skip_expression_newlines();
            let op = match self.peek_kind() {
                TokenKind::Star => "*",
                TokenKind::Slash => "/",
                TokenKind::Percent => "%",
                _ => break,
            };
            self.bump();
            self.skip_expression_newlines();
            let right = self.parse_unary()?;
            left = binary(op, left, right);
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ()> {
        self.skip_expression_newlines();
        if matches!(self.peek_kind(), TokenKind::Plus | TokenKind::Increment) {
            self.bump();
            return self.parse_unary();
        }
        if matches!(self.peek_kind(), TokenKind::Minus | TokenKind::Decrement) {
            let start = self.advance();
            self.skip_expression_newlines();
            let operand = self.parse_unary()?;
            let unary = unary_op("-", start.span, operand);
            if start.kind == TokenKind::Decrement {
                return Ok(unary_op("-", start.span, unary));
            }
            return Ok(unary);
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<Expr, ()> {
        let base = self.parse_postfix()?;
        self.skip_expression_newlines();
        if self.peek_kind() == TokenKind::DoubleStar {
            self.bump();
            self.skip_expression_newlines();
            let exponent = self.parse_unary()?;
            return Ok(binary("**", base, exponent));
        }
        Ok(base)
    }
}
