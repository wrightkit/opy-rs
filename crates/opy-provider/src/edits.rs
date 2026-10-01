//! `lpp/rename` and `lpp/validateEdits` (LPP spec sections 15 and 16).
//!
//! Rename is semantic, not textual: sites come from [`SemanticModel`]
//! (declaration plus resolved references) and every recorded span is narrowed
//! to the authored identifier token — the source text at a site must spell
//! the symbol name exactly. Sites that do not map to authored text (macro
//! expansions report the use site) refuse the rename instead of emitting a
//! partial edit set. Applying the result and re-checking the project proves
//! the rename: the new model must bind the same kind group at the same sites
//! under the new name, which catches collisions, shadowing, and
//! context-name capture (builtins, `eventPlayer`, enum domains, ...).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use opy_rs::diag::Position;
use opy_rs::lexer::{is_ident_continue, is_ident_start, is_identifier};
use opy_rs::tooling::{SemanticModel, SourceLocation, Symbol, SymbolKind};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Document, HandlerError, document_path, filesystem_path, path_string, path_to_file_uri,
    resolved_path, same_path, validate_documents,
};

/// Words the grammar consumes structurally — as operators or literals in
/// expressions (`and`, `if`, `lambda`, ...), as statement heads (`break`,
/// `del`, `while`, ...), or as top-level declaration heads (`def`, `rule`,
/// ...). They can never spell a renameable symbol name, so they are invalid
/// rename names rather than collisions.
const UNNAMEABLE_WORDS: [&str; 28] = [
    "None",
    "and",
    "break",
    "continue",
    "def",
    "del",
    "do",
    "else",
    "enum",
    "false",
    "for",
    "globalvar",
    "goto",
    "if",
    "in",
    "lambda",
    "macro",
    "not",
    "null",
    "or",
    "pass",
    "playervar",
    "return",
    "rule",
    "subroutine",
    "switch",
    "true",
    "while",
];

/// Diagnostics that describe sources or bindings outside a single document.
/// A document whose only failures are these is a project fragment — edits to
/// it are still well-formed, so `lpp/validateEdits` does not treat them as
/// `syntaxError`, and `lpp/rename` reads them as missing project sources.
const CROSS_FILE_CODES: [&str; 7] = [
    "include-not-found",
    "main-file-not-found",
    "script-not-found",
    "unknown-identifier",
    "unknown-action",
    "unknown-value",
    "unknown-member",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameParams {
    documents: BTreeMap<String, Document>,
    position_document_uri: String,
    position: LspPosition,
    new_name: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct LspPosition {
    line: u32,
    character: u32,
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

/// The checked model view of a document set: the outcome produced by using
/// `entry_uri` as the entry, plus the display root its file-registry paths
/// resolve against (the `#!mainFile` effective directory when the entry
/// redirects the project root).
struct ModelView {
    outcome: opy_rs::tooling::CheckOutcome,
    display_root: PathBuf,
    entry_uri: String,
}

/// An authored identifier token a semantic site narrows to.
#[derive(Debug, Clone, Copy)]
struct SiteToken {
    /// 1-based frontend line of the token.
    line: u32,
    /// 1-based frontend column of the token start.
    col: u32,
    /// Token length in characters (identifier characters are width 1).
    len: u32,
    /// 0-based line index for text access.
    line_index: usize,
    /// Char-index range of the token within its line.
    start_index: usize,
    end_index: usize,
}

/// Source text with LPP position mapping: 0-based lines, UTF-16 code units
/// within a line. Mirrors the conformance reference implementation so every
/// provider maps positions identically.
struct SourceText<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceText<'a> {
    fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self { text, line_starts }
    }

    /// Byte range of `line`, excluding a trailing `"\n"` or `"\r\n"`.
    fn line_byte_range(&self, line: usize) -> (usize, usize) {
        let start = self.line_starts[line];
        let mut end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len());
        if self.text.as_bytes().get(end.wrapping_sub(1)) == Some(&b'\n') {
            end -= 1;
            if self.text.as_bytes().get(end.wrapping_sub(1)) == Some(&b'\r') {
                end -= 1;
            }
        }
        (start, end)
    }

    fn line_text(&self, line: usize) -> Option<&'a str> {
        (line < self.line_starts.len()).then(|| {
            let (start, end) = self.line_byte_range(line);
            &self.text[start..end]
        })
    }

    /// Byte offset of an LPP position; `None` when the position is outside
    /// the document or inside a supplementary-plane character. A position at
    /// the end of a line is valid.
    fn byte_of(&self, position: LspPosition) -> Option<usize> {
        let start = *self.line_starts.get(position.line as usize)?;
        let line_text = self.line_text(position.line as usize)?;
        let mut units = 0u32;
        let mut byte = line_text.len();
        for (index, ch) in line_text.char_indices() {
            if units == position.character {
                byte = index;
                break;
            }
            if units + ch.len_utf16() as u32 > position.character {
                return None;
            }
            units += ch.len_utf16() as u32;
        }
        (units == position.character).then(|| start + byte)
    }
}

/// The frontend column of `index` (a char index) within `line`: 1-based, and
/// `\t` spans four columns — the lexer advances the column by four per tab.
fn frontend_column(line: &str, index: usize) -> u32 {
    line.chars()
        .take(index)
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum::<u32>()
        + 1
}

/// The char index at 1-based frontend `column` within `line`; `None` when the
/// column lands inside a tab's span or beyond the line's end.
fn char_index_at_column(line: &str, column: u32) -> Option<usize> {
    let mut current = 1u32;
    for (index, ch) in line.chars().enumerate() {
        if current == column {
            return Some(index);
        }
        current += if ch == '\t' { 4 } else { 1 };
    }
    (current == column).then(|| line.chars().count())
}

/// The char index at UTF-16 unit `character` within `line`; `None` when the
/// unit lands inside a supplementary-plane character or beyond the line's
/// end.
fn char_index_at_utf16(line: &str, character: u32) -> Option<usize> {
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

/// UTF-16 units consumed by `line`'s first `index` characters.
fn utf16_column(line: &str, index: usize) -> u32 {
    line.chars()
        .take(index)
        .map(|ch| ch.len_utf16() as u32)
        .sum()
}

/// The byte offset of `index` (a char index) within `line`.
fn byte_of_char_index(line: &str, index: usize) -> usize {
    line.char_indices()
        .nth(index)
        .map(|(byte, _)| byte)
        .unwrap_or(line.len())
}

/// The identifier token of `line` starting at char `index`, as a char-index
/// range; `None` when no identifier starts there.
fn ident_token_at(line: &str, index: usize) -> Option<(usize, usize)> {
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

fn position_leq(a: Position, b: Position) -> bool {
    (a.line, a.col) <= (b.line, b.col)
}

fn position_lt(a: Position, b: Position) -> bool {
    (a.line, a.col) < (b.line, b.col)
}

/// The parent directory of `path`, or `"."` for a bare file name.
fn document_root(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A path in canonical form when it exists, verbatim otherwise.
fn normalized(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The supplied document whose URI resolves to `path` under `root`.
fn supplied_document_for_path<'d>(
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

/// Overlay map keyed by both the raw and canonicalized document path so
/// include and `#!mainFile` resolution hit open buffers regardless of
/// symlink normalization. `texts` overrides document contents — used to
/// re-check the renamed project.
fn overlay_map(
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

/// The registry display root `check_with_overlay` produces for `text` loaded
/// from `root`: the `#!mainFile` effective directory when the first line
/// redirects the entry, mirroring `preprocess_with_overlay_outcome`.
fn display_root(text: &str, root: &Path, overlay: &BTreeMap<String, String>) -> PathBuf {
    let resolved_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let Some(first_line) = text.lines().next() else {
        return resolved_root;
    };
    let first_line = first_line.trim_end_matches('\r');
    let Some(value) = first_line.strip_prefix("#!mainFile").map(str::trim) else {
        return resolved_root;
    };
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        });
    let Some(main_file) = value.filter(|value| !value.is_empty()) else {
        return resolved_root;
    };
    let candidate = resolved_root.join(main_file);
    let canonical = candidate.canonicalize().ok();
    let overlay_hit = overlay.contains_key(main_file)
        || overlay.contains_key(&path_string(&candidate))
        || canonical
            .as_ref()
            .is_some_and(|path| overlay.contains_key(&path_string(path)));
    if overlay_hit {
        return candidate
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(resolved_root);
    }
    canonical
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or(resolved_root)
}

/// Narrow a recorded semantic location to the authored identifier token at
/// its start. Returns `None` when the location does not spell `name` exactly
/// in the source — macro-expanded or generated sites report positions whose
/// text cannot be rewritten for this symbol.
fn narrow_site(text: &str, location: &SourceLocation, name: &str) -> Option<SiteToken> {
    let line_index = location.start.line.checked_sub(1)? as usize;
    let line = SourceText::new(text).line_text(line_index)?;
    let index = char_index_at_column(line, location.start.col)?;
    let (start_index, end_index) = ident_token_at(line, index)?;
    let token: String = line
        .chars()
        .skip(start_index)
        .take(end_index - start_index)
        .collect();
    (token == name).then(|| SiteToken {
        line: location.start.line,
        col: location.start.col,
        len: (end_index - start_index) as u32,
        line_index,
        start_index,
        end_index,
    })
}

/// The LPP range of a narrowed token.
fn token_range(text: &str, token: SiteToken) -> Value {
    let line = SourceText::new(text)
        .line_text(token.line_index)
        .unwrap_or_default();
    json!({
        "start": { "line": token.line_index, "character": utf16_column(line, token.start_index) },
        "end": { "line": token.line_index, "character": utf16_column(line, token.end_index) },
    })
}

/// Byte range of a narrowed token inside `text`.
fn token_byte_range(text: &str, token: SiteToken) -> Option<(usize, usize)> {
    let source = SourceText::new(text);
    let (line_start, _) = source.line_byte_range(token.line_index);
    let line = source.line_text(token.line_index)?;
    Some((
        line_start + byte_of_char_index(line, token.start_index),
        line_start + byte_of_char_index(line, token.end_index),
    ))
}

/// The symbol-kind group a rename of `symbol` must rewrite together, or
/// `None` when the symbol is outside the renameable scope. A `subroutine`
/// declaration and a `def` of the same name index as separate symbols but
/// share one call-site namespace, so they move as one group; a `def` that is
/// not a subroutine's implementation body, plus constants and macros, are
/// unsupported rename targets.
fn rename_group(symbol: &Symbol, model: &SemanticModel) -> Option<&'static [SymbolKind]> {
    const CALLABLE: &[SymbolKind] = &[SymbolKind::Subroutine, SymbolKind::Def];
    match symbol.kind {
        SymbolKind::Global => Some(&[SymbolKind::Global]),
        SymbolKind::Player => Some(&[SymbolKind::Player]),
        SymbolKind::Subroutine => Some(CALLABLE),
        SymbolKind::Def => model
            .symbols()
            .iter()
            .any(|other| other.name == symbol.name && other.kind == SymbolKind::Subroutine)
            .then_some(CALLABLE),
        SymbolKind::Constant | SymbolKind::Macro => None,
    }
}

/// Whether `span` (half-open `[start, end)`) contains the point `position`.
fn site_contains(site: &SourceLocation, file_id: u32, position: Position) -> bool {
    site.file_id == file_id && position_leq(site.start, position) && position_lt(position, site.end)
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

/// Pick the check whose file registry covers the position document and the
/// most supplied documents: a document reached through `#!include` only
/// resolves through the project that includes it, so every supplied document
/// is tried as a candidate entry and the widest successful view wins.
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

    let mut best: Option<(ModelView, (usize, usize))> = None;
    for (uri, document) in documents {
        let path = document_path(document)?;
        let root = document_root(&path);
        let outcome = opy_rs::tooling::check_with_overlay(
            &document.text,
            &path_string(&path),
            &root,
            overlay,
        );
        if outcome.model.is_none() {
            continue;
        }
        let display_root = display_root(&document.text, &root, overlay);
        let covers = |target: &Path| {
            outcome
                .files
                .iter()
                .any(|file| same_path(&resolved_path(&display_root, &file.path), target))
        };
        if !covers(&position_path) {
            continue;
        }
        let covered_documents = document_paths.iter().filter(|path| covers(path)).count();
        let score = (covered_documents, outcome.files.len());
        if best
            .as_ref()
            .is_none_or(|(_, best_score)| score > *best_score)
        {
            best = Some((
                ModelView {
                    outcome,
                    display_root,
                    entry_uri: uri.clone(),
                },
                score,
            ));
        }
    }
    if let Some((view, _)) = best {
        return Ok(view);
    }

    // No supplied entry produced an analyzable project covering the position
    // document. Report what the document's own check found: missing project
    // sources refuse as `requiresDocument`; otherwise the document cannot be
    // analyzed and the position has no resolvable symbol.
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
        .filter(|diagnostic| CROSS_FILE_CODES.contains(&diagnostic.code.as_str()))
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(refusal(
            "rename.requiresDocument",
            json!({ "uri": position_document.uri, "missing": missing }),
            "the symbol resolves through sources not in the received document set",
        ));
    }
    Err(no_symbol(&position_document.uri))
}

/// Re-check the renamed project and verify the new-name symbols bind exactly
/// the moved sites. Any binding change — collision, shadowing, capture by a
/// builtin or context name, or a site that stops being authored text —
/// refuses the rename.
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

    // Apply the edits to produce each document's post-rename text.
    let mut edited = BTreeMap::new();
    for (uri, tokens) in sites {
        let document = &documents[uri];
        let mut ranges = Vec::with_capacity(tokens.len());
        for token in tokens {
            let Some(range) = token_byte_range(&document.text, *token) else {
                return Err(collision());
            };
            ranges.push(range);
        }
        let mut result = String::with_capacity(document.text.len());
        let mut cursor = 0;
        for (start, end) in ranges {
            result.push_str(&document.text[cursor..start]);
            result.push_str(new_name);
            cursor = end;
        }
        result.push_str(&document.text[cursor..]);
        edited.insert(uri.clone(), result);
    }

    // Expected post-rename sites: the narrowed tokens shifted by the
    // same-line edits that precede them.
    let new_len = new_name.chars().count() as i64;
    let mut expected = BTreeSet::new();
    for (uri, tokens) in sites {
        let document = &documents[uri];
        let path = normalized(&document_path(document)?);
        let mut current_line = u32::MAX;
        let mut shift = 0i64;
        for token in tokens {
            if token.line != current_line {
                current_line = token.line;
                shift = 0;
            }
            let start = token.col as i64 + shift;
            expected.insert((path.clone(), token.line, start, start + new_len));
            shift += new_len - token.len as i64;
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

    // Actual post-rename sites: every new-name site of the renamed kind
    // group. Sites that fail to narrow against authored text are kept under
    // their raw span so they can only mismatch.
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
                    narrow_site(text, site, new_name)
                })
                .or_else(|| {
                    fs::read_to_string(&resolved)
                        .ok()
                        .and_then(|text| narrow_site(&text, site, new_name))
                });
            let site_key = match token {
                Some(token) => (
                    resolved,
                    token.line,
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

/// `lpp/rename`: semantic rename across the received document set.
pub(crate) fn rename(params: Value) -> Result<Value, HandlerError> {
    let params: RenameParams =
        serde_json::from_value(params).map_err(|_| HandlerError::Standard {
            code: -32602,
            message: "Invalid params",
        })?;
    validate_documents(&params.documents)?;
    if !is_identifier(&params.new_name) || UNNAMEABLE_WORDS.contains(&params.new_name.as_str()) {
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

    // The request position is LPP (0-based line, UTF-16 units); the model
    // records frontend positions (1-based, tab-expanded columns).
    let source = SourceText::new(&position_document.text);
    let invalid_position = || HandlerError::Lpp {
        kind: "invalidPosition",
        details: json!({
            "uri": position_document.uri,
            "position": { "line": params.position.line, "character": params.position.character },
        }),
        message: "position outside document".to_string(),
    };
    let Some(line) = source.line_text(params.position.line as usize) else {
        return Err(invalid_position());
    };
    let Some(index) = char_index_at_utf16(line, params.position.character) else {
        return Err(invalid_position());
    };
    let position = Position::new(params.position.line + 1, frontend_column(line, index));

    let position_path = document_path(position_document)?;
    let Some(file_id) = view
        .outcome
        .files
        .iter()
        .find(|file| {
            same_path(
                &resolved_path(&view.display_root, &file.path),
                &position_path,
            )
        })
        .map(|file| file.id)
    else {
        return Err(no_symbol(&position_document.uri));
    };

    // Candidate symbols whose declaration or reference spans contain the
    // position. A narrowed hit means the position sits on the authored token
    // that spells the symbol name.
    let mut narrowed_hits: Vec<&Symbol> = Vec::new();
    let mut containing: Vec<&Symbol> = Vec::new();
    for symbol in model.symbols() {
        let mut narrowed = false;
        let mut contained = false;
        for site in std::iter::once(&symbol.declaration).chain(&symbol.references) {
            if !site_contains(site, file_id, position) {
                continue;
            }
            contained = true;
            if let Some(token) = narrow_site(&position_document.text, site, &symbol.name) {
                let token_end = Position::new(token.line, token.col + token.len);
                if position_lt(position, token_end) {
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
    // Prefer a renameable hit: a call site shared by a `subroutine` and a
    // `def` of the same name lands on both symbols.
    let symbol = narrowed_hits
        .iter()
        .copied()
        .find(|symbol| rename_group(symbol, model).is_some())
        .or_else(|| narrowed_hits.first().copied());
    let Some(symbol) = symbol else {
        if containing
            .iter()
            .any(|symbol| rename_group(symbol, model).is_some())
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
    let Some(group) = rename_group(symbol, model) else {
        return Err(refusal(
            "rename.unsupportedSymbolKind",
            json!({ "uri": position_document.uri, "kind": symbol.kind }),
            format!("symbols of kind '{:?}' are not renameable", symbol.kind),
        ));
    };
    let name = symbol.name.clone();

    // Collect the declaration plus every reference site of the same-name
    // symbols in the rename group.
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
            let Some(token) = narrow_site(&document.text, site, &name) else {
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
    for tokens in sites.values_mut() {
        tokens.sort_by_key(|token| (token.line_index, token.start_index));
    }

    verify_rename(&params.documents, &view, &params.new_name, group, &sites)?;

    let edits = sites
        .iter()
        .map(|(uri, tokens)| {
            let document = &params.documents[uri];
            json!({
                "documentUri": document.uri,
                "version": document.version,
                "textEdits": tokens
                    .iter()
                    .map(|token| json!({
                        "range": token_range(&document.text, *token),
                        "newText": params.new_name,
                    }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "edits": edits }))
}

/// `lpp/validateEdits`: normative edit application (spec section 16.3).
pub(crate) fn validate_edits(params: Value) -> Result<Value, HandlerError> {
    let params: ValidateEditsParams =
        serde_json::from_value(params).map_err(|_| HandlerError::Standard {
            code: -32602,
            message: "Invalid params",
        })?;
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

    let mut result = String::with_capacity(document.text.len());
    let mut cursor = 0;
    for &index in &order {
        result.push_str(&document.text[cursor..resolved[index].0]);
        result.push_str(&params.edits[index].new_text);
        cursor = resolved[index].1;
    }
    result.push_str(&document.text[cursor..]);

    let path = document_path(&document)?;
    let root = document_root(&path);
    let overlay = overlay_map(&singleton, None)?;
    let outcome =
        opy_rs::tooling::check_with_overlay(&result, &path_string(&path), &root, &overlay);
    let error = outcome.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == opy_rs::tooling::DiagnosticSeverity::Error
            && !CROSS_FILE_CODES.contains(&diagnostic.code.as_str())
    });
    if error {
        return invalid("syntaxError", None);
    }
    Ok(json!({ "valid": true, "version": version }))
}
