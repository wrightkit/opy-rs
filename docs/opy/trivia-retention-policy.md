# Trivia and Source-Provenance Retention Policy

Status: accepted policy — issue #3 acceptance
Scope: what the OPY frontend retains from authored source and what it
intentionally discards, for the Workshop-independent frontend surface

## Policy

| Input | Retained | Model |
| --- | --- | --- |
| Authored identifiers (declaration and reference spellings) | Yes | CST nodes carry the authored text; the semantic model preserves names exactly |
| Line comments (`# …`) and block comments (`/* … */`) | **No** | The lexer discards comments before tokenization (they never enter the token stream) |
| Whitespace and indentation | No (reconstructed deterministically) | The CST stores statements/blocks, not original indentation |
| Source spans | Yes | 1-based line/column spans per token and CST node; `FrontendError` diagnostics carry spans; the file registry maps span file ids to paths |
| File provenance | Yes | Preprocess `FileRecord` per file (id + path); HIR `SourceFile` entries; spans are attributed across include boundaries |
| Macro/define expansion provenance | Yes | `#!define` expansions carry the define's span; diagnostics attribute to authored and expansion sites |
| Settings blocks | No (consumed pre-lexing) | Parsed into the typed settings payload; source layout not retained |

## Rationale

The declared frontend surface is analysis-oriented: parsing, semantic
resolution, diagnostics, inspection, and validated source *editing* (which
operates on authored source ranges, not on regenerated files). Comment/trivia
retention exists to support byte-stable source *regeneration* and
reconstruction; that surface is deferred and classified `lowering-dependent`
(`workshop-rs` integration, roadmap issue #8). Reconstruct-grade trivia
retention will be introduced with the reconstruction work, not before, so the
lexer's token stream stays deterministic and cheap today.

## Acceptance mapping

Issue #3 requires "comments/trivia and authored identifiers are retained
sufficiently for diagnostics and source-edit tooling according to a documented
policy". This document is that policy: identifiers and spans are retained;
comments/trivia are not, because no declared consumer requires them until
reconstruction.
