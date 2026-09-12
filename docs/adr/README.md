# Architecture Decision Records

ADRs preserve point-in-time architecture decisions and their rationale. They
are history, not a database of current implementation reality.

Current durable architecture contracts are routed from
[`../architecture/README.md`](../architecture/README.md). Source, tests, Cargo
metadata, integrations, and releases establish current implementation reality.
An `Accepted` ADR records an approved decision; it does not by itself prove
that later implementation still conforms to it.

## Conventions

- ADR numbers are unique, zero-padded, and never reused for a different decision.
- `Proposed` means the decision was recorded but not yet accepted.
- `Accepted` means the decision was approved at that point in project history.
- `Superseded` decisions remain in place and link to the replacing decision or current contract.
- Backfilled ADRs use their actual documentation date and link the historical Issues and PRs from which the decision was reconstructed.
- Versions, support counts, Issue progress, migration state, and other mutable reality do not belong in ADR status prose.

## Index

- [ADR-0001: Public Rust embedding surface](0001-public-rust-embedding-surface.md)
- [ADR-0002: Independent offline OverPy conformance evidence](0002-independent-conformance-evidence.md)
- [ADR-0003: First-party provider ownership boundary](0003-first-party-provider-boundary.md)
- [ADR-0004: Typed semantic behavior and feature-local ownership](0004-typed-semantic-feature-locality.md)
- [ADR-0005: OPY lowering through canonical Workshop `Program`](0005-opy-canonical-program-lowering.md)

## Post-baseline audit (#250)

This registry was backfilled on 2026-09-12 from the repository split through
the current architecture contracts, with emphasis on decisions after the
Wright ADR-0010 baseline. The audit distinguishes durable decisions from
current contracts, implementation detail, and decisions owned elsewhere.

| Historical choice | Classification | Decision record |
| --- | --- | --- |
| #111 and PR #112 | Backfill required: intentional public Rust package and embedding boundary | [ADR-0001](0001-public-rust-embedding-surface.md) |
| #158 and PR #159 | Backfill required: independent oracle, stage-aware failure frontier, and canonical semantic comparison | [ADR-0002](0002-independent-conformance-evidence.md) |
| #170 and PR #171 | Backfill required: thin first-party provider over OPY-owned loading and semantics | [ADR-0003](0003-first-party-provider-boundary.md) |
| Current `language-core.md`, #202, #203/#205, #207/#210, #208/#213, #209/#214, and #221/#222 | Backfill required: upstream executable specification, typed behavior, declarative facts, and domain-local ownership | [ADR-0004](0004-typed-semantic-feature-locality.md) |
| #244/#248 and the Workshop owner decision | Backfill required for OPY consequences; canonical `Program` representation is externally owned | [ADR-0005](0005-opy-canonical-program-lowering.md), [workshop-rs ADR-0008](https://github.com/wrightkit/workshop-rs/blob/main/docs/adr/0008-canonical-public-program-boundary.md) |
| Repository extraction, source-language ownership, initial Workshop integration, and current support-contract routing (#9, #31, #32, #35, #36, #49, #63, #201) | Existing current contracts or historical implementation detail; no separate ADR required | [language core](../architecture/language-core.md), [Workshop boundary](../architecture/workshop-boundary.md) |
| Release tags, focused compatibility fixes, module/file moves, documentation cleanup, CI/build changes, and provider packaging follow-ups | Non-ADR detail or consequence; no independent durable ownership or public-boundary decision found | — |

Cross-repository decisions remain in their owner repositories. In particular,
the canonical Workshop `Program` versus internal WIR/storage boundary is
defined by [workshop-rs ADR-0008](https://github.com/wrightkit/workshop-rs/blob/main/docs/adr/0008-canonical-public-program-boundary.md);
this repository records only how OPY consumes that boundary.

No unresolved OPY architecture decision was identified in this audit. The ADRs
preserve recoverable rationale; current architecture documents remain the
authority for present boundaries, and implementation evidence must still be
checked separately.
