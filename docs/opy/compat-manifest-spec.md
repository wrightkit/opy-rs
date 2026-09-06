# OPY compatibility manifest — implementation note

The compatibility manifest remains part of the current `opy-rs` implementation, but it is **not** the semantic authority for the OverPy language.

Current architecture is defined by [`docs/architecture/language-core.md`](../architecture/language-core.md). For the declared core-language surface, upstream OverPy behavior is the executable specification; inventories, manifests, probes, differential tests, corpus fixtures, and real projects verify completeness and compatibility.

## Current implementation reality

The manifest under `crates/opy-rs/src/manifest/` currently carries a mixture of:

- declarative identities, names, aliases, signatures, catalog links, and provenance;
- behavioral metadata such as receiver restrictions, argument-binding modes, contextual dispatch, call-context restrictions, defaults with lowering meaning, and special-lowering classification.

The source implementation consumes those fields for semantic resolution and lowering today. This document does not change that behavior.

Under the current architecture contract, that mixture is an audit target rather than a pattern to extend. Purely declarative inventories may remain data-driven. Observable source-language behavior and invariants should normally be expressed in typed Rust close to the owning semantic feature instead of growing a generic metadata-interpreted semantic language.

In particular, adding another manifest field that makes generic code decide receiver/member semantics, keyword/positional binding, contextual dispatch, compile-time behavior, coercion, or special lowering requires an explicit architecture justification; existing fields are not sufficient precedent.

## Workshop boundary

Manifest catalog links may refer to canonical Workshop identities, but `opy-rs` does not own Workshop catalog membership, enum member lists, localization, settings content, WIR, validation, or emission. Those remain `workshop-rs` responsibilities.

OverPy-specific names, aliases, special forms, contextual behavior, and compiler policy remain `opy-rs` responsibilities even when they eventually lower to a canonical Workshop identity.

See [`docs/architecture/workshop-boundary.md`](../architecture/workshop-boundary.md).

## Evidence and provenance

Pinned upstream identity and licensing/provenance rules remain documented in [`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md). Manifest probes and differential tests remain useful compatibility evidence; they do not re-authorize or redefine established core features.

The prior detailed manifest schema and field-by-field rationale are preserved in Git history. They describe how the current implementation evolved, not the current language architecture contract.
