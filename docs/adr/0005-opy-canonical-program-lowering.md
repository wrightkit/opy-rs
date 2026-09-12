# ADR-0005: OPY lowering through canonical Workshop `Program`

- Status: Accepted
- Date: 2026-09-12 (backfilled)
- Historical evidence: [OPY Issue #244](https://github.com/wrightkit/opy-rs/issues/244), [OPY PR #248](https://github.com/wrightkit/opy-rs/pull/248), [Workshop Issue #179](https://github.com/wrightkit/workshop-rs/issues/179), [Workshop PR #185](https://github.com/wrightkit/workshop-rs/pull/185)
- Owner decision: [workshop-rs ADR-0008](https://github.com/wrightkit/workshop-rs/blob/main/docs/adr/0008-canonical-public-program-boundary.md)

## Context

OPY lowering needs a stable target contract without managing Workshop arena
allocation, storage-node IDs, or compatibility-only representation details.
The canonical Workshop public `Program` boundary was established in
`workshop-rs`; the representation decision itself belongs there, not in this
repository.

## Decision

OPY resolves OverPy meaning and applies OPY-specific lowering policy locally,
then constructs the canonical Workshop `Program`, `Value`, `Action`, `Event`,
and settings concepts through the public owner API. OPY must not recreate a
local WIR/storage/node-ID model and convert it afterward.

OPY retains ownership of helper/index allocation, preprocessing/macros,
source-language diagnostics, and provenance mapping. Where source provenance
is needed, OPY consumes the optional public provenance contract supplied by
`workshop-rs`. Workshop owns canonical semantics, validation, catalog,
localization/settings, and emission. Compatibility evidence may compare
canonical semantics, but it does not make Workshop's internal WIR/storage a
public OPY dependency.

## Alternatives and trade-offs

- Keep a local OPY WIR mirror and adapt later: it appears to isolate migration
  risk but duplicates canonical representation and can silently drop semantics
  or provenance.
- Move OPY lowering policy into `workshop-rs`: it simplifies one call site but
  violates source-language ownership and couples Workshop to OPY.
- Depend directly on Workshop storage APIs: it may expose more mechanics but
  freezes internal allocation and node identity as a consumer contract.

The chosen boundary makes the Workshop owner API the integration seam while
keeping source-language decisions local and preserving direct provenance paths.

## Consequences

Changes required to express canonical Workshop semantics belong in
`workshop-rs` first. OPY consumer work can validate that it uses the public
boundary without claiming ownership of the underlying representation. Current
architecture is documented in the [OPY/Workshop boundary contract](../architecture/workshop-boundary.md).
