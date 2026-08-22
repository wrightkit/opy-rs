# opy-rs Native .opy Frontend Support Matrix

Status: accepted baseline. A living .opy frontend support matrix for the
opy-rs evidence base (issue #2)
Scope: the `.opy` source-language surface the opy-rs native frontend targets,
with corpus/production evidence for each feature and explicitly deferred
constructs; the current per-feature implementation state is tracked
mechanically in [`compatibility/support-matrix.json`](../../compatibility/support-matrix.json)

This matrix records the **declared, corpus-evidenced surface**: every claimed
feature is backed by the compatibility corpus
(`compatibility/fixtures/**/source.opy`, `oracle.json` snapshots) or marked as
investigation. It is ported and adapted from the WrightKit project's evidence
base (wright `docs/opy/support-matrix.md`); the reference identity behind both
is recorded centrally in
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md).

The forward-looking, tiered baseline (what is planned, evidence-prioritized,
or demand-driven) lives in
[`compatibility-baseline.md`](compatibility-baseline.md), and the
machine-readable semantic contract for builtins is specified in
[`compat-manifest-spec.md`](compat-manifest-spec.md).

## Current implementation state

The standalone frontend foundation and #7 readiness work are merged on `main`
(issues #2–#7, #28–#30, and #33): the native pipeline (lexer →
preprocess → CST/parser → semantic resolution → Opy HIR v1), the bounded
JavaScript macro runtime, the tooling API/CLI, and the native differential
suite are implemented and CI-covered. The rows they evidence are flipped to
`frontend-supported`/`semantic-supported` in
`compatibility/support-matrix.json`, the mechanically checked state source;
features whose completion requires the full canonical Workshop surface remain
`lowering-dependent`; the bounded #35 adapter and #40 structural HIR →
canonical WIR lowering are separately recorded as `end-to-end-supported` and
do not reclassify broader Workshop-owned rows.
Here, `end-to-end-supported` is scoped to the explicitly evidenced feature or
vertical slice; it never means full-language OPY-to-Workshop parity. The wider
#8 lowering stage remains outside this frontend gate. Per-fixture differential status (resolve /
expected-diagnostic / divergence) is recorded in
`target/opy-differential-report.json` by `cargo test -p opy-frontend
--test differential`.

The declared pipeline is `lexer → preprocess → CST/parser → semantic
resolution → OPY semantic model (Opy HIR v1, see
[`docs/hir/opy-hir-v1.md`](../hir/opy-hir-v1.md))`, fully
Workshop-independent up to the documented integration boundary toward
`workshop-rs` (see [`architecture.md`](architecture.md)).

## Evidence sources

| Source | Use |
| --- | --- |
| `compatibility/fixtures/synthetic/{basic-rule,control-flow,declarations-rules,declarations-numbers,expressions-values,preprocessing,diagnostics,settings,receiver-calls,chase-*}/source.opy` | Synthetic corpus surface (WrightKit-authored; oracle snapshots in the same directory) |
| `compatibility/fixtures/real-world/overpy-*/` | Real-world surface from the pinned OverPy `examples/` tree (arrays, macros, effects, settings, subroutines, include closures; provenance in `compatibility/fixtures/README.md`) |
| `compatibility/fixtures/real-world/{ow1-emulator,6v6-adjustments}/` | Independent third-party projects (BSD-2-Clause), full include closures |
| `compatibility/fixtures/**/oracle.json` | Pinned OverPy 9.7.10 reference snapshots (normalized Workshop output, diagnostics, exit codes) |
| `compatibility/fixtures/synthetic/issue-35-integration/` | #35 OPY-to-Workshop vertical-slice evidence; oracle provenance remains separate from implementation-specific WIR/emission assertions |
| `compatibility/fixtures/synthetic/issue-40-structural/` | #40 pinned OverPy oracle evidence for subroutine identity, deterministic variable allocation, and player event filters |
| `crates/opy-compiler/src/lib.rs` structural tests | #40 declarations, subroutines, rules, event filters, deterministic indices, and source-attributed negative lowering evidence |
| `compatibility/support-matrix.json` | Machine-readable state tracking of every declared feature (the mechanically checkable artifact) |
 | `crates/opy-frontend/src/manifest/` | The opy-rs-owned semantic compatibility manifest and its oracle probes (ported with the frontend, issue #3/#4) |
 | `crates/opy-frontend/tests/differential.rs` + `compatibility/diff.py` | Native-vs-reference differential parity (issue #7): the rust suite runs every corpus fixture through the native pipeline in `cargo test` (no Node), compares status/rule-name evidence against the recorded `oracle.json` snapshots, and writes `target/opy-differential-report.json` |
 | `crates/opy-frontend/tests/fixtures/macros/` | JavaScript macro / post-compile hook end-to-end fixtures (issue #5/#6) |
 | `crates/opy-macro-js/tests/` | Bounded QuickJS runtime ABI fixtures (issue #6) |

## Supported surface (corpus-evidenced contract)

The sections below describe the declared surface with the corpus/oracle
evidence behind each item. They are the contract the merged frontend
implements; "reference" always means the pinned OverPy 9.7.10
(`889d9749d1def17f146548cbddb94ea1ab015847`).

### Lexing
- Identifiers, integer and decimal number literals (source text preserved),
  double-quoted strings with `\n`/`\t`/`\\` escapes, `true`/`false`/`None`.
- Line comments (`#`), block comments (`/* */`), `#!` directives.
- Operators: `+ - * / // % ** == != < <= > >= = += -= *= /= //= %= and or not`,
  `in`/`not in`, plus `.`/`,`/`:`/`(`/`)`/`[`/`]`/`@`.

### Pure OPY syntax and source semantics
- `switch`/`case`/`default` preserve source-order fallthrough; `break` is a
  real HIR statement valid in the innermost switch or loop.
- `do ... while`, hexadecimal literals, and expression-level `in`/`not in`
  are represented in the source-language HIR.
- String modifiers, including f-string interpolation, preserve semantic
  format text, interpolation expressions, and source spans; dict literals,
  keyed access, list comprehensions, and lambda binders preserve local scope.
- Lambda expressions are accepted only in the pinned signature-approved
  positions (`sorted` key and array `map`/`filter`/`all`/`any`); other
  positions produce structured diagnostics.

### Declarations
- `globalvar name` / `globalvar name = expr` / `globalvar name <index>`
  (the bare-integer form is an explicit Workshop variable index, matching the
  reference; integer-`0` literal initializers are dropped from HIR (matching
  the reference); non-zero and non-integer numeric initializers are
  preserved, e.g. `j = 5` and `k = 0.0` keep the source spelling).
  Initializer semantics are **lowering-dependent**: the Initialize rules are
  synthesized by the HIR → Workshop lowering, which is inventory-only until
  the `workshop-rs` integration stage.
- `playervar name` (same forms).
- `subroutine name`.
- `def name():` subroutine bodies (parameters are outside the declared
  surface; rejected explicitly).
- `enum Name: MEMBER, ...`: members fold to numeric constants
  (`Phase.FINISHED` → `1`), matching the reference.
- `macro name(params):` statement bodies with `MacroParam` references.

### Preprocessing
- `#!include "file.opy"`: root-relative include resolution, cycle detection
  (`include-cycle`), missing-file diagnostics (`include-not-found`), included
  files registered in the HIR file registry (reference behavior).
- `#!define NAME value`: object-like macros; recursive expansion at use sites
  (a define may reference earlier defines); recursion guard
  (`macro-recursion`).
- `#!define name(args) value`: function-like macros with argument
  substitution (`cakeBeam(start, end, yPos) → createBeam(...)`).
- `#!define name(args) __script__("path.js")`: OverPy-compatible
  **JavaScript macros** (see below).
- `#!undef NAME`.
- `#!mainFile "path.opy"` redirects the frontend entry point and preserves
  file provenance; `#!allowMacroRedeclaration` changes duplicate handling at
  `#!define`, enum-member, and AST-macro surfaces.
- `#!rulePrefix`/`#!rulePrefixTemplate` are retained as source state and
  rendered against every rule/subroutine (including rules before the template
  directive); include prefix state is restored after child includes.
- `#!translations`, `#!optimize*`, and `#!replace0By*` forms are parsed,
  validated, and exposed as frontend preprocessing state. Directive records
  retain include depth and state snapshots, so nested transitions are
  observable without claiming optimizer execution. Locale availability,
  `.po` content, generated translation helpers, optimizer rewrites, and
  replacement effects are lowering-dependent and are not fabricated here.
- `#!postCompileHook "hook.js"`: post-compile hook registration (see below).
- Unsupported directives fail explicitly (`unsupported-directive`).

### JavaScript macros and post-compile hooks (issue #5/#6)

The pinned reference ABI (`src/compiler/tokenizer.ts`, `src/quickjs.ts`,
`src/globalVars.ts`) is implemented through the embedded runtime
(`crates/opy-macro-js`, QuickJS-NG; no Node required):

- A function-like define whose replacement is `__script__("path.js")` is a
  script macro: the script path resolves root-relative at the define site
  (missing files: `script-not-found`, mirroring the reference's ENOENT
  failure), and each expansion executes the script with the call-site
  arguments injected as `var <name>=<raw>;` (raw argument text is
  reconstructed from tokens; string literals are re-quoted with JSON
  escaping, which is JavaScript-value-equivalent to the reference's raw text
  injection). The string completion value becomes the expanded text, which is
  re-lexed into the token stream at the call site. Script-macro expansion is
  compile-time behavior and is **frontend-supported**. Thrown exceptions,
  resource-limit aborts (`script-timeout` for the 1000 ms budget,
  `script-memory-limit` for the 64 MiB memory limit, `script-stack-limit`
  for the 512 KiB stack), and non-string results
  (`script-result-not-string`, with the reference's wording) map to
  structured `script-*` diagnostics carrying the script path and
  line/column. The reference's `vect(x, y, z)` helper and the constant
  objects (`Map`, `Hero`, `Gamemode`, `Color`, `Team`, `Button`) are always
  defined (constants empty until catalog data lands with `workshop-rs`).
- `#!postCompileHook "hook.js"` is recognized, parsed, validated, and
  **recorded only**: the frontend never executes the hook (duplicate
  declarations: `post-compile-hook-duplicate`, matching the reference). Real
  hook execution receives the **final Workshop text** produced by lowering
  and is **lowering-dependent** (`hooks/post-compile-workshop` in the
  support matrix, issue #8); the frontend never fabricates a Workshop
  payload. The runtime's hook ABI (content injection, console capture,
  result/error semantics, 2000 ms budget) is tested on synthetic content in
  `crates/opy-macro-js/tests/hooks.rs`.
- There is no `#!require` directive in the pinned reference; the script
  macro form is the only JavaScript declaration surface (verified against
  OverPy 9.7.10, `src/compiler/tokenizer.ts`).

### Rules and directives
- `rule "name":` with `@Event global` / `@Event eachPlayer` / `@Condition <expr>`.
- `@Team`/`@Slot` arguments, `@Hero`, `@Name`, `@Disabled`, `@Delimiter`,
  `@NewPage`, and `@SuppressWarnings` are parsed, validated, and retained in
  the OPY HIR. Hero/team/slot domain checks and Workshop UI effects remain
  lowering-dependent; malformed or misplaced annotations fail with structured
  source-located diagnostics. Issue #40 lowers the WIR-representable subset:
  disabled state, canonical event identities, and catalog-resolved team/slot/
  hero filters. Delimiter, new-page, suppression, and other metadata without a
  canonical WIR carrier remain explicit integration diagnostics.
- Statements: expression statements, `=` and augmented assignment,
  `if`/`elif`/`else`, `for x in range(...)`, `while`, `pass`.
- `for`-loop binder resolution: the loop variable must resolve to a global
  variable, either a declared `globalvar`, or an OverPy **default variable
  name** (`A`–`Z`, `AA`–`AZ`, …, `DA`–`DX`), which the pinned reference
  accepts as an implicit global at its fixed Workshop slot (e.g. `for I in
  range(0, 10):` with no declaration, the agent-lab regression). Nested
  same-name loops reuse the same implicit variable (no separate binding),
  matching the reference. An undeclared lowercase binder is rejected exactly
  like the reference rejects it (`unknown-identifier`, reference: "Unknown
  function name"). `range(stop)` / `range(start, stop)` /
  `range(start, stop, step)` are all supported.

### Expressions and resolution
- Literals, arrays `[...]`, parenthesized expressions.
- Calls (`range`, `len`, `abs`, `sqrt`, `debug`, `print`, `wait`,
  `createBeam`, `playEffect`, `getAllPlayers`, `disableInspector`, …).
- `vect(x, y, z)` → HIR `Vector` (3 arguments required; other arities are an
  explicit error).
- `"text".format(args)` → HIR `Format`; bare calls of declared subroutines →
  `CallSubroutine` statements; dotted module calls `random.uniform` /
  `random.choice` → `random.<name>` calls; `eventPlayer.member` →
  `PlayerVar`/receiver call on `EventPlayer`; variable receivers
  (`points.append`, `candlePos[i2]`) → `ReceiverCall`/`Index`.
- OPY-owned builtin action/value/member identity, signatures, receiver categories,
  parameter enum domains, and non-contextual aliases resolve through the OPY
  semantic compatibility manifest (`crates/opy-frontend/src/manifest/`, schema
  v1; spec in [`compat-manifest-spec.md`](compat-manifest-spec.md)), the
  single authoritative semantic table. Every manifest entry is
  probe-validated against the pinned OverPy 9.7.10 oracle
  (`crates/opy-frontend/src/manifest/probes/`). Unknown or misplaced builtins
  fail at semantic resolution with structured, source-located diagnostics
  (`unknown-action`, `unknown-value`, `unknown-member`, `invalid-arity`,
  `invalid-receiver`, `action-in-value-position`,
  `value-in-action-position`, `invalid-call-context`, `invalid-iterable`,
  plus the argument-binding codes `unknown-keyword`, `duplicate-argument`,
  `missing-argument`, `positional-after-keyword`, `keyword-required`,
  `keyword-unsupported`, `invalid-argument`), never as emitter catalog
  misses.
- Reference-validated evidence surface: `chaseOverTime(...)` (action; 3–4
  arguments, reevaluation defaults to `DESTINATION_AND_DURATION`),
  `isGameInProgress()` (value), `getPlayersInRadius(...)` (value; team
  `Team.ALL` and `LosCheck.OFF` defaults fill), `worldVector(...)` (value,
  `Transform` argument), and the enum-gated members
  `eventPlayer.setInvisibility(Invis.X)`,
  `eventPlayer.setStatusEffect(..., Status.X, ...)`, `eventPlayer.getThrottle()`.
- Receiver/member calls (`eventPlayer.setMoveSpeed(100)`,
  `eventPlayer.teleport(eventPlayer.getPosition())`,
  `target.setMoveSpeed(50)` on a player-valued global) lower to
  `ReceiverCall`; their Workshop emission resolves through the `workshop-rs`
  catalog, **lowering-dependent** (inventory-only until integration); the
  corpus-evidenced receiver methods are the `synthetic/receiver-calls`
  fixture methods plus the enum-gated members (en-US spellings per the
  oracle-transcribed evidence).
- Non-contextual source aliases resolve to their canonical names
  (`stopChasingVariable` → `stopChasing`; member aliases `getCurrentHero` →
  `getHero`, `hasStatusEffect` → `hasStatus`); their emission spellings are
  catalog-covered only at integration time (documented emission gap). The
  `ChaseReeval` contextual alias resolves only through the `chase` keyword
  call context and stays out of the alias table.
- Builtin Workshop enums from the manifest's reference-validated domains:
  `Beam.{GOOD,GRAPPLE}`, `Color.{YELLOW,WHITE,RED,ORANGE,GREEN,BLUE,BLACK,
  PURPLE,AQUA,VIOLET,ROSE}`, `DynamicEffect.{BAD_EXPLOSION,GOOD_EXPLOSION,
  RING_EXPLOSION,GOOD_PICKUP_EFFECT,BAD_PICKUP_EFFECT,BUFF_IMPACT_SOUND,
  DEBUFF_IMPACT_SOUND}`, `EffectReeval.{VISIBILITY,COLOR,VISIBILITY_AND_COLOR}`,
  `Wait.IGNORE_CONDITION`,
  `ChaseTimeReeval.{NONE,DESTINATION_AND_DURATION}` (reference-validated
  against the pinned OverPy 9.7.10 enum block and emission),
  `ChaseRateReeval.{NONE,DESTINATION_AND_RATE}` (`NONE` additionally
  corpus-evidenced by the real-world overpy-meipocalypse `ChaseReeval.NONE`
  rate-chase calls, which the reference resolves to the `ChaseRateReeval`
  domain), plus the evidence domains `Invis.{ALL,ENEMIES,NONE}`,
  `Transform.{ROTATION,ROTATION_AND_TRANSLATION}`,
  `Status.{ASLEEP,BURNING,FROZEN,HACKED,INVINCIBLE,KNOCKED_DOWN,PHASED_OUT,
  ROOTED,STUNNED,UNKILLABLE}`,   `LosCheck.{OFF,SURFACES,
  SURFACES_AND_ALL_BARRIERS,SURFACES_AND_ENEMY_BARRIERS}`, `Team.ALL`.
  Member accesses on declared domain identities resolve as **opaque
  identities**; Workshop enum member-existence and domain validation was
  removed from the frontend core and is **lowering-dependent** (#8). The
  checks are never approximated (custom, user-declared enum members are
  OPY-level source semantics and stay frontend-validated). Enum
  complete Workshop domain/member catalog remains `lowering-dependent` and is
  owned by `workshop-rs`; the manifest carries identity links only and never a
  copied member allowlist.
- `wait()` / `wait(time)` default-argument filling: the reference appends
  `Wait.IGNORE_CONDITION` (and `0.016` for the no-argument form).
- **Named/keyword arguments** (`name = expr` call arguments) bind against the
  manifest's canonical parameter names, the pinned reference's declared
  names (`wait(time=1)`, `wait(waitBehavior=Wait.IGNORE_CONDITION, time=2)`,
  `chaseOverTime(g, 10, duration=3)`,
  `chaseOverTime(g, 10, 3, reevaluation=ChaseTimeReeval.NONE)`,
  `vect(x=1, y=2, z=3)`, `getPlayersInRadius(center=…, radius=…,
  team=Team.ALL)`, `eventPlayer.setStatusEffect(assister=…, status=…,
  duration=…)`, `print(text="x")`, `len(array=…)`, `debug(value=…)`,
  `stopChasing(variable=g)`, member forms like
  `eventPlayer.setMaxHealth(healthPercent=100)`). Keyword arguments may
  appear in any order before the first positional argument; the reference
  rejects positional arguments after keyword arguments
  (`positional-after-keyword`), unknown keyword names (`unknown-keyword`),
  duplicate bindings (`duplicate-argument`), and missing required arguments
  (`missing-argument`), all structured, source-located diagnostics. The
  reference's generic binder is routed around for `range`, `random.*`, and
  `.format` (keyword arguments on those fail with `keyword-unsupported`),
  and for `macro` invocations.
- **The `chase` keyword form** (reference special form):
  `chase(variable, destination, rate=…, ChaseReeval.MEMBER)` and
  `chase(variable, destination, duration=…, ChaseReeval.MEMBER)`, exactly
  four arguments, the 3rd passed as the `rate`/`duration` keyword and the
  4th as a bare `ChaseReeval.MEMBER` access. `ChaseReeval` resolves **only**
  in this   call context: `rate=` selects the `ChaseRateReeval` domain and
  lowers the call to `chaseAtRate`; `duration=` selects `ChaseTimeReeval`
  and lowers to `chaseOverTime`. The contextual member is rewritten to the
  keyword-selected domain without membership checks. Member/domain
  validation is **lowering-dependent** (#8), matching the reference's
  "Unknown chaseratereeval" as a lowering-time outcome. Outside the chase
  signature `ChaseReeval` never
  resolves (a bare `g = ChaseReeval.NONE` is rejected like the reference).
  The first argument must be a variable (`invalid-argument` otherwise);
  emission dispatches on its kind (global vs player variable) and is
  **lowering-dependent**.
- `chaseOverTime(...)` requires a variable first argument like the
  reference (`invalid-argument` for `chaseOverTime(10, …)`).
- Undeclared identifiers, enum types without members, and unsupported member
  accesses are structured, source-located semantic errors
  (`unknown-identifier`, `enum-type-without-member`, `unsupported-member`).

### Diagnostics
- Malformed input produces structured frontend errors (stable codes like
  `parse-error`, `lex-error`) with 1-based source spans; the parser recovers
  at statement boundaries to report multiple useful errors.
- Frontend diagnostics map into the shared `wright-result/v1` contract (stage
  `frontend`, severity `error`) consumed by WrightKit tooling.

### Settings
- Top-of-file `settings { ... }` custom-game-settings blocks (JSONC: quoted
  keys, `"`/`'` strings with escapes, numbers, `true`/`false`, string lists,
  nested groups, trailing commas), recognized and consumed before lexing
  (scoped lexing: the block never enters the token stream and the lexer
  gains no global braces), parsed into the typed HIR `settings` payload.
- Placement rules: the block must be the first construct in the main file
  (`settings-placement` otherwise); a second block is rejected; a
  `settings "file"` form is rejected (`settings-invalid`); settings blocks
  in included files are rejected (`settings-placement` at the included
  file's keyword span).
- Emission of the Workshop `settings` section (the key table, enum values,
  map/hero list elements) is **lowering-dependent**: the typed payload is
  frontend-owned; the emission table and its domain data are Workshop data
  owned by `workshop-rs`. Key-existence and leaf-kind settings validation
  is Workshop schema content and **lowering-dependent** (#8). The frontend
  validates structure only (group shape, span validity, non-empty key
  names). The emitted `settings` section is deliberately not
  reparseable by the Workshop parser (a `.ws` decompiler is a non-goal).

## Deferred / out of scope

- **Reconstruction / decompilation (Workshop → OPY)**: deferred and
  `lowering-dependent`. Full Workshop-to-OPY reconstruction requires the
  canonical Workshop semantic representation from `workshop-rs` and is
  inventory-only (see `compatibility/support-matrix.json`, category
  `decompilation`; wright's implemented reconstruction surface is not ported
  as a claim; it becomes an opy-rs contract only at the integration stage).
- Macro/`#!define` values that require runtime evaluation are **implemented**
  for the reference's `__script__` ABI (see "JavaScript macros and
  post-compile hooks" above); the remaining runtime surface is
  `lowering-dependent`: hook output into Workshop emission and catalog
  constant population (`Map`/`Hero`/… objects stay empty).
- Canonical Workshop enum domains/members and emission spellings are
  `lowering-dependent` and owned by `workshop-rs`; they are not OPY frontend
  gaps. Manifest entries without a direct catalog id carry an explicit
  `catalogLink` reason (`special-lowering`, `legacy-alias`, or `catalog-gap`)
  and remain visible to the integration adapter.
 - Emission spellings for manifest-valid entries not yet catalog-covered
   (alias targets `stopChasing`/`getHero`/`hasStatus`, and enum members
   without a catalogged spelling); these fail at emission with catalog
   diagnostics once integration lands, never silently.
- Backslash line continuation (`\` at end of line inside string
  concatenations / macro bodies): rejected at lexing.
- Postfix increment/decrement (`++`/`--`): rejected at parsing.
- Triple-quoted strings / docstrings (`"""`): rejected at lexing.
- Subroutine parameters, default `@Team`/`@Slot` overrides, `raycast`
  `include=`/`exclude=` named-argument forms (no reference/corpus evidence
  in the declared surface; the reference's `raycast` special form is not
  manifest-declared), and macro keyword arguments (the reference's macro
  substitution treats them as raw text; rejected explicitly).
- Full OverPy formatting semantics: `debug()`/`print()` emission
  (`Create HUD Text` etc.) follows simplified semantic formatting
  (presentation differences are not compatibility bugs unless they change
  observable semantics).
- Emission presentation: variable references may emit as `Global.<name>`
  where the reference emits the bare variable name; observable semantics and
  round-trip validity are unchanged (output-text identity is explicitly not
  the compatibility contract; see `upstream-references.md`).

## Boundary contract

The frontend produces the Opy HIR v1 program model
([`docs/hir/opy-hir-v1.md`](../hir/opy-hir-v1.md)) with the protocol envelope,
file registry, declarations, and rules as specified there. It never requires
Node or OverPy at build/runtime; the oracle remains available as an explicit
`pnpm install --dir compatibility/oracle` step for the compatibility harness
only. Differential parity runs in `cargo test -p opy-frontend --test
differential` against the recorded oracle snapshots (issue #7); the report
and per-fixture native HIR dumps land in `target/`.
