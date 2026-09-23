# opy-rs Documentation

This directory is the documentation index for `opy-rs`. The root
[`README.md`](../README.md) is the user-facing overview.

## Documentation model

```text
architecture/README.md       current architecture routing
  ├─ language-core.md        current OverPy semantic/scope contract
  └─ workshop-boundary.md    current canonical Workshop boundary
adr/                         historical architecture decisions
language-support.md          current tested support
compatibility/               pinned reference/licensing
opy/ + hir/                  public/API/provider and implementation notes
source/tests/corpus          current implementation reality
Issues / PRs / releases      mutable execution state
```

For substantive implementation work, start from
[`architecture/README.md`](architecture/README.md), then inspect the relevant
source/tests and Issue contract. Current support is established by executable
tests and [`language-support.md`](language-support.md), not by architecture
intent alone.

## Current architecture

- [Architecture routing](architecture/README.md)
- [OverPy language core](architecture/language-core.md): upstream core as the
  executable specification, semantic ownership, typed implementation, and
  feature locality.
- [OverPy / Workshop boundary](architecture/workshop-boundary.md): lowering,
  canonical WIR, reconstruction, and dependency direction.
- [Repository agent guidance](../AGENTS.md): implementation preflight,
  source attribution, validation, and delivery.
- [ADR registry](adr/README.md): historical decisions and their rationale;
  current contracts remain under `docs/architecture/`.

Legacy links to [`opy/architecture.md`](opy/architecture.md) and
[`opy/implementation-role.md`](opy/implementation-role.md) are retained as
compatibility pointers rather than separate architecture authorities.

## Compatibility and current support

- [OverPy support contract](language-support.md): current tested feature
  coverage.
- [Compatibility references](compatibility/README.md): pinned upstream identity,
  oracle boundaries, source attribution, and licensing policy.
- [OverPy compatibility harness](../tools/overpy/README.md): tests/snapshots and
  differential testing.

Tests describe implementation completeness; they do not
narrow the established upstream core-language scope.

## APIs and implementation notes

- [Tooling API](opy/tooling-api.md): standalone Rust and CLI contracts.
- [LPP provider](opy/provider.md): integration/process contract.
- [Source-edit policy](opy/trivia-retention-policy.md): source mapping/trivia
  requirements.
- [Opy HIR v2](hir/opy-hir-v2.md): current HIR representation/wire contract,
  retaining the v1 baseline except where v2 explicitly changes it.
- [Opy HIR v1](hir/opy-hir-v1.md): retained baseline used by the v2 contract.
- [Compatibility manifest implementation note](opy/compat-manifest-spec.md):
  current manifest mechanism and its architecture limitations. It is not the
  semantic authority for OverPy.
- [Tooling notes](opy/tooling-notes.md): focused implementation notes.

> [!NOTE]
> Source-language behavior is implemented directly by `opy-rs`; canonical raw
> Workshop concepts remain owned by `workshop-rs`. Wright/provider integration
> does not redefine either language boundary.
