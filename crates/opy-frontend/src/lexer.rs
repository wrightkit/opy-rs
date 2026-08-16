//! The native `.opy` lexer.
//!
//! Produces a flat token stream (newlines and indentation included) from one
//! source file. Comments (`#`, `/* */`) are skipped; `#!` directives are
//! captured as a single directive token for the preprocessor. Positions are
//! 1-based line/column, matching the Opy HIR protocol.

use crate::diag::{FrontendError, FrontendResult, Position, Span};

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
    Comma,
    Colon,
    Dot,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    DoubleSlash,
    Percent,
    DoubleStar,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    DoubleSlashAssign,
    PercentAssign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// A bare `!` that is not `!=` (error unless followed by `=`).
    LexBang,
}

/// One token with its source span and payload text.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// The source text of this token (numbers keep their spelling; strings
    /// keep their unescaped value; identifiers keep their name).
    pub text: String,
    pub span: Span,
}

impl Token {
    fn new(kind: TokenKind, text: impl Into<String>, span: Span) -> Token {
        Token {
            kind,
            text: text.into(),
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
pub fn lex(input: LexInput<'_>) -> FrontendResult<Vec<Token>> {
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

    fn run(mut self) -> FrontendResult<Vec<Token>> {
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
                ' ' | '\t' | '\r' => {
                    self.advance();
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
                ',' => self.single(TokenKind::Comma),
                ':' => self.single(TokenKind::Colon),
                '.' => self.single(TokenKind::Dot),
                '@' => self.single(TokenKind::At),
                '=' => self.two(TokenKind::Assign, TokenKind::Eq, '='),
                '+' => self.lex_two(TokenKind::Plus, TokenKind::PlusAssign, '='),
                '-' => self.lex_two(TokenKind::Minus, TokenKind::MinusAssign, '='),
                '*' => {
                    if self.peek(1) == Some('*') {
                        self.advance();
                        self.single(TokenKind::DoubleStar)
                    } else {
                        self.lex_two(TokenKind::Star, TokenKind::StarAssign, '=')
                    }
                }
                '/' => {
                    if self.peek(1) == Some('/') {
                        self.advance();
                        self.single(TokenKind::DoubleSlash)
                    } else {
                        self.lex_two(TokenKind::Slash, TokenKind::SlashAssign, '=')
                    }
                }
                '%' => self.lex_two(TokenKind::Percent, TokenKind::PercentAssign, '='),
                '<' => self.two(TokenKind::Lt, TokenKind::Le, '='),
                '>' => self.two(TokenKind::Gt, TokenKind::Ge, '='),
                '!' => self.two(TokenKind::LexBang, TokenKind::Ne, '='),
                other => {
                    return Err(FrontendError::at(
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
    fn lex_hash(&mut self) -> FrontendResult<()> {
        if self.peek(1) == Some('!') {
            let start = self.here(2);
            self.advance();
            self.advance();
            let mut text = String::new();
            while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
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

    fn skip_block_comment(&mut self) -> FrontendResult<()> {
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
        Err(FrontendError::at(
            "lex-error",
            "unterminated block comment",
            start,
        ))
    }

    fn lex_string(&mut self, quote: char) -> FrontendResult<()> {
        let start = self.here(1);
        self.advance();
        let mut value = String::new();
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch == quote {
                self.advance();
                let end = self.here(0);
                self.tokens.push(Token::new(
                    TokenKind::String,
                    value,
                    Span::new(self.file_id, start.start, end.start),
                ));
                return Ok(());
            }
            if ch == '\\' {
                self.advance();
                if self.pos >= self.chars.len() {
                    break;
                }
                let escaped = self.chars[self.pos];
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
                return Err(FrontendError::at(
                    "lex-error",
                    "unterminated string literal",
                    start,
                ));
            }
            value.push(ch);
            self.advance();
        }
        Err(FrontendError::at(
            "lex-error",
            "unterminated string literal",
            start,
        ))
    }

    fn lex_number(&mut self) -> FrontendResult<()> {
        let start = self.here(1);
        let mut text = String::new();
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
    fn operators() {
        let tokens = lex_ok("a += b == c <= d != e // f");
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        for expected in [
            TokenKind::PlusAssign,
            TokenKind::Eq,
            TokenKind::Le,
            TokenKind::Ne,
            TokenKind::DoubleSlash,
        ] {
            assert!(
                kinds.contains(&expected),
                "missing {expected:?} in {kinds:?}"
            );
        }
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
}
