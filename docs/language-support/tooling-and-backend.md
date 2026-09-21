# Tooling and compiler backend

## Preprocessing, macros and hooks

| Source form | Status | Limit |
| --- | --- | --- |
| `#!define` and function/member macros | ✅ Supported | Expansion order and definition/use-site provenance follow OverPy. |
| `#!allowMacroRedeclaration` | ✅ Supported | Duplicate-definition policy is explicit in preprocessing state. |
| `#!mainFile`, `#!include`, `#!excludeVariablesInCompilation` | ✅ Supported | File selection, include closure and output filtering are separate operations. |
| `#!rulePrefix` and `#!rulePrefixTemplate` | ✅ Supported | Rule names are transformed before lowering. |
| Optimization controls such as `#!enableOptimizations`, `#!disableOptimizations`, `#!optimizeForSize`, `#!optimizeForSizeAggressive` and `#!optimizeStrict` | ✅ Supported | Optimization state is scoped to source spans; size, aggressive skip, strict-folding and wait-duration effects are lowered. Internal optimizer shape is not a contract. |
| Replacement directives such as `#!replace0By*`, `#!replace1ByMatchRound`, team and empty-string replacements | ✅ Supported | Observable replacements are lowered when size optimization is active and are excluded from Workshop-setting constructors, matching the pinned upstream boundary. |
| `#!extension` | ✅ Supported | The extension name is checked against the canonical Workshop schema. |
| `#!disableInspector`, `#!excludeVariablesInCompilation`, `#!setupTags`, `#!setupTx`, `#!globalvarInitRuleName` and `#!playervarInitRuleName` | ✅ Supported | Inspector/setup rules, output declaration filtering and generated initialization rule names affect forward Workshop output. |
| `#!translations` | 🚧 Partial | Translation state is recorded; the full translation-file lifecycle remains issue #331. |
| `#!suppressWarnings` | ✅ Supported | Matching preprocessing warning codes are omitted from the public diagnostics surface. |
| `#!debugElementCount` | ✅ Supported | The forward compiler emits a canonical-WIR total, a count-sorted per-rule summary, and per-condition/per-action element-count comments. |
| `#!translateWithPlayerVar`, `#!writeToOutputFile`, `#!disableTranslationSourceLines` and `#!keepUnusedTranslations` | ❌ Unsupported | These require translation-file or editor-output capabilities outside the forward compiler contract and are rejected with a source diagnostic. |
| `#!postCompileHook` | ✅ Supported | The hook runs after final Workshop emission and failures retain script provenance. |
| `__script__(...)` JavaScript macros | ✅ Supported | The embedded QuickJS runtime exposes the documented OverPy ABI, limits, isolation and string-result contract. |

## Compilation and CLI

| Capability | Status | Limit |
| --- | --- | --- |
| Standalone Rust compiler library | ✅ Supported | The source and Workshop boundaries described on this page apply. |
| CLI `check`, `compile`, JSON reports and source diagnostics | ✅ Supported | Failure status never produces a misleading successful artifact. |
| Compile metadata for variables, subroutines, warnings, translations and element count | 🚧 Partial | The JSON report exposes the fields implemented by the native API; it is not a byte-for-byte clone of the upstream JavaScript object. |
| Observable optimizer and replacement effects | ✅ Supported | Tested semantic and cost-relevant effects are preserved; formatting and upstream internal optimizer structure are not contracts. |
| OPY → Workshop emission through `workshop-rs` | 🚧 Partial | The supported language rows above compile end to end; unsupported source forms produce structured diagnostics. |

## Workshop output language

`opy-cli compile --language` and the `Compiler` language-selection APIs emit
localized Workshop source through the canonical `workshop-rs` catalog. The
supported pinned OverPy 9.7.10 output locales are:

`de-DE`, `en-US`, `es-ES`, `es-MX`, `fr-FR`, `it-IT`, `ja-JP`, `ko-KR`,
`pl-PL`, `pt-BR`, `ru-RU`, `th-TH`, `tr-TR`, `zh-CN`, and `zh-TW`.

This output-language selection is separate from OverPy `#!translations` and
`.po` files: output language selects the localized Workshop syntax emitted for
the game client, while runtime translation support selects user-authored string
translations in the compiled program.

## Reconstruction

| Capability | Status | Limit |
| --- | --- | --- |
| Workshop → OPY reconstruction | ❌ Unsupported | `opy-rs` does not currently expose a public decompiler contract. |
| Source-identity-preserving compile/decompile round trip | ❌ Unsupported | Workshop output cannot retain comments, macros, original names or formatting. |
