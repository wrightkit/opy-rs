# opy-rs Architecture

Status: accepted living architecture.

`opy-rs` is the standalone Rust implementation of OverPy in WrightKit. This
document describes its internal stages and its boundary with the canonical raw
Workshop implementation in `workshop-rs`. See
[`implementation-role.md`](implementation-role.md) for repository/product
terminology and [`support-matrix.md`](support-matrix.md) for current executable
coverage.

## Pipeline

```text
OPY source
   ↓
lexer / preprocessing / macros
   ↓
parser / source model
   ↓
semantic resolution
   ↓
OPY semantic model / HIR
   ├─→ check / inspect / source tooling
   │
   └─→ opy-compiler
          ↓
       OPY-specific lowering
          ↓
       workshop-rs canonical WIR
          ↓
       Workshop validation / emission
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

The first path is partially implemented end-to-end today; the reverse path is
not yet implemented. The support matrix, tests, and real-project evidence are
the authority for current support rather than the intended pipeline alone.

## Workshop-independent source path

The Workshop-independent source path in `opy-rs` is:

```text
source → preprocessing → parser → semantic model / HIR
```

It owns source syntax and meaning and is deliberately usable without complete
Workshop emission. This allows diagnostics, semantic queries, source-aware
analysis, and validated edit foundations to work without forcing every tooling
request through the compiler backend.


## Ownership

`opy-rs` owns:

- OverPy syntax and lexical/parser behavior;
- `#!include`, `#!define`, `#!undef`, settings syntax, macros, and bounded
  JavaScript macro execution;
- semantic resolution, OPY signatures/aliases/contextual dispatch, diagnostics,
  and source provenance;
- Opy HIR and OverPy-specific semantic identities;
- OverPy-specific compiler/lowering behavior;
- supported backend-affecting directives and post-compile-hook behavior;
- standalone `check`, `inspect`, support, compiler, and future reconstruction
  APIs/CLI surfaces;
- OPY compatibility evidence and Workshop→OPY reconstruction.

`workshop-rs` owns:

- canonical Workshop identities and catalog membership;
- raw Workshop parser and canonical WIR;
- Workshop validation and emission;
- settings schema/content, localization, and Workshop-owned gameplay/catalog
  data;
- Workshop conformance and client-side evidence.

Do not copy Workshop-owned tables, locale data, WIR, or emitter behavior into
`opy-rs`. A missing canonical Workshop capability is fixed in `workshop-rs`;
an OverPy-specific interpretation or lowering remains here.

## Dependency direction

```text
opy-rs
    ↓
opy-compiler
    ↓
workshop-rs
```

`opy-rs` remains independently buildable and usable for the semantic
workflows that do not require canonical Workshop output. `opy-compiler` is the
Workshop-dependent integration layer.

There is no dependency from `workshop-rs` back to OPY semantics, and Wright
integration must not become a dependency of this implementation.

## Semantic manifest and canonical identities

The OPY semantic manifest owns OverPy-facing function/member identity,
signatures, aliases, contextual dispatch, and links to canonical Workshop ids.
It does not own the Workshop catalog itself.

Where a construct maps to canonical Workshop behavior, compilation resolves the
link against `workshop-rs`. OverPy special forms without a direct catalog
identity remain `opy-rs` lowering responsibilities and must either be
implemented from evidence or fail explicitly.

## Compiler boundary

`opy-compiler` consumes resolved Opy HIR and the validated semantic manifest,
then produces canonical `workshop-rs` WIR with source provenance.

The compiler must:

- preserve source attribution across generated WIR where the public contract
  supports it;
- validate canonical ids through `workshop-rs` rather than local allowlists;
- fail with structured diagnostics for unsupported lowering instead of silently
  dropping or guessing nodes;
- treat output formatting, temporary allocation, and optimizer shape as
  non-contractual unless they change observable semantics;
- execute `#!postCompileHook` only against real final emitted Workshop text when
  that behavior is supported.

## Reconstruction boundary

Workshop→OPY is owned by `opy-rs`, but consumes canonical Workshop semantics
from `workshop-rs`. Reconstruction targets valid, useful OverPy with semantic
equivalence; it does not promise recovery of information already lost in raw
Workshop such as original macros, comments, formatting, source abstractions, or
names.

Do not introduce a second raw Workshop parser/catalog merely to implement
reconstruction.

## Stable contracts

- **Observable semantic compatibility, not output identity.** Formatting,
  temporary variables, optimizer internals, and text shape are evidence only
  unless they affect a declared observable contract.
- **Corpus-defined support.** Current support is derived from fixtures,
  real-project evidence, pinned reference observations, and machine-readable
  support state.
- **No WrightKit-only OPY dialect.** `opy-rs` follows the OverPy language rather
  than inventing source syntax for Wright convenience.
- **Source-aware edits by default.** Tooling uses semantic identities,
  provenance, and authored source spans; whole-file regeneration is not the
  default mutation model.
- **Explicit unsupported behavior.** Incomplete lowering or evidence gaps remain
  diagnostics/support states rather than guessed semantics.

## Provider integration

An LPP provider process is one optional integration role for `opy-rs`. It can
expose the implementation's semantic capabilities to Wright or other clients
without exposing internal AST/HIR types. LPP does not define the architecture
of `opy-rs`, and standalone library/CLI users do not need Wright.

## Current capability

The Workshop-independent semantic/tooling foundation is implemented and
corpus-backed. The OPY→Workshop compiler path exists but broader builtin/member,
control-flow, settings/locale, directive/hook, and real-project closure remains
partial. Workshop→OPY reconstruction is not yet implemented.

Do not infer a stronger claim from this document; use the support matrix and
current executable evidence.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
python3 -m unittest discover -s compatibility/tests
```

Oracle-required compatibility probes run separately against the pinned
reference. Real-project support claims require rerunning the affected project in
addition to focused tests.

## Authoritative references

- [`implementation-role.md`](implementation-role.md) — repository identity and
  relationship to Wright/workshop-rs.
- [`support-matrix.md`](support-matrix.md) — human-readable current support.
- [`../../compatibility/support-matrix.json`](../../compatibility/support-matrix.json)
  — machine-readable support state.
- [`compat-manifest-spec.md`](compat-manifest-spec.md) — OPY semantic manifest.
- [`../hir/opy-hir-v2.md`](../hir/opy-hir-v2.md) — current Opy HIR contract.
- [`tooling-api.md`](tooling-api.md) — standalone semantic/tooling API.
- [`../compatibility/upstream-references.md`](../compatibility/upstream-references.md)
  — pinned reference and provenance.
