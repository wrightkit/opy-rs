# ADR-0003: First-party provider ownership boundary

- Status: Accepted
- Date: 2026-09-12 (backfilled)
- Historical evidence: [Issue #170](https://github.com/wrightkit/opy-rs/issues/170), [PR #171](https://github.com/wrightkit/opy-rs/pull/171), [LPP entry-loading contract](https://github.com/wrightkit/language-provider-protocol/issues/16)

## Context

Wright needs an independently releasable OPY provider for real project
workflows. A provider that scans projects, reimplements preprocessing, or
reconstructs compiler semantics would create a second OPY implementation and
make diagnostics and source identity diverge from the standalone library.

## Decision

The first-party LPP provider is a thin owner process over `opy-rs`. It accepts
the selected entry through the approved LPP contract and delegates project
loading, `#!mainFile`, reachable includes, preprocessing, macros, source
semantics, diagnostics/provenance, and compilation to `opy-rs`. It advertises
only the capabilities its implementation supports and returns structured
protocol results without exposing OPY AST/HIR or Rust implementation types.

LPP owns the wire and process contract; Wright owns product orchestration and
integration. Neither is an alternate home for OPY language behavior. Canonical
Workshop semantics and emission remain owned by `workshop-rs`.

## Alternatives and trade-offs

- Let Wright load and compose OPY files: this centralizes product code but
  duplicates owner semantics and loses the standalone/provider identity.
- Implement provider-specific parsing or lowering: this may unblock one
  request quickly but creates incompatible behavior and two maintenance paths.
- Expose internal compiler structures over LPP: this reduces adapter code but
  freezes private Rust representation on a wire boundary.

The thin provider requires explicit owner capabilities and a narrow protocol,
but allows OPY releases and real-project support to advance independently of
Wright releases.

## Consequences

Provider acceptance must verify the owner path with structured diagnostics,
source closure, and successful/failed compile behavior on real projects.
Protocol versions, entry loading, process lifecycle, and artifact wire shape
must be documented in LPP and consumed by the provider rather than recreated
locally.
