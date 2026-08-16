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

pub mod hir;
