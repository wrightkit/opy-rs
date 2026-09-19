# Syntax and project composition

## Lexical and expression forms

| Source form | Status | Limit |
| --- | --- | --- |
| `#` line comments and `/* ... */` block comments | ✅ Supported | Comments are retained for source attribution where the API exposes it. |
| Identifiers, indentation, `rule` blocks and `def` subroutines | ✅ Supported | Subroutine declarations follow the OverPy no-parameter/no-return form. |
| Boolean, integer, float and `null` literals | ✅ Supported | Numeric values are normalized at the canonical Workshop boundary. |
| Strings, escaped strings, adjacent strings and f-strings | ✅ Supported | Supported string modifiers are listed below. |
| String modifiers `f`, `w`, `l`, `b`, `c`, `t` | ✅ Supported | Translation output remains subject to the translation limits below. |
| Array literals, indexing and comprehensions | ✅ Supported | Supported comprehensions lower through canonical array operations. |
| Dictionary literals and literal-key lookup | ✅ Supported | Literal keys fold to the selected value. |
| Dictionary mutation through a computed assignment target | ❌ Unsupported | Canonical Workshop has no equivalent mutable dictionary target in this boundary. |
| `lambda` element/index binders | ✅ Supported | Binders are accepted in the callable contexts that define them. |
| Member access, calls and postfix expressions | ✅ Supported | Receiver and argument errors retain source locations. |
| `del array[index]` | ✅ Supported | Global and player-variable targets are lowered through three dimensions; arbitrary expressions and four-dimensional deletion are rejected. |
| Conditional values: `a if condition else b` | ✅ Supported | Chained forms are right-associative. |
| `in`, `not in`, arithmetic, comparison, boolean and unary operators | ✅ Supported | String membership uses the canonical Workshop operation. |
| `++` and `--` postfix assignment modifiers | ✅ Supported | Statement-level global, player and single-level indexed forms are supported; prefix/embedded forms are rejected. |
| `0x` and `0X` hexadecimal literals | ✅ Supported | Both spellings have the same numeric meaning. |

## Assignments and declarations

| Source form | Status | Limit |
| --- | --- | --- |
| `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `**=` | ✅ Supported | Global, player and indexed targets use their distinct lowering rules. |
| `min=` and `max=` modifications | ✅ Supported | The operation is checked and lowered as a canonical variable modification. |
| `globalvar` and `playervar`, with explicit or implicit indices | ✅ Supported | Initializers and zero defaults follow the source contract. |
| `enum` declarations | ✅ Supported | See the domain rules in [callables and domains](callables-and-domains.md). |
| Object, function and member `macro` declarations | ✅ Supported | Defaults, keyword arguments and member receivers are checked separately. |

## Rules, control flow and files

| Source form | Status | Limit |
| --- | --- | --- |
| Global and player rules with event/team/hero/slot filters | ✅ Supported | Event and domain arguments are resolved independently. |
| `@Condition`, `@Name`, `@Disabled`, `@Delimiter`, `@NewPage`, `@SuppressWarnings` | ✅ Supported | Each annotation has its own arity and placement rules. |
| `if` / `elif` / `else`, `while`, `do ... while` | ✅ Supported | Entry and reevaluation behavior are preserved. |
| `for ... in range(...)` | ✅ Supported | Global and player-variable range binders are supported; arbitrary iterables are not. |
| `switch` / `case` / `default`, `break` | ✅ Supported | Supported fall-through and break forms lower to canonical actions. |
| `continue` in loops | ✅ Supported | The pinned `for`, `while` and `do ... while` forms, including accepted nested control-flow positions, lower through canonical skip/loop actions. |
| `goto`, labels and `loc+` targets | 🚧 Partial | Forward labels, `loc+` offsets, conditional dynamic jumps and `RULE_START` lower natively; backward and otherwise unrestricted jumps remain unsupported. |
| `pass` and `return` | ✅ Supported | Context restrictions remain source diagnostics. |
| `#!include` and nested include closure | ✅ Supported | Main-file and including-file-relative resolution are preserved, including include cycles and missing-file diagnostics. |
| `#!mainFile` and directory project input | ✅ Supported | The selected entry source remains part of the project identity. |

## Strings, translations and settings

| Source form | Status | Limit |
| --- | --- | --- |
| `#!translations` language selection | ✅ Supported | Invalid language codes are rejected explicitly. |
| Translation declarations and `.po` output lifecycle | 🚧 Partial | The native compiler preserves supported translation state and emits supported localized Workshop text; it does not claim the full upstream translation toolchain. |
| `settings { ... }` custom-game-settings block | 🚧 Partial | Main-file and included-file blocks use the canonical Workshop settings tables; only the currently exposed schema slice is accepted. |
| Settings enum, map, hero and numeric-range values | ✅ Supported | Validation and emission belong to `workshop-rs`. |
