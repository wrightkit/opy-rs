# Tooling and compiler backend

## Preprocessing, macros and hooks

| Source form | Status | Limit |
| --- | --- | --- |
| `#!define` and function/member macros | ✅ Supported | Expansion order and definition/use-site provenance follow OverPy. |
| `#!allowMacroRedeclaration` | ✅ Supported | Duplicate-definition policy is explicit in preprocessing state. |
| `#!mainFile`, `#!include`, `#!excludeVariablesInCompilation` | ✅ Supported | File selection, include closure and output filtering are separate operations. |
| `#!rulePrefix` and `#!rulePrefixTemplate` | ✅ Supported | Rule names are transformed before lowering. |
| Optimization controls such as `#!enableOptimizations`, `#!disableOptimizations`, `#!optimizeForSize`, `#!optimizeForSizeAggressive` and `#!optimizeStrict` | ✅ Supported | Optimization state is scoped to source spans; size, aggressive skip, strict-folding and wait-duration effects are lowered. Under `#!optimizeForSize`, `0`, `1`, `Null` and zero-vector arguments are spelled as `False`, `True` and `Null` per parameter as pinned OverPy 9.7.17 does, encoded in typed Rust (`tools/overpy/gen_literal_flags.cjs` records the upstream flags that a test checks the policy against); without it authored literals are kept; in rule-condition comparison operands only `0` becomes `Null`. The emitted structure converges on upstream apart from recorded exceptions, and the Bastion `main.opy` and `externalMain.opy` structural gate reports no unrecorded differences; the internal optimizer implementation is not a contract. |
| Replacement directives such as `#!replace0By*`, `#!replace1ByMatchRound`, team and empty-string replacements | ✅ Supported | Observable replacements are lowered when size optimization is active and are excluded from Workshop-setting constructors, matching the pinned upstream boundary. |
| `#!extension` | ✅ Supported | The extension name is checked against the canonical Workshop schema. |
| `#!disableInspector`, `#!excludeVariablesInCompilation`, `#!setupTags`, `#!setupTx`, `#!globalvarInitRuleName` and `#!playervarInitRuleName` | ✅ Supported | Inspector/setup rules, output declaration filtering and generated initialization rule names affect forward Workshop output. |
| `#!translations` and `#!translateWithPlayerVar` | ✅ Supported | Language selection, `.po` import/output, dynamic player-language lookup, and the `noDetectionRule`/`noTlErr` options lower through the forward compiler. |
| `#!suppressWarnings` | ✅ Supported | Matching preprocessing warning codes are omitted from the public diagnostics surface. |
| `#!debugElementCount` | ✅ Supported | The forward compiler emits a canonical-WIR total, a count-sorted per-rule summary, and per-condition/per-action element-count comments. |
| `#!writeToOutputFile`, `#!disableTranslationSourceLines` and `#!keepUnusedTranslations` | ❌ Unsupported | Intentionally out of scope: these are editor or translation-output controls, not forward-language semantics. The compiler records them and rejects them with a source-attributed diagnostic rather than pretending to implement their side effects. |
| `#!postCompileHook` | ✅ Supported | The hook runs after final Workshop emission and failures retain script provenance. |
| `__script__(...)` JavaScript macros | ✅ Supported | The embedded QuickJS runtime exposes the documented OverPy ABI, limits, isolation and string-result contract. |

## Compilation and CLI

| Capability | Status | Limit |
| --- | --- | --- |
| Standalone Rust compiler library | ✅ Supported | The source and Workshop boundaries described on this page apply. |
| CLI `check`, `compile`, JSON reports and source diagnostics | ✅ Supported | Failure status never produces a misleading successful artifact. |
| Compile metadata for variables, subroutines, warnings, translations and element count | 🚧 Partial | Tooling-only: the JSON report exposes the fields implemented by the native API; it is not a byte-for-byte clone of the upstream JavaScript object, and its shape is not a forward-language support gate. |
| Observable optimizer and replacement effects | ✅ Supported | Tested semantic and cost-relevant effects are preserved; formatting and upstream internal optimizer structure are not contracts. |
| Filtered-word escaping | ✅ Supported | Rule names and the `Mode Name` and `Description` settings strings lose invisible formatting characters and have each filtered word split with a soft hyphen, as the pinned upstream does ([ADR-0007](../adr/0007-filtered-word-escaping.md)). |
| Builtin constant folding and number spelling | ✅ Supported | Builtin calls on literal arguments fold to the value the pinned upstream computes, and every number is written cut after fifteen decimals. |
| Per-action rewrites | ✅ Supported | Empty HUD text becomes `Null` and a HUD text with no text is dropped; a `Null` Boolean argument is wrapped in `First Of`; other single-action rewrites follow the pinned upstream under the same optimization state. |
| Builtin-call structural convergence | ✅ Supported | Every catalog-backed builtin is probed against the pinned upstream ([`tools/overpy/README.md`](../../tools/overpy/README.md)). Every remaining difference is an approved exception with a recorded owner: probe-reachable families are listed with their cause and decision in `tools/overpy/probe-gaps.json`, and the families without a probe-gap record (the folded vector-`x - x` position, non-finite `log` folds, a non-array `sorted` argument, a numeric `Team` literal, and the `timeToString`/`buttonToString` shapes) are pinned by `compiler::tests::structural_convergence` tests. Both sets appear in the [language-core exception tables](../architecture/language-core.md#approved-exceptions). |
| OPY → Workshop emission through `workshop-rs` | ✅ Supported | Every source form marked `Supported` on these pages lowers end to end to canonical WIR and emits Workshop text; the emitted structure converges on the pinned reference apart from the recorded exceptions — the approved and pending exception tables in [`language-core.md`](../architecture/language-core.md#approved-exceptions) plus the known-gap corpus fixtures whose remaining semantic-WIR differences are pinned by `semanticDifferenceSha256`. A source form outside the contract fails with a source-attributed structured diagnostic instead of emitting an approximation — the rejected forms are named on the source pages and summarized in the row below. |
| Source forms outside the forward contract | ❌ Unsupported | `@NewPage`, parenthesized `@Event` arguments and unrecognized `@…` names, orphan `elif`/`else` markers that emit unbalanced `Else`/`Else If` actions upstream (wrightkit/opy-rs#516), and `goto <label>` outside the supported subset are rejected with a source-attributed diagnostic — see the per-form rows in [Syntax and project composition](syntax-and-projects.md). `.x`/`.y`/`.z` on the context-player receivers `eventPlayer`, `hostPlayer`, `localPlayer`, `attacker` and `victim` is rejected at canonical validation — see [Callables and domains](callables-and-domains.md). |

## Workshop output language

`opy-cli compile --language` and the `Compiler` language-selection APIs emit
localized Workshop source through the canonical `workshop-rs` catalog. The
supported pinned OverPy 9.7.17 output locales are:

`de-DE`, `en-US`, `es-ES`, `es-MX`, `fr-FR`, `it-IT`, `ja-JP`, `ko-KR`,
`pl-PL`, `pt-BR`, `ru-RU`, `th-TH`, `tr-TR`, `zh-CN`, and `zh-TW`.

This output-language selection is separate from OverPy `#!translations` and
`.po` files: output language selects the localized Workshop syntax emitted for
the game client, while runtime translation support selects user-authored string
translations in the compiled program.

## Reconstruction

| Capability | Status | Limit |
| --- | --- | --- |
| Workshop → OPY reconstruction | 🚧 Partial | `opy_rs::reconstruct` reconstructs the bounded subset the [reconstruction contract](../architecture/reconstruction.md) marks supported: a canonical `Program` without settings, within the contract's table, rule/event, condition, action, and value forms. Constructs outside the subset fail with a `ReconstructError` that names every unsupported location with a stable code and span — no partial output. The same bounded surface is served over LPP by `opy-provider`'s `lpp/reconstruct` (see [the provider documentation](../opy/provider.md#reconstruction)), which is what `wright convert --target opy` calls. The contract also records each contracted coverage extension and each justified refusal against the pinned upstream `decompileAllRules` input bar. |
| Source-identity-preserving compile/decompile round trip | ❌ Unsupported | Workshop output cannot retain comments, macros, original names or formatting; see the contract's unrecoverable-information boundary. |
