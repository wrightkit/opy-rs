# AGENTS.md

This repository is part of the **WrightKit** multi-repository workspace. Apply
the workspace-level `AGENTS.md` first, then this repository's local ownership,
architecture, validation, and delivery rules.

`opy-rs` is WrightKit's standalone Rust implementation of the OverPy `.opy`
language. Wright is a consumer that may integrate `opy-rs` through native APIs
or LPP, while `opy-rs` must remain independently usable as a library and CLI.

## Ownership boundary

`opy-rs` owns:

- OverPy syntax, lexer, parser, source model, preprocessing, macros,
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

## Architecture routing

For substantive implementation work, resolve the relevant current contract from
[`docs/architecture/README.md`](docs/architecture/README.md) before editing.
Older documents under `docs/opy/` may describe implementation mechanisms or
historical design, but they do not override the current architecture merely
because code still follows them.

The current contracts are:

- [`language-core.md`](docs/architecture/language-core.md) for OverPy scope,
  semantic ownership, typed implementation, and feature locality;
- [`workshop-boundary.md`](docs/architecture/workshop-boundary.md) for canonical
  Workshop dependency/lowering/reconstruction boundaries.

If the Issue, current contract, and source/tests disagree materially, stop and
surface the mismatch rather than selecting a design by implementation
convenience.

## Upstream reference and provenance

For the declared OverPy core-language surface, the established upstream OverPy
implementation is the executable specification. Core behavior is presumptively
in scope unless explicitly excluded as editor/browser/integration functionality
or demonstrated to be a non-contractual implementation artifact.

Inspect upstream implementation, docs, and tests to understand behavior; then
implement the behavior directly in clear Rust and verify it through
compatibility evidence. Do not mechanically translate, import, link, or bundle
upstream implementation/data into the `opy-rs` core or release artifacts.

OverPy is GPL-3.0; this repository is AGPL-3.0-or-later. Pinned identity,
fixture/probe provenance, and licensing boundaries are documented in
[`docs/compatibility/upstream-references.md`](docs/compatibility/upstream-references.md).

Inventories, manifests, probes, differential tests, corpus fixtures, and real
projects verify completeness and compatibility; they do not decide whether an
established core feature belongs in scope.

## Semantic implementation

Prefer typed Rust for observable OverPy behavior and invariants: receiver/member
semantics, argument binding, contextual dispatch, macro/directive behavior,
coercion/evaluation rules, and special lowering.

Declarative data may carry large mechanical inventories, names, provenance, and
facts that do not themselves program language behavior. Do not extend an
existing manifest/registry with new semantic control fields merely because the
current implementation already contains similar metadata; existing
metadata-driven semantics are an audit target, not architecture precedent.

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
details. If the smallest diff would deepen an already mixed semantic
responsibility, the smallest bounded extraction needed to keep the changed
feature cohesive is in scope; unrelated cleanup remains out of scope.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
python3 -m unittest discover -s tools/overpy/tests
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
