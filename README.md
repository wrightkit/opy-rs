# opy-rs

`opy-rs` is a standalone Rust compiler and library for the
[OverPy](https://github.com/Zezombye/overpy) (`.opy`) language for Overwatch
Workshop. It checks, compiles, and inspects OverPy projects with no Node.js
required.

## Features

- Preprocessor with object and function macros (`#!define`) and includes
  (`#!include`).
- `__script__("...")` JavaScript macros, run in an embedded runtime.
- Diagnostics with precise source locations across includes.
- Symbol and reference queries for editors and tools.
- Compilation to Workshop code, with clear errors for unsupported syntax.

`opy-rs` follows the OverPy language and adds no dialect of its own. Compiled
output matches the upstream OverPy compiler structurally (see
[`docs/language-support.md`](docs/language-support.md)).

## Install

```sh
cargo install --path crates/opy-cli
```

## Usage

```sh
opy-cli check main.opy
opy-cli compile main.opy
opy-cli inspect main.opy
```

For editors and other tools, `opy-provider` runs `opy-rs` as a
[Language Provider Protocol](https://github.com/wrightkit/language-provider-protocol)
process; [Wright](https://github.com/wrightkit/wright) uses it for lint and
analysis. The library API is documented in [`docs/opy/tooling-api.md`](docs/opy/tooling-api.md).

## Documentation

See [`docs/README.md`](docs/README.md) for the language support contract,
architecture, and compatibility details.

## Contributing

Read [`AGENTS.md`](AGENTS.md) first.

## License

AGPL-3.0-or-later. See [`LICENSE`](LICENSE).
