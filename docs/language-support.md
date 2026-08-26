# OverPy support

This is the canonical, human-readable compatibility contract for `opy-rs`.
The detailed inventories linked here are part of the same contract.

## Reference and audit boundary

| Field | Value |
| --- | --- |
| OverPy package | `9.7.10` |
| Content commit | `889d9749d1def17f146548cbddb94ea1ab015847` (`v9.7.10`) |
| Repository | <https://github.com/Zezombye/overpy> |
| Registry integrity | `sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw==` |
| Audited language | `en-US` |

The inventory was audited from the pinned upstream tree, from outside the
`opy-rs` implementation: the upstream README and public API declaration;
`src/compiler/` grammar, preprocessing, compiler, translation and decompiler
surfaces; `src/data/opy/` keyword, annotation, builtin, member, module, macro
and preprocessing registries; `src/data/` Workshop domains; upstream compile,
decompile, CLI and QuickJS tests; and the pinned executable oracle. Existing
`opy-rs` fixtures, HIR names, support matrix entries and issue lists were used
only to determine the second column, never to construct the audited set.

## Status vocabulary

Only these public states are used:

- `✅ Supported` — the claimed user-visible behavior works within the notes.
- `🚧 Coming soon` — the pinned capability is recognized, but current behavior
  is incomplete.
- `❌ Unsupported` — the capability is outside the current contract.

“Supported” is an end-to-end claim for the stated row. Parsing a construct or
having a name in a manifest is not enough to make a compilation row green.

## Audited capability summary

| Area | Status | Detailed inventory |
| --- | --- | --- |
| Source syntax, literals and expressions | 🚧 Coming soon | [syntax and project composition](language-support/syntax-and-projects.md) |
| Assignments, declarations, rules and control flow | 🚧 Coming soon | [syntax and project composition](language-support/syntax-and-projects.md) |
| Builtins, member functions, constants and contextual domains | 🚧 Coming soon | [callables and domains](language-support/callables-and-domains.md) and [complete registries](language-support/registries.md) |
| Preprocessing, includes, modules and macros | 🚧 Coming soon | [syntax and project composition](language-support/syntax-and-projects.md) and [complete registries](language-support/registries.md) |
| Strings, translations and custom-game settings | 🚧 Coming soon | [syntax and project composition](language-support/syntax-and-projects.md) |
| Compiler directives, optimization and post-compile hooks | 🚧 Coming soon | [tooling and backend](language-support/tooling-and-backend.md) and [complete registries](language-support/registries.md) |
| Standalone compiler and CLI | ✅ Supported | [tooling and backend](language-support/tooling-and-backend.md) |
| Workshop-to-OPY decompilation | ❌ Unsupported | [tooling and backend](language-support/tooling-and-backend.md) |

The summary is intentionally conservative: the audited upstream surface is
larger than the currently evidenced `opy-rs` surface. Detailed rows make gaps
explicit instead of hiding them in a category-level green row.

## Contract maintenance

`compatibility/support-matrix.json` is retained as **internal engineering
metadata** for fixture relationships, provenance and implementation tracking.
It is not a public inventory and its internal states are not public support
states. `docs/opy/support-matrix.md` is retained as historical context and
must not introduce another public status vocabulary.

The next step is a separate exhaustive conformance issue driven by the audited
leaf identities in these documents. This issue does not turn the inventory
into a fixed feature-count assertion or silently convert known gaps into
passing cases.
