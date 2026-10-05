# OPY compatibility manifest — implementation note

The compatibility manifest remains part of the current `opy-rs` implementation, but it is **not** the semantic authority for the OverPy language.

Current architecture is defined by [`docs/architecture/language-core.md`](../architecture/language-core.md). For the declared core-language surface, upstream OverPy behavior is the executable specification; inventories, manifests, probes, differential tests, corpus fixtures, and real projects verify completeness and compatibility.

## Current implementation reality

The manifest under `crates/opy-rs/src/manifest/` carries declarative compatibility data:

- identities, names, aliases, signatures (including parameter defaults, optionality,
  binding spellings, and enum-domain links), catalog links, and source attribution.

Typed feature-local lowering policy owns behavioral contextual dispatch, call-context
restrictions, and special argument/receiver requirements: currently `chase` selector
dispatch, `range`'s for-iterable-only rule, the chase family's variable first argument,
and the `.append`/`.remove`/`.format` receiver requirements live in
`crates/opy-rs/src/lower/policy.rs`. The manifest's `param.variable` flag and member
`receiver` category remain descriptive signature metadata; they do not select that
enforcement.

Declarative inventories remain data-driven. Observable source-language behavior and
invariants belong in typed Rust close to the owning semantic feature rather than a
generic metadata-interpreted semantic language.

In particular, adding another manifest field that makes generic code decide receiver/member semantics, keyword/positional binding, contextual dispatch, compile-time behavior, coercion, or special lowering requires an explicit architecture justification; existing fields are not sufficient precedent.

## Workshop boundary

Manifest catalog links may refer to canonical Workshop identities, but `opy-rs` does not own Workshop catalog membership, enum member lists, localization, settings content, WIR, validation, or emission. Those remain `workshop-rs` responsibilities.

OverPy-specific names, aliases, special forms, contextual behavior, and compiler policy remain `opy-rs` responsibilities even when they eventually lower to a canonical Workshop identity.

See [`docs/architecture/workshop-boundary.md`](../architecture/workshop-boundary.md).

## Reference tests and attribution

Pinned upstream identity, source attribution, and licensing rules remain
documented in [`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md).
Manifest probes and differential tests are ordinary compatibility tests; they
do not re-authorize or redefine established core features.

The prior detailed manifest schema and field-by-field rationale are preserved in Git history. They describe how the current implementation evolved, not the current language architecture contract.
