# opy-rs implementation role

`opy-rs` is an independently usable Rust implementation of the OverPy language.
Its product boundary includes parsing, preprocessing, semantic HIR, compiler
integration, diagnostics, tooling, and reconstruction, in addition to any LPP
provider process.

## Durable model

```text
OPY source
  ↓
opy-rs parsing / preprocessing / semantic HIR
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

## Provider

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

The repository exposes standalone check/inspect/support tooling and a bounded
versioned OPY → Workshop compile surface. Broader corpus gaps remain explicit,
and Workshop → OPY reconstruction is not yet implemented. These are
implementation-completeness gaps, not reasons to narrow `opy-rs` to one compiler stage.

Support claims must continue to follow the compatibility matrix and executable
evidence rather than this architectural intent alone.
