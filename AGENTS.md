# AGENTS.md

This repository is part of the **WrightKit** multi-repository workspace. Apply
the workspace-level `AGENTS.md` first, then this repository's local ownership,
architecture, validation, and delivery rules.

`opy-rs` is WrightKit's standalone Rust implementation of the OverPy `.opy`
language. It is not an internal Wright frontend repository. Wright is a
consumer that may integrate `opy-rs` through native APIs or LPP, while
`opy-rs` must remain independently usable as a library and CLI.

Terminology:

- **frontend** means the internal Workshop-independent source → syntax →
  semantic/HIR stage inside `opy-rs`;
- **provider** means an integration role exposed through LPP or another
  reviewed boundary;
- neither term replaces the repository's identity as an independent OverPy
  implementation.

## Ownership boundary

`opy-rs` owns:

- OverPy syntax, lexer, parser, CST/source model, preprocessing, macros,
  semantic resolution, diagnostics, provenance, and OPY HIR;
- OverPy-specific compiler/lowering semantics and backend-affecting behavior;
- standalone OPY tooling APIs and CLI surfaces;
- Workshop → OPY reconstruction when implemented;
- OPY compatibility evidence and support claims.

`workshop-rs` owns:

- canonical raw Workshop semantics and catalog identities;
- Workshop WIR, validation, settings/localization data, parser, and emitter;
- Workshop-observable contracts shared by all source-language implementations.

The durable dependency direction is:

```text
opy-rs → workshop-rs
```

Never copy canonical Workshop catalog, WIR, emitter, settings, or locale data
into this repository. Never add a dependency from `workshop-rs` back to
`opy-rs` merely to simplify integration.

The standalone source-analysis path must remain usable without requiring
Workshop emission. Compilation may depend on `workshop-rs`; `check`, semantic
inspection, source queries, and other Workshop-independent operations should
not be forced through the compiler pipeline without an evidence-backed need.

Do not invent WrightKit-only OPY syntax. Compatibility is observable semantics,
not output-text identity, optimizer implementation, formatting, temporary
variables, or upstream internal architecture.

See [`docs/opy/implementation-role.md`](docs/opy/implementation-role.md) for the
repository/product relationship and [`docs/opy/architecture.md`](docs/opy/architecture.md)
for implementation details.

## Upstream reference and provenance

OverPy is the pinned compatibility oracle (see
`docs/compatibility/upstream-references.md`). It is GPL-3.0; this repository is
AGPL-3.0-or-later. OverPy implementation or data (compiler sources,
`src/data/*` tables, internal AST/types, generated artifacts) is never imported
into, linked to, or bundled with the `opy-rs` core or release artifacts.
Reviewed upstream example/test fixtures may be retained only as isolated oracle
evidence with explicit provenance and licensing records.

## Development priority

Prioritize real OverPy project usability over architecture polish. When a real
project exposes a blocker:

1. reproduce it against the standalone `opy-rs` tooling;
2. fix it here if the missing behavior is OverPy-owned;
3. route missing canonical Workshop behavior to `workshop-rs`;
4. preserve a minimized regression where practical while keeping the full
   project evidence;
5. do not split implementation work into smaller issues solely for bookkeeping
   when one coherent change can be reviewed and validated safely.

Internal module layout and helper abstractions are revisable implementation
details unless they affect a public/versioned contract, repository ownership,
source provenance, or compatibility correctness.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
python3 -m unittest discover -s compatibility/tests
```

Oracle-required compatibility probes run separately against the pinned
reference. A local test count is not sufficient evidence when a change claims
real-project compatibility; rerun the affected full-project workflow.

## Delivery

- Never push directly to `main`; develop on independent branches and deliver
  through PRs.
- Keep commits focused and issue-linked where an issue exists.
- Keep compatibility/support claims synchronized with actual implementation and
  evidence.
- Never commit credentials, private runtime data, or unreviewed third-party
  material.
