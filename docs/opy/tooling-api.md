# opy-rs Tooling API

Workshop-independent tooling surface for the OPY frontend (issue #7): a
library API in `crates/opy-frontend` (`opy_frontend::tooling`) and a
standalone CLI in `crates/opy-cli` (`opy-cli`). Both operate on `.opy` source
only — no Workshop backend, catalog, Node, or OverPy is required or invoked.

The integration boundary toward `workshop-rs` remains documented, not
implemented: anything that needs canonical Workshop semantics is classified
`lowering-dependent` in the support matrix and never approximated here.

## Pipeline contract

`check` runs the same pipeline as `compile` — preprocess (includes,
`#!define` macros) → parse (CST) → resolve (Opy HIR) — so the two entry
points never disagree about a project's verdict. Resolution stops at the
Workshop-independent Opy HIR semantic model ([`hir::Program`]). There is no
Workshop emission step.

## Library API (`opy_frontend::tooling`)

```rust
pub fn check(source: &str, main_path: &str, root: &Path) -> CheckOutcome
pub fn check_with_overlay(source, main_path, root, overlay) -> CheckOutcome

pub struct CheckOutcome {
    pub diagnostics: Vec<Diagnostic>,   // empty ⟺ clean
    pub model: Option<SemanticModel>,   // present exactly when clean
    pub files: Vec<FileRecord>,         // file registry, retained on failure
}
```

* `Diagnostic { severity, code, message, span }` — structured, stable-coded,
  source-attributed (see the diagnostics contract below).
* `SourceLocation { file_id, path, start, end }` — a span resolved through
  the file registry to `(file id, path, line/col)`.

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
binding kind in the contract; the current frontend produces no constant
declarations (custom enums fold instead).

Source provenance: the file registry maps every span's file id to its path —
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
| `unknown-enum-member` / `enum-domain-mismatch` | resolve | Enum member/domain validation |
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

## Support-matrix accessor (`opy_frontend::support`)

`compatibility/support-matrix.json` is owned by the evidence workstream and
consumed read-only. It is embedded at build time via `include_str!` (the
crate rebuilds when the file changes), parsed once, and exposed as
`SupportMatrix`:

* `SupportMatrix::builtin() -> Result<&'static SupportMatrix, …>`
* `feature(id)` / `feature_state(id)` — feature lookup by id
* `features_by_category(category)` / `features_by_state(state)` — filtered
  slices
* `categories()`, `declared_states()`, `summary()` — declared surface

The five declared states (`planned`, `frontend-supported`,
`semantic-supported`, `lowering-dependent`, `end-to-end-supported`) are
documented in the matrix itself. Workshop-dependent items stay
`lowering-dependent`; nothing here approximates them.

## CLI (`opy-cli`)

```
opy-cli check <main.opy>                          # diagnostics → stderr; 0 clean / 1 diagnostics
opy-cli inspect <main.opy>                        # resolved model as JSON on stdout
opy-cli support [--json] [<category|feature-id>]  # embedded matrix (or slice) as JSON
opy-cli version                                   # crate + frontend protocol identity
```

Exit codes: `0` clean/success, `1` diagnostics found, `2` usage or I/O
errors. `check`/`inspect` resolve includes against the main file's parent
directory. The CLI runs anywhere the binary runs: no Node, no Workshop
backend, no runtime data files (the matrix and the semantic manifest are
embedded).

## Known limitations

* `def NAME():` bodies resolve, but calls resolve only against `subroutine
  NAME` declarations; a def-only subroutine call is an `unknown-action`
  diagnostic (existing frontend resolution contract; tracked as frontend
  follow-up).
* Custom enums fold to constants in the HIR (reference behavior); enum
  declarations are queryable through `SemanticModel::enums`, not the HIR
  declaration list.
* Workshop emission, decompilation, settings-section emission, and locale
  data are `lowering-dependent` (see the support matrix); the differential
  harness wiring that consumes this API end-to-end is a separate PR.
