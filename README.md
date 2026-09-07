# opy-rs

`opy-rs` is a standalone Rust compiler and library for the OverPy (`.opy`)
language. It parses, checks, compiles, and inspects OverPy projects independently
of external Node.js runtimes.

Downstream tools such as Wright integrate with `opy-rs` through native Rust APIs
or as a Language Provider Protocol (LPP) process for extended linting, analysis,
and editor support.

`opy-rs` owns OverPy syntax, preprocessing, macros, semantic resolution,
compiler lowering, diagnostics, provenance, and source reconstruction. Shared
Workshop semantics, catalog identities, and emission remain delegated to
`workshop-rs`.

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

The reverse path reconstructs OverPy source from canonical Workshop structures.
This keeps `opy-rs` focused on OverPy language behavior while reusing the shared
Workshop semantic model.

## Key features

- Source analysis: lexer, preprocessor, parser, and semantic HIR with precise
  source-spanned diagnostics across includes.
- Macros and preprocessing: full support for object and function macros
  (`#!define`, `#!undef`), file inclusions (`#!include`), and settings directives.
- Embedded JavaScript macros: executes `__script__("...")` blocks inside an
  embedded QuickJS-NG runtime without requiring Node.js.
- Semantic tooling: symbol inspection, reference queries, and AST-aware source
  checks.
- Workshop code generation: direct lowering to canonical `workshop-rs` WIR with
  explicit error reporting for unsupported syntax.
- Verified compatibility: validated against corpus fixtures, reference snapshots,
  and differential tests.

## CLI and library

The standalone CLI exposes both Workshop-independent tooling and the bounded
Workshop compiler surface:

```sh
opy-cli check main.opy
opy-cli compile main.opy
opy-cli compile --format json main.opy
opy-cli inspect main.opy
opy-cli completion bash
opy-cli version
```

The first-party LPP process is available as `opy-provider`; it supports LPP
1.1 entry-based OPY project requests so clients do not need to enumerate
includes:

```sh
cargo run --release -p opy-provider
```

See the [provider contract](docs/opy/provider.md) for its capabilities and
artifact format.

The Rust library surface, including the bounded Workshop compiler, lives in
`crates/opy-rs`; `opy-cli` is the standalone executable surface. See the
[tooling API reference](docs/opy/tooling-api.md) and
[current architecture routing](docs/architecture/README.md) for the durable boundary.

## Compatibility

Compatibility targets observable OverPy semantics for the declared support
surface, not byte-identical output, optimizer choices, formatting, temporary
variables, or upstream internal architecture. Support claims are backed by the
canonical [language-support contract](docs/language-support.md), its linked
inventories, and pinned OverPy reference evidence.

> [!IMPORTANT]
> `opy-rs` follows the OverPy language. It does not introduce a WrightKit-only
> OPY dialect.

The exhaustive per-feature evidence, pinned denominator, and current status are
maintained in the [canonical human-readable support contract](docs/language-support.md)
and its linked inventories.

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
python3 -m unittest discover -s tools/overpy/tests
```

CI additionally exercises the JavaScript macro runtime on macOS and Windows.
Oracle-dependent compatibility probes run separately from the normal Rust and
Python test suites.

## Documentation

Current architecture, compatibility evidence, APIs, HIR, provenance, and
maintainer references are indexed in [`docs/README.md`](docs/README.md).

## Contributing

This repository is part of the WrightKit multi-repository workspace. Follow the
workspace-level `AGENTS.md` first, then this repository's local rules.

## License

`opy-rs` is distributed under the GNU Affero General Public License v3.0 or
later. See [`LICENSE`](LICENSE).
