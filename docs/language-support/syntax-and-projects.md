# Syntax and project composition

## Lexical and expression forms

| Source form | Status | Limit |
| --- | --- | --- |
| `#` line comments and `/* ... */` block comments | ✅ Supported | Comments are retained for source attribution where the API exposes it. |
| Identifiers, indentation, `rule` blocks and `def` subroutines | ✅ Supported | Subroutine declarations follow the OverPy no-parameter/no-return form. |
| Boolean, integer, float and `null` literals | ✅ Supported | Numeric values are normalized at the canonical Workshop boundary. |
| Strings, escaped strings, named entities, adjacent strings and f-strings | ✅ Supported | Named `\&entity;` escapes use the pinned OverPy entity table and reject unknown or malformed entities with source diagnostics. Supported string modifiers are listed below. |
| String modifiers `f`, `w`, `l`, `b`, `c`, `t` | ✅ Supported | Translation output remains subject to the translation limits below. |
| Array literals, indexing and comprehensions | ✅ Supported | Supported comprehensions lower through canonical array operations. |
| Dictionary literals and literal-key lookup | ✅ Supported | Literal keys fold to the selected value. |
| Dictionary mutation through a computed assignment target | ❌ Unsupported | Canonical Workshop has no equivalent mutable dictionary target in this boundary. |
| `lambda` element/index binders | ✅ Supported | Binders are accepted in the callable contexts that define them. |
| Member access, calls and postfix expressions | ✅ Supported | Receiver and argument errors retain source locations. |
| `del array[index]` | ✅ Supported | Global and player-variable targets are lowered through four source indexes; five-index deletion and nested random outer/middle indexes are rejected at the pinned OverPy boundary. |
| Conditional values: `a if condition else b` | ✅ Supported | Chained forms are right-associative. |
| `in`, `not in`, arithmetic, comparison, boolean and unary operators | ✅ Supported | String membership uses the canonical Workshop operation. |
| `++` and `--` postfix assignment modifiers | ✅ Supported | Statement-level global, player and single-level indexed forms are supported; prefix/embedded forms are rejected. |
| `0x`/`0X`, `0b`/`0B` and `0o`/`0O` integer literals | ✅ Supported | The radix spellings have the same numeric meaning as the pinned JavaScript tokenizer. |

## Assignments and declarations

| Source form | Status | Limit |
| --- | --- | --- |
| `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `**=` | ✅ Supported | Global, player and indexed targets use their distinct lowering rules. |
| `min=` and `max=` modifications | ✅ Supported | The operation is checked and lowered as a canonical variable modification. |
| `globalvar` and `playervar`, with explicit or implicit indices | ✅ Supported | Initializers and zero defaults follow the source contract; an explicit index may be combined with an initializer. Explicit indices outside 0–127 are rejected at the pinned OverPy boundary. Non-decimal index spellings the reference's `parseInt` accepts (`0x10`, `3x`) are a recorded acceptance gap (`synthetic/variable-index-spelling-gap` fixture). |
| `subroutine` declarations, with explicit or implicit indices | ✅ Supported | An explicit index outside 0–127 is rejected at the pinned OverPy boundary. Tokens after `=` on the declaration line are dropped without being parsed, matching the pinned reference. |
| `enum` declarations | ✅ Supported | See the domain rules in [callables and domains](callables-and-domains.md). |
| Object, function and member `macro` declarations | ✅ Supported | Defaults, keyword arguments and member receivers are checked separately. |

## Rules, control flow and files

| Source form | Status | Limit |
| --- | --- | --- |
| Global and player rules with event/team/hero/slot filters | ✅ Supported | `@Event` takes a bare event name; the `@Team`, `@Slot` and `@Hero` filters resolve independently. |
| Parenthesized `@Event` arguments such as `@Event playerDied(victim)` | ❌ Unsupported | The pinned reference accepts only a bare event name (`takes 1 argument`); `opy-rs` rejects the argument list with a source-attributed diagnostic because canonical WIR carries structural event filters, not bound event variables. |
| `@Condition`, `@Name`, `@Disabled`, `@Delimiter`, `@SuppressWarnings` | ✅ Supported | Each annotation has its own arity and placement rules. `@Name` renames a `def` subroutine; on a `rule` it is rejected because the header already declares the name. |
| `@NewPage` | ❌ Unsupported | The pinned reference expands it into empty delimiter rules padding the list to a 100-rule page boundary; that page-layout expansion is outside the canonical WIR contract, so `opy-rs` rejects it with a source-attributed `unsupported-integration-surface` diagnostic (`synthetic/directives` corpus fixture). |
| Other `@…` annotation names | ❌ Unsupported | An unrecognized annotation is a parse diagnostic (`unsupported directive`), matching the reference's `Unknown annotation` rejection. |
| `if` / `elif` / `else`, `while`, `do ... while` | ✅ Supported | Entry and reevaluation behavior are preserved. A lone `elif`/`else` that does not directly continue an `if`/`elif` chain in the same block is rejected: the pinned reference warns `w_lone_else` and emits the marker anyway, producing unbalanced `Else If`/`Else` actions that canonical Workshop cannot represent (wrightkit/opy-rs#516; `synthetic/orphan-else-marker-gap` fixture). |
| `for ... in range(...)` | ✅ Supported | Global and player-variable range binders are supported; arbitrary iterables are not. |
| `switch` / `case` / `default`, `break` | ✅ Supported | Supported fall-through and break forms lower to canonical actions. |
| `continue` in loops | ✅ Supported | The pinned `for`, `while` and `do ... while` forms, including accepted nested control-flow positions, lower through canonical skip/loop actions. |
| `goto`, labels and `loc+` targets | ✅ Supported | Forward labels, `loc+` offsets, conditional dynamic jumps and `RULE_START` lower natively; named `goto`/`label` pairs resolve at rule and subroutine level, inside `if` branches, and through the `if …: goto <label>` jump-forward form in loop bodies. Backward named jumps are rejected by the pinned OverPy reference and are therefore outside the source contract. A bare `goto <label>` inside a `while`, `for`, `do ... while` or `switch` body — and any label inside a `switch` arm — is rejected with an `unsupported-integration-surface` diagnostic; a label inside a loop body is an inert jump target for the conditional form only. |
| `pass` and `return` | ✅ Supported | Context restrictions remain source diagnostics. |
| `#!include` and nested include closure | ✅ Supported | Main-file and including-file-relative resolution are preserved, including include cycles and missing-file diagnostics. |
| `#!mainFile` and directory project input | ✅ Supported | The selected entry source remains part of the project identity. |

## Strings, translations and settings

| Source form | Status | Limit |
| --- | --- | --- |
| `#!translations` language selection | ✅ Supported | Invalid language codes are rejected explicitly. |
| Translation declarations and `.po` output lifecycle | ✅ Supported | The compiler imports language-matched `.po` files, lowers `_`/`__`/`___`, and writes used entries for each non-base language. |
| `settings { ... }` and `settings "settings.opy.json"` custom-game-settings blocks | ✅ Supported | Inline and external JSONC objects use the canonical Workshop settings tables for validation and emission. |
| Settings enum, map, hero and numeric-range values | ✅ Supported | Validation and emission belong to `workshop-rs`. The emitted hero-ability settings keys are the one known divergence from the 9.7.17 pin: upstream re-slotted Roadhog's `Take a Breather` from `secondaryFire` to `ability2` (dropping `Pig Pen`), and `opy-rs` still emits the earlier names — a recorded pending exception (wrightkit/opy-rs#565). |
