# opy-rs implementation role

`opy-rs` is an independently usable Rust implementation of the OverPy language.
Its product boundary is larger than a parser/frontend and larger than an LPP
provider process.

## Durable model

```text
OPY source
  ↓
opy-rs frontend
  ↓
OPY semantic model / HIR
  ↓
opy-rs lowering / compiler behavior
  ↓
workshop-rs canonical WIR / validation / emission
  ↓
Workshop text
```

For the reverse direction:

```text
Workshop text
  ↓
workshop-rs parser / canonical WIR
  ↓
opy-rs reconstruction
  ↓
OPY source
```

`opy-rs` therefore owns the OverPy-specific semantics on both sides of the
Workshop boundary. It does not need to reimplement raw Workshop to be a complete
OverPy implementation; it deliberately reuses the canonical Workshop
implementation in `workshop-rs`.

## Terminology

### Frontend

A frontend is an internal stage: source text → parsed/source model → semantic
model/HIR. The frontend is intentionally Workshop-independent so diagnostics,
semantic queries, source tooling, and other non-emission workflows do not need
the compiler backend.

Do not use **frontend** as shorthand for the repository's overall product role.

### Provider

A provider is a process/API role through which an implementation can expose
language intelligence to a tooling client such as Wright. LPP may be one such
boundary. Provider support is an integration capability; it does not make
`opy-rs` subordinate to Wright and it must not force standalone users through
Wright.

### Wright

Wright is a downstream integration/tooling product. It combines `opy-rs`,
`del-rs`, and `workshop-rs` with additional cross-language capabilities such as
lint, analysis, source-edit transactions, agent tooling, CI presentation,
embedding, and language services.

## Ownership

`opy-rs` owns OverPy syntax, preprocessing, macros, semantic resolution,
OverPy-specific compiler behavior, diagnostics/provenance, standalone tooling,
compatibility evidence, and Workshop→OPY reconstruction.

`workshop-rs` owns raw Workshop parsing, canonical Workshop identities and
semantics, WIR, validation, settings/localization data, and emission.

The dependency direction is `opy-rs → workshop-rs`; there is no dependency from
`workshop-rs` back to OPY semantics.

## Current reality

The repository already exposes standalone check/inspect/support tooling. OPY →
Workshop compilation is only partially implemented, and Workshop → OPY
reconstruction is not yet implemented. These are implementation-completeness
gaps, not reasons to redefine `opy-rs` as a frontend-only repository.

Support claims must continue to follow the compatibility matrix and executable
evidence rather than this architectural intent alone.
