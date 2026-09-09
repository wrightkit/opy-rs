use super::*;

impl Preprocessor {
    pub(super) fn process_directives(
        &mut self,
        tokens: &mut Vec<Token>,
        allow_leading_main_file: bool,
    ) -> OpyResult<()> {
        let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
        for token in tokens.drain(..) {
            if token.kind == TokenKind::Directive {
                let is_leading_main_file = allow_leading_main_file && token.span.start.line == 1;
                self.handle_directive(token, &mut out, is_leading_main_file)?;
            } else if token.kind == TokenKind::Ident
                && matches!(token.text.as_str(), "rule" | "def")
                && self.preprocessing.rule_prefix.is_some()
            {
                let prefix = self
                    .preprocessing
                    .rule_prefix
                    .as_ref()
                    .map(|value| value.value.clone())
                    .unwrap_or_default();
                out.push(Token {
                    kind: TokenKind::RulePrefixMarker,
                    text: prefix,
                    raw: None,
                    span: token.span,
                });
                out.push(token);
            } else {
                out.push(token);
            }
        }
        *tokens = out;
        Ok(())
    }

    fn handle_directive(
        &mut self,
        token: Token,
        out: &mut Vec<Token>,
        allow_leading_main_file: bool,
    ) -> OpyResult<()> {
        let text = token.text.trim();
        let span = token.span;
        let (name, rest) = split_directive(text);
        if name == "include" {
            let rest = rest.trim();
            let include = rest
                .strip_prefix('"')
                .and_then(|r| r.strip_suffix('"'))
                .or_else(|| rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')));
            let Some(include) = include else {
                return Err(OpyError::at(
                    "include-invalid",
                    format!(
                        "invalid include directive: `{text}` (expected `#!include \"file.opy\"`)"
                    ),
                    span,
                ));
            };
            self.include(include, span, out)?;
            return Ok(());
        }
        if matches!(name, "define" | "defineMember") {
            self.define(rest.trim(), span, name == "defineMember")?;
            return Ok(());
        }
        if name == "undef" {
            let name = rest.trim();
            if name.is_empty() || name.chars().any(|ch| !is_identifier_char(ch)) {
                return Err(OpyError::at(
                    "undef-invalid",
                    "malformed `#!undef` directive: expected one macro name",
                    span,
                ));
            }
            self.macros.retain(|m| m.name != name);
            self.defines.retain(|define| define.name != name);
            self.record("undef", Some(name), span);
            return Ok(());
        }
        if name == "postCompileHook" {
            let rest = rest.trim();
            let Some(path) = strip_quoted(rest) else {
                return Err(OpyError::at(
                    "script-invalid",
                    format!(
                        "invalid postCompileHook directive: `{text}` (expected `#!postCompileHook \"hook.js\"`)"
                    ),
                    span,
                ));
            };
            if self.post_compile_hook.is_some() {
                return Err(OpyError::at(
                    "post-compile-hook-duplicate",
                    "post-compile hook is already defined".to_string(),
                    span,
                ));
            }
            let hook = self.resolve_script(path, span, &self.root)?;
            self.post_compile_hook = Some(PostCompileHook {
                path: hook.path,
                source: hook.source,
                span,
            });
            self.record("postCompileHook", Some(path), span);
            return Ok(());
        }
        if matches!(name, "setupTags" | "setupTx") {
            require_no_arguments(name, rest, span)?;
            self.record(name, None, span);
            return Ok(());
        }
        if name == "mainFile" {
            if allow_leading_main_file {
                let main_file = strip_quoted(rest.trim())
                    .filter(|main_file| !main_file.is_empty())
                    .ok_or_else(|| {
                        OpyError::at(
                            "main-file-invalid",
                            "`#!mainFile` expects one quoted path",
                            span,
                        )
                    })?;
                self.record(name, Some(main_file), span);
                return Ok(());
            }
            return Err(OpyError::at(
                "main-file-placement",
                "`#!mainFile` must be the first directive in the main source",
                span,
            ));
        }
        if name == "allowMacroRedeclaration" {
            self.preprocessing.allow_macro_redeclaration = true;
            self.record(name, None, span);
            return Ok(());
        }
        if name == "excludeVariablesInCompilation" {
            require_no_arguments(name, rest, span)?;
            self.record(name, None, span);
            return Ok(());
        }
        if name == "extension" {
            let extension = parse_single_word(rest, name, span)?;
            validate_extension_name(extension, span)?;
            self.record(name, Some(extension), span);
            return Ok(());
        }
        if name == "translateWithPlayerVar" {
            let options = rest.split_whitespace().collect::<Vec<_>>();
            if options
                .iter()
                .any(|option| !matches!(*option, "noDetectionRule" | "noTlErr"))
            {
                return Err(OpyError::at(
                    "directive-invalid",
                    "`#!translateWithPlayerVar` accepts only `noDetectionRule` and `noTlErr`",
                    span,
                ));
            }
            let value = (!options.is_empty()).then(|| options.join(" "));
            self.record(name, value.as_deref(), span);
            return Ok(());
        }
        if matches!(
            name,
            "disableInspector"
                | "writeToOutputFile"
                | "disableTranslationSourceLines"
                | "keepUnusedTranslations"
                | "useVariableForCompressionAlphabet"
                | "debugElementCount"
        ) {
            require_no_arguments(name, rest, span)?;
            self.record(name, None, span);
            return Ok(());
        }
        if matches!(name, "globalvarInitRuleName" | "playervarInitRuleName") {
            let value = strip_quoted(rest.trim()).ok_or_else(|| {
                OpyError::at(
                    "directive-invalid",
                    format!("`#!{name}` expects one quoted string"),
                    span,
                )
            })?;
            self.record(name, Some(value), span);
            return Ok(());
        }
        if name == "translations" {
            let languages = parse_translations(rest.trim(), span)?;
            self.preprocessing.translations = Some(TranslationState {
                languages: languages.clone(),
                span: Some(span.into()),
            });
            self.record(name, Some(&languages.join(" ")), span);
            return Ok(());
        }
        if name == "suppressWarnings" {
            let warnings = parse_words(rest, "suppressWarnings", span)?;
            self.preprocessing
                .suppressed_warnings
                .extend(warnings.clone());
            self.record(name, Some(&warnings.join(" ")), span);
            return Ok(());
        }
        if name == "rulePrefix" {
            let prefix = strip_quoted(rest.trim()).ok_or_else(|| {
                OpyError::at(
                    "rule-prefix-invalid",
                    "`#!rulePrefix` expects one quoted string",
                    span,
                )
            })?;
            self.preprocessing.rule_prefix = Some(DirectiveValue {
                value: prefix.to_string(),
                span: Some(span.into()),
            });
            self.record(name, Some(prefix), span);
            return Ok(());
        }
        if name == "rulePrefixTemplate" {
            if self.preprocessing.rule_prefix_template.is_some() {
                return Err(OpyError::at(
                    "rule-prefix-template-duplicate",
                    "a rule prefix template is already defined",
                    span,
                ));
            }
            let template = if rest.trim().is_empty() {
                r#"f"[{$pathTitle.replace('_', ' ')}] {$rule}" if $rule and not $isDelimiter else $rule"#
            } else {
                rest.trim()
            };
            self.preprocessing.rule_prefix_template = Some(DirectiveValue {
                value: template.to_string(),
                span: Some(span.into()),
            });
            self.record(name, Some(template), span);
            return Ok(());
        }
        if let Some((directive, control)) = optimization_directive(name) {
            apply_optimization(&mut self.preprocessing.optimization, control);
            self.record(directive, None, span);
            return Ok(());
        }
        if let Some(replacement) = replacement_directive(name) {
            let family = replacement_family(name).expect("replacement directive family");
            if self
                .preprocessing
                .directives
                .iter()
                .filter_map(|item| replacement_family(&item.name))
                .any(|item_family| item_family == family)
            {
                return Err(OpyError::at(
                    "replacement-duplicate",
                    format!("a replacement for `{family}` is already defined"),
                    span,
                ));
            }
            self.preprocessing.replacements.push(DirectiveValue {
                value: replacement.to_string(),
                span: Some(span.into()),
            });
            self.record(name, Some(replacement), span);
            return Ok(());
        }
        Err(OpyError::at(
            "unsupported-directive",
            format!("unsupported preprocessing directive `#!{text}`"),
            span,
        ))
    }

    pub(super) fn record(&mut self, name: &str, value: Option<&str>, span: Span) {
        let state = PreprocessingSnapshot {
            allow_macro_redeclaration: self.preprocessing.allow_macro_redeclaration,
            optimization: self.preprocessing.optimization.clone(),
            rule_prefix: self
                .preprocessing
                .rule_prefix
                .as_ref()
                .map(|value| value.value.clone()),
            rule_prefix_template: self
                .preprocessing
                .rule_prefix_template
                .as_ref()
                .map(|value| value.value.clone()),
            translations: self
                .preprocessing
                .translations
                .as_ref()
                .map(|translations| translations.languages.clone()),
            replacements: self
                .preprocessing
                .replacements
                .iter()
                .map(|value| value.value.clone())
                .collect(),
        };
        self.preprocessing.directives.push(DirectiveRecord {
            name: name.to_string(),
            value: value.map(str::to_string),
            scope_col: span.start.col,
            scope_depth: self.include_stack.len() as u32,
            state,
            span: Some(span.into()),
        });
    }
}

fn split_directive(text: &str) -> (&str, &str) {
    text.split_once(char::is_whitespace)
        .map_or((text, ""), |(name, rest)| (name, rest))
}

fn require_no_arguments(name: &str, rest: &str, span: Span) -> OpyResult<()> {
    if rest.trim().is_empty() {
        Ok(())
    } else {
        Err(OpyError::at(
            "directive-invalid",
            format!("`#!{name}` does not accept arguments"),
            span,
        ))
    }
}

fn parse_single_word<'a>(rest: &'a str, name: &str, span: Span) -> OpyResult<&'a str> {
    let value = rest.trim();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err(OpyError::at(
            "directive-invalid",
            format!("`#!{name}` expects one argument"),
            span,
        ));
    }
    Ok(value)
}

fn validate_extension_name(extension: &str, span: Span) -> OpyResult<()> {
    let path = [
        workshop_rs::settings::table::PathPart::Part("extensions"),
        workshop_rs::settings::table::PathPart::Part(extension),
    ];
    if workshop_rs::settings::definition(&path).is_some() {
        Ok(())
    } else {
        Err(OpyError::at(
            "directive-invalid",
            format!("unknown Workshop extension `{extension}`"),
            span,
        ))
    }
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn parse_words(rest: &str, directive: &str, span: Span) -> OpyResult<Vec<String>> {
    let words: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Err(OpyError::at(
            "directive-invalid",
            format!("`#!{directive}` expects at least one argument"),
            span,
        ));
    }
    if words
        .iter()
        .any(|word| word.chars().any(|ch| !is_identifier_char(ch)))
    {
        return Err(OpyError::at(
            "directive-invalid",
            format!("`#!{directive}` arguments must be identifiers"),
            span,
        ));
    }
    Ok(words)
}

fn parse_translations(rest: &str, span: Span) -> OpyResult<Vec<String>> {
    let values: Vec<String> = rest
        .split_whitespace()
        .map(|language| language.replace('-', "_").to_lowercase())
        .collect();
    if values.is_empty() {
        return Err(OpyError::at(
            "translations-invalid",
            "`#!translations` expects at least one language",
            span,
        ));
    }
    const PINNED_LANGUAGES: &[&str] = &[
        "de", "en", "es", "es_es", "es_mx", "fr", "it", "ja", "ko", "pl", "pt", "ru", "th", "tr",
        "zh", "zh_cn", "zh_tw",
    ];
    if values
        .iter()
        .any(|language| !PINNED_LANGUAGES.contains(&language.as_str()))
    {
        return Err(OpyError::at(
            "translations-invalid",
            "invalid translation language; expected one of the pinned OverPy language codes",
            span,
        ));
    }
    if values.iter().any(|value| value == "es")
        && values
            .iter()
            .any(|value| value == "es_es" || value == "es_mx")
    {
        return Err(OpyError::at(
            "translations-invalid",
            "cannot combine `es` with `es_es` or `es_mx`",
            span,
        ));
    }
    if values.iter().any(|value| value == "zh")
        && values
            .iter()
            .any(|value| value == "zh_cn" || value == "zh_tw")
    {
        return Err(OpyError::at(
            "translations-invalid",
            "cannot combine `zh` with `zh_cn` or `zh_tw`",
            span,
        ));
    }
    Ok(values)
}

#[derive(Clone, Copy)]
enum OptimizationControl {
    Enable,
    Disable,
    ForSize,
    DisableForSize,
    ForSizeAggressive,
    Strict,
    DisableStrict,
}

fn optimization_directive(name: &str) -> Option<(&str, OptimizationControl)> {
    Some(match name {
        "disableOptimizations" => (name, OptimizationControl::Disable),
        "enableOptimizations" => (name, OptimizationControl::Enable),
        "optimizeForSize" => (name, OptimizationControl::ForSize),
        "disableOptimizeForSize" => (name, OptimizationControl::DisableForSize),
        "optimizeForSizeAggressive" => (name, OptimizationControl::ForSizeAggressive),
        "optimizeStrict" => (name, OptimizationControl::Strict),
        "disableOptimizeStrict" => (name, OptimizationControl::DisableStrict),
        _ => return None,
    })
}

fn apply_optimization(state: &mut OptimizationState, control: OptimizationControl) {
    match control {
        OptimizationControl::Enable => state.enabled = true,
        OptimizationControl::Disable => state.enabled = false,
        OptimizationControl::ForSize => state.for_size = true,
        OptimizationControl::DisableForSize => state.for_size = false,
        OptimizationControl::ForSizeAggressive => state.for_size_aggressive = true,
        OptimizationControl::Strict => state.strict = true,
        OptimizationControl::DisableStrict => state.strict = false,
    }
}

fn replacement_directive(name: &str) -> Option<&str> {
    Some(match name {
        "replace0ByCapturePercentage" => "getCapturePercentage",
        "replace0ByPayloadProgressPercentage" => "getPayloadProgressPercentage",
        "replace0ByIsMatchComplete" => "isMatchComplete",
        "replace1ByMatchRound" => "getMatchRound",
        "replaceTeam1ByControlScoringTeam" => "getControlScoringTeam",
        "replaceEmptyStringByEmptyArray" => "emptyArray",
        "replaceEmptyStringByVariable" => "variable",
        _ => return None,
    })
}

fn replacement_family(name: &str) -> Option<&str> {
    Some(match name {
        "replace0ByCapturePercentage"
        | "replace0ByPayloadProgressPercentage"
        | "replace0ByIsMatchComplete" => "0",
        "replace1ByMatchRound" => "1",
        "replaceTeam1ByControlScoringTeam" => "team1",
        "replaceEmptyStringByEmptyArray" | "replaceEmptyStringByVariable" => "emptyString",
        _ => return None,
    })
}

pub(super) fn strip_quoted(text: &str) -> Option<&str> {
    text.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            text.strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
}
