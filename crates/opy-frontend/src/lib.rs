//! The standalone OverPy-compatible `.opy` frontend (opy-rs).
//!
//! Owns the OPY source-language surface of the `opy-rs` repository: a lexer,
//! an indentation-aware CST/parser with structured diagnostics and recovery,
//! token-level preprocessing (includes and `#!define` macros), semantic
//! resolution, and lowering into the opy-rs-owned Opy HIR contract
//! ([`hir::Program`]). Everything from source through the Opy HIR semantic
//! model is Workshop-independent: the frontend never depends on `workshop-rs`,
//! OverPy, or Node, and the integration boundary toward `workshop-rs` is
//! documented rather than implemented here.
//!
//! Pipeline: [`lexer::lex`] → [`preprocess::preprocess`] →
//! [`parser::parse`] → [`lower::lower`] → Opy HIR ([`hir`]).
//!
//! This crate is the extraction of the mature Wright frontend
//! (`crates/wright-opy`); module provenance and issue references follow the
//! original implementation. Workshop→OPY reconstruction and the differential
//! harness are not part of this crate (see the opy-rs roadmap).

pub mod cst;
pub mod diag;
pub mod hir;
pub mod lexer;
pub mod lower;
pub mod manifest;
pub mod parser;
pub mod preprocess;
pub mod settings;

use std::path::Path;

pub use diag::{FrontendError, FrontendResult};
pub use lower::lower;
pub use parser::parse;
pub use preprocess::{preprocess, preprocess_with_overlay};

/// The frontend's supported protocol identity for generated HIR.
///
/// The producer identity and the Opy HIR protocol envelope (`wright/opy-hir`
/// v1) are preserved from the Wright frontend so existing consumers keep
/// accepting this producer's payloads during the migration; renaming the
/// producer identity is a protocol decision for the opy-rs docs/architecture
/// workstream.
pub const FRONTEND_NAME: &str = "wright/opy-native";
pub const FRONTEND_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compile one `.opy` source end-to-end into the Opy HIR contract:
/// preprocess (includes/defines) → parse (CST) → lower (HIR).
///
/// `main_path` is the file's display path recorded in the HIR file registry;
/// `root` is the include base. `compile` never requires Node or OverPy.
pub fn compile(source: &str, main_path: &str, root: &Path) -> FrontendResult<hir::Program> {
    compile_with_overlay(source, main_path, root, &std::collections::BTreeMap::new())
}

/// Compile with open-document overlays: includes resolve to overlay text
/// (keyed by the include string or the resolved canonical path) before the
/// filesystem, so unsaved editor buffers participate in include resolution.
pub fn compile_with_overlay(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> FrontendResult<hir::Program> {
    let outcome = compile_with_overlay_outcome(source, main_path, root, overlay);
    match outcome.hir {
        Some(hir) => Ok(hir),
        None => Err(outcome
            .error
            .expect("a failed compile outcome always carries an error")),
    }
}

/// The outcome of a compile with overlays.
///
/// Unlike [`compile_with_overlay`], this retains the frontend file registry
/// even when parsing or lowering fails, so language tooling can map span file
/// ids to their actual source identities without building a diagnostics-only
/// project model.
pub struct CompileOutcome {
    pub hir: Option<hir::Program>,
    pub error: Option<FrontendError>,
    pub files: Vec<preprocess::FileRecord>,
}

/// Compile with open-document overlays while retaining the frontend file
/// registry on parse/lower failure.
pub fn compile_with_overlay_outcome(
    source: &str,
    main_path: &str,
    root: &Path,
    overlay: &std::collections::BTreeMap<String, String>,
) -> CompileOutcome {
    let preprocess::PreprocessOutcome { result, files } =
        preprocess::preprocess_with_overlay_outcome(source, main_path, root, overlay);
    let preprocessed = match result {
        Ok((preprocessed, _)) => preprocessed,
        Err(error) => {
            return CompileOutcome {
                hir: None,
                error: Some(error),
                files,
            };
        }
    };
    let parsed = parse(&preprocessed.tokens);
    if let Some(error) = parsed.errors.first() {
        return CompileOutcome {
            hir: None,
            error: Some(error.clone()),
            files,
        };
    }
    let mut program = parsed
        .program
        .expect("program present when errors are empty");
    // Parse the extracted settings block into the CST; errors flow through
    // the same error path (registry retained for span mapping, #86).
    if let Some(block) = &preprocessed.settings {
        match settings::parse_block(block) {
            Ok(parsed_settings) => program.settings = Some(parsed_settings),
            Err(error) => {
                return CompileOutcome {
                    hir: None,
                    error: Some(error),
                    files,
                };
            }
        }
    }
    let defines = preprocessed
        .defines
        .iter()
        .map(|define| hir::types::Define {
            name: define.name.clone(),
            is_function: define.is_function,
            span: define.span.map(Into::into),
        })
        .collect();
    let hir_files = files
        .iter()
        .map(|file| hir::types::SourceFile {
            id: file.id,
            path: file.path.clone(),
        })
        .collect();
    match lower(&program, hir_files, defines) {
        Ok(hir) => CompileOutcome {
            hir: Some(hir),
            error: None,
            files,
        },
        Err(error) => CompileOutcome {
            hir: None,
            error: Some(error),
            files,
        },
    }
}
