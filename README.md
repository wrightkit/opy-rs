# opy-rs

`opy-rs` is WrightKit's standalone Rust implementation of the OverPy `.opy`
language. It is intended to be useful on its own as a library and CLI for
parsing, preprocessing, checking, inspecting, compiling, and eventually
reconstructing supported OverPy projects.

Wright is a downstream consumer that integrates `opy-rs` with broader tooling
such as linting, analysis, source editing, agent workflows, CI, and language
services. An LPP **provider** in this repository is an
integration role that `opy-rs` may expose to Wright and other tooling clients,
not the reason this repository exists.

Canonical raw Workshop semantics are shared instead of reimplemented here.
`opy-rs` owns OverPy syntax, preprocessing, macros, semantic resolution,
OverPy-specific lowering, compiler behavior, diagnostics, provenance, and
Workshop-to-OPY reconstruction. `workshop-rs` owns canonical Workshop catalog
identities, WIR, validation, settings/localization data, raw Workshop parsing,
and emission.

```text
OPY source
   ↓
opy-rs parsing, preprocessing, and semantic HIR
   ↓
OPY semantic model / HIR
   ↓
opy-rs compiler + reconstruction logic
   ↓
workshop-rs canonical WIR / validation / emission
   ↓
Workshop text
```

The reverse path starts from Workshop text parsed by `workshop-rs`, then uses
`opy-rs`-owned reconstruction logic to produce useful OverPy source. This
architecture lets `opy-rs` remain a complete OverPy implementation without
maintaining a second raw Workshop implementation.

## Features

- **OverPy source analysis:** lexer, preprocessing, parser, semantic resolution,
  source-located diagnostics, and provenance across includes and macros.
- **Preprocessing and macros:** `#!include`, object- and function-like
  `#!define`, `#!undef`, settings blocks, and recorded `#!postCompileHook`.
- **JavaScript macros:** OverPy-compatible `__script__("...")` macros run in a
  bounded embedded QuickJS-NG runtime without Node.js.
- **Tooling APIs:** `check`, semantic inspection, source-aware queries, and
  validated source-edit foundations.
- **Compiler integration:** OPY semantic lowering into canonical
  `workshop-rs` WIR, with unsupported behavior kept explicit.
- **Compatibility evidence:** corpus fixtures, pinned oracle snapshots,
  semantic probes, and native differential tests.

## CLI and library

The standalone CLI exposes both Workshop-independent tooling and the bounded
Workshop compiler surface:

```sh
opy-cli check main.opy
opy-cli compile main.opy
opy-cli compile --format json main.opy
opy-cli inspect main.opy
opy-cli support --json
opy-cli completion bash
opy-cli version
```

The Rust library surface, including the bounded Workshop compiler, lives in
`crates/opy-rs`; `opy-cli` is the standalone executable surface. See the
[tooling API reference](docs/opy/tooling-api.md) and
[implementation role](docs/opy/implementation-role.md) for the durable boundary.

## Compatibility

Compatibility targets observable OverPy semantics for the declared support
surface, not byte-identical output, optimizer choices, formatting, temporary
variables, or upstream internal architecture. Support claims are backed by the
compatibility corpus and pinned OverPy reference evidence.

> [!IMPORTANT]
> `opy-rs` follows the OverPy language. It does not introduce a WrightKit-only
> OPY dialect.

| Capability | Status | Notes |
| --- | --- | --- |
| Core syntax & control flow | ✅ Supported | Workshop-independent parsing and semantic representation |
| Declarations | ✅ Supported | `globalvar`/`playervar`, `subroutine`, `def`, `enum`, `macro` |
| Preprocessing & macros | ✅ Supported | `#!include`, `#!define`, `#!undef`, bounded JavaScript macros |
| Rules & directives | ✅ Supported | Rules, events, conditions, team/slot context in the declared surface |
| Builtin actions & values | 🟡 Partial | Declared semantic subset works; full catalog-backed breadth is still being closed |
| Receiver/member functions | 🟡 Partial | Declared members work; full member breadth is not yet complete |
| Enums & constants | 🟡 Partial | Declared domains resolve; full domain breadth is not yet complete |
| Advanced directives, translations & optimizer controls | 🟡 Partial | Source state exists; Workshop-dependent effects remain incomplete |
| OPY → Workshop compilation | 🟡 Partial | The versioned library/CLI compile contract and bounded lowering surface are supported; remaining corpus gaps stay explicit |
| Workshop → OPY reconstruction | ⏳ Not yet | Will consume canonical `workshop-rs` semantics and remain owned by `opy-rs` |

Exact per-feature evidence remains available in the
[canonical human-readable support contract](docs/language-support.md). Internal
fixture and implementation metadata remains in
[compatibility/support-matrix.json](compatibility/support-matrix.json).

## Relationship with Wright

`opy-rs` can be used independently. Wright adds a unified product layer across
OverPy, DEL/OSTW, and raw Workshop and may consume `opy-rs` through native Rust
APIs and/or the Language Provider Protocol depending on the integration path.
Wright-specific lint, analysis, agent, CI, LSP, and orchestration behavior does
not belong in this repository unless it exposes a missing OverPy semantic
capability that `opy-rs` itself should own.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
python3 -m unittest discover -s compatibility/tests
```

CI additionally exercises the JavaScript macro runtime on macOS and Windows.
Oracle-dependent compatibility probes run separately from the normal Rust and
Python test suites.

## Documentation

Architecture, compatibility evidence, APIs, HIR, provenance, and maintainer
references are indexed in [`docs/README.md`](docs/README.md).

## Contributing

This repository is part of the WrightKit multi-repository workspace. Follow the
workspace-level `AGENTS.md` first, then this repository's local rules.

## License

`opy-rs` is distributed under the GNU Affero General Public License v3.0 or
later. See [`LICENSE`](LICENSE).
