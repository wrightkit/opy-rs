# ADR-0002: Independent offline OverPy conformance evidence

- Status: Superseded by [ADR-0006](0006-tests-first-compatibility.md)
- Date: 2026-09-12 (backfilled)
- Related history: [Issue #158](https://github.com/wrightkit/opy-rs/issues/158), [PR #159](https://github.com/wrightkit/opy-rs/pull/159)

This record preserves the historical decision. The current compatibility
contract is defined by [ADR-0006](0006-tests-first-compatibility.md).

## Context

Compatibility results derived only from the current implementation or from
coarse compile-status parity cannot distinguish a genuine language match from
two implementations failing at different stages. They also make a native
output snapshot an accidental authority for the behavior it is meant to test.

## Decision

Offline OverPy compatibility is specified by evidence independent of the native
implementation: pinned OverPy executable behavior, public language evidence,
provenance-linked projects, and reviewed Workshop contracts. The support
contract is human-readable; executable claims live in the corpus, oracle
snapshots, differential expectations and Rust integration tests rather than in
a second support database.

For reference-success inputs, compare the canonical Workshop semantics parsed
from the reference output with direct OPY lowering through the
`workshop-rs`-owned canonical contract. Text formatting, temporary names, and
emitter shape are not the compatibility authority. For reference failures,
compare the first meaningful pipeline stage and construct, not only the final
exit status. Native divergences remain generated evidence until an independent
contract changes; they are not turned into expected passes to make the suite
green.

## Alternatives and trade-offs

- Compare generated text: it is easy to inspect but overfits formatting and
  implementation choices.
- Compare only exit status: it is cheap but hides earlier native failures.
- Derive the inventory from native output: it is convenient but cannot prove
  completeness or correctness independently.

The evidence-first model costs provenance and a richer runner, but it makes a
compatibility claim falsifiable and keeps implementation work from rewriting
its own oracle.

## Consequences

Differential reports distinguish proven matches, divergences, known gaps and
inconclusive cases. The source-language oracle remains an OPY concern, while
canonical Workshop parsing, validation, normalization, and emission remain
`workshop-rs` responsibilities.
