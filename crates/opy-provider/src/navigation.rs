//! `lpp/symbols`, `lpp/definition`, and `lpp/references` (LPP spec sections
//! 12–14).
//!
//! All three methods read the [`SemanticModel`] symbol index: declaration
//! and reference sites carry frontend spans that resolve through the file
//! registry to authored `#!include` sources. Reported locations are narrowed
//! to the authored identifier token, so a `reset()` call site yields the
//! `reset` range; reference sites that cannot map to authored text
//! (macro-expanded uses report the call site) are not considered symbol
//! positions.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use opy_rs::diag::Position;
use opy_rs::tooling::{CheckOutcome, SourceLocation, Symbol};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::edits::{
    LspPosition, SiteToken, SourceText, carries_source, char_index_at_column, char_index_at_utf16,
    document_root, frontend_column, ident_token_at, narrow_site, normalized, overlay_map,
    token_range,
};
use crate::{
    Document, HandlerError, document_path, effective_entry, lsp_position, path_string,
    path_to_file_uri, resolved_path, same_path, validate_documents,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SymbolsParams {
    documents: Option<BTreeMap<String, Document>>,
    #[allow(dead_code)]
    project_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionParams {
    document: Document,
    position: LspPosition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferencesParams {
    document: Document,
    position: LspPosition,
    include_declaration: bool,
}

/// Param field names the LPP spec defines for some request. A field the
/// receiving method's schema does not include — `entry` on `lpp/symbols`,
/// `documents` on `lpp/definition`, `includeDeclaration` on `lpp/definition`
/// — is a schema violation (-32602), not an ignorable unknown field
/// (spec §19.1).
const SPEC_PARAM_FIELDS: &[&str] = &[
    "documents",
    "projectRoot",
    "entry",
    "locale",
    "acceptedArtifactFormats",
    "document",
    "position",
    "positionDocumentUri",
    "newName",
    "edits",
    "includeDeclaration",
    "languageId",
    "query",
    "kind",
    "within",
    "limit",
    "calls",
    "enums",
    "artifact",
    "protocolVersion",
    "clientInfo",
];

fn parse_params<T: serde::de::DeserializeOwned>(
    params: Value,
    allowed: &[&str],
) -> Result<T, HandlerError> {
    if let Some(object) = params.as_object()
        && object.keys().any(|key| {
            SPEC_PARAM_FIELDS.contains(&key.as_str()) && !allowed.contains(&key.as_str())
        })
    {
        return Err(invalid_params());
    }
    serde_json::from_value(params).map_err(|_| invalid_params())
}

/// The position document's resolved project view: its check outcome, its
/// filesystem path, the request position in frontend coordinates (1-based
/// line, tab-expanded column), and the registry ids carrying its source.
struct PositionView {
    outcome: CheckOutcome,
    position_path: PathBuf,
    file_ids: BTreeSet<u32>,
    position: Position,
}

fn invalid_params() -> HandlerError {
    HandlerError::Standard {
        code: -32602,
        message: "Invalid params",
    }
}

fn no_symbol(code: &'static str, uri: &str) -> HandlerError {
    HandlerError::refusal(code, json!({ "uri": uri }), "no symbol at position")
}

/// Registry ids in `outcome` whose records resolve to `path` and carry
/// analyzed source.
fn file_ids_covering(outcome: &CheckOutcome, path: &Path) -> BTreeSet<u32> {
    outcome
        .files
        .iter()
        .filter(|file| carries_source(outcome, file))
        .filter(|file| same_path(&resolved_path(&outcome.display_root, &file.path), path))
        .map(|file| file.id)
        .collect()
}

/// The authored name token a declaration site presents. Global, player,
/// subroutine, and `def` sites record a name span so [`narrow_site`]
/// resolves directly; `macro NAME(...)`/`macro NAME = expr` declarations
/// record a span starting at the `macro` keyword, so the name token is the
/// first identifier spelling `name` later on the same line.
fn declaration_token(text: &str, site: &SourceLocation, name: &str) -> Option<SiteToken> {
    let source = SourceText::new(text);
    if let Some(token) = narrow_site(&source, site, name) {
        return Some(token);
    }
    let line_index = site.start.line.checked_sub(1)? as usize;
    let line = source.line_text(line_index)?;
    let mut index = char_index_at_column(line, site.start.col)?;
    let char_count = line.chars().count();
    while index < char_count {
        match ident_token_at(line, index) {
            Some((token_start, token_end)) => {
                let token: String = line
                    .chars()
                    .skip(token_start)
                    .take(token_end - token_start)
                    .collect();
                if token == name {
                    return Some(SiteToken {
                        col: frontend_column(line, token_start),
                        line_index,
                        start_index: token_start,
                        end_index: token_end,
                    });
                }
                index = token_end;
            }
            None => index += 1,
        }
    }
    None
}

/// Whether `position` (frontend coordinates) is on `token`.
fn token_contains(token: &SiteToken, position: Position) -> bool {
    token.line() == position.line
        && token.col <= position.col
        && position.col < token.col + token.len()
}

/// The LPP range a site reports: the authored identifier token when found,
/// otherwise the recorded span — always authored source coordinates.
fn site_range(text: &str, site: &SourceLocation, token: Option<SiteToken>) -> Value {
    if let Some(token) = token {
        return token_range(&SourceText::new(text), token);
    }
    json!({
        "start": lsp_position(text, site.start.line, site.start.col),
        "end": lsp_position(text, site.end.line, site.end.col),
    })
}

/// Order locations by `range.start`, then `uri` for determinism.
fn compare_by_range_start(a: &Value, b: &Value) -> std::cmp::Ordering {
    (
        a["range"]["start"]["line"].as_u64().unwrap_or_default(),
        a["range"]["start"]["character"]
            .as_u64()
            .unwrap_or_default(),
        a["uri"].as_str().unwrap_or_default(),
    )
        .cmp(&(
            b["range"]["start"]["line"].as_u64().unwrap_or_default(),
            b["range"]["start"]["character"]
                .as_u64()
                .unwrap_or_default(),
            b["uri"].as_str().unwrap_or_default(),
        ))
}

/// The `Location` of a site for a single-document request: the position
/// document's URI and text when the site resolves to it, else a `file:` URI
/// and the on-disk text of the reached source. `declaration` sites use the
/// lenient [`declaration_token`] so the reported range matches the position
/// the hit test accepts.
fn site_location(
    document: &Document,
    position_path: &Path,
    display_root: &Path,
    site: &SourceLocation,
    name: &str,
    declaration: bool,
) -> Value {
    let resolved = resolved_path(display_root, &site.path);
    let (uri, text) = if same_path(&resolved, position_path) {
        (document.uri.clone(), document.text.clone())
    } else {
        (
            path_to_file_uri(&normalized(&resolved)),
            fs::read_to_string(&resolved).unwrap_or_default(),
        )
    };
    let token = if declaration {
        declaration_token(&text, site, name)
    } else {
        narrow_site(&SourceText::new(&text), site, name)
    };
    json!({
        "uri": uri,
        "range": site_range(&text, site, token),
    })
}

/// Check the position document — its `#!include` and `#!mainFile` targets
/// resolve through the filesystem and the document's own text — and place
/// the LPP request position in it. A document with no analyzable project
/// covering it refuses with the method's `noSymbolAtPosition` code.
fn position_view(
    document: &Document,
    position: LspPosition,
    no_symbol_code: &'static str,
) -> Result<PositionView, HandlerError> {
    let mut singleton = BTreeMap::new();
    singleton.insert(document.uri.clone(), document.clone());
    validate_documents(&singleton)?;
    let invalid_position = || HandlerError::Lpp {
        kind: "invalidPosition",
        details: json!({
            "uri": document.uri,
            "position": { "line": position.line, "character": position.character },
        }),
        message: "position outside document".to_string(),
    };
    let source = SourceText::new(&document.text);
    let Some(line) = source.line_text(position.line as usize) else {
        return Err(invalid_position());
    };
    let Some(index) = char_index_at_utf16(line, position.character) else {
        return Err(invalid_position());
    };
    let frontend_position = Position::new(position.line + 1, frontend_column(line, index));
    let position_path = document_path(document)?;
    let overlay = overlay_map(&singleton, None)?;
    let outcome = opy_rs::tooling::check_with_overlay(
        &document.text,
        &path_string(&position_path),
        &document_root(&position_path),
        &overlay,
    );
    if outcome.model.is_none() {
        return Err(no_symbol(no_symbol_code, &document.uri));
    }
    let file_ids = file_ids_covering(&outcome, &position_path);
    if file_ids.is_empty() {
        return Err(no_symbol(no_symbol_code, &document.uri));
    }
    Ok(PositionView {
        outcome,
        position_path,
        file_ids,
        position: frontend_position,
    })
}

/// The symbols whose authored name token the request position sits on —
/// through the declaration site or one of the reference sites. A position
/// inside a recorded span but off the authored token (a call's argument, a
/// macro use site's replacement) is not a symbol position.
fn symbols_at<'v>(view: &'v PositionView, document: &Document) -> Vec<&'v Symbol> {
    let model = view
        .outcome
        .model
        .as_ref()
        .expect("view guarantees a model");
    let source = SourceText::new(&document.text);
    model
        .symbols()
        .iter()
        .filter(|symbol| {
            (view.file_ids.contains(&symbol.declaration.file_id)
                && declaration_token(&document.text, &symbol.declaration, &symbol.name)
                    .is_some_and(|token| token_contains(&token, view.position)))
                || symbol.references.iter().any(|site| {
                    view.file_ids.contains(&site.file_id)
                        && narrow_site(&source, site, &symbol.name)
                            .is_some_and(|token| token_contains(&token, view.position))
                })
        })
        .collect()
}

/// `lpp/symbols`: each received document lists the symbols the analyzed
/// project declares in it, in declaration order. A document that no clean
/// outcome covers — its own fragment failed, or it is a `#!mainFile` stub —
/// lists no symbols.
pub(crate) fn symbols(params: Value) -> Result<Value, HandlerError> {
    let params: SymbolsParams = parse_params(params, &["documents", "projectRoot"])?;
    let Some(documents) = params.documents else {
        return Err(invalid_params());
    };
    validate_documents(&documents)?;
    let overlay = overlay_map(&documents, None)?;

    // One project check per effective entry — the same grouping
    // `lpp/check` uses — then each document reads its declarations from the
    // first clean outcome whose registry covers it, so a document reached
    // through an includer's project is attributed even when its own
    // fragment does not analyze standalone.
    let mut groups: BTreeMap<PathBuf, Vec<&Document>> = BTreeMap::new();
    for document in documents.values() {
        let path = document_path(document)?;
        groups
            .entry(effective_entry(document, &path))
            .or_default()
            .push(document);
    }
    let mut outcomes: Vec<CheckOutcome> = Vec::with_capacity(groups.len());
    for (entry, members) in &groups {
        let representative = members
            .iter()
            .copied()
            .find(|document| document_path(document).is_ok_and(|path| same_path(&path, entry)))
            .unwrap_or(members[0]);
        let path = document_path(representative)?;
        outcomes.push(opy_rs::tooling::check_with_overlay(
            &representative.text,
            &path_string(&path),
            &document_root(&path),
            &overlay,
        ));
    }

    let mut result = Vec::with_capacity(documents.len());
    for (uri, document) in &documents {
        let path = document_path(document)?;
        let mut declared: Vec<&Symbol> = Vec::new();
        for outcome in &outcomes {
            let file_ids = file_ids_covering(outcome, &path);
            if file_ids.is_empty() || outcome.model.is_none() {
                continue;
            }
            let model = outcome.model.as_ref().expect("model checked");
            declared = model
                .symbols()
                .iter()
                .filter(|symbol| file_ids.contains(&symbol.declaration.file_id))
                .collect();
            declared.sort_by_key(|symbol| {
                (symbol.declaration.start.line, symbol.declaration.start.col)
            });
            break;
        }
        result.push(json!({
            "uri": uri,
            "version": document.version,
            "symbols": declared
                .iter()
                .map(|symbol| {
                    json!({
                        "name": symbol.name,
                        "kind": symbol.kind,
                        "range": site_range(
                            &document.text,
                            &symbol.declaration,
                            declaration_token(
                                &document.text,
                                &symbol.declaration,
                                &symbol.name,
                            ),
                        ),
                    })
                })
                .collect::<Vec<_>>(),
        }));
    }
    Ok(json!({ "documents": result }))
}

/// `lpp/definition`: the declaration location(s) of the symbol at the
/// position — a call site shared by a `subroutine` declaration and its `def`
/// resolves both.
pub(crate) fn definition(params: Value) -> Result<Value, HandlerError> {
    let params: PositionParams = parse_params(params, &["document", "position"])?;
    let view = position_view(
        &params.document,
        params.position,
        "definition.noSymbolAtPosition",
    )?;
    let symbols = symbols_at(&view, &params.document);
    if symbols.is_empty() {
        return Err(no_symbol(
            "definition.noSymbolAtPosition",
            &params.document.uri,
        ));
    }
    let mut locations: Vec<Value> = Vec::new();
    for symbol in symbols {
        let location = site_location(
            &params.document,
            &view.position_path,
            &view.outcome.display_root,
            &symbol.declaration,
            &symbol.name,
            true,
        );
        if !locations.contains(&location) {
            locations.push(location);
        }
    }
    // Multiple declarations resolve when a call site is shared by a
    // `subroutine` and its `def`; order them deterministically.
    locations.sort_by(compare_by_range_start);
    Ok(json!({ "locations": locations }))
}

/// `lpp/references`: every reference site of the symbol at the position,
/// sorted by `range.start`; `includeDeclaration` lists the declaration
/// location(s) first.
pub(crate) fn references(params: Value) -> Result<Value, HandlerError> {
    let params: ReferencesParams =
        parse_params(params, &["document", "position", "includeDeclaration"])?;
    let view = position_view(
        &params.document,
        params.position,
        "references.noSymbolAtPosition",
    )?;
    let symbols = symbols_at(&view, &params.document);
    if symbols.is_empty() {
        return Err(no_symbol(
            "references.noSymbolAtPosition",
            &params.document.uri,
        ));
    }
    let mut declarations: Vec<Value> = Vec::new();
    let mut references: Vec<Value> = Vec::new();
    for symbol in symbols {
        if params.include_declaration {
            let declaration = site_location(
                &params.document,
                &view.position_path,
                &view.outcome.display_root,
                &symbol.declaration,
                &symbol.name,
                true,
            );
            if !declarations.contains(&declaration) {
                declarations.push(declaration);
            }
        }
        for site in &symbol.references {
            let location = site_location(
                &params.document,
                &view.position_path,
                &view.outcome.display_root,
                site,
                &symbol.name,
                false,
            );
            if !references.contains(&location) {
                references.push(location);
            }
        }
    }
    // A call site shared by a `subroutine` and its `def` lists both
    // declarations; the spec orders the remaining locations by range start,
    // so the declaration block sorts the same way for determinism.
    declarations.sort_by(compare_by_range_start);
    references.sort_by(compare_by_range_start);
    declarations.extend(references);
    Ok(json!({ "locations": declarations }))
}
