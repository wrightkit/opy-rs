use super::*;

impl Parser<'_> {
    pub(super) fn skip_expression_newlines(&mut self) {
        if self.peek_kind() != TokenKind::Newline {
            return;
        }
        let previous = self.tokens[..self.pos]
            .iter()
            .rev()
            .find(|token| !matches!(token.kind, TokenKind::Indent(_)));
        let inside_delimiter_group = self.inside_delimiter_group();
        let previous_allows_continuation = previous.is_some_and(|token| {
            matches!(
                token.kind,
                TokenKind::LParen
                    | TokenKind::LBracket
                    | TokenKind::LBrace
                    | TokenKind::Comma
                    | TokenKind::Colon
                    | TokenKind::Assign
            ) || is_binary_operator(token.kind)
                || (token.kind == TokenKind::Ident
                    && (matches!(token.text.as_str(), "and" | "or" | "in" | "not" | "if")
                        || (inside_delimiter_group && token.text == "else")))
        });
        let mut next = self.pos;
        while self.tokens[next].kind == TokenKind::Newline {
            next += 1;
        }
        let next_allows_continuation = is_binary_operator(self.tokens[next].kind)
            || (inside_delimiter_group
                && matches!(
                    self.tokens[next].kind,
                    TokenKind::LParen
                        | TokenKind::LBracket
                        | TokenKind::Dot
                        | TokenKind::RParen
                        | TokenKind::RBracket
                        | TokenKind::RBrace
                ))
            || (self.tokens[next].kind == TokenKind::Ident
                && matches!(
                    self.tokens[next].text.as_str(),
                    "and" | "or" | "in" | "not" | "else"
                ))
            || (inside_delimiter_group
                && self.tokens[next].kind == TokenKind::Ident
                && matches!(self.tokens[next].text.as_str(), "if" | "for"));
        if previous_allows_continuation || next_allows_continuation {
            self.skip_newlines();
        }
    }
}
