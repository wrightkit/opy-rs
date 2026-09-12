use super::directives::strip_quoted;
use super::*;

pub(super) fn first_main_file_directive(text: &str) -> Option<(String, Span)> {
    let line = text.lines().next()?.trim_end_matches('\r');
    let rest = line.strip_prefix("#!mainFile")?;
    let value = rest.trim();
    let value = strip_quoted(value)?.to_string();
    let end_col = line.chars().count() as u32 + 1;
    Some((
        value,
        Span::new(
            0,
            crate::diag::Position::new(1, 1),
            crate::diag::Position::new(1, end_col),
        ),
    ))
}

pub(super) fn display_path(
    candidate: &Path,
    canonical: Option<&Path>,
    root: &Path,
    fallback: &str,
) -> String {
    let path = canonical.unwrap_or(candidate);
    let Some(relative) = path.strip_prefix(root).ok() else {
        return path.to_string_lossy().replace('\\', "/");
    };
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                components.push("..".to_string());
            }
            std::path::Component::Normal(component) => {
                components.push(component.to_string_lossy().into_owned());
            }
            _ => {}
        }
    }
    if components.is_empty() {
        fallback.to_string()
    } else {
        components.join("/")
    }
}

impl Preprocessor {
    pub(super) fn include(
        &mut self,
        include: &str,
        span: Span,
        out: &mut Vec<Token>,
    ) -> OpyResult<()> {
        // Includes resolve relative to the source file containing the
        // directive. The main source uses the project root as its base.
        let include = include.replace('\\', "/");
        let candidate = self.include_base().join(&include);
        let canonical = std::fs::canonicalize(&candidate).ok();
        if canonical.as_deref().is_some_and(Path::is_dir) {
            let mut files = std::fs::read_dir(&candidate)
                .map_err(|error| {
                    OpyError::at(
                        "include-not-found",
                        format!("cannot read included directory '{include}': {error}"),
                        span,
                    )
                })?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("opy"))
                        && path.is_file()
                })
                .collect::<Vec<_>>();
            files.sort();
            if files.is_empty() {
                return Err(OpyError::at(
                    "include-not-found",
                    format!("included directory '{include}' has no .opy files"),
                    span,
                ));
            }
            for file in files {
                self.include_file(&file, &include, span, out)?;
            }
        } else {
            self.include_file(&candidate, &include, span, out)?;
        }
        self.record("include", Some(&include), span);
        Ok(())
    }

    fn include_file(
        &mut self,
        candidate: &Path,
        requested: &str,
        span: Span,
        out: &mut Vec<Token>,
    ) -> OpyResult<()> {
        let canonical = std::fs::canonicalize(candidate).ok();
        let candidate_path = candidate.to_string_lossy().into_owned();
        let canonical_path = display_path(
            candidate,
            canonical.as_deref(),
            &self.display_root,
            requested,
        );
        let lexical_path = display_path(candidate, None, &self.display_root, requested);
        let overlay_text = self
            .overlay
            .get(requested)
            .or_else(|| self.overlay.get(&candidate_path))
            .or_else(|| self.overlay.get(&lexical_path))
            .or_else(|| self.overlay.get(&canonical_path))
            .or_else(|| {
                canonical
                    .as_ref()
                    .and_then(|path| self.overlay.get(&path.to_string_lossy().into_owned()))
            })
            .cloned();
        let uses_overlay = overlay_text.is_some();
        let identity = canonical.clone().unwrap_or_else(|| candidate.to_path_buf());
        if self.include_stack.contains(&identity) {
            return Err(OpyError::at(
                "include-cycle",
                format!(
                    "include cycle detected: '{}' is already being included",
                    identity.display()
                ),
                span,
            ));
        }
        let import_identity = candidate.to_path_buf();
        if self.imported_files.contains(&import_identity) {
            self.warnings.push(PreprocessWarning {
                code: "w_already_imported".to_string(),
                message: format!(
                    "The file '{}' was already imported and will not be imported again.",
                    import_identity.display()
                ),
                span,
            });
            return Ok(());
        }
        self.imported_files.insert(import_identity);

        let text = match overlay_text {
            Some(text) => text,
            None => {
                let canonical = canonical.ok_or_else(|| {
                    OpyError::at(
                        "include-not-found",
                        format!(
                            "cannot find included file '{requested}' under root '{}'",
                            self.root.display()
                        ),
                        span,
                    )
                })?;
                std::fs::read_to_string(&canonical).map_err(|error| {
                    OpyError::at(
                        "include-not-found",
                        format!("cannot read included file '{requested}': {error}"),
                        span,
                    )
                })?
            }
        };
        let file_id = self.next_file_id;
        self.next_file_id += 1;
        self.files.push(FileRecord {
            id: file_id,
            path: if uses_overlay {
                lexical_path
            } else {
                canonical_path
            },
        });
        self.include_stack.push(identity.clone());
        let saved_prefix = self.preprocessing.rule_prefix.clone();
        let saved_optimization = self.preprocessing.optimization.clone();
        let mut leaves_macro_file_context = false;
        let result = (|| {
            let settings = match crate::settings::find_blocks(&text, file_id) {
                Err(error) => return Err(error),
                Ok(mut blocks) => blocks.pop(),
            };
            let owns_settings = settings.is_some();
            if let Some(block) = settings {
                if self.settings.is_some() {
                    return Err(OpyError::at(
                        "settings-placement",
                        "only one settings block is supported in a project".to_string(),
                        block.keyword_span,
                    ));
                }
                self.settings = Some(block);
            }
            let sanitized = self
                .settings
                .as_ref()
                .filter(|block| block.span.file == file_id)
                .map(|block| crate::settings::sanitize_for_lex(&text, block));
            let mut included = lex(LexInput {
                file_id,
                text: sanitized.as_deref().unwrap_or(&text),
            })?;
            let allow_leading_main_file = text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .is_some_and(|line| line.starts_with("#!mainFile"));
            self.process_directives(&mut included, allow_leading_main_file)?;
            leaves_macro_file_context = owns_settings && self.contains_macro_use(&text, file_id);
            included.retain(|token| token.kind != TokenKind::Eof);
            Ok(included)
        })();
        self.preprocessing.rule_prefix = saved_prefix;
        self.preprocessing.optimization = saved_optimization;
        self.include_stack.pop();
        let included = result?;
        if leaves_macro_file_context {
            self.last_macro_include_path = Some(identity);
        }
        out.extend(included);
        Ok(())
    }

    fn contains_macro_use(&self, text: &str, file_id: u32) -> bool {
        let mut source_without_macro_declarations = String::new();
        let mut in_macro_declaration = false;
        for line in text.lines() {
            let trimmed = line.trim_start();
            if in_macro_declaration {
                if trimmed.is_empty() || line.starts_with(char::is_whitespace) {
                    continue;
                }
                in_macro_declaration = false;
            }
            if trimmed.starts_with("macro ") {
                in_macro_declaration = true;
                continue;
            }
            source_without_macro_declarations.push_str(line);
            source_without_macro_declarations.push('\n');
        }
        let Ok(tokens) = lex(LexInput {
            file_id,
            text: &source_without_macro_declarations,
        }) else {
            return false;
        };
        tokens.iter().enumerate().any(|(index, token)| {
            if token.kind != TokenKind::Ident
                || tokens.get(index.wrapping_sub(1)).is_some_and(|previous| {
                    previous.kind == TokenKind::Ident && previous.text == "macro"
                })
            {
                return false;
            }
            let Some(mac) = self.macros.iter().find(|mac| mac.name == token.text) else {
                return false;
            };
            mac.is_multiline
                && (!mac.is_function
                    || tokens
                        .get(index + 1)
                        .is_some_and(|next| next.kind == TokenKind::LParen))
        })
    }

    pub(super) fn include_base(&self) -> PathBuf {
        self.include_stack
            .last()
            .or(self.last_macro_include_path.as_ref())
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone())
    }
}
