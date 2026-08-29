# opy-rs Tooling API

The OPY source implementation exposes Workshop-independent checking and
inspection through `crates/opy-rs` (`opy_rs::tooling`). The
Workshop-dependent compiler is exposed by `crates/opy-compiler`, and
`crates/opy-cli` provides both surfaces without requiring Node or OverPy.

## Pipeline contract

`check` runs the source pipeline through resolution (preprocess (includes,
`#!define` macros) → parse (CST) → resolve (Opy HIR)). `compile` continues
from that resolved model through canonical WIR lowering, validation, and
localized Workshop emission. A compile report retains the source-attributed
diagnostic when either the frontend or integration stage fails.

## Compile API

The Workshop-dependent compiler is exposed by opy-compiler. Its
Compiler::compile_source_report method runs the explicit source → HIR →
canonical WIR → validation → localized Workshop pipeline and returns a
versioned CompileReport. Compiler::compile_source remains available when a
caller needs the typed CompilationArtifact.

The report schema version is 1. It contains compiler and catalog identities,
compile status, exit code, failure class, source-attributed diagnostics, exact
Workshop output, and normalized Workshop output. Frontend failures use the
frontend class; canonical Workshop, locale, directive, and hook failures use
the integration class. Normalized output removes line-ending and trailing
presentation noise, while exact output preserves the emitted artifact.

When a compatibility caller supplies a pinned Workshop reference,
`compile_source_report_with_semantic_reference` additionally emits optional
`compile.semanticWIR` evidence. The evidence records schema version 1, the
`workshop-rs::roundtrip::equivalent` algorithm, and SHA-256 hashes for both
the source input and reference input. The comparison parses only the pinned
reference Workshop text and compares it directly with the native lowered WIR;
native emitted text is not reparsed as a substitute. A normal compile report
does not include this optional field.

The compatibility runner uses the separate
[`compatibility/compiler-expectations.json`](../../compatibility/compiler-expectations.json)
baseline for compiler outcomes. The source/frontend expectation contract is
kept in `differential-expectations.json`; it is not reused as compiler parity
evidence. Compiler gaps must carry durable evidence and an owner, while
expectation mismatches remain blocking.

## Library API (`opy_rs::tooling`)

```rust
pub fn check(source: &str, main_path: &str, root: &Path) -> CheckOutcome
pub fn check_with_overlay(source, main_path, root, overlay) -> CheckOutcome

pub struct CheckOutcome {
    pub diagnostics: Vec<Diagnostic>,   // empty ⟺ clean
    pub model: Option<SemanticModel>,   // present exactly when clean
    pub files: Vec<FileRecord>,         // file registry, retained on failure
    pub post_compile_hook: Option<PostCompileHook>, // declared hook record
}
```

* `Diagnostic { severity, code, message, span }`: structured, stable-coded,
  source-attributed (see the diagnostics contract below).
* `SourceLocation { file_id, path, start, end }`: a span resolved through
  the file registry to `(file id, path, line/col)`.
* `PostCompileHook`: the declared `#!postCompileHook` script (root-relative
  path plus directive span), present only when the source declared one and
  the project checked clean. It is a declaration record only; the compiler
  executes it after successful final Workshop emission.

`SemanticModel` wraps the resolved program and answers queries:

| Query | Returns |
| --- | --- |
| `declarations()` | HIR declarations (globals, players, subroutines, constants, macros) |
| `rules()` | Rule listing (rules and `def` subroutine definitions) |
| `defines()` | Recorded `#!define` macros with their definition-site spans |
| `enums()` | Custom `enum` declarations (CST-retained; they fold to constants in the HIR) |
| `symbols()` / `symbol(name)` | Program-scope bindings with declaration site and reference sites |
| `symbol_at(span)` | The binding or reference owner at a span |
| `provenance(span)` | Span → `(file id, path, line/col)` through the file registry |
| `file(id)` | The registry path of a file id |

Symbols are indexed per binding: a `subroutine NAME` declaration and a
`def NAME():` definition of the same name are separate entries, and call
sites are attached to both. Rules are listed but are not symbols (rule names
are not name-resolvable identifiers in OPY). `Constant` is a declared
binding kind in the contract; the current source implementation produces no constant
declarations (custom enums fold instead).

Source provenance: the file registry maps every span's file id to its path.
id 0 is the main file, then one entry per include, in include order. Macro
expansion stamps expanded tokens with the use-site span; the recorded
`defines` carry their definition-site spans, so both define attribution and
include attribution are queryable through `provenance`.

## Diagnostics contract

Every diagnostic is `{ severity, code, message, span }`. `code` and `span`
are the machine contract; `message` and wording are not. All current
diagnostics are `error` severity.

Span layout: `file_id` indexes the registry, positions are 1-based
`(line, col)`; the CLI renders them as `path:line:col`.

### Stable codes

| Code | Stage | Meaning |
| --- | --- | --- |
| `lex-error` | lex | Tokenization failure (e.g. expression-level `{}`) |
| `break-context` | lower | `break` is outside the innermost switch/loop context |
| `lambda-context` | lower | `lambda` is outside a signature-approved argument position |
| `include-invalid` | preprocess | Malformed `#!include` directive |
| `include-not-found` | preprocess | Included file missing under the root |
| `include-cycle` | preprocess | Include cycle detected |
| `unsupported-directive` | preprocess | Unknown `#!` directive |
| `define-invalid` | preprocess | Malformed `#!define` |
| `macro-invalid` / `macro-arity` / `macro-recursion` | preprocess | Macro expansion failures |
| `settings-invalid` / `settings-placement` | preprocess | Settings block parse / placement failures |
| `parse-error` | parse | Syntax error (parser recovers at statement boundaries) |
| `manifest-error` | resolve | Semantic manifest load failure |
| `unknown-identifier` / `enum-type-without-member` | resolve | Unresolved names |
| `unknown-action` / `unknown-value` / `unknown-member` | resolve | Unknown builtins |
| `unsupported-member` | resolve | Member access outside the declared surface |
| `unknown-enum-member` | resolve | Custom (user-declared) enum member validation |
| `invalid-arity` / `missing-argument` / `invalid-argument` | resolve | Signature validation |
| `keyword-unsupported` / `unknown-keyword` / `duplicate-argument` / `keyword-required` / `positional-after-keyword` | resolve | Keyword binding |
| `invalid-iterable` / `invalid-call-context` | resolve | Position/context validation |
| `value-in-action-position` / `action-in-value-position` | resolve | Action/value identity |
| `invalid-receiver` | resolve | Receiver category validation |
| `vect-arity` | resolve | `vect` arity |

Parse diagnostics are reported in full (recovery collects several);
semantic-resolution diagnostics follow the compile contract and report the
first error. `check` and `compile` agree on the verdict; only the parse-stage
reporting depth differs.

## Support-matrix accessor (`opy_rs::support`)

`compatibility/support-matrix.json` is the repository's machine-readable
support state source (merged with the evidence base, PR #10) and is consumed
read-only. It is embedded at build time via `include_str!` (the
crate rebuilds when the file changes), parsed once, and exposed as
`SupportMatrix`:

* `SupportMatrix::builtin() -> Result<&'static SupportMatrix, …>`
* `feature(id)` / `feature_state(id)`: feature lookup by id
* `features_by_category(category)` / `features_by_state(state)`: filtered
  slices
* `categories()`, `declared_states()`, `summary()`: declared surface

The five declared states (`planned`, `source-supported`,
`semantic-supported`, `lowering-dependent`, `end-to-end-supported`) are
documented in the matrix itself. Workshop-dependent items stay
`lowering-dependent`; nothing here approximates them.

## CLI (`opy-cli`)

```
opy-cli check <main.opy>                          # diagnostics → stderr; 0 clean / 1 diagnostics
opy-cli check --format json <main.opy>             # machine JSON result/diagnostics on stdout
opy-cli compile <main.opy>                        # Workshop text → stdout
opy-cli compile --format json <main.opy>           # versioned compile report → stdout
opy-cli compile --language zh-CN <main.opy>        # catalog-declared locale
opy-cli inspect <main.opy>                        # resolved model as JSON on stdout
opy-cli support [--json] [<category|feature-id>]  # embedded matrix (or slice) as JSON
opy-cli completion bash|zsh|fish|powershell       # static completion from the command model
opy-cli version                                   # crate + source implementation protocol identity
```

Exit codes: `0` clean/success, `1` diagnostics found, `2` usage or I/O
errors. `check`/`inspect` resolve includes against the main file's parent
directory. The CLI runs anywhere the binary runs: no Node, no Workshop
backend, no runtime data files (the matrix and the semantic manifest are
embedded).

### Presentation candidate for Issue #43

The following additive CLI surface is implemented as a candidate pending the
main-thread contract review; existing command defaults and exit codes remain
unchanged:

* `--renderer auto|terminal|plain|github-actions` selects presentation. In
  `auto`, truthy `GITHUB_ACTIONS` selects GitHub Actions, then truthy `CI` or a
  non-TTY selects plain output, and an interactive terminal selects terminal
  output. An explicit renderer overrides this detection.
* `--color auto|always|never` controls ANSI color. An explicit color policy
  overrides `NO_COLOR`; `auto` disables color when `NO_COLOR` is present.
  GitHub Actions never receives ANSI even when `always` is requested.
* `--format json` is currently available on `check`. It writes only `{ok,
  diagnostics}` JSON to stdout and returns the same 0/1 result code. `inspect`
  and `support` remain JSON by default and bypass presentation entirely.
  Compile format json writes the versioned compile report described above;
  compiler failures return 1. If the required input path is missing, or the path cannot be read, the CLI
  cannot produce a machine result: it returns exit `2`, writes the human I/O
  error to stderr, and leaves stdout empty.

The GitHub Actions renderer writes source-located diagnostics as escaped
workflow annotations on stderr, groups a concise PASS/ERROR status, and
appends `opy-cli` status to `GITHUB_STEP_SUMMARY` when that variable names a
usable file. Human and workflow presentation never contaminates machine JSON
stdout.

## Known limitations

* `def NAME():` bodies resolve, but calls resolve only against `subroutine
  NAME` declarations; a def-only subroutine call is an `unknown-action`
  diagnostic (existing source implementation resolution contract; tracked as source implementation
  follow-up).
* Custom enums fold to constants in the HIR (reference behavior); enum
  declarations are queryable through `SemanticModel::enums`, not the HIR
  declaration list.
* Broader Workshop emission, decompilation, and unsupported source constructs
  remain explicit in the support matrix and the native corpus report. The
  compile contract never counts an inconclusive normalized-output comparison
  as successful parity.
