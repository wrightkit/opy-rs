# OverPy audited inventory: syntax and project composition

Source: pinned OverPy `9.7.10`, content commit
`889d9749d1def17f146548cbddb94ea1ab015847`. The source surfaces used are
`README.md`, `src/compiler/tokenizer.ts`, `parser.ts`, `astParser.ts`,
`src/data/opy/keywords.ts`, `annotations.ts`, `preprocessing.ts`,
`modules.ts`, `macros.ts`, and the upstream files under `src/tests/`.

## Lexical and expression surface

| Feature | Status | Notes |
| --- | --- | --- |
| `#` line comments and `/* ... */` block comments | ✅ Supported | Source parsing is covered by the native pipeline and corpus. |
| Identifiers, indentation and rule/subroutine blocks | ✅ Supported | Includes `rule "name":` and `def name():`. |
| Boolean, integer, float and `null` literals | ✅ Supported | Numeric edge cases remain conformance work. |
| Strings, escaped strings and implicit concatenation | ✅ Supported | String modifiers are separate rows. |
| f-string/interpolated strings | ✅ Supported | Supported formatting subset is fixture-covered. |
| String modifiers `f`, `w`, `l`, `b`, `c`, `t` | ✅ Supported | Each modifier is a distinct lexical form. |
| Array literals and indexing | ✅ Supported | Includes nested arrays. |
| Dictionary literals and keyed access | 🚧 Coming soon | Source analysis exists; full compilation is incomplete. |
| List comprehensions | ✅ Supported | Mapping and filtering are separate behaviors. |
| `lambda` with element/index binders | ✅ Supported | Valid positions are contextual. |
| Member access, calls and postfix expressions | ✅ Supported | Receiver and dispatch checks are contract-sensitive. |
| `del` array element statement | 🚧 Coming soon | Audited upstream keyword; compilation support is incomplete. |
| Conditional value `a if condition else b` | ✅ Supported | Chained forms are right-associative; distinct from statement `if`. |
| `in` and `not in` membership | ✅ Supported | String containment uses `strContains`. |
| Arithmetic, comparison, boolean and unary operators | ✅ Supported | Augmented forms are separate rows below. |
| `++` and `--` postfix modifiers | 🚧 Coming soon | Audited upstream operator surface. |
| `0x`/`0X` hexadecimal literals | ✅ Supported | Case variants are one semantic capability. |

## Assignments and declarations

| Feature | Status | Notes |
| --- | --- | --- |
| Simple assignment `=` | ✅ Supported | Global, player and indexed forms differ at lowering. |
| `+=`, `-=`, `*=`, `/=`, `%=` | ✅ Supported | Each spelling is independently audited. |
| `**=` augmented assignment | ✅ Supported | Separate from `**`; uses Raise To Power. |
| `min=` and `max=` modification forms | 🚧 Coming soon | Recognized by the audit; Workshop support is not claimed. |
| `globalvar name [index]` | ✅ Supported | Explicit and implicit index forms are distinct. |
| `playervar name [index]` | ✅ Supported | Explicit and implicit index forms are distinct. |
| Variable initializer `globalvar/playervar name = value` | ✅ Supported | Constant-zero behavior is observable. |
| `enum` declarations and inferred member values | ✅ Supported | Contextual enum use is separate. |
| `macro` constants and function macros | ✅ Supported | Member/default-parameter forms are separate contracts. |
| `def` subroutines, calls and `return` | ✅ Supported | Upstream subroutines have no parameters or returns. |

## Rules, control flow and project composition

| Feature | Status | Notes |
| --- | --- | --- |
| Rule events: `global`, `eachPlayer`, team/hero/slot domains | ✅ Supported | Event and domain arguments are distinct. |
| `@Condition` and multiple conditions | ✅ Supported | |
| `@Name`, `@Disabled`, `@Delimiter`, `@NewPage`, `@SuppressWarnings` | ✅ Supported | Each annotation has independent effects. |
| `if` / `elif` / `else` statements | ✅ Supported | Inline conditional values are separate. |
| `for ... in range(start, stop, step)` | ✅ Supported | Global/player binders are separate. |
| `while` and `do ... while` loops | ✅ Supported | Distinct entry-condition behavior. |
| `switch` / `case` / `default` | ✅ Supported | Fall-through and `break` are separate. |
| `break` in loops and switch arms | ✅ Supported | |
| `continue` in loops | 🚧 Coming soon | Upstream keyword exists; end-to-end support is incomplete. |
| `goto`, labels and dynamic `loc+` targets | 🚧 Coming soon | Audited from keyword registry and `src/tests/gotos.opy`. |
| `pass` and `return` statements | ✅ Supported | Context restrictions remain conformance work. |
| `#!include` root-relative composition | ✅ Supported | Missing files and cycles have distinct failures. |
| Nested include closure and main-file selection | ✅ Supported | Project behavior is not inferred from one-file tests. |

## Settings, strings and translations

| Feature | Status | Notes |
| --- | --- | --- |
| `settings { ... }` custom-game-settings block | ✅ Supported | Lowered through the canonical `workshop-rs` settings carrier and emitter. |
| Schema keys, enum values and map/hero list settings | ✅ Supported (bounded) | Validation and spellings come from the Workshop-owned catalog; unsupported keys remain explicit failures. |
| `#!translations` and `.po` translation sources | 🚧 Coming soon | Declaration and output lifecycle are separate. |
| `_`, `__`, `___` translation functions | 🚧 Coming soon | One- and two-argument modes differ. |
| Localized output language selection | ✅ Supported (catalog-declared) | Undeclared locales and missing mappings fail explicitly; no guessed fallback. |
