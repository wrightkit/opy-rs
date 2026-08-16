# AGENTS.md

This repository is part of the **WrightKit** multi-repository workspace. Apply
the workspace-level `AGENTS.md` first, then this repository's local ownership,
architecture, validation, and delivery rules.

Within WrightKit, `opy-rs` is the OPY language provider: it owns OverPy `.opy`
syntax, lexer, parser, CST, preprocessing, macros, semantic resolution, and the
OPY semantic model (Opy HIR). It does **not** own canonical Workshop semantics,
catalog, WIR, emitter, or locale data — those belong to `workshop-rs`.

## Ownership boundary

- OPY source → OPY CST/AST → OPY semantic model lives here, fully
  Workshop-independent.
- The integration boundary toward `workshop-rs` is documented, not implemented
  here. Workshop-dependent features are classified `lowering-dependent` in the
  support matrix, never approximated with a temporary Workshop IR.
- Do not copy Workshop catalog, WIR, emitter, or locale data into this
  repository.
- Do not invent WrightKit-only OPY syntax.
- Compatibility is observable semantics, not output-text identity, optimizer
  implementation, or upstream internal architecture.

## Upstream reference and provenance

OverPy is the pinned compatibility oracle (see
`docs/compatibility/upstream-references.md`). It is GPL-3.0; this repository
is AGPL-3.0-or-later. OverPy implementation or data — compiler sources,
`src/data/*` tables, internal AST/types, generated artifacts — is never
imported into, linked to, or bundled with the opy-rs core (`crates/`) or
release artifacts. Provenance/license/redistribution-reviewed upstream
example and test fixtures (e.g. the GPL-3.0 OverPy `examples/` corpus) may be
retained under `compatibility/fixtures/` as documented, isolated oracle
evidence, with per-file provenance records; they are never imported by core
code and never bundled into core builds or releases. Fixtures and derived
evidence must carry provenance; unclear-provenance or unlicensed content is
prohibited.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
python3 -m unittest discover -s compatibility/tests   # oracle-free harness checks
```

CI runs the Rust quality gates on Ubuntu (Rust stable and 1.85.0) and the
`opy-macro-js` runtime suite on macOS and Windows on every push/PR. The
oracle-free compatibility harness tests run as standalone validation (they
are not part of CI); oracle-required steps (`compatibility/run_oracle.py`,
the manifest probe validator) run locally against the pinned oracle. Do not
merge work that fails these checks.

## Delivery

- Never push directly to `main`; develop on independent branches and deliver
  Draft PRs per change.
- Keep commits focused and issue-linked; do not mix unrelated changes.
- Keep issue/PR state and the support matrix in sync with the code:
  `compatibility/support-matrix.json` is the single mechanically checked
  state source (`docs/opy/support-matrix.md` describes it and links to it).
