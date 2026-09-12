# ADR-0001: Public Rust embedding surface

- Status: Accepted
- Date: 2026-09-12 (backfilled)
- Historical evidence: [Issue #111](https://github.com/wrightkit/opy-rs/issues/111), [PR #112](https://github.com/wrightkit/opy-rs/pull/112)

## Context

The extracted OPY implementation initially exposed compiler and macro-runtime
packages as separate crates. That package layout made ordinary Rust consumers
assemble implementation details and made the public boundary follow repository
history rather than the OverPy capability being consumed. The CLI also needs a
stable owner library without making CLI concerns part of ordinary embedding.

## Decision

`opy-rs` is the single public Rust embedding crate for the supported OverPy
implementation surface. It owns the public parse, check, inspect, compile,
diagnostic, and relevant canonical-interoperability APIs. `opy-cli` remains a
standalone executable package and consumes those APIs as a thin presentation
layer.

Compiler stages and the JavaScript macro runtime remain internal modules. They
may retain clear internal separation, but they are not independent public
dependency contracts. Common OPY compilation does not require callers to
construct `workshop-rs` implementation types; an explicitly advanced canonical
interoperability surface may expose the owner model where that is the purpose
of the integration.

## Alternatives and trade-offs

- Keep each implementation crate public: this preserves the old assembly
  shape but freezes internal decomposition and burdens consumers with it.
- Fold the CLI into `opy-rs`: this reduces package count but couples embedding
  to command-line concerns.
- Add a second facade crate: this keeps old packages alive and creates another
  public owner without improving the contract.

The chosen boundary leaves internal modularity available while making the
consumer-facing ownership and future release surface explicit.

## Consequences

Public API changes are concentrated in `opy-rs`, while internal compiler and
macro changes can evolve without publishing new implementation contracts.
Historical crate versions remain available, but future package publication and
documentation must follow the intentional owner surface. This decision does
not move canonical Workshop semantics, catalog, validation, or emission into
`opy-rs`.
