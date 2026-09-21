# OverPy support

This page is the user-facing support contract for `opy-rs`. It describes the
OverPy source forms that can be used today, the limits of bounded support, and
the capabilities that are intentionally outside the current compiler boundary.

The compatibility reference is the npm package `overpy@9.7.10`. Its byte-
verified content commit is `889d9749d1def17f146548cbddb94ea1ab015847`
(`v9.7.10`); the package metadata also records `gitHead`
`1e2688954302a402d076944b46db07efb14d7b61`, which is retained as package
metadata and is not the content commit. Both identities are recorded in
`tools/overpy/oracle/oracle-metadata.json` and each oracle snapshot. The
reference is used as an independent behavior oracle; its implementation and
data are not copied into `opy-rs`.

## Status vocabulary

- `✅ Supported` — the listed source form is accepted and lowered within the
  stated contract.
- `🚧 Partial` — the listed family has an explicit supported subset; the same
  row names the forms that remain outside it.
- `❌ Unsupported` — the source form is rejected or the capability is outside
  the current `opy-rs` contract.

`Partial` is not an unknown or unreviewed state. A row is useful only when its
supported subset and limitation are stated in user terms.

## Support at a glance

| Area | Status | Details |
| --- | --- | --- |
| Lexing, literals, expressions and assignments | ✅ Supported | [Syntax and project composition](language-support/syntax-and-projects.md) |
| Rules, annotations and ordinary control flow | ✅ Supported | [Syntax and project composition](language-support/syntax-and-projects.md) |
| Arrays, dictionaries and lambdas | 🚧 Partial | [Syntax and project composition](language-support/syntax-and-projects.md) |
| Functions, member functions, enums and constants | ✅ Supported | [Callables and domains](language-support/callables-and-domains.md) |
| Multiple files, includes, macros and preprocessing | ✅ Supported | [Syntax and project composition](language-support/syntax-and-projects.md) and [Tooling and backend](language-support/tooling-and-backend.md) |
| Strings, translations and custom-game settings | ✅ Supported | [Syntax and project composition](language-support/syntax-and-projects.md) |
| Compiler directives and post-compile hooks (forward subset) | ✅ Supported | [Tooling and backend](language-support/tooling-and-backend.md) |
| Embedded JavaScript macros | ✅ Supported | [Tooling and backend](language-support/tooling-and-backend.md) |
| Workshop output language | ✅ Supported | [Tooling and backend](language-support/tooling-and-backend.md) |
| OPY → Workshop compilation | 🚧 Partial | [Tooling and backend](language-support/tooling-and-backend.md) |
| Workshop → OPY reconstruction | ❌ Unsupported | [Tooling and backend](language-support/tooling-and-backend.md) |

## Tests and ownership

Support claims are grounded in executable tests and reference comparisons:

- the corpus under `crates/opy-rs/tests/fixtures/corpus/` contains source,
  compiler, regression, and real-project test inputs;
- `tools/overpy/oracle/` and each fixture's `oracle.json` provide pinned
  upstream reference results;
- `tools/overpy/run_native.py` and `tools/overpy/diff.py` compare native output,
  diagnostics, normalized Workshop output, and canonical-WIR comparisons;
- Rust integration tests exercise source semantics and canonical lowering;
- `cargo test -p opy-rs --test differential` checks the native source pipeline
  against the fixture manifests and pinned snapshots.

`opy-rs` owns OverPy syntax, preprocessing, source semantics, diagnostics and
OverPy-specific lowering. `workshop-rs` owns canonical Workshop identities,
settings, validation, representation and emission. A limitation caused by the
canonical Workshop boundary is reported as unsupported or partial here; it is
not hidden in a Wright-side adapter.
