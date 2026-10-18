# OverPy support

This page is the user-facing support contract for `opy-rs`. It describes the
OverPy source forms that can be used today, the limits of bounded support, and
the capabilities that are intentionally outside the current compiler boundary.

The compatibility reference is the npm package `overpy@9.7.17`. Its byte-
verified content commit is `5a7d0e294b8cad73b9701987bb584d0551d7fa4d`
(`v9.7.17`); the package metadata also records `gitHead`
`ec763c1d245aa5485ef61af5b2bda4975bce4877`, which is retained as package
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

Emitted output is compared against the pinned reference structurally; every
remaining difference is a recorded exception with an owner and a pinning test,
listed in the [`language-core.md` exception tables](architecture/language-core.md#approved-exceptions).
A `Supported` row therefore still names its recorded limitations rather than
claiming byte-identical output.

## Support at a glance

| Area | Status | Details |
| --- | --- | --- |
| Lexing, literals, expressions and assignments | ✅ Supported | [Syntax and project composition](language-support/syntax-and-projects.md) |
| Rules, annotations and ordinary control flow | ✅ Supported | [Syntax and project composition](language-support/syntax-and-projects.md) |
| Arrays, dictionaries and lambdas | ✅ Supported | [Syntax and project composition](language-support/syntax-and-projects.md) |
| Functions, member functions, enums and constants | ✅ Supported | [Callables and domains](language-support/callables-and-domains.md) |
| Multiple files, includes, macros and preprocessing | ✅ Supported | [Syntax and project composition](language-support/syntax-and-projects.md) and [Tooling and backend](language-support/tooling-and-backend.md) |
| Strings, translations and custom-game settings | ✅ Supported | [Syntax and project composition](language-support/syntax-and-projects.md) |
| Compiler directives and post-compile hooks (forward subset) | ✅ Supported | [Tooling and backend](language-support/tooling-and-backend.md) |
| Embedded JavaScript macros | ✅ Supported | [Tooling and backend](language-support/tooling-and-backend.md) |
| Workshop output language | ✅ Supported | [Tooling and backend](language-support/tooling-and-backend.md) |
| OPY → Workshop compilation | ✅ Supported | [Tooling and backend](language-support/tooling-and-backend.md) |
| Workshop → OPY reconstruction | 🚧 Partial | [Tooling and backend](language-support/tooling-and-backend.md) |

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
