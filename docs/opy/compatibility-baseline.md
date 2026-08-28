# Proactive OPY Compatibility Baseline

Status: accepted baseline (planning). A proactive OPY compatibility baseline
for opy-rs (issue #2), ported and adapted from the WrightKit evidence base.
Scope: forward-looking, tiered inventory of the OPY language surface against
the pinned OverPy 9.7.10 reference, classifying every category by
implementation tier and by support dimension

This document is the planning counterpart to
[`support-matrix.md`](support-matrix.md): the support matrix records the
corpus-evidenced surface the opy-rs source implementation targets, while this baseline
records how the remaining surface is **tiered and sequenced**. A construct is
not called supported merely because it parses; each row states parse,
semantic, compilation, tooling/analysis, and reference coverage separately.

The reference identity is the pinned OverPy 9.7.10 content
(`889d9749d1def17f146548cbddb94ea1ab015847`); see
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md)
for provenance. Evidence claims in this document were verified against the
pinned oracle (the declared corpus now contains 42 provenance-linked
snapshots). The opy-rs source implementation foundation and #7 readiness work are
implemented on `main` (issues #3–#7, #28–#30, and #33); the category table is the
**tier assignment contract** for the remaining surface. The state column of
`compatibility/support-matrix.json` tracks actual implementation progress
against it, and rows marked `baseline-supported` in this document are
implemented unless the table says otherwise.

## Tier taxonomy

| Tier | Meaning |
| --- | --- |
| `baseline-supported` | Implemented and corpus/reference-evidenced; part of the declared supported surface |
| `baseline-planned` | Stable, high-fan-out, systematically implementable; contract is discoverable and reference-testable; not yet implemented |
| `evidence-prioritized` | Complex or broad feature with clear tooling value; corpus/consumer evidence determines ordering |
| `legacy-quirk/demand-driven` | Rare historical quirks, upstream bugs, obsolete aliases, scripting hooks; implemented only when the declared compatibility target requires them |
| `reference-limited/inconclusive` | Cannot be resolved from the pinned reference; needs a demonstrated need, a pin change, or further investigation |

## Support dimensions

For each category the following dimensions are distinguished:

* **Parse**: accepted by the opy-rs source implementation grammar;
* **Semantic resolution**: resolved to a meaningful HIR/semantic value
  (names, members, enums, call semantics);
* **Compilation**: standalone compile/emission through the `workshop-rs`
backend succeeds with reference-equivalent semantics. The bounded #35
vertical slice is now evidenced through workshop-rs v0.1.1; the remaining
surface is **lowering-dependent** and stays inventory-only until later #8
work;
* **Tooling/analysis**: `check`/`analyze`/`lint`/`inspect` and language
  services can operate on the construct;
* **Reference coverage**: oracle probes/fixtures validate the behavior.

In the table, `✅` marks a dimension that is part of the declared contract for
the tier (evidenced by the merged source implementation via the corpus and the native
differential suite), `❌` a deliberately
rejected/documented-absent dimension, `—` an inapplicable dimension, and
`partial` a bounded subset.

## Category inventory

| # | Category | Tier | Parse | Sem | Comp | Tooling | Ref |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | **Expression/postfix/member/call grammar**: operators and precedence, `[]` indexing, `.` member, calls, `del`, `in`/`not in`, hex `0x` | `baseline-supported` for the corpus subset (operators, indexing, calls, member/call); `++`/`--` remain tracked by opy-rs#59 | ✅ corpus | ✅ | ✅ (integration) | ✅ | ✅ differential (issue #7) |
| 1a | `switch`/`case`/`default`, `break`, `do…while`, `in`/`not in`, `0x` hex literals | `baseline-supported` for the pinned source implementation surface; Workshop control-flow lowering remains integration-owned | ✅ | ✅ | partial (integration) | ✅ | ✅ oracle probes |
| 1b | String modifiers (`f`/`w`/`l`/`b`/`c`/`t`), dict literals, list comprehensions, signature-gated `lambda` | `baseline-supported` for the pinned source implementation surface; formatting/emission remains lowering-dependent | ✅ | ✅ | partial (integration) | ✅ | ✅ oracle probes |
| 2 | **Declarations**: `globalvar`/`playervar` (index + initializer forms), `subroutine`, `enum`, `macro` constants (incl. member constants) | `baseline-supported` | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 3 | **Assignments & control flow**: `=`, evidenced augmented (`+= -= *= /= %= **=`), `if`/`elif`/`else`, `for … in range(...)`, `while`, `pass` | `baseline-supported`; `min=`/`max=` remain workshop-rs#95 follow-up | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 4 | **Rule directives & annotations**: `@Event`, `@Condition`, bare `@Team`/`@Slot`, rule name, event defaults (`global`, `all` team/player) | `baseline-supported` (bare forms) | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 4a | `@Team`/`@Slot` with arguments, `@Name`, `@Hero`, `@Disabled`, `@Delimiter`, `@NewPage`, `@SuppressWarnings` | `baseline-supported` for source implementation state; Workshop domain/UI effects remain lowering-dependent | ✅ | ✅ | partial | ✅ | ✅ oracle probes |
| 5 | **Preprocessing/include/macro**: `#!include`, `#!define` (object- and function-like), `#!undef`, include cycle detection | `baseline-supported` | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 5a | `#!mainFile`, `#!allowMacroRedeclaration`, `#!optimize*`/`#!replace0By*` family, `#!translations`, `#!rulePrefix*`, `__script__` JS hooks | `baseline-supported` for source implementation state; rule-prefix metadata, catalog locale emission, and post-compile hooks have bounded compiler evidence, while optimizer/replacement effects remain explicit gaps | ✅ | ✅ | partial | ✅ | ✅ oracle probes |
| 6 | **OPY builtin actions & values (generic)**: manifest identities, signatures, aliases, and call semantics | `baseline-supported` for the probe-validated manifest overlay; canonical Workshop existence/content/emission is `lowering-dependent` | ✅ | ✅ | partial (integration) | ✅ | ✅ probes |
| 6a | **Canonical Workshop builtin action/value catalog**: full catalog existence, content, and emission | `lowering-dependent` (`workshop-rs`, #8) | — | — | ❌ (integration) | — | ✅ inventory/oracle evidence |
| 7 | **OPY receiver/member semantics**: receiver categories, explicit-argument signatures, variable receivers | `baseline-supported` for the manifest-declared OPY overlay; canonical member existence/content/emission is `lowering-dependent` | ✅ | ✅ | partial (integration) | ✅ | ✅ probes |
| 7a | **Canonical Workshop receiver/member catalog**: member existence, content, and emission | `lowering-dependent` (`workshop-rs`, #8) | — | — | ❌ (integration) | — | ✅ inventory/oracle evidence |
| 7b | Bare playervar receiver member access (`A = B.C`) | `baseline-supported` for the OPY member-expression representation; canonical member existence remains lowering-dependent | ✅ | ✅ | partial (integration) | ✅ | ✅ oracle evidence |
| 8 | **OPY enum/domain semantics**: declared domain identities and contextual dispatch | `baseline-supported` for manifest identity links; canonical member lists/membership/emission are `lowering-dependent` | ✅ (identities) | ✅ | partial (integration) | partial | ✅ probes |
| 8a | **Canonical Workshop enum/domain catalog**: member lists, membership, and emission | `lowering-dependent` (`workshop-rs`, #8) | — | — | ❌ (integration) | — | ✅ inventory/oracle evidence |
| 9 | **Aliases**: old function names (`stopChasingVariable`→`stopChasing`, `getCurrentHero`→`getHero`, `hasStatusEffect`→`hasStatus`, …), hero renames (`MCCREE`→`CASSIDY`), `ChaseReeval` contextual alias | `baseline-supported` for the manifest-declared non-contextual aliases and the `ChaseReeval` call-context resolution; the remaining alias surface stays `legacy-quirk/demand-driven` | ✅ (declared) | ✅ | ✅ (chase forms catalog-covered at integration) | ✅ | ✅ |
| 10 | **Modules**: `random.{randint,uniform,choice,shuffle}` | `baseline-supported` (corpus: `random.uniform`, `random.choice`) | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 11 | **Named/keyword arguments**: `chase(A, B, rate=30, …)`, generic `name = expr` binding against manifest signatures | `baseline-supported` for the evidence surface (generic keyword binding plus the `chase`/`ChaseReeval` call-context forms); `raycast` `include=`/`exclude=` forms and macro keyword arguments stay `evidence-prioritized` (no corpus/reference evidence in the declared surface) | ✅ | ✅ | ✅ (integration) | ✅ | ✅ probes |
| 12 | **Settings/content metadata**: `settings { … }` blocks | `baseline-supported` (JSONC subset + typed HIR payload); the Workshop `settings` emission table is **lowering-dependent** | ✅ | ✅ | ❌ (integration) | ✅ | ✅ |
| 12a | `settings "file"`, richer settings expressions, hero/map/ability content beyond the pin | `legacy-quirk/demand-driven` / `reference-limited` | ❌/partial | ❌ | ❌ | ❌ | partial (data newer than pin unavailable per the pinning policy) |
| 13 | **Source identity & diagnostics**: structured, source-located source implementation errors, `wright-result/v1` | `baseline-supported` | ✅ | ✅ | — | ✅ | ✅ S/D |

## Current `planned` entries

There are no remaining `planned` entries in
`compatibility/support-matrix.json`. The pinned OPY source implementation surface from
#28/#29/#30/#33 is represented as source implementation- or semantic-supported; Workshop
catalog, emission, and runtime effects remain explicitly
`lowering-dependent`. Their tiers above
distinguish **evidence-prioritized** work (broad or high-fan-out surface with
clear tooling value, ordered by corpus/consumer evidence) from
**legacy-quirk/demand-driven** compatibility (rare historical quirks and
upstream behaviors implemented only when the declared compatibility target
requires them), not every upstream quirk is a planned implementation.

## Residual evidence items (classified)

Verified against the pinned oracle. Each item is classified with the tier it
belongs to; none is a per-symbol implementation request. Items marked
*manifest-covered* resolve through the OPY semantic compatibility manifest
(`crates/opy-rs/src/manifest/`), which is merged on `main`; rows below
record their current opy-rs status against the current support-matrix baseline,
and remaining gaps stay classified rather than being filed per-symbol.

| Evidence | Oracle 9.7.10 | opy-rs status | Classification |
| --- | --- | --- | --- |
| **Bare playervar receiver**: `A = B.C` (declared playervar member on a player-valued receiver) | accept (`__playerVar__`) | implemented as a provenance-preserving OPY member expression; canonical member existence remains lowering-dependent | `baseline-supported` (receiver/member semantics; category 7) |
| **Value member as statement**: `B.isAlive()` on its own line | **reject** ("Expected an action, but got … a value") | implemented: rejects with `value-in-action-position` | `baseline-supported` (reviewed difference; recorded in the probe set) |
| **Generic action gap**: `chaseOverTime(A, 0, 30, ChaseTimeReeval.NONE)` | accept (warning recorded) | implemented: manifest-declared; differential fixture: `synthetic/chase-condition-agentlab`, probe `chase-over-time` | `baseline-supported` (manifest-covered); emission via catalog spelling is lowering-dependent |
| **Generic value gap**: `@Condition isGameInProgress() == true` | accept | implemented: manifest entry; probe `is-game-in-progress` | `baseline-supported` (manifest-covered) |
| **Member value/signature gap**: `getPlayersInRadius(...).setStatusEffect(eventPlayer, 30)` | **reject** (arity: `.setStatusEffect` needs `player, assister, status, duration`) | implemented: rejects with a structured arity diagnostic (`missing-argument`); probe `invalid-arity-member` | `baseline-supported` (manifest-covered) |
| **Enum-gated members**: `eventPlayer.setInvisibility(Invis.ALL)`, `eventPlayer.getThrottle()`, `worldVector(...)` (args typed `Invis`/`Transform`) | accept | implemented: manifest domain identities resolve as opaque members; member-existence validation is **lowering-dependent** (#8); probe `enum-gated-members` | `baseline-supported` (manifest-covered); catalog spellings lowering-dependent |
| **Named arguments / `ChaseReeval` alias**: `chase(A, 10, rate=2, ChaseReeval.NONE)` | accept (contextual alias resolution) | implemented: generic `name = expr` binding plus the `chase` special form; probes `chase-keywords`, `chase-reeval-context`, `chase-keyword-binding`, the `chase-*` diagnostic probes, and the `synthetic/chase-keywords` corpus fixture | `baseline-supported` (manifest-covered) |
| **Ambiguous Workshop enum spelling**: `ChaseTimeReeval.NONE`, `ChaseRateReeval.NONE`, and `Invis.NONE` all emit as bare `None` | — | emission-context resolution is **lowering-dependent** (needs the Workshop emission context); source implementation-side signature-pinned resolution stays source implementation-owned | `lowering-dependent` for context-free `None`; signature-pinned contexts are `baseline-supported` |
| **Constant-0 canonicalization**: `globalvar A = 0` drops the initializer; `= 5`/`= 0.0` preserved via the Initialize rule; `globalvar A 0` is an explicit index | canonical | implemented: `globalvar A = 0` drops the initializer and `= 5`/`= 0.0` are preserved, matching the reference; Initialize-rule synthesis is lowering-dependent | `baseline-supported` (source implementation part); lowering-dependent (Initialize synthesis) |
| **Diagnostic provenance**: unresolved action/value errors surface as structured semantic diagnostics, not emitter catalog misses | — | implemented: structured semantic diagnostics (`unknown-action`, `unknown-value`, `unknown-member`, `invalid-arity`, `invalid-receiver`, `action-in-value-position`, `value-in-action-position`, `invalid-call-context`, `invalid-iterable`, argument-binding codes); Workshop enum member/domain mismatch codes were removed with the catalog validation (PR #9) and stay `lowering-dependent` | `baseline-supported` (manifest-covered) |

## Boundaries

* **No per-symbol issues.** These evidence items are grouped into semantic
  categories; none justifies a one-symbol implementation issue.
* **No temporary Workshop IR.** Workshop-dependent features are classified
  `lowering-dependent` in the support matrix and inventory-only until the
  `workshop-rs` integration stage (#8). Nothing here starts `workshop-rs`
  work or duplicates catalog/WIR/emitter data.
* **Runtime content registry stays deferred.** This baseline is
  compile-time language-compatibility metadata, versioned with the pinned
  reference identity. A runtime content registry (heroes/maps/abilities
  content data, extension boundaries, independent version identities) is not
  triggered by this inventory (see [`compat-manifest-spec.md`](compat-manifest-spec.md)
  for the boundary).
* **Sequencing.** The bounded child-issue categories and their ordering are
  recorded in the issue #1 execution list (#3–#7); this baseline does not
  create implementation issues.

## Related documents

* [`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md): pinned reference identity and provenance
* [`support-matrix.md`](support-matrix.md): corpus-evidenced declared surface
* [`compat-manifest-spec.md`](compat-manifest-spec.md): machine-readable semantic manifest specification (data in `crates/opy-rs/src/manifest/`)
* [`compatibility/support-matrix.json`](../../compatibility/support-matrix.json): machine-readable state tracking
* [`compatibility/README.md`](../../compatibility/README.md): corpus and harness layout
