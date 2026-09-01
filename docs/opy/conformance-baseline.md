# Offline OverPy conformance baseline

Issue #158 establishes the conformance boundary used before further OPY
compatibility implementation. The durable machine-readable inventory is
[`compatibility/conformance-manifest.json`](../../compatibility/conformance-manifest.json);
the executable runner is [`compatibility/conformance.py`](../../compatibility/conformance.py).

## Independent inventory

The inventory categories are derived from the pinned OverPy 9.7.10 source
registries and public project/test evidence recorded in
[`upstream-references.md`](../compatibility/upstream-references.md) and
[`language-support/registries.md`](../language-support/registries.md). The
manifest declares structural contracts for each category. Every contract has
a reviewable claim, one or more probe kinds (`positive`, `negative`,
`contextual`, or `composition`), and one or more executable fixture probes;
the validator rejects missing, unknown, or empty mappings. It does not derive
its declared surface, reference outcome, or expected behavior from the current
Rust implementation.

The categories are:

| Category | Contract covered |
| --- | --- |
| `syntax.lexing` | tokenization, literals, string modifiers, and lexical failures |
| `syntax.parser-and-control-flow` | expressions, statements, loops, switches, breaks, and postfix forms |
| `preprocessing.project-composition` | includes, directives, main-file selection, and multi-file state |
| `semantics.declarations-and-settings` | declarations, rule annotations, settings, and their source diagnostics |
| `semantics.builtins-members-and-enums` | manifest-backed functions, receiver members, domains, aliases, and argument binding |
| `lowering.canonical-wir` | successful OPY-to-canonical-WIR semantic equivalence |
| `diagnostics.failure-frontiers` | negative cases with pinned reference stage and first construct |
| `projects.real-world` | provenance-preserving complete projects and include closures |
| `ownership.workshop-feature-census` | opaque consumer evidence for the workshop-rs-owned census |

The existing `docs/language-support/registries.md` remains the leaf inventory
of upstream names. This baseline adds the executable structural probes and
does not duplicate upstream catalog data in `opy-rs`.

## Comparison contract

For a reference-success case, native compilation must succeed and the
feature-gated compatibility producer must report:

```text
workshop-rs::roundtrip::equivalent(native WIR, parse(reference Workshop))
```

The native Workshop text is not reparsed as a substitute for native WIR
evidence. Formatting, temporary variables, and emitter shape are not part of
the compatibility claim.

For a reference-failure case, the pinned snapshot records an audited
`stage` (`lex`, `parse`, `preprocess`, `semantic`, or `lowering`) and
`construct`. The runner classifies the native first diagnostic into the same
frontier vocabulary. A status match without a matching frontier is therefore
not a conformance match. Diagnostic text and locations are retained in the
generated report for provenance, but wording identity is not required.

The report is generated under `target/` and is intentionally not checked in.
Its summary groups results by the manifest's root capability and owner, and
its fixture entries retain both reference and native diagnostics. It is the
current divergence inventory used to group follow-up work. No current native
failure is entered into the manifest as an expected pass.
