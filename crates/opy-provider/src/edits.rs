use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use opy_rs::diag::Position;
use opy_rs::lexer::{self, TokenKind, is_ident_continue, is_ident_start, is_identifier};
use opy_rs::lower::{NameNamespace, is_reserved_name};
use opy_rs::tooling::{SourceLocation, Symbol, SymbolKind};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Document, HandlerError, document_path, effective_entry, filesystem_path, path_string,
    path_to_file_uri, resolved_path, same_path, validate_documents,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnosticClass {
    DocumentSyntax,
    MissingSource,
    NameBinding,
    MergedTable,
    MacroTable,
    Script,
    ExternalFile,
    Catalog,
    Capability,
    Semantic,
    Internal,
    WarningOnly,
}

const CODE_CLASSES: &[(DiagnosticClass, &[&str])] = &[
    (
        DiagnosticClass::DocumentSyntax,
        &[
            "lex-error",
            "parse-error",
            "workshop-source",
            "directive-invalid",
            "unsupported-directive",
            "include-invalid",
            "define-invalid",
            "main-file-invalid",
            "main-file-placement",
            "rule-prefix-invalid",
            "settings-invalid",
            "settings-placement",
            "macro-invalid",
            "do-while-placement",
            "invalid-string-entity",
            "unknown-string-entity",
            "script-invalid",
            "enum-increment",
        ],
    ),
    (
        DiagnosticClass::MissingSource,
        &[
            "include-not-found",
            "main-file-not-found",
            "script-not-found",
            "unknown-identifier",
            "unknown-action",
            "unknown-value",
            "unknown-member",
        ],
    ),
    (
        DiagnosticClass::NameBinding,
        &[
            "duplicate-argument",
            "duplicate-declaration",
            "duplicate-definition",
            "duplicate-rule-name",
            "enum-type-without-member",
            "invalid-arity",
            "vect-arity",
            "unresolved-reference",
            "unknown-keyword",
        ],
    ),
    (
        DiagnosticClass::MergedTable,
        &[
            "rule-prefix-template-duplicate",
            "replacement-duplicate",
            "post-compile-hook-duplicate",
            "macro-redeclaration",
        ],
    ),
    (
        DiagnosticClass::MacroTable,
        &["macro-arity", "macro-recursion"],
    ),
    (
        DiagnosticClass::Script,
        &[
            "script-error",
            "script-timeout",
            "script-stack-limit",
            "script-memory-limit",
            "script-result-not-string",
            "script-internal",
        ],
    ),
    (
        DiagnosticClass::ExternalFile,
        &[
            "include-cycle",
            "settings-not-found",
            "translations-invalid",
            "settings-expression",
            "rule-prefix-template-invalid",
        ],
    ),
    (
        DiagnosticClass::Catalog,
        &[
            "catalog-error",
            "manifest-error",
            "unknown-enum-member",
            "missing-mapping",
            "unknown-texture-member",
        ],
    ),
    (
        DiagnosticClass::Capability,
        &[
            "backend-directive-unsupported",
            "four-dimensional-assignment",
            "four-dimensional-delete",
            "index-collision",
            "index-exhausted",
            "index-out-of-range",
            "random-indexed-delete",
            "split-dict-array-arguments",
            "split-dict-array-arity",
            "split-dict-array-key",
            "symbol-collision",
            "workshop-action-layout",
            "unsupported-action-call",
            "unsupported-construct",
            "unsupported-control-flow",
            "unsupported-dangling",
            "unsupported-duplicate-name",
            "unsupported-enum-domain",
            "unsupported-enum-domain-mismatch",
            "unsupported-enum-member",
            "unsupported-index",
            "unsupported-indexed-initializer",
            "unsupported-integration-surface",
            "unsupported-invalid-argument",
            "unsupported-invalid-arity",
            "unsupported-localized-string",
            "unsupported-member",
            "unsupported-member-assignment",
            "unsupported-missing-argument",
            "unsupported-modify-op",
            "unsupported-non-finite-number",
            "unsupported-rule-order",
            "unsupported-settings",
            "unsupported-subroutine-value",
            "unsupported-value-call",
        ],
    ),
    (
        DiagnosticClass::Semantic,
        &[
            "action-in-value-position",
            "break-context",
            "continue-context",
            "dict-access",
            "invalid-argument",
            "invalid-call-context",
            "invalid-identifier",
            "invalid-iterable",
            "invalid-range-binder",
            "invalid-receiver",
            "invalid-span",
            "invalid-string-modifier",
            "invalid-structure",
            "keyword-required",
            "keyword-unsupported",
            "lambda-context",
            "missing-argument",
            "positional-after-keyword",
            "settings-hero-lists",
            "settings-workshop-value",
            "tabular-arguments",
            "value-in-action-position",
        ],
    ),
    (
        DiagnosticClass::Internal,
        &[
            "incompatible-protocol",
            "malformed-payload",
            "unsupported-node",
            "workshop-emission",
        ],
    ),
    (DiagnosticClass::WarningOnly, &["unknown-setting"]),
];

fn class_of(code: &str) -> Option<DiagnosticClass> {
    CODE_CLASSES
        .iter()
        .find_map(|(class, codes)| codes.contains(&code).then_some(*class))
}

#[cfg(test)]
const NON_DIAGNOSTIC_LITERALS: &[&str] = &["kebab-case", "non-empty"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameParams {
    documents: BTreeMap<String, Document>,
    position_document_uri: String,
    position: LspPosition,
    new_name: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LspPosition {
    pub(crate) line: u32,
    pub(crate) character: u32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct LspRange {
    start: LspPosition,
    end: LspPosition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextEditParam {
    range: LspRange,
    new_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidateEditsParams {
    document: Document,
    edits: Vec<TextEditParam>,
}

struct ModelView {
    outcome: opy_rs::tooling::CheckOutcome,
    display_root: PathBuf,
    entry_uri: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SiteToken {
    pub(crate) col: u32,
    pub(crate) line_index: usize,
    pub(crate) start_index: usize,
    pub(crate) end_index: usize,
}

impl SiteToken {
    pub(crate) fn line(&self) -> u32 {
        self.line_index as u32 + 1
    }

    pub(crate) fn len(&self) -> u32 {
        (self.end_index - self.start_index) as u32
    }
}

/// Source text with LPP position mapping: 0-based lines, UTF-16 code units
/// within a line. Mirrors the conformance reference implementation so every
/// provider maps positions identically.
pub(crate) struct SourceText<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceText<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self { text, line_starts }
    }

    fn line_byte_range(&self, line: usize) -> (usize, usize) {
        let start = self.line_starts[line];
        let mut end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len());
        if end > start && self.text.as_bytes()[end - 1] == b'\n' {
            end -= 1;
            if end > start && self.text.as_bytes()[end - 1] == b'\r' {
                end -= 1;
            }
        }
        (start, end)
    }

    pub(crate) fn line_text(&self, line: usize) -> Option<&'a str> {
        (line < self.line_starts.len()).then(|| {
            let (start, end) = self.line_byte_range(line);
            &self.text[start..end]
        })
    }

    fn byte_of(&self, position: LspPosition) -> Option<usize> {
        let start = *self.line_starts.get(position.line as usize)?;
        let line = self.line_text(position.line as usize)?;
        Some(start + byte_of_char_index(line, char_index_at_utf16(line, position.character)?))
    }
}

pub(crate) fn frontend_column(line: &str, index: usize) -> u32 {
    line.chars()
        .take(index)
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum::<u32>()
        + 1
}

pub(crate) fn char_index_at_column(line: &str, column: u32) -> Option<usize> {
    let mut current = 1u32;
    for (index, ch) in line.chars().enumerate() {
        if current == column {
            return Some(index);
        }
        current += if ch == '\t' { 4 } else { 1 };
    }
    (current == column).then(|| line.chars().count())
}

pub(crate) fn char_index_at_utf16(line: &str, character: u32) -> Option<usize> {
    let mut units = 0u32;
    for (index, ch) in line.chars().enumerate() {
        if units == character {
            return Some(index);
        }
        if units + ch.len_utf16() as u32 > character {
            return None;
        }
        units += ch.len_utf16() as u32;
    }
    (units == character).then(|| line.chars().count())
}

fn utf16_column(line: &str, index: usize) -> u32 {
    line.chars()
        .take(index)
        .map(|ch| ch.len_utf16() as u32)
        .sum()
}

pub(crate) fn utf16_character(line: &str, column: u32) -> u32 {
    let mut units = 0u32;
    let mut current = 1u32;
    for ch in line.chars() {
        if current >= column {
            break;
        }
        units += ch.len_utf16() as u32;
        current += if ch == '\t' { 4 } else { 1 };
    }
    units
}

fn byte_of_char_index(line: &str, index: usize) -> usize {
    line.char_indices()
        .nth(index)
        .map(|(byte, _)| byte)
        .unwrap_or(line.len())
}

pub(crate) fn ident_token_at(line: &str, index: usize) -> Option<(usize, usize)> {
    let (byte, first) = line.char_indices().nth(index)?;
    if !is_ident_start(first) {
        return None;
    }
    let len = line[byte..]
        .chars()
        .take_while(|ch| is_ident_continue(*ch))
        .count();
    Some((index, index + len))
}

fn mentions_identifier(text: &str, name: &str) -> bool {
    let Ok(tokens) = lexer::lex(lexer::LexInput { file_id: 0, text }) else {
        return true;
    };
    tokens.iter().any(|token| match token.kind {
        TokenKind::Ident => token.text == name,
        TokenKind::Directive => token
            .text
            .split(|c: char| !is_ident_continue(c))
            .any(|piece| piece == name),
        _ => false,
    })
}

pub(crate) fn document_root(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(crate) fn normalized(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn supplied_document_for_path<'d>(
    documents: &'d BTreeMap<String, Document>,
    root: &Path,
    path: &str,
) -> Option<&'d Document> {
    let resolved = resolved_path(root, path);
    documents.values().find(|document| {
        filesystem_path(&document.uri)
            .is_some_and(|document_path| same_path(&document_path, &resolved))
    })
}

pub(crate) fn overlay_map(
    documents: &BTreeMap<String, Document>,
    texts: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>, HandlerError> {
    let mut overlays = BTreeMap::new();
    for (uri, document) in documents {
        let path = document_path(document)?;
        let text = texts
            .and_then(|texts| texts.get(uri))
            .map_or(document.text.as_str(), String::as_str);
        overlays.insert(path_string(&path), text.to_string());
        if let Ok(canonical) = path.canonicalize() {
            overlays.insert(path_string(&canonical), text.to_string());
        }
    }
    Ok(overlays)
}

pub(crate) fn narrow_site(
    source: &SourceText,
    location: &SourceLocation,
    name: &str,
) -> Option<SiteToken> {
    let line_index = location.start.line.checked_sub(1)? as usize;
    let line = source.line_text(line_index)?;
    let index = char_index_at_column(line, location.start.col)?;
    let (start_index, end_index) = ident_token_at(line, index)?;
    let token = &line[byte_of_char_index(line, start_index)..byte_of_char_index(line, end_index)];
    (token == name).then_some(SiteToken {
        col: location.start.col,
        line_index,
        start_index,
        end_index,
    })
}

pub(crate) fn token_range(source: &SourceText, token: SiteToken) -> Value {
    let line = source.line_text(token.line_index).unwrap_or_default();
    json!({
        "start": { "line": token.line_index, "character": utf16_column(line, token.start_index) },
        "end": { "line": token.line_index, "character": utf16_column(line, token.end_index) },
    })
}

fn token_byte_range(source: &SourceText, token: SiteToken) -> Option<(usize, usize)> {
    let (line_start, _) = source.line_byte_range(token.line_index);
    let line = source.line_text(token.line_index)?;
    Some((
        line_start + byte_of_char_index(line, token.start_index),
        line_start + byte_of_char_index(line, token.end_index),
    ))
}

fn rename_group(symbol: &Symbol) -> Option<&'static [SymbolKind]> {
    const CALLABLE: &[SymbolKind] = &[SymbolKind::Subroutine, SymbolKind::Def];
    match symbol.kind {
        SymbolKind::Global => Some(&[SymbolKind::Global]),
        SymbolKind::Player => Some(&[SymbolKind::Player]),
        SymbolKind::Subroutine | SymbolKind::Def => Some(CALLABLE),
        SymbolKind::Constant | SymbolKind::Macro => None,
    }
}

fn rename_namespace(kind: SymbolKind) -> NameNamespace {
    match kind {
        SymbolKind::Global => NameNamespace::Global,
        SymbolKind::Player => NameNamespace::Player,
        _ => NameNamespace::Subroutine,
    }
}

pub(crate) fn site_contains(
    site: &SourceLocation,
    file_ids: &BTreeSet<u32>,
    position: Position,
) -> bool {
    file_ids.contains(&site.file_id) && site.start <= position && position < site.end
}

const MAIN_FILE_STUB_ID: u32 = 0;

fn is_main_file_redirect_stub(
    outcome: &opy_rs::tooling::CheckOutcome,
    file: &opy_rs::preprocess::FileRecord,
) -> bool {
    file.id == MAIN_FILE_STUB_ID
        && outcome
            .model
            .as_ref()
            .is_some_and(|model| model.hir.preprocessing.main_file.is_some())
}

pub(crate) fn carries_source(
    outcome: &opy_rs::tooling::CheckOutcome,
    file: &opy_rs::preprocess::FileRecord,
) -> bool {
    !is_main_file_redirect_stub(outcome, file)
}

pub(crate) fn file_ids_covering(
    outcome: &opy_rs::tooling::CheckOutcome,
    path: &Path,
) -> BTreeSet<u32> {
    outcome
        .files
        .iter()
        .filter(|file| carries_source(outcome, file))
        .filter(|file| same_path(&resolved_path(&outcome.display_root, &file.path), path))
        .map(|file| file.id)
        .collect()
}

fn refusal(code: &'static str, details: Value, message: impl Into<String>) -> HandlerError {
    HandlerError::refusal(code, details, message)
}

fn no_symbol(uri: &str) -> HandlerError {
    refusal(
        "rename.noSymbolAtPosition",
        json!({ "uri": uri }),
        "no symbol at position",
    )
}

pub(crate) fn invalid_position(uri: &str, position: LspPosition) -> HandlerError {
    HandlerError::Lpp {
        kind: "invalidPosition",
        details: json!({
            "uri": uri,
            "position": { "line": position.line, "character": position.character },
        }),
        message: "position outside document".to_string(),
    }
}

fn evaluate_candidates<T: Send>(
    candidates: &[&Document],
    f: &(impl Fn(&Document) -> T + Sync),
) -> Vec<T> {
    let workers = std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1)
        .min(candidates.len());
    if workers <= 1 {
        return candidates.iter().map(|document| f(document)).collect();
    }
    let chunk = candidates.len().div_ceil(workers);
    std::thread::scope(|scope| {
        let handles: Vec<_> = candidates
            .chunks(chunk)
            .map(|c| scope.spawn(|| c.iter().map(|d| f(d)).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("worker"))
            .collect()
    })
}

fn model_view(
    documents: &BTreeMap<String, Document>,
    position_document: &Document,
    overlay: &BTreeMap<String, String>,
) -> Result<ModelView, HandlerError> {
    let position_path = document_path(position_document)?;
    let document_paths = documents
        .values()
        .map(document_path)
        .collect::<Result<Vec<_>, _>>()?;

    let evaluate = |document: &Document| {
        let path = document_path(document).expect("paths collected above");
        let root = document_root(&path);
        let outcome = opy_rs::tooling::check_with_overlay(
            &document.text,
            &path_string(&path),
            &root,
            overlay,
        );
        outcome.model.as_ref()?;
        let display_root = outcome.display_root.clone();
        let covers = |target: &Path| {
            outcome
                .files
                .iter()
                .filter(|file| carries_source(&outcome, file))
                .any(|file| same_path(&resolved_path(&display_root, &file.path), target))
        };
        if !covers(&position_path) {
            return None;
        }
        let covered_documents = document_paths.iter().filter(|path| covers(path)).count();
        Some((outcome, display_root, covered_documents))
    };
    let mut representatives: Vec<(&String, &Document)> = Vec::new();
    let mut seen_entries = std::collections::HashSet::new();
    for ((uri, document), path) in documents.iter().zip(&document_paths) {
        if seen_entries.insert(effective_entry(document, path)) {
            representatives.push((uri, document));
        }
    }
    let candidates: Vec<&Document> = representatives.iter().map(|(_, d)| *d).collect();
    let evaluated = evaluate_candidates(&candidates, &evaluate);

    let mut best: Option<(ModelView, (usize, usize))> = None;
    for ((uri, _), evaluation) in representatives.iter().zip(evaluated) {
        let Some((outcome, display_root, covered_documents)) = evaluation else {
            continue;
        };
        let score = (covered_documents, outcome.files.len());
        if best
            .as_ref()
            .is_none_or(|(_, best_score)| score > *best_score)
        {
            best = Some((
                ModelView {
                    outcome,
                    display_root,
                    entry_uri: (*uri).clone(),
                },
                score,
            ));
        }
    }
    if let Some((view, _)) = best {
        return Ok(view);
    }

    let path = document_path(position_document)?;
    let root = document_root(&path);
    let outcome = opy_rs::tooling::check_with_overlay(
        &position_document.text,
        &path_string(&path),
        &root,
        overlay,
    );
    let missing: Vec<&str> = outcome
        .diagnostics
        .iter()
        .filter(|diagnostic| class_of(&diagnostic.code) == Some(DiagnosticClass::MissingSource))
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(refusal(
            "rename.requiresDocument",
            json!({ "uri": position_document.uri, "missing": missing }),
            "the symbol resolves through sources not in the received document set",
        ));
    }
    if let Some(model) = outcome.model.as_ref()
        && model.hir.preprocessing.main_file.is_some()
        && let Some(target) = outcome.files.iter().find(|file| file.id == 1)
    {
        let entry = path_to_file_uri(&resolved_path(&outcome.display_root, &target.path));
        return Err(refusal(
            "rename.requiresDocument",
            json!({ "uri": position_document.uri, "missing": [entry] }),
            "the position document resolves through a project entry that does not include it",
        ));
    }
    Err(no_symbol(&position_document.uri))
}

fn splice<'a>(text: &str, edits: impl Iterator<Item = (usize, usize, &'a str)>) -> String {
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end, replacement) in edits {
        result.push_str(&text[cursor..start]);
        result.push_str(replacement);
        cursor = end;
    }
    result.push_str(&text[cursor..]);
    result
}

fn verify_rename(
    documents: &BTreeMap<String, Document>,
    view: &ModelView,
    new_name: &str,
    group: &[SymbolKind],
    sites: &BTreeMap<String, Vec<SiteToken>>,
) -> Result<(), HandlerError> {
    let collision = || {
        refusal(
            "rename.nameCollision",
            json!({ "newName": new_name }),
            "the rename would collide with an existing binding or change what the name resolves to",
        )
    };

    let mut edited = BTreeMap::new();
    for (uri, tokens) in sites {
        let document = &documents[uri];
        let source = SourceText::new(&document.text);
        let mut ranges = Vec::with_capacity(tokens.len());
        for token in tokens {
            let Some(range) = token_byte_range(&source, *token) else {
                return Err(collision());
            };
            ranges.push(range);
        }
        let result = splice(
            &document.text,
            ranges.iter().map(|&(start, end)| (start, end, new_name)),
        );
        edited.insert(uri.clone(), result);
    }

    let new_len = new_name.chars().count() as i64;
    let mut expected = BTreeSet::new();
    for (uri, tokens) in sites {
        let document = &documents[uri];
        let path = normalized(&document_path(document)?);
        let mut current_line = u32::MAX;
        let mut shift = 0i64;
        for token in tokens {
            if token.line() != current_line {
                current_line = token.line();
                shift = 0;
            }
            let start = token.col as i64 + shift;
            expected.insert((path.clone(), token.line(), start, start + new_len));
            shift += new_len - token.len() as i64;
        }
    }

    let overlay = overlay_map(documents, Some(&edited))?;
    let entry = &documents[&view.entry_uri];
    let entry_path = document_path(entry)?;
    let entry_root = document_root(&entry_path);
    let entry_text = edited
        .get(&view.entry_uri)
        .map_or(entry.text.as_str(), String::as_str);
    let outcome = opy_rs::tooling::check_with_overlay(
        entry_text,
        &path_string(&entry_path),
        &entry_root,
        &overlay,
    );
    let Some(model) = outcome.model else {
        return Err(collision());
    };

    let mut actual = BTreeSet::new();
    for symbol in model
        .symbols()
        .iter()
        .filter(|symbol| symbol.name == new_name && group.contains(&symbol.kind))
    {
        for site in std::iter::once(&symbol.declaration).chain(&symbol.references) {
            let resolved = normalized(&resolved_path(&view.display_root, &site.path));
            let token = supplied_document_for_path(documents, &view.display_root, &site.path)
                .and_then(|document| {
                    let text = edited
                        .get(&document.uri)
                        .map_or(document.text.as_str(), String::as_str);
                    narrow_site(&SourceText::new(text), site, new_name)
                })
                .or_else(|| {
                    fs::read_to_string(&resolved)
                        .ok()
                        .and_then(|text| narrow_site(&SourceText::new(&text), site, new_name))
                });
            let site_key = match token {
                Some(token) => (
                    resolved,
                    token.line(),
                    token.col as i64,
                    token.col as i64 + new_len,
                ),
                None => (resolved, site.start.line, site.start.col as i64, i64::MAX),
            };
            actual.insert(site_key);
        }
    }
    if actual != expected {
        return Err(collision());
    }
    Ok(())
}

pub(crate) fn rename(params: Value) -> Result<Value, HandlerError> {
    let params: RenameParams =
        serde_json::from_value(params).map_err(|_| HandlerError::invalid_params())?;
    validate_documents(&params.documents)?;
    if !is_identifier(&params.new_name) {
        return Err(refusal(
            "rename.invalidName",
            json!({ "newName": params.new_name }),
            format!("'{}' is not a valid OPY identifier", params.new_name),
        ));
    }
    let Some(position_document) = params.documents.get(&params.position_document_uri) else {
        return Err(HandlerError::invalid_document(
            Some(&params.position_document_uri),
            "positionDocumentUriNotInSet",
        ));
    };
    let overlay = overlay_map(&params.documents, None)?;
    let view = model_view(&params.documents, position_document, &overlay)?;
    let model = view
        .outcome
        .model
        .as_ref()
        .expect("view guarantees a model");

    let source = SourceText::new(&position_document.text);
    let Some(line) = source.line_text(params.position.line as usize) else {
        return Err(invalid_position(&position_document.uri, params.position));
    };
    let Some(index) = char_index_at_utf16(line, params.position.character) else {
        return Err(invalid_position(&position_document.uri, params.position));
    };
    let position = Position::new(params.position.line + 1, frontend_column(line, index));

    let position_path = document_path(position_document)?;
    let file_ids = file_ids_covering(&view.outcome, &position_path);
    if file_ids.is_empty() {
        return Err(no_symbol(&position_document.uri));
    }

    let mut narrowed_hits: Vec<&Symbol> = Vec::new();
    let mut containing: Vec<&Symbol> = Vec::new();
    for symbol in model.symbols() {
        let mut narrowed = false;
        let mut contained = false;
        for site in std::iter::once(&symbol.declaration).chain(&symbol.references) {
            if !site_contains(site, &file_ids, position) {
                continue;
            }
            contained = true;
            if let Some(token) = narrow_site(&source, site, &symbol.name) {
                let token_end = Position::new(token.line(), token.col + token.len());
                if position < token_end {
                    narrowed = true;
                }
            }
        }
        if narrowed {
            narrowed_hits.push(symbol);
        } else if contained {
            containing.push(symbol);
        }
    }
    let symbol = narrowed_hits
        .iter()
        .copied()
        .find(|symbol| rename_group(symbol).is_some())
        .or_else(|| narrowed_hits.first().copied());
    let Some(symbol) = symbol else {
        if containing
            .iter()
            .any(|symbol| rename_group(symbol).is_some())
        {
            return Err(no_symbol(&position_document.uri));
        }
        if let Some(&symbol) = containing.first() {
            return Err(refusal(
                "rename.unsupportedSymbolKind",
                json!({ "uri": position_document.uri, "kind": symbol.kind }),
                format!("symbols of kind '{:?}' are not renameable", symbol.kind),
            ));
        }
        return Err(no_symbol(&position_document.uri));
    };
    let Some(group) = rename_group(symbol) else {
        return Err(refusal(
            "rename.unsupportedSymbolKind",
            json!({ "uri": position_document.uri, "kind": symbol.kind }),
            format!("symbols of kind '{:?}' are not renameable", symbol.kind),
        ));
    };
    let name = symbol.name.clone();

    if is_reserved_name(&params.new_name, rename_namespace(symbol.kind)) {
        return Err(refusal(
            "rename.invalidName",
            json!({ "newName": params.new_name }),
            format!("'{}' is a reserved OPY name", params.new_name),
        ));
    }

    let mut sites: BTreeMap<String, Vec<SiteToken>> = BTreeMap::new();
    let mut missing: Vec<String> = Vec::new();
    for symbol in model
        .symbols()
        .iter()
        .filter(|symbol| symbol.name == name && group.contains(&symbol.kind))
    {
        for site in std::iter::once(&symbol.declaration).chain(&symbol.references) {
            let Some(document) =
                supplied_document_for_path(&params.documents, &view.display_root, &site.path)
            else {
                let uri = path_to_file_uri(&resolved_path(&view.display_root, &site.path));
                if !missing.contains(&uri) {
                    missing.push(uri);
                }
                continue;
            };
            let document_source = if document.uri == position_document.uri {
                &source
            } else {
                &SourceText::new(&document.text)
            };
            let Some(token) = narrow_site(document_source, site, &name) else {
                return Err(refusal(
                    "rename.unsupportedReference",
                    json!({ "uri": document.uri, "line": site.start.line }),
                    "a reference does not map to authored source text for this symbol",
                ));
            };
            let tokens = sites.entry(document.uri.clone()).or_default();
            if !tokens.iter().any(|other| {
                other.line_index == token.line_index && other.start_index == token.start_index
            }) {
                tokens.push(token);
            }
        }
    }
    if !missing.is_empty() {
        missing.sort();
        return Err(refusal(
            "rename.requiresDocument",
            json!({ "uris": missing }),
            "the rename touches documents outside the received document set",
        ));
    }

    // Spec §15.2: every reference to the symbol in the *received* documents
    // must be covered, or the rename must refuse. A received document outside
    // the chosen project view that still spells the name — as an identifier
    // token or inside a directive's payload — may hold references this model
    // cannot see (a second project root over a shared include), so renaming
    // would leave it broken.
    let mut unaccounted = Vec::new();
    for document in params.documents.values() {
        let path = document_path(document)?;
        let covered = view
            .outcome
            .files
            .iter()
            .filter(|file| carries_source(&view.outcome, file))
            .any(|file| same_path(&resolved_path(&view.display_root, &file.path), &path));
        if !covered && mentions_identifier(&document.text, &name) {
            unaccounted.push(document.uri.clone());
        }
    }
    if !unaccounted.is_empty() {
        unaccounted.sort();
        return Err(refusal(
            "rename.requiresDocument",
            json!({ "uris": unaccounted }),
            "the rename may touch documents outside the analyzed project",
        ));
    }

    for tokens in sites.values_mut() {
        tokens.sort_by_key(|token| (token.line_index, token.start_index));
    }

    verify_rename(&params.documents, &view, &params.new_name, group, &sites)?;

    let edits = sites
        .iter()
        .map(|(uri, tokens)| {
            let document = &params.documents[uri];
            let document_source = if document.uri == position_document.uri {
                &source
            } else {
                &SourceText::new(&document.text)
            };
            json!({
                "documentUri": document.uri,
                "version": document.version,
                "textEdits": tokens
                    .iter()
                    .map(|token| json!({
                        "range": token_range(document_source, *token),
                        "newText": params.new_name,
                    }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "edits": edits }))
}

pub(crate) fn validate_edits(params: Value) -> Result<Value, HandlerError> {
    let params: ValidateEditsParams =
        serde_json::from_value(params).map_err(|_| HandlerError::invalid_params())?;
    let document = params.document;
    let mut singleton = BTreeMap::new();
    singleton.insert(document.uri.clone(), document.clone());
    validate_documents(&singleton)?;
    let version = document.version;
    let invalid = |reason: &'static str, failing_edit_index: Option<usize>| {
        let mut result = json!({
            "valid": false,
            "version": version,
            "reason": reason,
        });
        if let Some(index) = failing_edit_index {
            result["failingEditIndex"] = json!(index);
        }
        Ok(result)
    };

    let source = SourceText::new(&document.text);
    let mut resolved = Vec::with_capacity(params.edits.len());
    for (index, edit) in params.edits.iter().enumerate() {
        match (
            source.byte_of(edit.range.start),
            source.byte_of(edit.range.end),
        ) {
            (Some(start), Some(end)) if start <= end => resolved.push((start, end)),
            _ => return invalid("rangeOutOfBounds", Some(index)),
        }
    }
    let mut order: Vec<usize> = (0..params.edits.len()).collect();
    order.sort_by_key(|&index| resolved[index].0);
    for pair in order.windows(2) {
        if resolved[pair[0]].1 > resolved[pair[1]].0 {
            return invalid("overlappingEdits", Some(pair[1]));
        }
    }

    let result = splice(
        &document.text,
        order.iter().map(|&index| {
            (
                resolved[index].0,
                resolved[index].1,
                params.edits[index].new_text.as_str(),
            )
        }),
    );

    let path = document_path(&document)?;
    let root = document_root(&path);
    let overlay = overlay_map(&singleton, None)?;

    let outcome =
        opy_rs::tooling::check_with_overlay(&result, &path_string(&path), &root, &overlay);
    let malformed = outcome.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == opy_rs::tooling::DiagnosticSeverity::Error
            && class_of(&diagnostic.code) == Some(DiagnosticClass::DocumentSyntax)
            && diagnostic.span.as_ref().is_some_and(|span| {
                same_path(&resolved_path(&outcome.display_root, &span.path), &path)
            })
    });
    if malformed {
        return invalid("syntaxError", None);
    }
    Ok(json!({ "valid": true, "version": version }))
}

#[cfg(test)]
mod tests;
