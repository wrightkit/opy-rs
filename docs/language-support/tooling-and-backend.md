# OverPy audited inventory: tooling and backend behavior

Source: pinned OverPy `9.7.10`, content commit
`889d9749d1def17f146548cbddb94ea1ab015847`. Evidence surfaces are the
upstream README, `overpy.d.ts`, `cli.js`, compiler/decompiler sources,
`runTests.mjs`, `runCliTests.mjs`, QuickJS fixtures and the executable oracle.

## Preprocessing, macros and hooks

| Feature | Status | Notes |
| --- | --- | --- |
| `#!define` object/function macros and `#!undef` | ✅ Supported | Expansion, precedence and recursion are distinct checks. |
| `#!allowMacroRedeclaration` | 🚧 Coming soon | Changes duplicate-definition failure behavior. |
| `#!mainFile`, `#!include`, `#!excludeVariablesInCompilation` | ✅ Supported | Selection and output filtering have separate effects. |
| Optimization controls (`#!enableOptimizations`, `#!disableOptimizations`, `#!optimize*`) | 🚧 Coming soon | Recognition is not backend-effect support. |
| Replacement directives (`#!replace0By*`, team/string replacements) | 🚧 Coming soon | Each replacement target has its own output contract. |
| `#!rulePrefix` and `#!rulePrefixTemplate` | ✅ Supported | Source preprocessing applies the resulting rule names before compiler lowering. |
| `#!extension` and extension-point accounting | 🚧 Coming soon | Output metadata is part of the contract. |
| `macro name(params)` function/constant macros | ✅ Supported | Defaults, keywords and member macros differ. |
| `__script__` JavaScript macros | 🚧 Coming soon | QuickJS return ABI and limits are observable. |
| `#!postCompileHook` | ✅ Bounded compiler slice | Runs only after final Workshop emission; failures keep directive and script provenance. |

## Compilation, CLI and API

| Feature | Status | Notes |
| --- | --- | --- |
| Standalone `.opy` compiler library | ✅ Supported | Supported within the documented source/compile scope. |
| CLI compile/check invocation and structured diagnostics | ✅ Supported | Exit behavior and source attribution are contractual. |
| Upstream JS `compile(content, language, rootPath, mainFileName)` API | 🚧 Coming soon | API shape audited; Rust parity is incomplete. |
| Compile metadata: variables, subroutines, warnings, translations, element count | 🚧 Coming soon | Fields have independent completeness requirements. |
| Localized Workshop text and custom settings emission | ✅ Bounded compiler slice | The compiler delegates validation and emission to `workshop-rs`; undeclared locales fail explicitly. |
| Observable optimization/replacement effects | 🚧 Coming soon | Formatting is not a target unless observable. |

## Decompilation and round trips

| Feature | Status | Notes |
| --- | --- | --- |
| `decompileAllRules` Workshop-to-OPY reconstruction | ❌ Unsupported | Outside the current `opy-rs` contract. |
| `decompileActions` and `decompileConditions` | ❌ Unsupported | Same boundary as full decompilation. |
| Workshop settings decompilation | ❌ Unsupported | No claim of recovering original source abstractions. |
| Compile/decompile round trip preserving source identity | ❌ Unsupported | Comments, macros, names and formatting are not promised. |

The upstream source is GPL-3.0-only and is used as an external audit
reference/oracle. Its implementation and data are not copied into `opy-rs`.
The exhaustive conformance follow-up should derive stable leaf cases from the
audited registries, preserve negative behavior, and compare observable
semantics rather than Workshop formatting or internal compiler structure.
