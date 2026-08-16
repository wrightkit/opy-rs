# opy-rs

Standalone, Workshop-independent Rust frontend for the OverPy `.opy` source
language. It is the OPY language provider within WrightKit: it parses,
checks, inspects, and resolves OverPy `.opy` projects into a
Workshop-independent semantic model (Opy HIR), and exposes that surface as a
library API and a CLI. No Node, OverPy, Workshop backend, or catalog is
required to build or run it.

Pipeline: `OPY source → lexer → preprocess → CST/parser → semantic resolution
→ OPY semantic model (Opy HIR v1)`, with a documented integration boundary
toward `workshop-rs` for canonical Workshop lowering and emission. See
[`docs/opy/architecture.md`](docs/opy/architecture.md) for the full
architecture, ownership boundary, and stable contracts.

## What is implemented

The standalone frontend foundation is CI-covered:

* the Workshop-independent frontend pipeline to Opy HIR v1, with structured,
  source-located diagnostics and full source provenance (spans, file
  registry, include/macro attribution);
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

Compatibility means **observable semantic compatibility for the declared
support surface**, not byte/text, optimizer, or format identity: the
accepted/rejected surface, diagnostics, and the semantic model are the
contract; presentation differences are not bugs unless they change
observable semantics. The declared surface is the machine-readable
[`compatibility/support-matrix.json`](compatibility/support-matrix.json)
(35 entries), backed by the corpus under `compatibility/fixtures/`, the
tiered [`docs/opy/compatibility-baseline.md`](docs/opy/compatibility-baseline.md),
and the pinned OverPy 9.7.10 oracle
([`docs/compatibility/upstream-references.md`](docs/compatibility/upstream-references.md)).

| Capability | Matrix scope | State |
| --- | --- | --- |
| Frontend pipeline (lexing, expressions, declarations, control flow) | `compilation/frontend-pipeline`, `syntax/lexing`, `syntax/expressions`, `syntax/declarations`, `syntax/assignments-control-flow` | frontend-supported |
| Settings blocks | `syntax/settings-blocks` | frontend-supported |
| Preprocessing (include / define / undef) | `preprocessing/include`, `preprocessing/define-undef` | frontend-supported |
| JavaScript macros and runtime hooks | `macros/definitions`, `macros/javascript`, `runtime/js-hooks` | frontend-supported |
| Rule directives and model | `directives/rule-annotations`, `directives/rule-model` | frontend-supported |
| Structured diagnostics | `semantics/diagnostics` | frontend-supported |
| Declaration resolution, aliases, modules, keyword arguments | `semantics/declaration-resolution`, `semantics/for-binder`, `semantics/aliases`, `semantics/modules`, `semantics/keyword-arguments` | semantic-supported |
| Full builtin actions/values surface | `semantics/builtin-actions-values` | planned |
| Receiver/member functions (full surface) | `semantics/receiver-members` | planned |
| Enum/constant domains (full surface) | `semantics/enum-domains` | planned |
| `switch` / string modifiers / advanced directives / translations / optimization controls | `syntax/switch`, `syntax/string-modifiers`, `preprocessing/advanced-directives`, `translations/directive`, `optimization/controls` | planned |
| Workshop lowering, emission, catalog, settings/locale emission | `compilation/workshop-lowering`, `compilation/end-to-end`, `semantics/settings-emission`, `translations/locale-emission`, `optimization/emission-form`, `hooks/post-compile-workshop` | lowering-dependent |
| Workshop → OPY reconstruction | `decompilation/*` | lowering-dependent |

Matrix snapshot (source of truth: `compatibility/support-matrix.json`):
14 `frontend-supported`, 5 `semantic-supported`, 8 `planned`, 8
`lowering-dependent`, 0 `end-to-end-supported`.

Stable contracts:

* **Corpus-defined support.** Every declared feature is backed by the
  compatibility corpus or explicitly marked as investigation; the support
  matrix is the single mechanically checked state source, with states
  `planned`, `frontend-supported`, `semantic-supported`,
  `lowering-dependent`, `end-to-end-supported`.
* There is **no WrightKit-only OPY dialect**: the surface targets the pinned
  OverPy 9.7.10 reference, and deviations are documented, corpus-evidenced
  differences, not new dialect features.
* The default tooling model is **source-aware validated edits**: tooling
  operates on authored source ranges with full provenance and validates
  before editing, rather than regenerating whole files
  ([`trivia-retention-policy.md`](docs/opy/trivia-retention-policy.md)).

## Current limitations

* Eight Workshop-independent features remain unimplemented: the full
  builtin action/value, receiver/member, and enum/constant-domain surfaces
  beyond the manifest-declared evidence, `switch` and string modifiers,
  advanced preprocessing directives, translations, and optimization controls
  ([`compatibility-baseline.md`](docs/opy/compatibility-baseline.md)).
* Workshop lowering, emission, catalog/member/domain/settings validation,
  locale data, and `#!postCompileHook` execution against the final Workshop
  text are not implemented in this repository; they belong to the
  `workshop-rs` integration layer. `opy-rs` never approximates Workshop-owned
  validation or a temporary Workshop IR.

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
