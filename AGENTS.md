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
`docs/compatibility/upstream-references.md`). It is GPL-3.0; this repository is
AGPL-3.0-or-later and never links to, bundles, or copies OverPy source. Oracle
use is limited to documented, isolated evaluation through the compatibility
harness. Fixtures and derived evidence must carry provenance.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
```

CI runs these on every push/PR. Do not merge work that fails them.

## Delivery

- Never push directly to `main`; develop on independent branches and deliver
  Draft PRs per change.
- Keep commits focused and issue-linked; do not mix unrelated changes.
- Keep issue/PR state and the support matrix (`docs/opy/support-matrix.md`) in
  sync with the code.
