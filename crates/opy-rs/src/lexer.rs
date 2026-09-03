//! The native `.opy` lexer.
//!
//! Produces a flat token stream (newlines and indentation included) from one
//! source file. Comments (`#`, `/* */`) are skipped; `#!` directives are
//! captured as a single directive token for the preprocessor. Positions are
//! 1-based line/column, matching the Opy HIR protocol.

use crate::diag::{OpyError, OpyResult, Position, Span};

/// The kind of a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// An identifier or keyword (keywords are resolved by the parser).
    Ident,
    /// A numeric literal (`text` holds the source spelling).
    Number,
    /// A string literal (`text` holds the unescaped value).
    String,
    /// A `#!` directive line (`text` holds everything after `#!`).
    Directive,
    /// A preprocessing marker carrying the rule-prefix state active at the
    /// following top-level rule or subroutine.
    RulePrefixMarker,
    /// `@Event` / `@Condition` / other `@` directives.
    At,
    Newline,
    /// Indentation change: the column of the current line.
    Indent(u32),
    /// End of file.
    Eof,
    // Punctuation and operators.
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Semicolon,
    Dot,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    DoubleStar,
    PlusAssign,
    MinusAssign,
    Increment,
    Decrement,
    StarAssign,
    SlashAssign,
    PercentAssign,
    DoubleStarAssign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// A bare `!` that is not `!=`; the parser rejects it as unsupported OPY.
    LexBang,
}

/// One token with its source span and payload text.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// The source text of this token (numbers keep their spelling; strings
    /// keep their unescaped value; identifiers keep their name).
    pub text: String,
    /// The exact characters between string quotes, before escape decoding.
    /// Other token kinds leave this unset. It is retained so source-language
    /// constructs such as f-string interpolations can recover expression
    /// spans without losing provenance during preprocessing.
    pub raw: Option<String>,
    pub span: Span,
}

impl Token {
    fn new(kind: TokenKind, text: impl Into<String>, span: Span) -> Token {
        Token {
            kind,
            text: text.into(),
            raw: None,
            span,
        }
    }
}

/// The lexer input: one file's text with its file id.
pub struct LexInput<'a> {
    pub file_id: u32,
    pub text: &'a str,
}

/// Lex one source file into a token stream.
pub fn lex(input: LexInput<'_>) -> OpyResult<Vec<Token>> {
    Lexer::new(input.file_id, input.text).run()
}

struct Lexer {
    file_id: u32,
    chars: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
    tokens: Vec<Token>,
}

impl Lexer {
    fn new(file_id: u32, text: &str) -> Lexer {
        Lexer {
            file_id,
            chars: text.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
        }
    }

    fn run(mut self) -> OpyResult<Vec<Token>> {
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            match ch {
                '\n' => {
                    self.tokens
                        .push(Token::new(TokenKind::Newline, "\n", self.here(1)));
                    self.advance();
                    self.line += 1;
                    self.col = 1;
                }
                ' ' | '\r' => {
                    self.advance();
                }
                '\t' => {
                    self.pos += 1;
                    self.col += 4;
                }
                '\\' => {
                    if !self.skip_line_continuation() {
                        return Err(OpyError::at(
                            "lex-error",
                            "unexpected character '\\'",
                            self.here(1),
                        ));
                    }
                }
                '#' => self.lex_hash()?,
                '/' if self.peek(1) == Some('*') => self.skip_block_comment()?,
                '"' | '\'' => self.lex_string(ch)?,
                c if c.is_ascii_digit() => self.lex_number()?,
                c if is_ident_start(c) => self.lex_ident(),
                '(' => self.single(TokenKind::LParen),
                ')' => self.single(TokenKind::RParen),
                '[' => self.single(TokenKind::LBracket),
                ']' => self.single(TokenKind::RBracket),
                '{' => self.single(TokenKind::LBrace),
                '}' => self.single(TokenKind::RBrace),
                ',' => self.single(TokenKind::Comma),
                ':' => self.single(TokenKind::Colon),
                ';' => self.single(TokenKind::Semicolon),
                '.' => self.single(TokenKind::Dot),
                '@' => self.single(TokenKind::At),
                '=' => self.two(TokenKind::Assign, TokenKind::Eq, '='),
                '+' => {
                    if self.peek(1) == Some('+') {
                        self.lex_duplicate(TokenKind::Increment, "++");
                    } else {
                        self.lex_two(TokenKind::Plus, TokenKind::PlusAssign, '=');
                    }
                }
                '-' => {
                    if self.peek(1) == Some('-') {
                        self.lex_duplicate(TokenKind::Decrement, "--");
                    } else {
                        self.lex_two(TokenKind::Minus, TokenKind::MinusAssign, '=');
                    }
                }
                '*' => {
                    if self.peek(1) == Some('*') {
                        if self.peek(2) == Some('=') {
                            let start = self.here(3);
                            self.advance();
                            self.advance();
                            self.advance();
                            let end = self.here(0);
                            self.tokens.push(Token::new(
                                TokenKind::DoubleStarAssign,
                                "**=",
                                Span::new(self.file_id, start.start, end.start),
                            ));
                        } else {
                            self.advance();
                            self.single(TokenKind::DoubleStar)
                        }
                    } else {
                        self.lex_two(TokenKind::Star, TokenKind::StarAssign, '=')
                    }
                }
                '/' => self.lex_two(TokenKind::Slash, TokenKind::SlashAssign, '='),
                '%' => self.lex_two(TokenKind::Percent, TokenKind::PercentAssign, '='),
                '<' => self.two(TokenKind::Lt, TokenKind::Le, '='),
                '>' => self.two(TokenKind::Gt, TokenKind::Ge, '='),
                '!' => self.two(TokenKind::LexBang, TokenKind::Ne, '='),
                other => {
                    return Err(OpyError::at(
                        "lex-error",
                        format!("unexpected character '{other}'"),
                        self.here(1),
                    ));
                }
            }
        }
        let here = self.here(0);
        self.tokens.push(Token::new(TokenKind::Eof, "", here));
        Ok(self.tokens)
    }

    /// `#` starts a `#!` directive (captured as one token) or a comment.
    fn lex_hash(&mut self) -> OpyResult<()> {
        if self.peek(1) == Some('!') {
            let start = self.here(2);
            self.advance();
            self.advance();
            let mut text = String::new();
            while self.pos < self.chars.len() {
                if self.chars[self.pos] == '\\' && self.skip_line_continuation() {
                    continue;
                }
                if self.chars[self.pos] == '\n' {
                    break;
                }
                text.push(self.chars[self.pos]);
                self.advance();
            }
            let end = self.here(0);
            self.tokens.push(Token::new(
                TokenKind::Directive,
                text,
                Span::new(self.file_id, start.start, end.start),
            ));
        } else {
            while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
                self.advance();
            }
        }
        Ok(())
    }

    fn skip_block_comment(&mut self) -> OpyResult<()> {
        let start = self.here(2);
        self.advance();
        self.advance();
        while self.pos < self.chars.len() {
            if self.chars[self.pos] == '*' && self.peek(1) == Some('/') {
                self.advance();
                self.advance();
                return Ok(());
            }
            if self.chars[self.pos] == '\n' {
                self.advance();
                self.line += 1;
                self.col = 1;
            } else {
                self.advance();
            }
        }
        Err(OpyError::at(
            "lex-error",
            "unterminated block comment",
            start,
        ))
    }

    fn lex_string(&mut self, quote: char) -> OpyResult<()> {
        let start = self.here(1);
        self.advance();
        let mut value = String::new();
        let mut raw = String::new();
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch == quote {
                self.advance();
                let end = self.here(0);
                let mut token = Token::new(
                    TokenKind::String,
                    value,
                    Span::new(self.file_id, start.start, end.start),
                );
                token.raw = Some(raw);
                self.tokens.push(token);
                return Ok(());
            }
            if ch == '\\' {
                raw.push(ch);
                self.advance();
                if self.pos >= self.chars.len() {
                    break;
                }
                let escaped = self.chars[self.pos];
                raw.push(escaped);
                value.push(match escaped {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '\\' => '\\',
                    '"' => '"',
                    '\'' => '\'',
                    other => other,
                });
                self.advance();
                continue;
            }
            if ch == '\n' {
                return Err(OpyError::at(
                    "lex-error",
                    "unterminated string literal",
                    start,
                ));
            }
            raw.push(ch);
            value.push(ch);
            self.advance();
        }
        Err(OpyError::at(
            "lex-error",
            "unterminated string literal",
            start,
        ))
    }

    fn skip_line_continuation(&mut self) -> bool {
        let mut offset = 1;
        while matches!(self.peek(offset), Some(' ' | '\r')) {
            offset += 1;
        }
        if self.peek(offset) != Some('\n') {
            return false;
        }
        for _ in 0..=offset {
            self.advance();
        }
        self.line += 1;
        self.col = 1;
        true
    }

    fn lex_number(&mut self) -> OpyResult<()> {
        let start = self.here(1);
        let mut text = String::new();
        if self.chars[self.pos] == '0' && matches!(self.peek(1), Some('x' | 'X')) {
            text.push('0');
            self.advance();
            text.push(self.chars[self.pos]);
            self.advance();
            let digits_start = self.pos;
            while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_hexdigit() {
                text.push(self.chars[self.pos]);
                self.advance();
            }
            if self.pos == digits_start {
                return Err(OpyError::at(
                    "lex-error",
                    "hexadecimal literal requires at least one hexadecimal digit",
                    Span::new(self.file_id, start.start, self.here(0).start),
                ));
            }
            let end = self.here(0);
            self.tokens.push(Token::new(
                TokenKind::Number,
                text,
                Span::new(self.file_id, start.start, end.start),
            ));
            return Ok(());
        }
        while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
            text.push(self.chars[self.pos]);
            self.advance();
        }
        if self.pos < self.chars.len()
            && self.chars[self.pos] == '.'
            && self.peek(1).is_some_and(|c| c.is_ascii_digit())
        {
            text.push('.');
            self.advance();
            while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
                text.push(self.chars[self.pos]);
                self.advance();
            }
        }
        // Optional exponent (not exercised by the corpus, supported for
        // completeness of the number surface).
        if self.pos < self.chars.len()
            && (self.chars[self.pos] == 'e' || self.chars[self.pos] == 'E')
        {
            let mut lookahead = self.pos + 1;
            if lookahead < self.chars.len()
                && (self.chars[lookahead] == '+' || self.chars[lookahead] == '-')
            {
                lookahead += 1;
            }
            if lookahead < self.chars.len() && self.chars[lookahead].is_ascii_digit() {
                text.push('e');
                self.advance();
                if self.pos < self.chars.len()
                    && (self.chars[self.pos] == '+' || self.chars[self.pos] == '-')
                {
                    text.push(self.chars[self.pos]);
                    self.advance();
                }
                while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
                    text.push(self.chars[self.pos]);
                    self.advance();
                }
            }
        }
        let end = self.here(0);
        self.tokens.push(Token::new(
            TokenKind::Number,
            text,
            Span::new(self.file_id, start.start, end.start),
        ));
        Ok(())
    }

    fn lex_ident(&mut self) {
        let start = self.here(1);
        let mut text = String::new();
        while self.pos < self.chars.len() && is_ident_continue(self.chars[self.pos]) {
            text.push(self.chars[self.pos]);
            self.advance();
        }
        let end = self.here(0);
        self.tokens.push(Token::new(
            TokenKind::Ident,
            text,
            Span::new(self.file_id, start.start, end.start),
        ));
    }

    fn single(&mut self, kind: TokenKind) {
        let start = self.here(1);
        let text = self.chars[self.pos].to_string();
        self.advance();
        let end = self.here(0);
        self.tokens.push(Token::new(
            kind,
            text,
            Span::new(self.file_id, start.start, end.start),
        ));
    }

    /// Two-char operator where the second char may be `=`.
    fn lex_two(&mut self, plain: TokenKind, assign: TokenKind, second: char) {
        let start = self.here(1);
        if self.peek(1) == Some(second) {
            self.advance();
            let text = format!("{}{}", self.chars[self.pos - 1], second);
            self.advance();
            let end = self.here(0);
            self.tokens.push(Token::new(
                assign,
                text,
                Span::new(self.file_id, start.start, end.start),
            ));
        } else {
            let text = self.chars[self.pos].to_string();
            self.advance();
            let end = self.here(0);
            self.tokens.push(Token::new(
                plain,
                text,
                Span::new(self.file_id, start.start, end.start),
            ));
        }
    }

    fn lex_duplicate(&mut self, kind: TokenKind, text: &str) {
        let start = self.here(1);
        self.advance();
        self.advance();
        let end = self.here(0);
        self.tokens.push(Token::new(
            kind,
            text,
            Span::new(self.file_id, start.start, end.start),
        ));
    }

    /// Two-char operator with a fixed second char (e.g. `==`, `<=`).
    fn two(&mut self, plain: TokenKind, combined: TokenKind, second: char) {
        let start = self.here(1);
        let text = self.chars[self.pos].to_string();
        if self.peek(1) == Some(second) {
            self.advance();
            let combined_text = format!("{}{}", text, second);
            self.advance();
            let end = self.here(0);
            self.tokens.push(Token::new(
                combined,
                combined_text,
                Span::new(self.file_id, start.start, end.start),
            ));
        } else {
            self.advance();
            let end = self.here(0);
            self.tokens.push(Token::new(
                plain,
                text,
                Span::new(self.file_id, start.start, end.start),
            ));
        }
    }

    fn here(&self, width: usize) -> Span {
        Span::new(
            self.file_id,
            Position::new(self.line, self.col),
            Position::new(self.line, self.col + width as u32),
        )
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) {
        self.pos += 1;
        self.col += 1;
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_ok(text: &str) -> Vec<Token> {
        lex(LexInput { file_id: 0, text }).unwrap()
    }

    #[test]
    fn lexes_basic_rule() {
        let tokens = lex_ok("rule \"setup\":\n    @Event global\n    disableInspector()\n");
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::Ident));
        assert!(kinds.contains(&TokenKind::String));
        assert!(kinds.contains(&TokenKind::Colon));
        assert!(kinds.contains(&TokenKind::At));
        assert!(kinds.contains(&TokenKind::LParen));
        assert!(kinds.contains(&TokenKind::Eof));
    }

    #[test]
    fn numbers_preserve_text() {
        let tokens = lex_ok("1 2.5 0.016 100");
        let numbers: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Number)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(numbers, vec!["1", "2.5", "0.016", "100"]);
    }

    #[test]
    fn directives_and_comments() {
        let tokens = lex_ok("#!define X 1\n# comment\nrule \"r\":\n");
        let directive = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Directive)
            .unwrap();
        assert_eq!(directive.text, "define X 1");
        assert!(!tokens.iter().any(|t| t.text == "comment"));
    }

    #[test]
    fn directive_line_continuation_is_part_of_one_directive() {
        let tokens = lex_ok("#!define X first + \\\n  second\n");
        let directive = tokens
            .iter()
            .find(|token| token.kind == TokenKind::Directive)
            .unwrap();
        assert_eq!(directive.text, "define X first +   second");
        assert_eq!(directive.span.start, Position::new(1, 1));
        assert_eq!(directive.span.end, Position::new(2, 9));
    }

    #[test]
    fn operators() {
        let tokens = lex_ok("a += b == c <= d != e / f");
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        for expected in [
            TokenKind::PlusAssign,
            TokenKind::Eq,
            TokenKind::Le,
            TokenKind::Ne,
            TokenKind::Slash,
        ] {
            assert!(
                kinds.contains(&expected),
                "missing {expected:?} in {kinds:?}"
            );
        }
    }

    #[test]
    fn power_operators_disambiguate() {
        // The pinned OverPy 9.7.10 reference lexes `**=` as one power-assign
        // operator distinct from `**` and `*=`.
        let tokens = lex_ok("a **= b ** c *= d");
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident,
                TokenKind::DoubleStarAssign,
                TokenKind::Ident,
                TokenKind::DoubleStar,
                TokenKind::Ident,
                TokenKind::StarAssign,
                TokenKind::Ident,
                TokenKind::Eof,
            ]
        );
        let assign = tokens
            .iter()
            .find(|t| t.kind == TokenKind::DoubleStarAssign)
            .unwrap();
        assert_eq!(assign.text, "**=");
    }

    #[test]
    fn postfix_operators_are_single_tokens() {
        let tokens = lex_ok("counter++ points--");
        assert_eq!(
            tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
            vec![
                TokenKind::Ident,
                TokenKind::Increment,
                TokenKind::Ident,
                TokenKind::Decrement,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[1].span.start.col, 8);
        assert_eq!(tokens[1].span.end.col, 10);
    }

    #[test]
    fn unterminated_string_is_structured() {
        let error = lex(LexInput {
            file_id: 0,
            text: "rule \"x\n",
        })
        .unwrap_err();
        assert_eq!(error.code, "lex-error");
        assert!(error.span.is_some());
    }

    #[test]
    fn backslash_line_continuation_is_not_a_token() {
        let tokens = lex_ok("one \\\ntwo");
        assert_eq!(
            tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
            vec![TokenKind::Ident, TokenKind::Ident, TokenKind::Eof]
        );
        assert_eq!(tokens[1].span.start.line, 2);
        assert_eq!(tokens[1].span.start.col, 1);
    }

    #[test]
    fn crlf_line_continuation_tracks_the_next_line() {
        let tokens = lex_ok("one \\\r\ntwo");
        assert_eq!(tokens[1].span.start, Position::new(2, 1));
    }

    #[test]
    fn whitespace_before_line_ending_is_part_of_the_continuation() {
        let tokens = lex_ok("one \\  \ntwo");
        assert_eq!(tokens[1].span.start, Position::new(2, 1));
    }

    #[test]
    fn non_newline_backslash_remains_a_lex_error() {
        for text in ["one \\ two", "one \\", "one \\ \t\ntwo"] {
            let error = lex(LexInput { file_id: 0, text }).unwrap_err();
            assert_eq!(error.code, "lex-error");
            assert_eq!(error.message, "unexpected character '\\'");
            assert_eq!(error.span.unwrap().start, Position::new(1, 5));
        }
    }
}
