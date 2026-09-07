//! Registration and recursive expansion of textual OPY macros.

use super::directives::strip_quoted;
use super::*;

/// A registered macro: object-like, function-like, or a script macro.
pub(super) struct MacroDef {
    pub(super) name: String,
    pub(super) params: Vec<String>,
    pub(super) body: Vec<Token>,
    /// True when the body came from a `#!define name(args) value` form.
    pub(super) is_function: bool,
    /// The resolved `__script__` backing, when the replacement is one.
    pub(super) script: Option<ScriptMacro>,
}

impl Preprocessor {
    pub(super) fn define(&mut self, rest: &str, span: Span, is_member: bool) -> OpyResult<()> {
        let rest = rest.trim();
        let first_open = rest.find('(').unwrap_or(rest.len());
        let first_space = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let is_function_like = first_open < first_space;

        let (name, params, body_text) = if is_function_like {
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
            (name.to_string(), params, body.to_string())
        } else {
            let name = rest[..first_space].trim();
            let body = rest[first_space..].trim().to_string();
            (name.to_string(), Vec::new(), body)
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
        let body_tokens = lex(LexInput {
            file_id: span.file,
            text: &body_text,
        })?;
        // Drop the trailing EOF token from the value.
        let body_tokens: Vec<Token> = body_tokens
            .into_iter()
            .filter(|t| t.kind != TokenKind::Eof)
            .collect();
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
            body: body_tokens,
            is_function,
            script,
        });
        Ok(())
    }

    /// Expand all macros across the token stream, recursively.
    pub(super) fn expand(&self, tokens: Vec<Token>) -> OpyResult<Vec<Token>> {
        let mut out: Vec<Token> = Vec::new();
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
                    if mac.is_function {
                        // Expect `(` args `)` immediately after the name.
                        let cursor = index + 1;
                        if cursor < tokens.len() && tokens[cursor].kind == TokenKind::LParen {
                            let (args, after) = self.collect_args(&tokens, cursor)?;
                            let mut expanded = self.expand_macro(mac, args, token.span)?;
                            self.expand_into(&mut expanded, &mut Vec::new(), 0)?;
                            out.append(&mut expanded);
                            index = after;
                            continue;
                        }
                        // A function-like macro used without arguments: leave
                        // the name as an ordinary identifier.
                        out.push(token.clone());
                        index += 1;
                        continue;
                    }
                    let mut expanded = self.expand_macro(mac, Vec::new(), token.span)?;
                    self.expand_into(&mut expanded, &mut Vec::new(), 0)?;
                    out.append(&mut expanded);
                    index += 1;
                    continue;
                }
            }
            out.push(token.clone());
            index += 1;
        }
        Ok(out)
    }

    /// Collect the argument token lists of a function-like macro call,
    /// returning `(args, index_after_closing_paren)`.
    fn collect_args(&self, tokens: &[Token], open: usize) -> OpyResult<(Vec<Vec<Token>>, usize)> {
        let mut args: Vec<Vec<Token>> = Vec::new();
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
                        args.push(std::mem::take(&mut current));
                    }
                    return Ok((args, cursor + 1));
                }
                depth -= 1;
                current.push(tokens[cursor].clone());
            } else if matches!(kind, TokenKind::RBracket | TokenKind::RBrace) {
                depth = depth.saturating_sub(1);
                current.push(tokens[cursor].clone());
            } else if kind == TokenKind::Comma && depth == 0 {
                args.push(std::mem::take(&mut current));
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

    /// Substitute macro params with the call arguments and stamp every
    /// expanded token with the use-site span.
    ///
    /// Expanded tokens share the use-site span: the differential suite
    /// normalizes spans away, and stamping the whole expansion with one
    /// monotonic span keeps downstream span validation trivially valid.
    fn expand_macro(
        &self,
        mac: &MacroDef,
        args: Vec<Vec<Token>>,
        use_site: Span,
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
            return self.expand_script(mac, script, args, use_site);
        }
        let mut out = Vec::new();
        for token in &mac.body {
            if mac.is_function
                && token.kind == TokenKind::Ident
                && mac.params.iter().any(|p| p == &token.text)
            {
                let param_index = mac
                    .params
                    .iter()
                    .position(|p| p == &token.text)
                    .expect("checked above");
                let mut replacement = args.get(param_index).cloned().unwrap_or_default();
                for replacement_token in &mut replacement {
                    replacement_token.span = use_site;
                }
                out.extend(replacement);
            } else {
                let mut token = token.clone();
                token.span = use_site;
                out.push(token);
            }
        }
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
                            let mut expanded = self.expand_macro(mac, args, token.span)?;
                            stack.push(name.clone());
                            self.expand_into(&mut expanded, stack, depth + 1)?;
                            stack.pop();
                            out.append(&mut expanded);
                            index = after;
                            continue;
                        }
                        out.push(token.clone());
                        index += 1;
                        continue;
                    }
                    let mut expanded = self.expand_macro(mac, Vec::new(), token.span)?;
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
