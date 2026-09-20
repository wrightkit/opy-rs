# ADR-0006: Tests-first compatibility verification

- Status: Accepted
- Date: 2026-09-20
- Related history: [opy-rs Issue #337](https://github.com/wrightkit/opy-rs/issues/337), [wrightkit/.github Issue #60](https://github.com/wrightkit/.github/issues/60)
- Supersedes: [ADR-0002](0002-independent-conformance-evidence.md)

## Context

OPY compatibility is a behavior requirement. The repository already has
ordinary source, compiler, regression, differential, and real-project tests,
along with pinned OverPy results and canonical Workshop comparisons. Separate
expectation tables and generic verification metadata duplicate those tests and
make the test contract harder to find.

## Decision

Each compatibility fixture owns its test input, concrete source attribution or
licensing data when required, pinned oracle snapshot, and the native
expectations needed by its source and compiler tests. The source and compiler
runners read those declarations directly from the fixture manifest.

OverPy remains the pinned reference implementation. Its snapshots are test
data; canonical Workshop comparisons, diagnostic-code checks, normalized
output checks, and source status comparisons remain ordinary test assertions.
Generated reports are run artifacts under `target/` or CI artifacts and are
not current test metadata.

Source mapping remains part of diagnostics and HIR where consumers need it.
Third-party source attribution, licensing, immutable revisions, checksums, and
reproducibility details remain only where the corresponding workflow needs
them.

## Consequences

Contributors can understand a compatibility check as fixture input plus native
execution, pinned reference result, and assertion. There is no separate
evidence manifest, generic provenance schema, expectation owner field, or
task-history table to maintain. The pinned oracle and canonical Workshop
ownership remain unchanged.
