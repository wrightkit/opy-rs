# Workshop → OPY Reconstruction Contract

This document is the owner contract for reconstructing OverPy source from the
canonical public `workshop_rs::Program` model. It records the accepted
supported subset, the refusal model, the semantic-equivalence criteria, the
input-coverage bar against the pinned upstream decompiler, and the recorded
exceptions and follow-up work. Issue `wrightkit/opy-rs#254`; the owner
decisions of 2026-10-18 recorded there, amended 2026-10-19 (initializer rules
keep their program position, `wrightkit/opy-rs#597`), are the authority for
this contract. `wrightkit/opy-rs#602` and `wrightkit/opy-rs#603` implemented
the syntax-produced action/value extensions recorded below (player-variable
loops and arbitrary-player access, self-referential `Set`, the
remove/min/max modify ops, indexed assignment, `async`, skip/label
generation, terminal abort/loop forms, negative numbers, localized
strings, syntax-produced value ids, member/component access, and
lambda binders).

## Purpose

Reconstruction exists so users can migrate existing Workshop modes — exported
from the game client or another compiler — into maintainable OverPy source.
This is the same purpose as the upstream `decompileAllRules` decompiler.
Reconstruction is not limited to programs shaped like `opy-rs`'s own compiler
output; the contract covers what real Workshop exports contain.

## Boundary

```text
Workshop text        (owned by workshop-rs: parse, validate, represent)
   ↓  workshop_rs::parser
workshop_rs::Program (canonical public model — the reconstruction input)
   ↓  opy_rs::reconstruct
OPY source           (deterministic, valid, standalone)
   ↓  opy-rs compile + workshop_rs::parser   (round-trip verification)
workshop_rs::Program ≡ input under the equivalence contract below
```

- The input is the canonical public `Program`. Reconstruction uses public
  `Program` concepts and provenance spans only; it never reads internal WIR
  or arena storage and never parses raw `.ws` text itself.
- The output is standalone OPY text: no project files, no external metadata,
  no required side channel. Output must be deterministic for a given input —
  the same `Program` always reconstructs to the same source.
- The public entry points are `opy_rs::reconstruct::reconstruct(&Program)`
  and `opy_rs::reconstruct::reconstruct_with(&Program, &Manifest, &Catalog,
  &Locale)`. The catalog locale selects the spelling layer used for
  cross-checks; emitted OPY is locale-independent (`en-US` is the catalog's
  declared surface).
- `opy-rs` owns the reconstruction mapping and the generated OPY structure.
  `workshop-rs` owns canonical Workshop meaning, the `Program` model,
  settings/localization facts, parsing, emission, and any comparison of
  canonical Workshop semantics. No OPY-local Workshop semantic model or
  parser fallback is introduced; a genuinely missing canonical capability is
  a `workshop-rs` requirement (see [Missing workshop-rs
  capabilities](#missing-workshop-rs-capabilities)).

## Failure model

Reconstruction refuses the whole program on any unsupported construct: no
partial output is ever returned. The emitter collects every unsupported
location and returns `ReconstructError { issues }`, where each
`ReconstructIssue` carries a stable `code`, a human `message`, and the
`Program` provenance `span` when one exists. Message wording is not a
contract; codes and span attribution are.

Two refusal classes exist and must stay distinguishable:

- **Surface refusals** — the construct is outside the OPY reconstruction
  surface (no valid OPY form exists, or none that recompiles to an
  equivalent program). These are the `unsupported-*` codes.
- **Guard refusals** — the input cannot be emitted faithfully or the
  reconstruction environment failed (`manifest-error`, `catalog-error`,
  `unsupported-dangling`, `unsupported-control-flow` on malformed action
  streams, `unsupported-invalid-arity`, `unsupported-missing-argument`).
  Guard refusals are not coverage gaps; they remain regardless of upstream
  coverage.

A refusal is never a best guess: when no justified equivalent form exists,
reconstruction refuses rather than approximating semantics.

## Semantic equivalence

Reconstructed OPY is correct when it recompiles to a program
behaviorally equivalent to the input. The comparison is canonical Workshop
meaning, not emitted-text identity and not strict structural identity.

`workshop_rs::roundtrip::equivalent` is **not** the contract. It requires
identical settings, tables, rule names, and action/value identity, which is
stricter than the goal and is the reason several constructs are refused
today. It remains valid evidence for the frontend-shaped subset — a program
that round-trips under it is also behaviorally equivalent — but it must not
be required by the contract.

### Compared structure

The comparator checks behavior-relevant structure:

- rule order, except the relative order of subroutine-event rules (below);
- rule events, including team/hero/slot filters;
- enabled rule conditions, in order;
- actions and values, recursively, with variable/subroutine links resolved
  through declaration indices;
- global-variable, player-variable, and subroutine indices;
- settings.

### Allowed differences

Each allowed difference is enumerated here with the reason it cannot change
observable behavior. A difference outside this list is a defect.

| Allowed difference | Why it cannot change behavior |
| --- | --- |
| Rule names | Rule names are display metadata; nothing dispatches on them. |
| Variable and subroutine names | Identity is the declaration index; every reference rebinds through the declared index. Consistent renames (e.g. identifier mangling) are unobservable. |
| Declaration table order | Only the name → index binding matters; table position carries no execution effect. |
| Relative order of subroutine-event rules | Subroutine-event rules run only when called, never by program position. |
| `Set x = x <op> v` ↔ `Modify x <op> v` | The frontend normalizes the self-referential Set to the Modify; both spellings produce the same canonical action. |
| `eachPlayer` event ↔ `eachPlayer` + `All`/`All` filters | The unfiltered event and the all-filters form select the same players (existing `equivalent` behavior, retained). |
| Disabled rules, conditions, and actions | Disabled constructs never execute. They are reconstructed for fidelity (see the mapping table) but carry no observable weight. |
| Rules with no actions | A rule with no actions performs no effect regardless of its conditions. |
| Source spans, file identities, and emitted text layout | Provenance and formatting are not program semantics. |

Algebraic or cost identities beyond this list — `Add(x, 0)` ≡ `x`,
`Add(x, y)` ≡ `Add(y, x)`, `Set` ordering across independent variables — are
**not** allowed differences. Neither is relocating a non-subroutine rule,
including an initializer rule: within one tick the execution order of Global
Ongoing rules is observable, so moving an initializer rule to the front can
change behavior (owner decision 2026-10-19). Reconstruction must not require
the comparator to reason about evaluation equivalences; see the
`#!optimizeStrict` requirement below.

### Comparator ownership

The comparator belongs to `workshop-rs`: it judges canonical Workshop
meaning and must be usable by every source-language implementation. The
compared structure and allowed differences above are the
`wrightkit/workshop-rs#490` contract, delivered as
`workshop_rs::compare::equivalent` / `workshop_rs::compare::difference`
(workshop-rs ≥ 1.20). This contract does not license an OPY-local
comparator; `workshop_rs::roundtrip::equivalent` remains valid evidence
for the strict-identity subset only.

## Supported surface by canonical construct

Statuses: **supported** — implemented today (the strict-subset round-trip
suite is the evidence); **extension** — contracted mapping requiring an
implementation follow-up; **refusal** — recorded, justified exception (see
the coverage section).

### Program shape and declarations

| Canonical construct | OPY mapping | Status |
| --- | --- | --- |
| `program.settings` | `settings { … }` JSONC block built from canonical settings data | extension |
| Global variable, ascending index order, no initializer | `globalvar <name> <index>` | supported |
| Global variable, out-of-order indices | Emit declarations sorted by index with explicit indices | extension (relaxes `unsupported-global-order`) |
| Global variable with initializer, re-lowered lowest-free slot = index | `globalvar <name> = <value>` | supported |
| Global variable with initializer and any other index | Requires `globalvar <name> <index> = <value>` (the combined form is part of the grammar) or recorded refusal | extension / refusal |
| Player variable, any order, no initializer | `playervar <name> <index>` | supported |
| Player variable with initializer | `playervar <name> = <value>` | supported |
| Subroutine at table position | `subroutine <name>` + `def <name>():` | supported |
| Subroutine at non-position index | Requires `subroutine <name> <index>` (the indexed form is part of the grammar) or recorded refusal | extension / refusal |
| Duplicate names inside one table | No faithful declaration form exists | refusal (`unsupported-duplicate-name`) |
| Names that are not valid OPY identifiers or collide with keywords | Deterministic documented mangling, or refusal when no mangling is recorded | extension |

### Rules and events

| Canonical construct | OPY mapping | Status |
| --- | --- | --- |
| `Event::Global` rule | `rule "<name>": @Event global` | supported |
| `Event::EachPlayer`, incl. `All`/`All` filters | `rule "<name>": @Event eachPlayer` | supported |
| `Event::EachPlayerWithFilters` (non-`All`) | `@Event eachPlayer` + `@Team`/`@Hero`/`@Slot` | extension (relaxes `unsupported-rule-event`) |
| `Event::Player { kind, team, target }` | `@Event <catalog-id>` + filters | extension |
| `Event::Subroutine` body rules | `def` blocks | supported in canonical layout; ordering/name-shape relaxations under the allowed differences are extension |
| Disabled rule | `@Disabled` | extension (relaxes `unsupported-disabled-rule`) |
| Rule with zero actions | Inert; may be omitted or emitted as `@Delimiter` | supported (omitted) |
| Subroutine rule with zero actions | `def` body needs a statement (`pass`) to parse | extension |
| Canonical initializer rules (frontend-shaped name, event, all-Set, table order) | Declaration initializers | supported |
| Initializer-named rules not in the frontend's canonical leading position, or not otherwise frontend-shaped | Ordinary `rule` emission at the rule's original position — initializer rules are never relocated | extension (relaxes `unsupported-init-rule` for position or shape mismatches that the ordinary-rule path reproduces) |

### Conditions

| Canonical construct | OPY mapping | Status |
| --- | --- | --- |
| Enabled condition | `@Condition <value>` | supported |
| Disabled condition | `#`-commented `@Condition` line (unobservable; kept for fidelity) or dropped | extension (relaxes `unsupported-disabled-condition`) |

### Actions

| Canonical construct | OPY mapping | Status |
| --- | --- | --- |
| `If`/`ElseIf`/`Else`/`End` structure | `if`/`elif`/`else` blocks | supported |
| `While` | `while <cond>:` | supported |
| `ForGlobalVariable` | `for <g> in range(<start>, <stop>, <step>):` | supported |
| `ForPlayerVariable` | `for <player>.<v> in range(...):` (the OPY `for` binder accepts a player-variable member on any receiver) | supported |
| `SetGlobalVariable` | `<g> = <value>` | supported |
| `SetPlayerVariable` on `EventPlayer` | `eventPlayer.<v> = <value>` | supported |
| `Set/ModifyPlayerVariable` on an arbitrary player expression | `(<player>).<v> = <value>` / `<player>.<v> <op>= <value>` (player-variable member access accepts any receiver) | supported |
| `Set` whose value is a self-referential binary (`x = x <op> v`) | Emit as written; recompiles to `Modify` (allowed difference) | supported |
| `Modify` Add/Subtract/Multiply/Divide/Modulo/RaiseToPower | `<v> <op>= <value>` or `<v> = <v> <op> <value>` | supported |
| `Modify` AppendToArray | `<v>.append(<value>)` | supported |
| `Modify` RemoveFromArrayByValue / RemoveFromArrayByIndex / Min / Max | `<v>.remove(<value>)` / `del <v>[<i>]` / `min=` / `max=` | supported |
| `AssignMember` (dynamic member-write target) | No OPY source form; `.x`/`.y`/`.z` member writes are also a pending canonical-validation exception | refusal (`unsupported-member-assignment`) |
| `CallSubroutine` to a declared subroutine | `<name>()` | supported |
| Disabled action | `#`-commented statement line (unobservable; kept for fidelity) | extension (relaxes `unsupported-disabled-action`) |
| `Action::Call` resolvable through the manifest (incl. member calls) | `<fn>(<args>)` / `<recv>.<fn>(<args>)` | supported |
| `Action::Call` for syntax-produced ids — `set*/modify*AtIndex`, `startRule`, `skip`, `skipIf`, `abortIf*`, `loopIf*` | Indexed assignment forms `v[i] = x` / `v[i] <op>= x`; `async(<sub>, <behavior>)`; `goto`/label or dynamic-`goto` synthesis; `if <c>: return` / `if <c>: loop()`; `if [not] ruleCondition: return` / `loop()` | supported |
| `Action::Call` with no OPY form and no syntax mapping | — | refusal (`unsupported-action-call`) |
| `Action::Call` `append` (dedicated lowering) | Never produced canonically | guard (`unsupported-action-call`) |
| Missing `End` / malformed marker sequence | No faithful source form | guard (`unsupported-control-flow`) |
| Variable-required parameter not a variable (`chase*` arg 0) | Cannot reproduce the typed policy | guard (`unsupported-invalid-argument`) |
| Omitted non-optional parameter / arity overflow | Recompiling would default-fill or reject | guard (`unsupported-missing-argument`, `unsupported-invalid-arity`) |
| Dangling variable/subroutine reference | Cannot rebind | guard (`unsupported-dangling`) |
| Zero-argument action ambiguous with a same-named subroutine | Ambiguous surface | refusal (`unsupported-action-call`) |

### Values

| Canonical construct | OPY mapping | Status |
| --- | --- | --- |
| Finite non-negative `Number` | Canonical number spelling (`workshop_rs::format`) | supported |
| Negative `Number` | `-<abs>` (the frontend folds unary minus on literals back to `Number`) | supported |
| Non-finite `Number` (`NaN`, ±∞) | No OPY spelling produces a non-finite literal | refusal (`unsupported-non-finite-number`) |
| `String` | Escaped `"…"` literal | supported |
| `LocalizedString` | `l"…"` localized literal (and `l"…".format(...)` for argument forms) | supported |
| `Bool`, `Null` | `true` / `false` / `null` | supported |
| `Array` literal | `[e, …]` | supported |
| `Vector` | `vect(x, y, z)` | supported |
| `Enum` member in a source-exposed domain with an OPY spelling | `<Domain>.<SOURCE-SPELLING>` (never the catalog id) | supported |
| `Enum` member in a non-source domain, or without an OPY spelling | — | refusal (`unsupported-enum-domain`, `unsupported-enum-member`, `unsupported-enum-domain-mismatch`) |
| `GlobalVariable`, `PlayerVariable` on `EventPlayer` | `<name>` / `eventPlayer.<name>` | supported |
| `PlayerVariable` on an arbitrary player expression | `(<player>).<name>` | supported |
| `Subroutine` as a value (e.g. `startRule` argument) | Bare subroutine name in the positions OPY accepts one (e.g. `async` argument); refusal elsewhere | supported in accepted positions |
| `EventPlayer` | `eventPlayer` | supported |
| Operator calls (`+ - * / % ** == != < <= > >= and or`, unary `not`, unary `-`) | `(a <op> b)`, `(not a)`, `(-a)` | supported |
| `customString` with a string receiver | `"…".format(args…)` | supported |
| `localPlayer` (bare) | `localPlayer` | supported |
| `Value::Call` resolvable through the manifest (incl. member calls) | `<fn>(args…)` / `<recv>.<fn>(args…)` | supported |
| `Value::Call` for syntax-produced ids — `valueInArray`, `sortedArray`, `mappedArray`, `filteredArray`, `isTrueForAll`/`isTrueForAny`, `arrayContains`, `firstOf`, `emptyArray`, `slice`, `ifThenElse`, `roundToInteger`, `currentArrayElement`/`Index`, `string` (localized first arg), `memberAccess`, `__xComponentOf__`/`__yComponentOf__`/`__zComponentOf__` | `(a)[i]` (literal-zero index stays refused — it re-lowers to `firstOf`), `sorted(a[, lambda e: k])`, `(a).map`/`.filter`/`.all`/`.any` + `lambda`, `(x in a)`, `(a)[0]`/`[]`, `(a).slice(start[, count])`, `(t if c else e)`, `round`/`floor`/`ceil`, deterministic lambda binders (`__elem_N`/`__idx_N`), `l"…"`/`l"…".format(...)`, `(recv).member`, `(recv).x`/`.y`/`.z` | supported |
| Dedicated-lowered value ids (`vect`, `range`, `chase`) outside their syntax positions | `range` only inside `for`; `vect`/`chase` are not canonical value-call names | refusal / guard (`unsupported-value-call`) |
| `Value::Call` with no OPY form and no syntax mapping | — | refusal (`unsupported-value-call`) |
| A value call only valid as a `for` iterable in value position | `range` in value position has no OPY form | refusal (`unsupported-value-call`) |

### Refusal code inventory

Every code the implementation reports today, with its contract disposition.
`extension` rows name the contracted mapping that relaxes the refusal;
`refusal` rows are recorded exceptions; `guard` rows are permanent input or
environment checks.

| Code | Trigger | Disposition |
| --- | --- | --- |
| `manifest-error` | Builtin OPY manifest fails to load | guard |
| `catalog-error` | Builtin catalog fails to load; a manifest `catalogId` is absent from the catalog | guard |
| `unsupported-settings` | `program.settings` is present | extension → `settings` JSONC |
| `unsupported-name` | Table name is not a valid OPY identifier or collides with a keyword/literal | extension → recorded mangling; unrepresentable residue stays refused |
| `unsupported-duplicate-name` | A name repeats inside one declaration table | refusal |
| `unsupported-global-order` | Global indices are not in ascending order | extension → emit sorted with explicit indices |
| `unsupported-indexed-initializer` | Initializer-bearing global at a non-lowest-free slot | extension/refusal → `globalvar name <index> = <init>` grammar parity landed; the emission mapping is the remaining work |
| `unsupported-rule-order` | Subroutine rule after a normal rule, out of table order, wrong synthesized name, or carrying conditions; `Subroutine` event outside `def` layout | extension for order/name shape (allowed differences); refusal for conditions on subroutine rules |
| `unsupported-init-rule` | Initializer rule carries conditions, non-Set actions, non-eventPlayer target, or out-of-order Sets | extension → ordinary-rule emission where reproducible |
| `unsupported-disabled-rule` | `rule.disabled` | extension → `@Disabled` |
| `unsupported-rule-event` | Filtered `eachPlayer`, any `Player` event | extension → event + `@Team`/`@Hero`/`@Slot` |
| `unsupported-disabled-condition` | `condition.disabled` | extension → `#`-commented `@Condition` |
| `unsupported-control-flow` | Unbalanced/unexpected `If`/`ElseIf`/`Else`/`End`/`While`/`For` markers, missing `End` | guard (malformed action stream) |
| `unsupported-per-player-loop` | A `ForPlayerVariable` reaching the single-action path (malformed stream residue; the supported `for <player>.<v>` form is emitted by the block walker) | residual guard; the mapping itself is implemented |
| ~~`unsupported-set-binary`~~ | — | implemented: self-referential `Set` emits as written; `Modify` is an allowed difference |
| ~~`unsupported-arbitrary-player-target`~~ | — | implemented: `(<player>).<v>` read/write and `for` binder forms |
| `unsupported-member-assignment` | `AssignMember` dynamic member-write | refusal |
| `unsupported-dangling` | Variable/subroutine reference with no declaration | guard |
| `unsupported-disabled-action` | `Action::Disabled` | extension → `#`-commented line |
| `unsupported-action-call` | Call with no OPY form, dedicated-lowering name, kind mismatch, or subroutine-ambiguous bare call | refusal; syntax-produced ids (`set*/modify*AtIndex`, `startRule`, `skip*`, `abortIf*`, `loopIf*`) are implemented |
| `unsupported-modify-op` | A modify operation beyond `Add`/`Subtract`/`Multiply`/`Divide`/`Modulo`/`RaiseToPower`/`AppendToArray`/`RemoveFromArrayByValue`/`RemoveFromArrayByIndex`/`Min`/`Max` (a future op) | refusal for unknown ops; the four syntax ops are implemented |
| `unsupported-invalid-arity` | Member call without receiver; args exceed declared params | guard |
| `unsupported-missing-argument` | A non-optional parameter omitted | guard |
| `unsupported-invalid-argument` | Variable-required parameter (`chase*` arg 0) not a variable | guard |
| `unsupported-non-finite-number` | `NaN`/±∞ literal | refusal |
| ~~`unsupported-negative-number`~~ | — | implemented: `-<abs>` emission |
| `unsupported-localized-string` | `LocalizedString` id with no catalog spelling in the en-US layer | residual guard; the `l"…"`/`l"…".format(...)` mapping itself is implemented |
| `unsupported-subroutine-value` | `Value::Subroutine` in a value position OPY does not resolve (bare names bind only in `async` arguments and call statements) | refusal outside accepted positions |
| `unsupported-enum-domain` | Enum domain not source-exposed | refusal |
| `unsupported-enum-member` | Member outside the domain or without an OPY spelling | refusal |
| `unsupported-enum-domain-mismatch` | Argument enum domain ≠ declared parameter domain | refusal |
| `unsupported-value-call` | Value call with no OPY form, dedicated-lowering name, `for`-iterable-only name in value position, `valueInArray` at literal index 0 (re-lowers to `firstOf`), a non-identifier `memberAccess` member, or a lambda binder outside a callback | refusal; the other syntax-produced ids are implemented |

### Frontend re-normalization traps

Two frontend behaviors make a naive source emission change the program it
recompiles to, and the contract accounts for both:

- Reconstructed output begins with `#!optimizeStrict` (extension; not yet
  emitted). Without it, the frontend's default optimizations fold values
  such as `Add(x, 0)` into a different canonical structure than the input
  carries. `#!optimizeForSize` and other optimization-changing directives
  are never emitted. Upstream emits the same directive for the same reason.
- Any construct the frontend re-normalizes on the way to canonical form —
  re-associated variadic calls, folded expressions, synthesized rule
  placement — must be emitted in the shape that survives re-lowering, or
  refused when no such shape exists. Recompile equivalence is the check,
  not emitted-text plausibility.

## Input coverage vs. the pinned upstream decompiler

The input-coverage bar is the pinned upstream decompiler: `overpy@9.7.17`
`decompileAllRules` at content commit
`5a7d0e294b8cad73b9701987bb584d0551d7fa4d` (`v9.7.17`, the identity recorded
in `tools/overpy/oracle/oracle-metadata.json`). Anything upstream
decompiles, `opy-rs` either reconstructs or refuses as a recorded exception.
Output need not match upstream text or shape; it must be idiomatic,
maintainable OPY that recompiles to an equivalent program under the
equivalence contract.

The starting corpus is upstream's decompiler test set: the 16 pinned inputs
under `src/tests/decompiler/*.txt` at the pinned content commit —
`customGameSettings`, `shionSettings`, `mysteryUltimates`, and the 13 `z_*`
full-gamemode programs — each with its pinned `results/*.opy` upstream
output. A harness that parses each input through `workshop_rs::parser`,
reconstructs, recompiles, and compares under the relaxed comparator is a
follow-up; the fixtures enter only through the repository's attribution and
fixture policy.

Recorded exception classes against upstream input coverage (each names its
justification; "extension" rows are contracted coverage, "refusal" rows are
permanent unless a later owner decision changes them):

- **Settings** — upstream emits `settings` JSONC; `opy-rs` refuses
  (`unsupported-settings`) pending the contracted settings mapping.
- **Disabled constructs** — upstream comments them out or emits
  `@Disabled`; `opy-rs` refuses today; the contracted mapping preserves
  them as annotations/comments. Unobservable under the equivalence
  contract either way.
- **Filtered and player events** — upstream emits event names and filters;
  `opy-rs` refuses today; contracted mapping exists on the OPY surface.
- ~~**Arbitrary player targets and per-player loops**~~ — implemented:
  `(<player>).<v>` read/write forms and `for <player>.<v> in range(...)`.
- ~~**Indexed assignments and syntax-produced call ids**~~ — implemented:
  `v[i]`, `del`, `in`, `sorted`, `.map`/`.filter`/`.slice`, conditional
  values, `l"…"`, generated labels + `goto`, `if …: return`/`loop()`,
  `async`, `break` inside loops, and member/component value forms.
- ~~**Modify RemoveFromArray*/Min/Max**~~ — implemented: `.remove`, `del`,
  `min=`/`max=`.
- ~~**Localized strings**~~ — implemented: `l"…"` and `l"…".format(...)`;
  `Value::LocalizedString` is also the reconstruction of canonical `string`
  calls with a localized first argument.
- ~~**Negative numbers**~~ — implemented: `-<abs>`; non-finite numbers stay
  refused.
- **Subroutine/global indices in declaration+initializer combined forms**
  — upstream accepts `subroutine name index` and
  `globalvar name index = init`, and `opy-rs` now parses and lowers both;
  the emission-side mapping that would lift the recorded refusals is the
  remaining extension.
- **Name collisions / invalid identifiers** — upstream mangles names
  (`translateNameToAvoidKeywords`); `opy-rs` refuses today — a recorded
  deterministic mangling is a contracted extension; unrepresentable
  residue stays refused.
- **Dynamic member writes (`AssignMember`)** — upstream's accepted surface
  here overlaps the pending `.x`/`.y`/`.z` canonical-validation exception;
  recorded refusal.
- **Ambiguous call surface** — a bare call that is simultaneously an
  action name and a declared subroutine name cannot be emitted
  unambiguously; recorded refusal.
- **Non-finite numbers, non-source enum domains/members, calls with no
  OPY form** — no recompile-equivalent OPY exists; recorded refusals.
- **Duplicate table names, dangling references, malformed control flow,
  arity/argument violations** — cannot recompile to the input; guard
  refusals, permanent.

## Deterministic vs. heuristic choices

Deterministic by contract: identity resolution through the manifest and
catalog (canonical-name spelling preferred over aliases sharing a
`catalogId`), declaration and rule layout, refusal decisions, string
escaping, number formatting, and the `#!optimizeStrict` preamble.

Heuristic choices — positions where several OPY forms recompile to the same
canonical structure — are permitted but must be deterministic, recorded in
the implementation or its tests, and justified as
recompile-equivalent. Current sanctioned heuristics:

- **Source-alias choice** for canonical calls reachable by more than one
  OPY spelling (canonical `allPlayers` reconstructs as
  `getPlayers(Team.ALL)` deterministically rather than `getAllPlayers()`
  or bare `allPlayers`; `hudText` argument patterns → `hudHeader` /
  `hudSubheader` / `hudSubtext`; `roundToInteger` modes → `round` /
  `floor` / `ceil`).
- **Modify spelling** — `<v> <op>= <x>` vs `<v> = <v> <op> <x>` vs
  `++`/`--` for `±1`.
- **Disabled-construct rendering** — comment fidelity vs. omission.
- **`goto`/label synthesis** for `skip`/`skipIf` — label names are
  generated deterministically (`__op_label_N`, skipping declared
  identifiers); a non-literal distance synthesizes `goto loc + <n>`.
  Original label intent is unrecoverable. **Lambda binders** for
  `currentArrayElement`/`currentArrayIndex` are generated likewise
  (`__elem_N`/`__idx_N`).
- **Declaration elision and mangling** — skipping declarations whose names
  are the Workshop defaults at their index; mangling invalid or colliding
  names under a recorded rule.
- **`@Delimiter` emission** for actionless rules.
- **Settings JSONC layout** — key ordering and inline/block style within
  canonical settings meaning.

A heuristic may never change observable semantics; when no
recompile-equivalent choice is justified, reconstruction refuses.

## Optional authored-source metadata

Source mappings, comments, and other authored-source metadata may improve
reconstruction but are never required for ordinary canonical programs:

- `Program` provenance spans already drive issue attribution and may drive
  optional comment/layout hints.
- A `workshop-rs` source map (`workshop-rs/mapped-text-v1`) may attach
  authored-source identity to canonical nodes; when present it may inform
  naming or comment hints.
- Hints affect source quality only: acceptance, refusal codes, and the
  emitted program's semantics must be identical with and without them.
  Reconstruction must never depend on metadata the canonical model does
  not guarantee.

## Unrecoverable information

The canonical `Program` does not carry the original source's authoring
intent; reconstruction does not promise recovery of:

- original formatting, whitespace, indentation, or file layout;
- comments and documentation (Workshop text carries none);
- preprocessor and macro structure: `#!define`/`#!defineMember` bodies,
  `macro`/`enum` declarations, `#!include` file partitioning, `#!mainFile`;
- source abstractions: user `enum` members, constants, macro-generated
  bindings, lambdas as authored (only their canonical lowerings survive);
- authored local naming beyond declaration names;
- authored helper structure: `ruleCondition`-style synthesized bindings,
  optimizer helpers, `Filtered Array` wrappers are re-derivable shapes,
  not the original source intent;
- `goto`/label names (canonical keeps only skip distances);
- annotation surface: `@Name`, `@Delimiter`/page layout (`@NewPage` has no
  canonical meaning), `@SuppressWarnings`;
- `#!rulePrefix` effects — canonical rule names are post-prefix;
- `#!translations` `.po` files — canonical carries localized strings only;
- optimizer shape: emitted structure is canonical, not a record of the
  author's optimization state.

Where a Workshop concept carries the information (settings, disabled
constructs, indices, names), the contract maps it; where the source intent
is erased, output presents the reconstructed equivalent without claiming
origin fidelity.

## Missing workshop-rs capabilities

Contract-level only; no OPY-local substitutes are introduced.

1. ~~**Relaxed behavior comparator**~~ — landed:
   `workshop_rs::compare::equivalent` / `workshop_rs::compare::difference`
   in workshop-rs ≥ 1.20 (`wrightkit/workshop-rs#490`).

If implementation uncovers a genuinely missing canonical Workshop
capability beyond this (for example a canonical operation reconstruction
needs that `workshop-rs` does not model), it is a `workshop-rs` requirement
recorded against the boundary contract, not approximated locally.

## Verification contract

- Every accepted input must produce OPY that checks clean and recompiles
  to a program equivalent to the input under this contract's comparator —
  `Program → OPY → Program`. A round-trip failure on an accepted input is
  a defect, regardless of which stage produced it.
- The behavior-comparator gate is `workshop_rs::compare::equivalent`;
  `workshop_rs::roundtrip::equivalent` remains valid evidence for the
  strict-identity subset (a program that round-trips under it is also
  behaviorally equivalent).
- Refusal classes keep executable evidence: each recorded refusal has a
  test naming its code; each extension has a round-trip test once
  implemented.
- The upstream decompiler corpus harness (the 16 pinned inputs) is the
  coverage gate once landed; recorded exceptions are the only permitted
  failures in it.

## Recorded follow-up work

Decomposed only after this contract is reviewed and merged:

1. Coverage extensions per the mapping tables and exception list.
2. ~~`workshop-rs` relaxed behavior comparator~~ — landed (workshop-rs
   ≥ 1.20).
3. Upstream decompiler corpus harness over the 16 pinned inputs.
4. `opy-provider` `lpp/reconstruct` advertisement so `wright convert
   --target opy` works — landed: the provider advertises `reconstruct`
   and serves this contract's bounded surface for its emitted artifact
   formats (`wrightkit/wright#736`, `wrightkit/skills#20` track the
   remaining client-side doc drift).
5. Lifting the recorded refusals now that opy-rs grammar parity for
   `subroutine name <index>` and `globalvar/playervar name <index> = <init>`
   has landed (upstream accepts both).
