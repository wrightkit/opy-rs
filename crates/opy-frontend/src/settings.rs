//! Scoped `settings { ... }` extraction and JSONC parsing (#86).
//!
//! The settings block is recognized and consumed *before* lexing, so the
//! lexer never gains global `{`/`}` tokens (meipocalypse's dict literal keeps
//! failing as a `lex-error`). [`find_blocks`] locates a top-of-file block
//! with a logical-line keyword scan, [`sanitize_for_lex`] blanks the block
//! region out of the text handed to the lexer (newlines preserved, so
//! positions after the block are unchanged), and [`parse_block`] turns the
//! JSONC text into a typed [`cst::Settings`] tree with source spans.

use crate::cst;
use crate::diag::{FrontendError, FrontendResult, Position, Span};

/// A top-of-file `settings { ... }` block.
#[derive(Debug, Clone)]
pub struct SettingsBlock {
    /// The raw JSONC text between the braces (braces excluded).
    pub text: String,
    /// The whole block: the `settings` keyword through the closing brace.
    pub span: Span,
    /// The `settings` keyword token (diagnostic anchor).
    pub keyword_span: Span,
    /// The char offset of the `settings` keyword (for sanitization).
    pub start: usize,
    /// The char offset just past the closing brace (for sanitization).
    pub end: usize,
    /// The position of the first char of `text` (just past the opening brace).
    pub text_start: Position,
}

/// Locate every `settings { ... }` block in a source text.
///
/// Rules: 0 blocks -> `Ok(vec![])`; the first block must be the first
/// non-comment construct (`settings-placement` otherwise); after `settings`
/// a `{` is required (`settings "file"` form -> `settings-invalid`);
/// a second/later block is `settings-placement` at its keyword span; brace
/// matching respects `"`/`'` strings, `\` escapes, and nesting; an
/// unterminated block is `settings-invalid`.
pub fn find_blocks(text: &str, file_id: u32) -> FrontendResult<Vec<SettingsBlock>> {
    let chars: Vec<char> = text.chars().collect();
    let mut scanner = Scanner {
        chars: &chars,
        pos: 0,
        line: 1,
        col: 1,
    };
    let mut blocks = Vec::new();
    let mut in_block_comment = false;
    let mut seen_first_construct = false;
    while scanner.pos < scanner.chars.len() {
        let ch = scanner.chars[scanner.pos];
        if in_block_comment {
            if ch == '*' && scanner.peek(1) == Some('/') {
                in_block_comment = false;
                scanner.advance(2);
            } else {
                scanner.advance(1);
            }
            continue;
        }
        if ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n' {
            scanner.advance(1);
            continue;
        }
        if ch == '#' {
            scanner.skip_to_eol();
            continue;
        }
        if ch == '/' && scanner.peek(1) == Some('*') {
            scanner.advance(2);
            in_block_comment = true;
            continue;
        }
        // A construct token: the first non-comment token of a logical line.
        if is_ident_start(ch) {
            let keyword_start = scanner.here();
            let keyword_offset = scanner.pos;
            let word = scanner.read_word();
            if word == "settings" {
                let keyword_span = Span::new(file_id, keyword_start, scanner.here());
                if seen_first_construct || !blocks.is_empty() {
                    return Err(FrontendError::at(
                        "settings-placement",
                        "settings block must be the first construct in the file".to_string(),
                        keyword_span,
                    ));
                }
                let block = match_block(&mut scanner, keyword_start, keyword_offset, keyword_span)?;
                blocks.push(block);
                seen_first_construct = true;
                continue;
            }
            seen_first_construct = true;
            continue;
        }
        seen_first_construct = true;
        scanner.advance(1);
    }
    Ok(blocks)
}

/// Match the braces of one `settings { ... }` block, returning the extracted
/// block. `scanner` is positioned just past the `settings` keyword.
fn match_block(
    scanner: &mut Scanner<'_>,
    keyword_start: Position,
    keyword_offset: usize,
    keyword_span: Span,
) -> FrontendResult<SettingsBlock> {
    scanner.skip_whitespace();
    if scanner.chars.get(scanner.pos) != Some(&'{') {
        return Err(FrontendError::at(
            "settings-invalid",
            "settings block must be a `settings { ... }` block (the `settings \"file\"` form is not supported)"
                .to_string(),
            keyword_span,
        ));
    }
    let mut depth = 0usize;
    let mut string_quote: Option<char> = None;
    let mut escaped = false;
    let mut text_start_offset = None;
    let mut text_start = None;
    loop {
        let Some(ch) = scanner.chars.get(scanner.pos).copied() else {
            return Err(FrontendError::at(
                "settings-invalid",
                "unterminated settings block (missing closing brace)".to_string(),
                keyword_span,
            ));
        };
        if let Some(quote) = string_quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                string_quote = None;
            }
            scanner.advance(1);
            continue;
        }
        match ch {
            '"' | '\'' => string_quote = Some(ch),
            '{' => {
                if depth == 0 {
                    text_start_offset = Some(scanner.pos + 1);
                    text_start = Some(scanner.here_after(1));
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let text = scanner
                        .chars
                        .get(text_start_offset.expect("text start offset set on '{'")..scanner.pos)
                        .map(|slice| slice.iter().collect::<String>())
                        .unwrap_or_default();
                    return Ok(SettingsBlock {
                        text,
                        span: Span::new(keyword_span.file, keyword_start, scanner.here_after(1)),
                        keyword_span,
                        start: keyword_offset,
                        end: scanner.pos + 1,
                        text_start: text_start.expect("text start set on '{'"),
                    });
                }
            }
            _ => {}
        }
        scanner.advance(1);
    }
}

/// Replace every char of the block region with a space, preserving newlines,
/// so tokens after the block keep their exact original line/col.
pub fn sanitize_for_lex(text: &str, block: &SettingsBlock) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, ch) in text.chars().enumerate() {
        if index >= block.start && index < block.end {
            out.push(if ch == '\n' { '\n' } else { ' ' });
        } else {
            out.push(ch);
        }
    }
    out
}

/// Parse a settings block's JSONC text into a typed CST settings tree.
///
/// Grammar: quoted keys, `"`/`'` strings with `\` escapes, int/float numbers
/// (f64), `true`/`false`, arrays of strings, nested objects, trailing commas
/// in objects and arrays. Rejections (`settings-invalid`): duplicate keys,
/// non-object root, missing `gamemodes` group, malformed values.
pub fn parse_block(block: &SettingsBlock) -> FrontendResult<cst::Settings> {
    let mut parser = Jsonc {
        text: &block.text,
        pos: 0,
        line: block.text_start.line,
        col: block.text_start.col,
        file: block.span.file,
    };
    parser.skip_whitespace();
    // The block's own braces delimit the root object; the text between them
    // parses as its members.
    let children = parser.parse_members(true)?;
    parser.skip_whitespace();
    if parser.pos < parser.text.len() {
        return Err(parser.error(
            "settings-invalid",
            "unexpected content after the settings object".to_string(),
        ));
    }
    if !children
        .iter()
        .any(|node| matches!(node, cst::SettingsNode::Group { name, .. } if name == "gamemodes"))
    {
        return Err(FrontendError::at(
            "settings-invalid",
            "settings block must contain a gamemodes group".to_string(),
            block.span,
        ));
    }
    Ok(cst::Settings {
        span: block.span,
        children,
    })
}

/// A char scanner with 1-based line/col tracking.
struct Scanner<'a> {
    chars: &'a [char],
    pos: usize,
    line: u32,
    col: u32,
}

impl Scanner<'_> {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.pos + ahead).copied()
    }

    fn here(&self) -> Position {
        Position::new(self.line, self.col)
    }

    fn here_after(&self, n: usize) -> Position {
        let mut line = self.line;
        let mut col = self.col;
        for i in 0..n {
            if self.chars.get(self.pos + i) == Some(&'\n') {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        Position::new(line, col)
    }

    fn advance(&mut self, n: usize) {
        for _ in 0..n {
            if self.pos >= self.chars.len() {
                return;
            }
            if self.chars[self.pos] == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.pos += 1;
        }
    }

    fn skip_to_eol(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
            self.advance(1);
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len() && matches!(self.chars[self.pos], ' ' | '\t' | '\r') {
            self.advance(1);
        }
    }

    fn read_word(&mut self) -> String {
        let mut word = String::new();
        while self.pos < self.chars.len() && is_ident_continue(self.chars[self.pos]) {
            word.push(self.chars[self.pos]);
            self.advance(1);
        }
        word
    }
}

/// A JSONC parser over the block text.
struct Jsonc<'a> {
    text: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    file: u32,
}

impl Jsonc<'_> {
    fn here(&self) -> Position {
        Position::new(self.line, self.col)
    }

    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn error(&self, code: &str, message: String) -> FrontendError {
        FrontendError::at(
            code,
            message,
            Span::new(self.file, self.here(), self.here()),
        )
    }

    fn error_at(&self, code: &str, message: String, span: Span) -> FrontendError {
        FrontendError::at(code, message, span)
    }

    fn parse_object(&mut self) -> FrontendResult<(Vec<cst::SettingsNode>, Span)> {
        let open = self.here();
        if self.advance() != Some('{') {
            return Err(self.error(
                "settings-invalid",
                "settings block must be a JSONC object".to_string(),
            ));
        }
        let members = self.parse_members(false)?;
        let span = Span::new(self.file, open, self.here());
        Ok((members, span))
    }

    /// Parse `key: value, ...` members. `root` is true when the enclosing
    /// object's braces are the settings block's own braces (the text runs to
    /// the end of the block, and a trailing comma before it is allowed).
    fn parse_members(&mut self, root: bool) -> FrontendResult<Vec<cst::SettingsNode>> {
        let mut nodes = Vec::new();
        let mut names = Vec::new();
        self.skip_whitespace();
        if (!root && self.peek() == Some('}')) || (root && self.pos >= self.text.len()) {
            if !root {
                self.advance();
            }
            return Ok(nodes);
        }
        loop {
            self.skip_whitespace();
            let key_start = self.here();
            let key = match self.parse_string_value() {
                Some(value) => value,
                None => {
                    return Err(self.error(
                        "settings-invalid",
                        "settings keys must be quoted strings".to_string(),
                    ));
                }
            };
            let key_span = Span::new(self.file, key_start, self.here());
            if names.contains(&key) {
                return Err(self.error_at(
                    "settings-invalid",
                    format!("duplicate settings key '{key}'"),
                    key_span,
                ));
            }
            names.push(key.clone());
            self.skip_whitespace();
            if self.advance() != Some(':') {
                return Err(self.error_at(
                    "settings-invalid",
                    format!("expected ':' after settings key '{key}'"),
                    key_span,
                ));
            }
            self.skip_whitespace();
            let (node, value_end) = self.parse_value()?;
            let node = build_node(key, node, value_end, key_start, self.file);
            nodes.push(node);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    self.advance();
                    self.skip_whitespace();
                    if (!root && self.peek() == Some('}')) || (root && self.pos >= self.text.len())
                    {
                        if !root {
                            self.advance();
                        }
                        return Ok(nodes);
                    }
                }
                Some('}') if !root => {
                    self.advance();
                    return Ok(nodes);
                }
                None if root => return Ok(nodes),
                _ => {
                    return Err(self.error(
                        "settings-invalid",
                        "expected ',' or '}' in settings object".to_string(),
                    ));
                }
            }
        }
    }

    /// Parse one value; returns the built node (name placeholder) and the
    /// position after it.
    fn parse_value(&mut self) -> FrontendResult<(cst::SettingsNode, Position)> {
        let start = self.here();
        let ch = self.peek();
        let node = match ch {
            Some('"') | Some('\'') => {
                let value = self.parse_string_value().ok_or_else(|| {
                    self.error(
                        "settings-invalid",
                        "unterminated string in settings value".to_string(),
                    )
                })?;
                cst::SettingsNode::String {
                    name: String::new(),
                    value,
                    span: Span::new(self.file, start, self.here()),
                }
            }
            Some('t') => {
                self.expect_word("true")?;
                cst::SettingsNode::Bool {
                    name: String::new(),
                    value: true,
                    span: Span::new(self.file, start, self.here()),
                }
            }
            Some('f') => {
                self.expect_word("false")?;
                cst::SettingsNode::Bool {
                    name: String::new(),
                    value: false,
                    span: Span::new(self.file, start, self.here()),
                }
            }
            Some(c) if c.is_ascii_digit() || c == '-' => {
                let value = self.parse_number()?;
                cst::SettingsNode::Number {
                    name: String::new(),
                    value,
                    span: Span::new(self.file, start, self.here()),
                }
            }
            Some('[') => {
                let elements = self.parse_list()?;
                cst::SettingsNode::List {
                    name: String::new(),
                    elements,
                    span: Span::new(self.file, start, self.here()),
                }
            }
            Some('{') => {
                let (children, _) = self.parse_object()?;
                cst::SettingsNode::Group {
                    name: String::new(),
                    children,
                    span: Span::new(self.file, start, self.here()),
                }
            }
            _ => {
                return Err(self.error(
                    "settings-invalid",
                    "expected a value in settings block".to_string(),
                ));
            }
        };
        let end = self.here();
        Ok((node, end))
    }

    fn expect_word(&mut self, word: &str) -> FrontendResult<()> {
        let start = self.here();
        for expected in word.chars() {
            if self.advance() != Some(expected) {
                return Err(self.error_at(
                    "settings-invalid",
                    format!("expected '{word}' in settings block"),
                    Span::new(self.file, start, self.here()),
                ));
            }
        }
        Ok(())
    }

    fn parse_number(&mut self) -> FrontendResult<f64> {
        let start = self.here();
        let mut text = String::new();
        if self.peek() == Some('-') {
            text.push(self.advance().unwrap());
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                text.push(self.advance().unwrap());
            } else {
                break;
            }
        }
        if self.peek() == Some('.') {
            text.push(self.advance().unwrap());
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    text.push(self.advance().unwrap());
                } else {
                    break;
                }
            }
        }
        text.parse::<f64>().map_err(|_| {
            self.error_at(
                "settings-invalid",
                format!("invalid number '{text}' in settings block"),
                Span::new(self.file, start, self.here()),
            )
        })
    }

    fn parse_list(&mut self) -> FrontendResult<Vec<cst::SettingsListElement>> {
        self.advance(); // '['
        let mut elements = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.advance();
            return Ok(elements);
        }
        loop {
            self.skip_whitespace();
            let start = self.here();
            let value = match self.parse_string_value() {
                Some(value) => value,
                None => {
                    return Err(self.error(
                        "settings-invalid",
                        "settings list elements must be strings".to_string(),
                    ));
                }
            };
            let span = Span::new(self.file, start, self.here());
            elements.push(cst::SettingsListElement { value, span });
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    self.advance();
                    self.skip_whitespace();
                    if self.peek() == Some(']') {
                        self.advance();
                        return Ok(elements);
                    }
                }
                Some(']') => {
                    self.advance();
                    return Ok(elements);
                }
                _ => {
                    return Err(self.error(
                        "settings-invalid",
                        "expected ',' or ']' in settings list".to_string(),
                    ));
                }
            }
        }
    }

    /// Parse a quoted string value; `None` when no string is here or the
    /// string is unterminated before end-of-line.
    fn parse_string_value(&mut self) -> Option<String> {
        let quote = self.peek()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        self.advance();
        let mut value = String::new();
        loop {
            let ch = self.advance()?;
            if ch == '\n' {
                return None;
            }
            if ch == quote {
                return Some(value);
            }
            if ch == '\\' {
                let escaped = self.advance()?;
                match escaped {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'r' => value.push('\r'),
                    other => value.push(other),
                }
            } else {
                value.push(ch);
            }
        }
    }
}

/// Attach the key name and a key..value span to a parsed value node.
fn build_node(
    key: String,
    node: cst::SettingsNode,
    value_end: Position,
    key_start: Position,
    file: u32,
) -> cst::SettingsNode {
    let span = Span::new(file, key_start, value_end);
    match node {
        cst::SettingsNode::Group { children, .. } => cst::SettingsNode::Group {
            name: key,
            children,
            span,
        },
        cst::SettingsNode::Number { value, .. } => cst::SettingsNode::Number {
            name: key,
            value,
            span,
        },
        cst::SettingsNode::Bool { value, .. } => cst::SettingsNode::Bool {
            name: key,
            value,
            span,
        },
        cst::SettingsNode::String { value, .. } => cst::SettingsNode::String {
            name: key,
            value,
            span,
        },
        cst::SettingsNode::List { elements, .. } => cst::SettingsNode::List {
            name: key,
            elements,
            span,
        },
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

    fn block(text: &str) -> SettingsBlock {
        let mut blocks = find_blocks(text, 0).unwrap();
        assert_eq!(blocks.len(), 1, "one block expected in: {text}");
        blocks.pop().unwrap()
    }

    #[test]
    fn finds_block_after_comments_and_blanks() {
        let text = "/* header */\n# comment\n\nsettings {\n    \"gamemodes\": {}\n}\nrule \"r\":\n";
        let found = block(text);
        assert!(found.text.contains("gamemodes"));
        assert_eq!(found.keyword_span.start.line, 4);
        assert_eq!(found.span.end.line, 6);
    }

    #[test]
    fn no_blocks_for_plain_program() {
        assert!(
            find_blocks("rule \"r\":\n    pass\n", 0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn settings_not_first_construct_is_placement_error() {
        let error = find_blocks("rule \"r\":\n    pass\nsettings {\n}\n", 0).unwrap_err();
        assert_eq!(error.code, "settings-placement");
        assert_eq!(error.span.unwrap().start.line, 3);
    }

    #[test]
    fn second_block_is_placement_error() {
        let error =
            find_blocks("settings {\n    \"gamemodes\": {}\n}\nsettings {\n}\n", 0).unwrap_err();
        assert_eq!(error.code, "settings-placement");
        assert_eq!(error.span.unwrap().start.line, 4);
    }

    #[test]
    fn settings_file_form_is_invalid() {
        let error = find_blocks("settings \"file.opy\"\n", 0).unwrap_err();
        assert_eq!(error.code, "settings-invalid");
    }

    #[test]
    fn unterminated_block_is_invalid() {
        let error = find_blocks("settings {\n    \"gamemodes\": {\n", 0).unwrap_err();
        assert_eq!(error.code, "settings-invalid");
    }

    #[test]
    fn braces_inside_strings_do_not_unbalance() {
        let found =
            block("settings {\n    \"description\": \"a { b }\",\n    \"gamemodes\": {}\n}\n");
        assert!(found.text.contains("a { b }"));
    }

    #[test]
    fn sanitize_preserves_post_block_positions() {
        let text = "settings {\n    \"gamemodes\": {}\n}\nrule \"r\":\n    pass\n";
        let found = block(text);
        let sanitized = sanitize_for_lex(text, &found);
        let lines: Vec<&str> = sanitized.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[3], "rule \"r\":");
        assert_eq!(lines[4], "    pass");
        // The rule keyword is at the same char offset as in the original.
        assert_eq!(sanitized.find("rule"), text.find("rule"));
    }

    #[test]
    fn non_object_after_settings_is_invalid() {
        // `settings [..]` is rejected at extraction (a `{` is required).
        let error = find_blocks("settings [1, 2]\n", 0).unwrap_err();
        assert_eq!(error.code, "settings-invalid");
    }

    #[test]
    fn parse_block_rejects_missing_gamemodes() {
        let found = block("settings {\n    \"main\": { \"description\": \"x\" }\n}\n");
        let error = parse_block(&found).unwrap_err();
        assert_eq!(error.code, "settings-invalid");
        assert!(error.message.contains("gamemodes"));
    }

    #[test]
    fn parse_block_rejects_duplicate_keys() {
        let found = block("settings {\n    \"gamemodes\": {},\n    \"gamemodes\": {}\n}\n");
        let error = parse_block(&found).unwrap_err();
        assert_eq!(error.code, "settings-invalid");
        assert!(error.message.contains("duplicate"));
    }

    #[test]
    fn parse_block_accepts_trailing_commas() {
        let found = block(
            "settings {\n    \"gamemodes\": {\n        \"general\": {\n            \"heroLimit\": \"off\",\n        },\n    },\n}\n",
        );
        let parsed = parse_block(&found).unwrap();
        assert_eq!(parsed.children.len(), 1);
    }

    #[test]
    fn parse_block_handles_escapes_and_quotes() {
        let found = block(
            "settings {\n    \"main\": { \"description\": \"line\\n\\t\\\"quoted\\\"\" },\n    \"gamemodes\": {}\n}\n",
        );
        let parsed = parse_block(&found).unwrap();
        let cst::SettingsNode::Group { children, .. } = &parsed.children[0] else {
            panic!("main group");
        };
        let cst::SettingsNode::String { value, .. } = &children[0] else {
            panic!("description");
        };
        assert_eq!(value, "line\n\t\"quoted\"");
    }

    #[test]
    fn parse_block_types_values() {
        let found = block(
            "settings {\n    \"lobby\": { \"ffaSlots\": 6 },\n    \"gamemodes\": { \"general\": { \"enableRandomHeroes\": true, \"respawnTime%\": 30, \"heroLimit\": \"off\" } },\n    \"heroes\": { \"allTeams\": { \"enabledHeroes\": [\"mei\"] } }\n}\n",
        );
        let parsed = parse_block(&found).unwrap();
        let lobby = match &parsed.children[0] {
            cst::SettingsNode::Group { name, children, .. } => {
                assert_eq!(name, "lobby");
                children
            }
            other => panic!("{other:?}"),
        };
        assert!(matches!(
            lobby[0],
            cst::SettingsNode::Number { value: 6.0, .. }
        ));
    }

    #[test]
    fn spans_are_computed_from_block_base() {
        let text = "settings {\n    \"lobby\": {\n        \"ffaSlots\": 6\n    },\n    \"gamemodes\": {}\n}\n";
        let found = block(text);
        let parsed = parse_block(&found).unwrap();
        let cst::SettingsNode::Group { children, .. } = &parsed.children[0] else {
            panic!("lobby");
        };
        let cst::SettingsNode::Number { span, .. } = &children[0] else {
            panic!("ffaSlots");
        };
        assert_eq!(span.start.line, 3);
        assert_eq!(span.start.col, 9);
    }

    #[test]
    fn keyword_span_carries_the_file_id() {
        let found = block("settings {\n    \"gamemodes\": {}\n}\n");
        assert_eq!(found.keyword_span.file, 0);
        assert_eq!(found.keyword_span.start.col, 1);
        assert_eq!(found.keyword_span.end.col, 9);
    }
}
