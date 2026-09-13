use super::directives::strip_quoted;
use super::*;

pub(super) struct MacroDef {
    pub(super) name: String,
    pub(super) params: Vec<String>,
    pub(super) body_text: String,
    /// True when the body came from a `#!define name(args) value` form.
    pub(super) is_function: bool,
    /// True when the replacement spans multiple source lines.
    pub(super) is_multiline: bool,
    /// The resolved `__script__` backing, when the replacement is one.
    pub(super) script: Option<ScriptMacro>,
}

pub(super) struct MacroArgument {
    pub(super) raw: String,
}

impl Preprocessor {
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
        if self.macros.iter().any(|macro_def| macro_def.name == name) {
            if !self.preprocessing.allow_macro_redeclaration {
                return Err(OpyError::at(
                    "macro-redeclaration",
                    format!("macro '{name}' is already defined"),
                    span,
                ));
            }
            self.macros.retain(|macro_def| macro_def.name != name);
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
            let (expanded, after) = self.expand_one(&tokens, index, &out)?;
            out.extend(expanded);
            index = after;
        }
        Ok(out)
    }

    pub(super) fn expand_one(
        &self,
        tokens: &[Token],
        index: usize,
        output: &[Token],
    ) -> OpyResult<(Vec<Token>, usize)> {
        let token = &tokens[index];
        if token.kind != TokenKind::Ident
            || output.last().is_some_and(|previous| {
                previous.kind == TokenKind::Ident && previous.text == "macro"
            })
        {
            return Ok((vec![token.clone()], index + 1));
        }
        let Some(mac) = self.macros.iter().find(|mac| mac.name == token.text) else {
            return Ok((vec![token.clone()], index + 1));
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
        let mut expanded = self.expand_macro(mac, args, token.span, line_indent(tokens, index))?;
        self.expand_into(&mut expanded, &mut Vec::new(), 0)?;
        Ok((expanded, after))
    }

    fn collect_args(
        &self,
        tokens: &[Token],
        open: usize,
    ) -> OpyResult<(Vec<MacroArgument>, usize)> {
        let mut args: Vec<MacroArgument> = Vec::new();
        let mut current: Vec<Token> = Vec::new();
        let mut depth = 0usize;
        let mut cursor = open + 1;
        while cursor < tokens.len() {
            let kind = tokens[cursor].kind;
            if matches!(
                kind,
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace
            ) {
                depth += 1;
                current.push(tokens[cursor].clone());
            } else if kind == TokenKind::RParen {
                if depth == 0 {
                    if !current.is_empty() || !args.is_empty() {
                        args.push(self.finish_argument(std::mem::take(&mut current)));
                    }
                    return Ok((args, cursor + 1));
                }
                depth -= 1;
                current.push(tokens[cursor].clone());
            } else if matches!(kind, TokenKind::RBracket | TokenKind::RBrace) {
                depth = depth.saturating_sub(1);
                current.push(tokens[cursor].clone());
            } else if kind == TokenKind::Comma && depth == 0 {
                args.push(self.finish_argument(std::mem::take(&mut current)));
            } else {
                current.push(tokens[cursor].clone());
            }
            cursor += 1;
        }
        Err(OpyError::new(
            "macro-invalid",
            "unterminated macro invocation: missing closing `)`",
        ))
    }

    fn finish_argument(&self, tokens: Vec<Token>) -> MacroArgument {
        let raw = self
            .raw_source_text(&tokens)
            .unwrap_or_else(|| raw_arg_text(&tokens));
        MacroArgument {
            raw: raw.trim().to_string(),
        }
    }

    fn raw_source_text(&self, tokens: &[Token]) -> Option<String> {
        let first = tokens.first()?.span;
        let last = tokens.last()?.span;
        if first.file != last.file {
            return None;
        }
        let source = self.source_texts.get(&first.file)?;
        let start = position_offset(source, first.start)?;
        let end = position_offset(source, last.end)?;
        let raw = source.get(start..end)?.to_string();
        let lexed = lex(LexInput {
            file_id: first.file,
            text: &raw,
        })
        .ok()?;
        let lexed: Vec<Token> = lexed
            .into_iter()
            .filter(|token| token.kind != TokenKind::Eof)
            .collect();
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
        use_site: Span,
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
                use_site,
            ));
        }
        if let Some(script) = &mac.script {
            return self.expand_script(mac, script, args, use_site, line_indent);
        }
        let mut replacement = mac.body_text.clone();
        if mac.is_function {
            for (index, param) in mac.params.iter().enumerate() {
                let argument = args
                    .get(index)
                    .map_or_else(String::new, |argument| argument.raw.clone());
                replacement = replace_identifier(&replacement, param, &argument);
            }
        }
        replacement = replacement.replace("\\\n", "\n");
        if replacement.contains('\n') {
            let indent = " ".repeat(line_indent as usize);
            replacement = replacement.replace('\n', &format!("\n{indent}"));
        }
        let mut out = lex(LexInput {
            file_id: use_site.file,
            text: &replacement,
        })?;
        out.retain(|token| token.kind != TokenKind::Eof);
        shift_expansion_spans(&mut out, use_site);
        Ok(out)
    }

    /// Expand a script macro: run the resolved script through the bounded
    /// runtime with the call-site arguments injected, then lex the string
    /// completion value back into the token stream at the use site.
    ///
    /// Argument text is reconstructed from the call-site tokens (see
    /// [`raw_arg_text`]); the reference injects the raw source text, and the
    /// reconstruction is JavaScript-value-equivalent to it (string literals
    /// are re-quoted with JSON escaping, so quoting-style differences are
    /// unobservable to the script). The reference's per-line indentation rule
    /// is applied to the expansion text before lexing; the frontend parser
    /// never consumes indentation, so this is preserved in the text only.
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
            if token.kind == TokenKind::Ident
                && !out.last().is_some_and(|previous| {
                    previous.kind == TokenKind::Ident && previous.text == "macro"
                })
            {
                let name = token.text.clone();
                if let Some(mac) = self.macros.iter().find(|m| m.name == name) {
                    if stack.iter().any(|s| s == &name) {
                        return Err(OpyError::at(
                            "macro-recursion",
                            format!("recursive macro expansion detected for '{name}'"),
                            token.span,
                        ));
                    }
                    if mac.is_function {
                        if index + 1 < tokens.len() && tokens[index + 1].kind == TokenKind::LParen {
                            let (args, after) = self.collect_args(tokens, index + 1)?;
                            let mut expanded = self.expand_macro(
                                mac,
                                args,
                                token.span,
                                line_indent(tokens, index),
                            )?;
                            stack.push(name.clone());
                            self.expand_into(&mut expanded, stack, depth + 1)?;
                            stack.pop();
                            out.append(&mut expanded);
                            index = after;
                            continue;
                        }
                        let mut expanded = self.expand_macro(
                            mac,
                            Vec::new(),
                            token.span,
                            line_indent(tokens, index),
                        )?;
                        stack.push(name.clone());
                        self.expand_into(&mut expanded, stack, depth + 1)?;
                        stack.pop();
                        out.append(&mut expanded);
                        index += 1;
                        continue;
                    }
                    let mut expanded =
                        self.expand_macro(mac, Vec::new(), token.span, line_indent(tokens, index))?;
                    stack.push(name.clone());
                    self.expand_into(&mut expanded, stack, depth + 1)?;
                    stack.pop();
                    out.append(&mut expanded);
                    index += 1;
                    continue;
                }
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
        .map_or(0, |token| token.span.start.col.saturating_sub(1))
}

fn position_offset(source: &str, position: crate::diag::Position) -> Option<usize> {
    let mut line = 1;
    let mut col = 1;
    if position.line == line && position.col == col {
        return Some(0);
    }
    for (offset, character) in source.char_indices() {
        if character == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
        if position.line == line && position.col == col {
            return Some(offset + character.len_utf8());
        }
    }
    (position.line == line && position.col == col).then_some(source.len())
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

pub(super) fn shift_expansion_spans(tokens: &mut [Token], origin: Span) {
    fn shift(
        position: crate::diag::Position,
        origin: crate::diag::Position,
    ) -> crate::diag::Position {
        crate::diag::Position::new(
            origin.line + position.line.saturating_sub(1),
            if position.line == 1 {
                origin.col + position.col.saturating_sub(1)
            } else {
                position.col
            },
        )
    }
    for token in tokens {
        token.span = Span::new(
            origin.file,
            shift(token.span.start, origin.start),
            shift(token.span.end, origin.start),
        );
    }
}

fn replace_identifier(source: &str, identifier: &str, replacement: &str) -> String {
    fn is_word(character: Option<char>) -> bool {
        character.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
    }

    let source_chars: Vec<char> = source.chars().collect();
    let identifier_chars: Vec<char> = identifier.chars().collect();
    if identifier_chars.is_empty() {
        return source.to_string();
    }
    let mut result = String::with_capacity(source.len());
    let mut index = 0;
    while index < source_chars.len() {
        let end = index + identifier_chars.len();
        if end <= source_chars.len()
            && source_chars[index..end] == identifier_chars
            && !is_word(
                index
                    .checked_sub(1)
                    .and_then(|position| source_chars.get(position).copied()),
            )
            && !is_word(source_chars.get(end).copied())
        {
            result.push_str(replacement);
            index = end;
        } else {
            result.push(source_chars[index]);
            index += 1;
        }
    }
    result
}
