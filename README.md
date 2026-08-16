# opy-rs

`opy-rs` is WrightKit's Rust implementation of the OverPy `.opy` language. It
provides a standalone library and CLI for parsing, checking, inspecting, and
resolving `.opy` projects with structured diagnostics and source provenance.
No Node.js or upstream OverPy runtime is required.

The implementation is Workshop-independent until the documented
[Workshop integration boundary](docs/opy/architecture.md): canonical Workshop
semantics and emission are provided by `workshop-rs` rather than duplicated in
this repository.

## Features

- **OverPy source analysis:** lexer, preprocessing, parser, semantic resolution,
  source-located diagnostics, and provenance across includes and macros.
- **Preprocessing and macros:** `#!include`, object- and function-like
  `#!define`, `#!undef`, settings blocks, and recorded `#!postCompileHook`.
- **JavaScript macros:** OverPy-compatible `__script__("...")` macros run in a
  bounded embedded QuickJS-NG runtime without Node.js.
- **Tooling APIs:** `check`, semantic inspection, source-aware queries, and
  [validated source-edit foundations](docs/opy/trivia-retention-policy.md).
- **Compatibility evidence:** a [26-fixture corpus](compatibility/README.md),
  pinned oracle snapshots, semantic probes, and native differential tests.

## CLI and library

The standalone CLI exposes the current tooling surface:

```sh
opy-cli check main.opy       # diagnostics; exit 0 clean / 1 diagnostics
opy-cli inspect main.opy     # resolved semantic model as JSON
opy-cli support --json       # detailed machine-readable support data
opy-cli version
```

The Rust library surface lives in `crates/opy-frontend`; see the
[tooling API reference](docs/opy/tooling-api.md) for integration details.

## Compatibility

Compatibility targets observable OverPy semantics for the declared support
surface, not byte-identical output, optimizer choices, or formatting. Support
claims are backed by the compatibility corpus and a pinned OverPy 9.7.10
reference.[^overpy-reference]

> [!IMPORTANT]
> `opy-rs` follows the OverPy language. It does not introduce a WrightKit-only
> OPY dialect.

| Capability | Status | Notes |
| --- | --- | --- |
| Core syntax & control flow | ✅ Supported | Lexing, expressions, assignments, `if`/`elif`/`else`, `for`/`while`, settings blocks |
| Declarations | ✅ Supported | `globalvar`/`playervar`, `subroutine`, `def`, `enum`, `macro` |
| Preprocessing & macros | ✅ Supported | `#!include`, `#!define`, `#!undef` |
| JavaScript macros | ✅ Supported | `#!define name(...) __script__("...")` with a bounded embedded runtime |
| Rules & directives | ✅ Supported | `rule` blocks, `@Event`, `@Condition`, bare `@Team`/`@Slot` |
| Builtin actions & values | 🟡 Partial | The declared subset works; the full OverPy surface is not implemented yet |
| Receiver/member functions | 🟡 Partial | Declared members work; the full member surface is not implemented yet |
| Enums & constants | 🟡 Partial | Declared enum domains resolve; the full domain surface is not implemented yet |
| `switch` / string modifiers | ⏳ Not yet | |
| Advanced directives, translations & optimization controls | ⏳ Not yet | `#!translations`, the `#!optimize` family, `#!mainFile`, and similar |
| OPY → Workshop compilation | ⏳ Not yet | Requires the `workshop-rs` integration path |
| Workshop → OPY reconstruction | ⏳ Not yet | Requires the `workshop-rs` integration path |

Exact per-feature evidence remains available in the
[human-readable support reference](docs/opy/support-matrix.md) and
[machine-readable support matrix](compatibility/support-matrix.json).

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
workspace-level `AGENTS.md` first, then this repository's local rules when
contributing changes.

## License

`opy-rs` is distributed under the GNU Affero General Public License v3.0 or
later. See [`LICENSE`](LICENSE).

[^overpy-reference]: The exact package identity, source revision, and provenance
    are recorded in the [upstream reference record](docs/compatibility/upstream-references.md).
