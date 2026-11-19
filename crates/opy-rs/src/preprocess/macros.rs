use super::directives::strip_quoted;
use super::*;

#[derive(Debug, Clone)]
pub(crate) struct MacroDef {
    pub(crate) name: String,
    pub(crate) params: Vec<String>,
    pub(crate) body_text: String,
    pub(crate) is_function: bool,
    pub(super) is_multiline: bool,
    pub(crate) script: Option<ScriptMacro>,
}

pub(super) struct MacroArgument {
    pub(super) raw: String,
}

pub(super) struct LineOffsets {
    starts: Vec<usize>,
}

impl LineOffsets {
    fn new(text: &str) -> LineOffsets {
        let mut starts = vec![0];
        for (offset, character) in text.char_indices() {
            if character == '\n' {
                starts.push(offset + 1);
            }
        }
        LineOffsets { starts }
    }

    fn offset_of(&self, text: &str, position: crate::diag::Position) -> Option<usize> {
        let width = position.col.checked_sub(1)? as usize;
        let line = position.line as usize;
        let start = *self.starts.get(line.checked_sub(1)?)?;
        let mut chars_seen = 0usize;
        for (offset, character) in text[start..].char_indices() {
            if character == '\n' {
                return (chars_seen == width).then_some(start + offset);
            }
            if chars_seen == width {
                return Some(start + offset);
            }
            chars_seen += 1;
        }
        (chars_seen == width).then_some(text.len())
    }
}

impl Preprocessor<'_> {
    pub(super) fn define(&mut self, rest: &str, span: Span, is_member: bool) -> OpyResult<()> {
        let rest = rest.trim();
        let first_open = rest.find('(').unwrap_or(rest.len());
        let first_space = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let is_function_like = first_open < first_space;

        let (name, params, body_text, macro_text) = if is_function_like {
            let name = rest[..first_open].trim();
            let Some(close) = rest[first_open..].find(')') else {
                return Err(OpyError::at(
                    "define-invalid",
                    format!("malformed function-like define `#!define {rest}`: missing `)`"),
                    span,
                ));
            };
            let close = first_open + close;
            let params: Vec<String> = rest[first_open + 1..close]
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            let body = rest[close + 1..].trim();
            (
                name.to_string(),
                params,
                body.to_string(),
                rest[..=close].trim().to_string(),
            )
        } else {
            let name = rest[..first_space].trim();
            let body = rest[first_space..].trim().to_string();
            (name.to_string(), Vec::new(), body, name.to_string())
        };
        if name.is_empty() {
            return Err(OpyError::at(
                "define-invalid",
                "malformed `#!define` directive: missing macro name",
                span,
            ));
        }
        if body_text.is_empty() {
            return Err(OpyError::at(
                "define-invalid",
                format!("malformed `#!define {rest}`: missing replacement"),
                span,
            ));
        }
        if body_text == macro_text {
            return Err(OpyError::at(
                "macro-recursion",
                format!("macro '{name}' references itself"),
                span,
            ));
        }
        if self.macro_index.contains_key(&name) {
            if !self.preprocessing.allow_macro_redeclaration {
                return Err(OpyError::at(
                    "macro-redeclaration",
                    format!("macro '{name}' is already defined"),
                    span,
                ));
            }
            self.macros.retain(|macro_def| macro_def.name != name);
            self.macro_index = self
                .macros
                .iter()
                .enumerate()
                .map(|(index, macro_def)| (macro_def.name.clone(), index))
                .collect();
            self.defines.retain(|define| define.name != name);
        }
        let script = if is_function_like && body_text.starts_with("__script__(") {
            // The OverPy script-macro ABI: the replacement is exactly
            // `__script__("path.js")`; the reference extracts the path from
            // the text between the parentheses and resolves it relative to the
            // definition file (missing files fail at compile time).
            let inner = &body_text["__script__(".len()..];
            let inner = inner.strip_suffix(')').ok_or_else(|| {
                OpyError::at(
                    "script-invalid",
                    format!(
                        "malformed script macro `#!define {rest}`: expected `__script__(\"path.js\")`"
                    ),
                    span,
                )
            })?;
            let Some(path) = strip_quoted(inner.trim()) else {
                return Err(OpyError::at(
                    "script-invalid",
                    format!(
                        "malformed script macro `#!define {rest}`: expected a quoted script path"
                    ),
                    span,
                ));
            };
            let base = self.include_base();
            Some(self.resolve_script(path, span, &base)?)
        } else {
            None
        };
        let is_function = is_function_like;
        self.defines.push(DefineRecord {
            name: name.clone(),
            is_function,
            is_member,
            span: Some(span),
        });
        self.macro_index.insert(name.clone(), self.macros.len());
        self.macros.push(MacroDef {
            name,
            params,
            body_text: body_text.clone(),
            is_function,
            is_multiline: span.end.line > span.start.line || body_text.contains('\n'),
            script,
        });
        Ok(())
    }

    pub(super) fn expand(&self, tokens: Vec<Token>) -> OpyResult<Vec<Token>> {
        let mut out: Vec<Token> = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            index = self.expand_one(&tokens, index, &mut out)?;
        }
        Ok(out)
    }

    pub(super) fn find_macro(&self, name: &str) -> Option<&MacroDef> {
        self.macro_index.get(name).map(|&index| &self.macros[index])
    }

    pub(super) fn expand_one(
        &self,
        tokens: &[Token],
        index: usize,
        output: &mut Vec<Token>,
    ) -> OpyResult<usize> {
        let token = &tokens[index];
        if token.kind != TokenKind::Ident
            || output.last().is_some_and(|previous| {
                previous.kind == TokenKind::Ident && previous.text == "macro"
            })
        {
            output.push(token.clone());
            return Ok(index + 1);
        }
        let Some(mac) = self.find_macro(&token.text) else {
            output.push(token.clone());
            return Ok(index + 1);
        };
        let (args, after) = if mac.is_function
            && tokens
                .get(index + 1)
                .is_some_and(|next| next.kind == TokenKind::LParen)
        {
            self.collect_args(tokens, index + 1)?
        } else {
            (Vec::new(), index + 1)
        };
        let mut expanded = self.expand_macro(mac, args, token, line_indent(tokens, index))?;
        self.expand_into(&mut expanded, &mut Vec::new(), 0)?;
        output.append(&mut expanded);
        Ok(after)
    }

    fn collect_args(
        &self,
        tokens: &[Token],
        open: usize,
    ) -> OpyResult<(Vec<MacroArgument>, usize)> {
        let mut args: Vec<MacroArgument> = Vec::new();
        let mut arg_start = open + 1;
        let mut depth = 0usize;
        for (cursor, token) in tokens.iter().enumerate().skip(open + 1) {
            match token.kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen if depth == 0 => {
                    if arg_start != cursor || !args.is_empty() {
                        args.push(self.finish_argument(&tokens[arg_start..cursor]));
                    }
                    return Ok((args, cursor + 1));
                }
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    depth = depth.saturating_sub(1);
                }
                TokenKind::Comma if depth == 0 => {
                    args.push(self.finish_argument(&tokens[arg_start..cursor]));
                    arg_start = cursor + 1;
                }
                _ => {}
            }
        }
        Err(OpyError::new(
            "macro-invalid",
            "unterminated macro invocation: missing closing `)`",
        ))
    }

    fn finish_argument(&self, tokens: &[Token]) -> MacroArgument {
        let raw = self.raw_source_text(tokens).map_or_else(
            || raw_arg_text(tokens).trim().to_string(),
            |text| text.trim().to_string(),
        );
        MacroArgument { raw }
    }

    fn raw_source_text(&self, tokens: &[Token]) -> Option<&str> {
        let first = tokens.first()?.span;
        let last = tokens.last()?.span;
        if first.file != last.file {
            return None;
        }
        let source = self.source_texts.get(&first.file)?;
        let mut offsets = self.line_offsets.borrow_mut();
        let offsets = offsets
            .entry(first.file)
            .or_insert_with(|| LineOffsets::new(source));
        let start = offsets.offset_of(source, first.start)?;
        let end = offsets.offset_of(source, last.end)?;
        let raw = source.get(start..end)?;
        let mut lexed = lex(LexInput {
            file_id: first.file,
            text: raw,
        })
        .ok()?;
        lexed.retain(|token| token.kind != TokenKind::Eof);
        (lexed.len() == tokens.len()
            && lexed
                .iter()
                .zip(tokens)
                .all(|(left, right)| left.kind == right.kind && left.text == right.text))
        .then_some(raw)
    }

    fn expand_macro(
        &self,
        mac: &MacroDef,
        args: Vec<MacroArgument>,
        use_site: &Token,
        line_indent: u32,
    ) -> OpyResult<Vec<Token>> {
        if mac.is_function && args.len() != mac.params.len() {
            return Err(OpyError::at(
                "macro-arity",
                format!(
                    "macro '{}' expects {} argument(s) but got {}",
                    mac.name,
                    mac.params.len(),
                    args.len()
                ),
                use_site.span,
            ));
        }
        if let Some(script) = &mac.script {
            return self.expand_script(mac, script, args, use_site, line_indent);
        }
        let mut replacement = mac.body_text.clone();
        if mac.is_function {
            for (param, argument) in mac.params.iter().zip(&args) {
                replacement = replace_identifier(&replacement, param, &argument.raw);
            }
        }
        replacement = replacement.replace("\\\n", "\n");
        if replacement.contains('\n') {
            let indent = " ".repeat(line_indent as usize);
            replacement = replacement.replace('\n', &format!("\n{indent}"));
        }
        let mut out = lex(LexInput {
            file_id: use_site.span.file,
            text: &replacement,
        })?;
        out.retain(|token| token.kind != TokenKind::Eof);
        attribute_expansion_spans(&mut out, use_site);
        Ok(out)
    }

    fn expand_into(
        &self,
        tokens: &mut Vec<Token>,
        stack: &mut Vec<String>,
        depth: usize,
    ) -> OpyResult<()> {
        if depth > 64 {
            let message =
                "macro expansion exceeded the recursion limit (possible recursive define)";
            return Err(tokens.first().map_or_else(
                || OpyError::new("macro-recursion", message),
                |token| OpyError::at("macro-recursion", message, token.span),
            ));
        }
        let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            let macro_use = (token.kind == TokenKind::Ident
                && !out.last().is_some_and(|previous| {
                    previous.kind == TokenKind::Ident && previous.text == "macro"
                }))
            .then(|| self.find_macro(&token.text))
            .flatten();
            if let Some(mac) = macro_use {
                let name = token.text.clone();
                if stack.iter().any(|s| s == &name) {
                    return Err(OpyError::at(
                        "macro-recursion",
                        format!("recursive macro expansion detected for '{name}'"),
                        token.span,
                    ));
                }
                let (args, after) = if mac.is_function
                    && tokens
                        .get(index + 1)
                        .is_some_and(|next| next.kind == TokenKind::LParen)
                {
                    self.collect_args(tokens, index + 1)?
                } else {
                    (Vec::new(), index + 1)
                };
                let mut expanded =
                    self.expand_macro(mac, args, token, line_indent(tokens, index))?;
                stack.push(name);
                self.expand_into(&mut expanded, stack, depth + 1)?;
                stack.pop();
                out.append(&mut expanded);
                index = after;
                continue;
            }
            out.push(token.clone());
            index += 1;
        }
        *tokens = out;
        Ok(())
    }
}

fn line_indent(tokens: &[Token], index: usize) -> u32 {
    let line_start = tokens[..index]
        .iter()
        .rposition(|token| token.kind == TokenKind::Newline)
        .map_or(0, |position| position + 1);
    tokens
        .get(line_start..=index)
        .and_then(|line| line.iter().find(|token| token.kind != TokenKind::Newline))
        .map_or(0, |token| token.layout.start.col.saturating_sub(1))
}

fn raw_arg_text(tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token.kind {
            TokenKind::String => {
                out.push_str(&serde_json::to_string(&token.text).expect("string serialization"));
            }
            TokenKind::Newline => out.push('\n'),
            _ => out.push_str(&token.text),
        }
    }
    out
}

pub(super) fn attribute_expansion_spans(tokens: &mut [Token], use_site: &Token) {
    for token in tokens {
        token.layout = token.span.shifted(use_site.layout.start);
        token.layout.file = use_site.span.file;
        token.span = use_site.span;
    }
}

/// Expand `#!define` text macros inside a string-interpolation fragment.
/// The reference re-tokenizes `{expr}` regions with the active define
/// table, so `f"{S}!"` observes the same `S` replacement as source-level
/// code — token-level expansion also means a define name inside a nested
/// string literal (`f"{'S'}"`) is literal text, not a macro use, so
/// quoted regions are skipped. `__script__` macros stay unexpanded: they
/// need the script runtime, which the parser does not drive.
pub(crate) fn expand_define_text(text: &str, macros: &[MacroDef]) -> Result<String, OpyError> {
    if macros.is_empty() {
        return Ok(text.to_string());
    }
    let mut out = text.to_string();
    for _ in 0..64 {
        let mut changed = false;
        for mac in macros {
            if mac.script.is_some() || !out.contains(&mac.name) {
                continue;
            }
            let next = if mac.is_function {
                expand_function_define(&out, mac)
            } else {
                replace_identifier_impl(&out, &mac.name, &mac.body_text, true)
            };
            if next != out {
                out = next;
                changed = true;
            }
        }
        if !changed {
            return Ok(out);
        }
    }
    Err(OpyError::new(
        "macro-recursion",
        "macro expansion exceeded the recursion limit (possible recursive define)",
    ))
}

fn quoted_literal_end(chars: &[char], start: usize) -> usize {
    let quote = chars[start];
    let mut index = start + 1;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 2,
            character if character == quote => return index + 1,
            _ => index += 1,
        }
    }
    chars.len()
}

fn identifier_at(source: &[char], start: usize, identifier: &[char]) -> bool {
    let end = start + identifier.len();
    source.get(start..end) == Some(identifier)
        && !start
            .checked_sub(1)
            .and_then(|previous| source.get(previous).copied())
            .is_some_and(crate::lexer::is_ident_continue)
        && !source
            .get(end)
            .copied()
            .is_some_and(crate::lexer::is_ident_continue)
}

fn expand_function_define(text: &str, mac: &MacroDef) -> String {
    let chars: Vec<char> = text.chars().collect();
    let name: Vec<char> = mac.name.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if matches!(chars[index], '"' | '\'') {
            index = quoted_literal_end(&chars, index);
            continue;
        }
        let end = index + name.len();
        if identifier_at(&chars, index, &name) {
            let mut cursor = end;
            while chars.get(cursor).is_some_and(|c| c.is_whitespace()) {
                cursor += 1;
            }
            if chars.get(cursor) != Some(&'(') {
                index = end;
                continue;
            }
            let mut depth = 0usize;
            let mut quote: Option<char> = None;
            let mut escaped = false;
            let mut close = None;
            for (position, character) in chars.iter().enumerate().skip(cursor) {
                if let Some(active) = quote {
                    if escaped {
                        escaped = false;
                    } else if *character == '\\' {
                        escaped = true;
                    } else if *character == active {
                        quote = None;
                    }
                    continue;
                }
                match character {
                    '"' | '\'' => quote = Some(*character),
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(position);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(close) = close else {
                return text.to_string();
            };
            let inner: String = chars[cursor + 1..close].iter().collect();
            let mut args = Vec::new();
            let mut depth = 0usize;
            let mut quote: Option<char> = None;
            let mut escaped = false;
            let mut start = 0;
            for (offset, character) in inner.char_indices() {
                if let Some(active) = quote {
                    if escaped {
                        escaped = false;
                    } else if character == '\\' {
                        escaped = true;
                    } else if character == active {
                        quote = None;
                    }
                    continue;
                }
                match character {
                    '"' | '\'' => quote = Some(character),
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth = depth.saturating_sub(1),
                    ',' if depth == 0 => {
                        args.push(inner[start..offset].trim().to_string());
                        start = offset + 1;
                    }
                    _ => {}
                }
            }
            args.push(inner[start..].trim().to_string());
            if args == [String::new()] {
                args.clear();
            }
            let mut replacement = mac.body_text.clone();
            for (position, param) in mac.params.iter().enumerate() {
                let argument = args.get(position).map_or("", String::as_str);
                replacement = replace_identifier(&replacement, param, argument);
            }
            let mut out = String::with_capacity(text.len() + replacement.len());
            out.extend(&chars[..index]);
            out.push_str(&replacement);
            out.extend(&chars[close + 1..]);
            return out;
        }
        index += 1;
    }
    text.to_string()
}

pub(crate) fn replace_identifier(source: &str, identifier: &str, replacement: &str) -> String {
    replace_identifier_impl(source, identifier, replacement, false)
}

fn replace_identifier_impl(
    source: &str,
    identifier: &str,
    replacement: &str,
    skip_quoted: bool,
) -> String {
    if identifier.is_empty() || !source.contains(identifier) {
        return source.to_string();
    }
    let source_chars: Vec<char> = source.chars().collect();
    let identifier_chars: Vec<char> = identifier.chars().collect();
    let mut result = String::with_capacity(source.len());
    let mut index = 0;
    while index < source_chars.len() {
        if skip_quoted && matches!(source_chars[index], '"' | '\'') {
            let end = quoted_literal_end(&source_chars, index);
            result.extend(&source_chars[index..end]);
            index = end;
        } else if identifier_at(&source_chars, index, &identifier_chars) {
            result.push_str(replacement);
            index += identifier_chars.len();
        } else {
            result.push(source_chars[index]);
            index += 1;
        }
    }
    result
}
