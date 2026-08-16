# Proactive OPY Compatibility Baseline

Status: accepted baseline (planning) — proactive OPY compatibility baseline
for opy-rs (issue #2); ported and adapted from the WrightKit evidence base
Scope: forward-looking, tiered inventory of the OPY language surface against
the pinned OverPy 9.7.10 reference, classifying every category by
implementation tier and by support dimension

This document is the planning counterpart to
[`support-matrix.md`](support-matrix.md): the support matrix records the
corpus-evidenced surface the opy-rs frontend targets, while this baseline
records how the remaining surface is **tiered and sequenced**. A construct is
not called supported merely because it parses; each row states parse,
semantic, compilation, tooling/analysis, and reference coverage separately.

The reference identity is the pinned OverPy 9.7.10 content
(`889d9749d1def17f146548cbddb94ea1ab015847`); see
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md)
for provenance. Evidence claims in this document were verified against the
pinned oracle (all 26 corpus snapshots match on 2026-08-16). The opy-rs
frontend is not yet implemented on `main` (issues #3–#7), so the category
table below is the **tier assignment contract** — the state column of
`compatibility/support-matrix.json` tracks actual implementation progress
against it.

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

* **Parse** — accepted by the opy-rs frontend grammar;
* **Semantic resolution** — resolved to a meaningful HIR/semantic value
  (names, members, enums, call semantics);
* **Compilation** — standalone compile/emission through the `workshop-rs`
  backend succeeds with reference-equivalent semantics (**lowering-dependent**;
  inventory-only until integration, issue #8);
* **Tooling/analysis** — `check`/`analyze`/`lint`/`inspect` and language
  services can operate on the construct;
* **Reference coverage** — oracle probes/fixtures validate the behavior.

In the table, `✅` marks a dimension that is part of the declared contract for
the tier (to be evidenced by the frontend workstream), `❌` a deliberately
rejected/documented-absent dimension, `—` an inapplicable dimension, and
`partial` a bounded subset.

## Category inventory

| # | Category | Tier | Parse | Sem | Comp | Tooling | Ref |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | **Expression/postfix/member/call grammar** — operators and precedence, `[]` indexing, `.` member, calls, `++`/`--`, `del`, `in`/`not in`, hex `0x` | `baseline-supported` for the corpus subset (operators, indexing, calls, member/call); sub-forms below | ✅ corpus | ✅ | ✅ (integration) | ✅ | ✅ differential (issue #7) |
| 1a | `switch`/`case`/`default`, `do…while`, `not in`, `0x` hex literals | `evidence-prioritized` | ❌ (rejected, documented) | ❌ | ❌ | ❌ | ✅ oracle probes |
| 1b | String modifiers (`f`/`w`/`l`/`b`/`c`/`t`), dict literals, list comprehensions, `lambda` beyond `.map`/`sorted` | `legacy-quirk/demand-driven` (dicts, modifiers) / `evidence-prioritized` (comprehensions) | ❌ | ❌ | ❌ | ❌ | ✅ oracle probes |
| 2 | **Declarations** — `globalvar`/`playervar` (index + initializer forms), `subroutine`, `enum`, `macro` constants (incl. member constants) | `baseline-supported` | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 3 | **Assignments & control flow** — `=`, augmented (`+= … **=`, `min=`, `max=`), `if`/`elif`/`else`, `for … in range(...)`, `while`, `pass` | `baseline-supported` | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 4 | **Rule directives & annotations** — `@Event`, `@Condition`, bare `@Team`/`@Slot`, rule name, event defaults (`global`, `all` team/player) | `baseline-supported` (bare forms) | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 4a | `@Team`/`@Slot` with arguments, `@Name`, `@Hero`, `@Disabled`, `@Delimiter`, `@NewPage`, `@SuppressWarnings` | `evidence-prioritized` | ❌ | ❌ | ❌ | ❌ | ✅ oracle probes |
| 5 | **Preprocessing/include/macro** — `#!include`, `#!define` (object- and function-like), `#!undef`, include cycle detection | `baseline-supported` | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 5a | `#!mainFile`, `#!allowMacroRedeclaration`, `#!optimize*`/`#!replace0By*` family, `#!translations`, `#!rulePrefix*`, `__script__` JS hooks | `legacy-quirk/demand-driven` | ❌ | ❌ | ❌ | ❌ | partial |
| 6 | **Builtin actions & values (generic)** — the 225 action / 267 value Workshop surface | `baseline-supported` for the manifest-declared evidence surface (chaseOverTime, isGameInProgress, getPlayersInRadius, worldVector, the corpus call surface); the full surface stays **`baseline-planned`** | ✅ | ✅ | ✅ (catalog subset, integration) | ✅ | ✅ probes |
| 7 | **Receiver/member functions** — `eventPlayer.setMoveSpeed(100)`, `eventPlayer.isAlive()`, variable receivers | `baseline-supported` for the manifest-declared member surface (receiver categories, explicit-arg signatures); **`baseline-planned`** for the full member surface | ✅ | ✅ | ✅ (catalog subset, integration) | ✅ | ✅ |
| 8 | **Builtin enum/constant domains** — 46 upstream domains (incl. `Hero`/`Map`/`Gamemode` literals) | `baseline-supported` for the manifest-declared domains (reference-validated member lists); **`baseline-planned`** (systematic) for the full surface | ✅ (declared domains) | ✅ | partial | partial | ✅ probes |
| 9 | **Aliases** — old function names (`stopChasingVariable`→`stopChasing`, `getCurrentHero`→`getHero`, `hasStatusEffect`→`hasStatus`, …), hero renames (`MCCREE`→`CASSIDY`), `ChaseReeval` contextual alias | `baseline-supported` for the three manifest-declared non-contextual aliases and the `ChaseReeval` call-context resolution; the remaining alias surface stays `legacy-quirk/demand-driven` | ✅ (declared) | ✅ | ✅ (chase forms catalog-covered at integration) | ✅ | ✅ |
| 10 | **Modules** — `random.{randint,uniform,choice,shuffle}` | `baseline-supported` (corpus: `random.uniform`, `random.choice`) | ✅ | ✅ | ✅ (integration) | ✅ | ✅ |
| 11 | **Named/keyword arguments** — `chase(A, B, rate=30, …)`, generic `name = expr` binding against manifest signatures | `baseline-supported` for the evidence surface (generic keyword binding plus the `chase`/`ChaseReeval` call-context forms); `raycast` `include=`/`exclude=` forms and macro keyword arguments stay `evidence-prioritized` (no corpus/reference evidence in the declared surface) | ✅ | ✅ | ✅ (integration) | ✅ | ✅ probes |
| 12 | **Settings/content metadata** — `settings { … }` blocks | `baseline-supported` (JSONC subset + typed HIR payload); the Workshop `settings` emission table is **lowering-dependent** | ✅ | ✅ | ❌ (integration) | ✅ | ✅ |
| 12a | `settings "file"`, richer settings expressions, hero/map/ability content beyond the pin | `legacy-quirk/demand-driven` / `reference-limited` | ❌/partial | ❌ | ❌ | ❌ | partial (data newer than pin unavailable per the pinning policy) |
| 13 | **Source identity & diagnostics** — structured, source-located frontend errors, `wright-result/v1` | `baseline-supported` | ✅ | ✅ | — | ✅ | ✅ S/D |

## Residual evidence items (classified, not yet implemented)

Verified against the pinned oracle. Each item is classified with the tier it
belongs to; none is a per-symbol implementation request. Items marked
*manifest-covered* resolve through the OPY semantic compatibility manifest
(`crates/opy-frontend/src/manifest/`; frontend workstream) once the frontend
lands.

| Evidence | Oracle 9.7.10 | opy-rs status | Classification |
| --- | --- | --- | --- |
| **Bare playervar receiver** — `A = B.C` (declared playervar member on a player-valued receiver) | accept (`__playerVar__`) | not implemented — must accept per the manifest contract | `baseline-planned` (receiver/member semantics + playervar member resolution, category 7) |
| **Value member as statement** — `B.isAlive()` on its own line | **reject** ("Expected an action, but got … a value") | not implemented — must reject with `value-in-action-position` | `baseline-supported` (reviewed difference; recorded in the probe set) |
| **Generic action gap** — `chaseOverTime(A, 0, 30, ChaseTimeReeval.NONE)` | accept (warning recorded) | not implemented — resolves through the manifest; regression fixture: `synthetic/chase-condition-agentlab`, probe `chase-over-time` | `baseline-supported` (manifest-covered); emission via catalog spelling is lowering-dependent |
| **Generic value gap** — `@Condition isGameInProgress() == true` | accept | not implemented — manifest entry; probe `is-game-in-progress` | `baseline-supported` (manifest-covered) |
| **Member value/signature gap** — `getPlayersInRadius(...).setStatusEffect(eventPlayer, 30)` | **reject** (arity: `.setStatusEffect` needs `player, assister, status, duration`) | not implemented — must reject with `invalid-arity`; probe `invalid-arity-member` | `baseline-supported` (manifest-covered) |
| **Enum-gated members** — `eventPlayer.setInvisibility(Invis.ALL)`, `eventPlayer.getThrottle()`, `worldVector(...)` (args typed `Invis`/`Transform`) | accept | not implemented — member entries + enum domains (`Invis`, `Status`, `Transform`) in the manifest; probes `enum-gated-members`, `builtin-enums` | `baseline-supported` (manifest-covered); catalog spellings lowering-dependent |
| **Named arguments / `ChaseReeval` alias** — `chase(A, 10, rate=2, ChaseReeval.NONE)` | accept (contextual alias resolution) | not implemented — generic `name = expr` binding plus the `chase` special form; probes `chase-keywords`, `chase-reeval-context`, `chase-keyword-binding`, the `chase-*` diagnostic probes, and the `synthetic/chase-keywords` corpus fixture | `baseline-supported` (manifest-covered) |
| **Ambiguous Workshop enum spelling** — `ChaseTimeReeval.NONE`, `ChaseRateReeval.NONE`, and `Invis.NONE` all emit as bare `None` | — | emission-context resolution is **lowering-dependent** (needs the Workshop emission context); frontend-side signature-pinned resolution stays frontend-owned | `lowering-dependent` for context-free `None`; signature-pinned contexts are `baseline-supported` |
| **Constant-0 canonicalization** — `globalvar A = 0` drops the initializer; `= 5`/`= 0.0` preserved via the Initialize rule; `globalvar A 0` is an explicit index | canonical | not implemented — frontend preserves/omits per the reference; Initialize-rule synthesis is lowering-dependent | `baseline-supported` (frontend part); lowering-dependent (Initialize synthesis) |
| **Diagnostic provenance** — unresolved action/value errors surface as structured semantic diagnostics, not emitter catalog misses | — | manifest-covered (`unknown-action`, `unknown-value`, `unknown-member`, `invalid-arity`, `invalid-receiver`, `enum-domain-mismatch`, `action-in-value-position`, `value-in-action-position`, `invalid-call-context`, `invalid-iterable`, argument-binding codes) | `baseline-supported` (manifest-covered) |

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

* [`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md) — pinned reference identity and provenance
* [`support-matrix.md`](support-matrix.md) — corpus-evidenced declared surface
* [`compat-manifest-spec.md`](compat-manifest-spec.md) — machine-readable semantic manifest specification (data in `crates/opy-frontend/src/manifest/`)
* [`compatibility/support-matrix.json`](../../compatibility/support-matrix.json) — machine-readable state tracking
* [`compatibility/README.md`](../../compatibility/README.md) — corpus and harness layout
