# OverPy / Workshop Boundary

This document defines the current boundary between the OverPy implementation and canonical Workshop.

## Dependency direction

```text
OPY source
   ↓
opy-rs source + semantic implementation
   ↓
opy-rs-specific lowering / compiler behavior
   ↓
workshop-rs canonical WIR / validation / emission
   ↓
Workshop text
```

For reconstruction:

```text
Workshop text
   ↓
workshop-rs parser / canonical WIR
   ↓
opy-rs reconstruction
   ↓
OPY source
```

The durable Rust dependency direction is `opy-rs → workshop-rs`. `workshop-rs` never depends back on OverPy semantics.

## Ownership at the boundary

`opy-rs` resolves the complete OverPy meaning of a source construct before crossing the canonical Workshop boundary. This includes source aliases, member calls, directives, contextual semantics, and OverPy-specific lowering choices.

`workshop-rs` owns only the resulting canonical Workshop concepts: WIR, catalog identities, raw Workshop validation, settings/localization, and emission.

The ordinary consumer boundary is the public `workshop-rs::Program` model;
arena-backed WIR/storage and node-ID mechanics are internal to `workshop-rs`,
as recorded by [workshop-rs ADR-0008](https://github.com/wrightkit/workshop-rs/blob/main/docs/adr/0008-canonical-public-program-boundary.md).
OPY may use canonical semantic APIs and approved provenance access, but must
not recreate or depend on that internal storage representation.

Do not move OverPy names, aliases, contextual dispatch records, compiler helper identities, or reconstruction carriers into canonical Workshop merely to simplify compilation.

If OverPy behavior needs a Workshop primitive that is genuinely missing from the canonical Workshop model, establish the Workshop requirement in `workshop-rs` first. If no semantically correct Workshop lowering exists, `opy-rs` reports an explicit unsupported boundary.

## Tooling path

The Workshop-independent path remains:

```text
source → preprocessing → parsing → semantic model / HIR
```

`check`, semantic inspection, source query, and source-aware edit foundations should not require full Workshop emission unless the requested operation actually depends on target Workshop semantics.

## Reconstruction

Workshop→OPY targets semantically equivalent, useful OverPy source. It cannot recover macros, comments, formatting, abstractions, names, or other source information already erased by Workshop text/WIR and does not promise literal source recovery.

## Wright/provider integration

Wright and LPP are consumers/integration surfaces. They do not own OverPy syntax or semantics and must not compensate for missing `opy-rs` behavior by implementing a parallel OPY language path.
