# Tooling and compiler backend

## Preprocessing, macros and hooks

| Source form | Status | Limit |
| --- | --- | --- |
| `#!define` and function/member macros | ✅ Supported | Expansion order and definition/use-site provenance follow OverPy. |
| `#!allowMacroRedeclaration` | ✅ Supported | Duplicate-definition policy is explicit in preprocessing state. |
| `#!mainFile`, `#!include`, `#!excludeVariablesInCompilation` | ✅ Supported | File selection, include closure and output filtering are separate operations. |
| `#!rulePrefix` and `#!rulePrefixTemplate` | ✅ Supported | Rule names are transformed before lowering. |
| Optimization controls such as `#!enableOptimizations`, `#!disableOptimizations`, `#!optimizeForSize` and `#!optimizeStrict` | 🚧 Partial | State, directive boundaries and the tested strict-sensitive lowering forms are preserved; every upstream optimizer transformation is not promised. |
| Replacement directives such as `#!replace0By*` and team/string replacements | 🚧 Partial | Directive state is recorded and supported replacements are lowered; backend-only transformations remain outside the contract. |
| `#!extension` | ✅ Supported | The extension name is checked against the canonical Workshop schema. |
| Translation, inspection, output and initialization directives | 🚧 Partial | Supported state and source diagnostics are preserved; directives requiring external output or an upstream-only backend are not claimed. |
| `#!postCompileHook` | ✅ Supported | The hook runs after final Workshop emission and failures retain script provenance. |
| `__script__(...)` JavaScript macros | ✅ Supported | The embedded QuickJS runtime exposes the documented OverPy ABI, limits, isolation and string-result contract. |

## Compilation and CLI

| Capability | Status | Limit |
| --- | --- | --- |
| Standalone Rust compiler library | ✅ Supported | The source and Workshop boundaries described on this page apply. |
| CLI `check`, `compile`, JSON reports and source diagnostics | ✅ Supported | Failure status never produces a misleading successful artifact. |
| Compile metadata for variables, subroutines, warnings, translations and element count | 🚧 Partial | The JSON report exposes the fields implemented by the native API; it is not a byte-for-byte clone of the upstream JavaScript object. |
| Observable optimizer and replacement effects | 🚧 Partial | Tested semantic and cost-relevant effects are preserved; formatting and upstream internal optimizer structure are not contracts. |
| OPY → Workshop emission through `workshop-rs` | 🚧 Partial | The supported language rows above compile end to end; unsupported source forms produce structured diagnostics. |

## Reconstruction

| Capability | Status | Limit |
| --- | --- | --- |
| Workshop → OPY reconstruction | ❌ Unsupported | `opy-rs` does not currently expose a public decompiler contract. |
| Source-identity-preserving compile/decompile round trip | ❌ Unsupported | Workshop output cannot retain comments, macros, original names or formatting. |
