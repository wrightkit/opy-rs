# opy-rs

`opy-rs` is WrightKit's Rust implementation of the OverPy `.opy` language.
It is a standalone, Workshop-independent library and CLI: it parses, checks,
inspects, and resolves `.opy` projects into a semantic model, with structured
diagnostics and full source provenance. No Node, OverPy, Workshop backend, or
catalog is required to build or run it.

Pipeline: `OPY source → lexer → preprocess → CST/parser → semantic resolution
→ OPY semantic model`, with a documented integration boundary toward
`workshop-rs` for canonical Workshop lowering and emission. See
[`docs/opy/architecture.md`](docs/opy/architecture.md) for the full
architecture, ownership boundary, and stable contracts.

## What is implemented

The standalone frontend foundation is CI-covered:

* a full Workshop-independent pipeline from `.opy` source to a semantic
  model, with structured, source-located diagnostics and full source
  provenance (spans, file registry, include/macro attribution);
* preprocessing: `#!include`, `#!define` (object- and function-like),
  `#!undef`, `#!postCompileHook` (record-only), and settings blocks;
* OverPy-compatible `__script__("…")` JavaScript macros through the bounded
  embedded QuickJS-NG runtime (`crates/opy-macro-js`; no Node);
* the OPY semantic compatibility manifest
  ([`compat-manifest-spec.md`](docs/opy/compat-manifest-spec.md)) with
  oracle-validated probes: builtin action/value/member identities,
  signatures, aliases, and `catalogId` links;
* the 26-fixture compatibility corpus
  ([`compatibility/`](compatibility/README.md)) with pinned OverPy 9.7.10
  oracle snapshots and the native differential suite.

## Library and CLI surfaces

* `crates/opy-frontend`: `compile`/`compile_with_overlay_outcome` and the
  Workshop-independent tooling API (`opy_frontend::tooling`:
  `check`/`check_with_overlay` → `CheckOutcome` with diagnostics, semantic
  model, and file registry; `opy_frontend::support` exposes the embedded
  support matrix). See [`docs/opy/tooling-api.md`](docs/opy/tooling-api.md).
* `crates/opy-cli`: `opy-cli check|inspect|support|version`. Example:

  ```sh
  opy-cli check main.opy       # diagnostics → stderr; exit 0 clean / 1 diagnostics
  opy-cli inspect main.opy     # resolved semantic model as JSON
  opy-cli support --json       # embedded support matrix (or a filtered slice)
  ```

## Compatibility

Compatibility is **observable semantic compatibility for the declared
support surface**, not byte/text, optimizer, or format identity. What matters
is whether the same `.opy` project is accepted, what diagnostics it produces,
and what the semantic model says about it. A presentation difference (for
example `Global.<name>` vs a bare variable name in emitted text) only matters
when it changes observable semantics.

| Capability | Status | Notes |
| --- | --- | --- |
| Core syntax & control flow | ✅ Supported | Lexing, expressions, assignments, `if`/`elif`/`else`, `for`/`while`, settings blocks |
| Declarations | ✅ Supported | `globalvar`/`playervar`, `subroutine`, `def`, `enum`, `macro` |
| Preprocessing & macros | ✅ Supported | `#!include`, `#!define`, `#!undef` |
| JavaScript macros | ✅ Supported | `#!define name(...) __script__("...")` with a bounded embedded runtime; no Node |
| Rules & directives | ✅ Supported | `rule` blocks, `@Event`, `@Condition`, bare `@Team`/`@Slot` |
| Builtin actions & values | 🟡 Partial | The declared subset works; the full OverPy surface is not implemented yet |
| Receiver/member functions | 🟡 Partial | Declared members work; the full member surface is not implemented yet |
| Enums & constants | 🟡 Partial | Declared enum domains resolve as opaque values; the full domain surface is not implemented yet |
| `switch` / string modifiers | ⏳ Not yet | |
| Advanced directives, translations & optimization controls | ⏳ Not yet | `#!translations`, the `#!optimize` family, `#!mainFile`, and similar |
| OPY → Workshop compilation | ⏳ Not yet | Integrated through `workshop-rs` |
| Workshop → OPY reconstruction | ⏳ Not yet | Integrated through `workshop-rs` |

Per-feature evidence and the machine-readable matrix live in
[`docs/opy/support-matrix.md`](docs/opy/support-matrix.md) and
[`compatibility/support-matrix.json`](compatibility/support-matrix.json).

Stable contracts:

* **Corpus-defined support.** Every declared feature is backed by the
  compatibility corpus or explicitly marked as investigation.
* **No WrightKit-only OPY dialect.** The surface targets the pinned OverPy
  9.7.10 reference; deviations are documented, corpus-evidenced differences,
  not new dialect features.
* **Source-aware validated edits.** Tooling operates on authored source
  ranges with full provenance and validates before editing, instead of
  regenerating whole files
  ([`trivia-retention-policy.md`](docs/opy/trivia-retention-policy.md)).

## Current limitations

* The full builtin action/value, receiver/member, and enum/constant-domain
  surfaces are not implemented yet; only the declared subset works today.
  `switch`/`case`, string modifiers, advanced preprocessing directives,
  translations, and optimization controls are not implemented yet either
  ([`compatibility-baseline.md`](docs/opy/compatibility-baseline.md)).
* Workshop lowering, emission, catalog validation, locale data, and
  `#!postCompileHook` execution against the final Workshop text require the
  `workshop-rs` integration layer and are not part of this repository today.
  `opy-rs` never approximates Workshop-owned behavior with a temporary
  implementation.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
python3 -m unittest discover -s compatibility/tests   # oracle-free harness checks
```

CI additionally runs the `opy-macro-js` runtime suite on macOS and Windows.
Oracle-required steps (`compatibility/run_oracle.py`, the manifest probe
validator) run standalone.

## Documentation

* [`docs/opy/architecture.md`](docs/opy/architecture.md): pipeline, ownership
  boundary, stable contracts, readiness.
* [`docs/opy/support-matrix.md`](docs/opy/support-matrix.md): declared
  corpus-evidenced surface.
* [`docs/opy/compatibility-baseline.md`](docs/opy/compatibility-baseline.md):
  tiered planning for the remaining surface.
* [`docs/opy/compat-manifest-spec.md`](docs/opy/compat-manifest-spec.md):
  semantic manifest schema and ownership boundary.
* [`docs/hir/opy-hir-v1.md`](docs/hir/opy-hir-v1.md): the Opy HIR v1 wire
  contract.
* [`docs/opy/tooling-api.md`](docs/opy/tooling-api.md): library API and CLI
  contract.
* [`compatibility/README.md`](compatibility/README.md): corpus and harness
  layout.

This repository is part of the WrightKit multi-repository workspace. Follow
the workspace-level `AGENTS.md` first, then this repository's local rules.

License: AGPL-3.0-or-later (see `LICENSE`).
