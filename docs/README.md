# opy-rs Documentation

This directory is the documentation index for `opy-rs`. The root
[`README.md`](../README.md) is the user-facing project overview; detailed
architecture, compatibility evidence, APIs, and internal contracts live here.

## Architecture and APIs

- [Implementation role](opy/implementation-role.md): standalone OverPy
  implementation identity, relationship with `workshop-rs`, and Wright
  integration terminology.
- [Architecture](opy/architecture.md): source parsing, semantic HIR,
  compiler/reconstruction boundaries, and dependency direction.
- [Tooling API](opy/tooling-api.md): Rust library and CLI contracts for checking,
  inspection, overlays, diagnostics, and support queries.
- [LPP provider](opy/provider.md): first-party provider capabilities, entry-based
  project loading, artifact boundary, and release archive contract.
- [Source-edit policy](opy/trivia-retention-policy.md): provenance and trivia
  requirements for validated source-oriented edits.

## Compatibility

- [OverPy support contract](language-support.md): audited, human-readable OverPy
  feature coverage.
- [Compatibility baseline](opy/compatibility-baseline.md): planning/reference
  inventory for remaining OverPy surface and compatibility priorities.
- [Offline conformance baseline](opy/conformance-baseline.md): independent
  oracle, failure-frontier, and canonical-WIR comparison contract.
- [Upstream references](compatibility/upstream-references.md): pinned reference
  identity, provenance, licensing notes, and oracle boundaries.
- [Compatibility harness](../compatibility/README.md): fixtures, snapshots,
  differential testing, and machine-readable support data.

## Internals

- [Opy HIR v2](hir/opy-hir-v2.md): current semantic representation and wire contract.
- [Opy HIR v1](hir/opy-hir-v1.md): prior wire contract and migration baseline.
- [Semantic compatibility manifest](opy/compat-manifest-spec.md): builtin,
  signature, alias, and catalog-link metadata owned by the OPY implementation.
- [Tooling notes](opy/tooling-notes.md): focused implementation notes that do
  not belong in the public README.

> [!NOTE]
> GitHub issues and pull requests own implementation sequencing and acceptance
> criteria. Documents here describe durable architecture, interfaces, evidence,
> or current compatibility boundaries.
