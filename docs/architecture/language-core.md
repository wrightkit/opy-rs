# OverPy Language Core Contract

`opy-rs` is an independently usable Rust implementation of the OverPy language. This document defines the current semantic ownership and implementation model; it does not claim that every core feature is already implemented.

## Upstream core is the executable specification

For the declared OverPy core-language surface, the established upstream OverPy implementation is the executable specification. Core behavior is presumptively in scope unless it is explicitly excluded as editor/browser/integration functionality or is demonstrated to be a non-contractual implementation artifact.

Compatibility inventory, probes, differential tests, corpus fixtures, real projects, and compiler-output comparison verify completeness and compatibility. They do not decide feature-by-feature whether established core language behavior belongs in `opy-rs`.

Upstream implementation structure is not an architecture mandate. `opy-rs` should understand the source behavior and implement it directly in clear Rust rather than mechanically translating upstream internals.

## Semantic ownership

`opy-rs` owns:

- lexical and source syntax;
- preprocessing, includes, defines, macros, and source directives;
- source-language name/member/signature resolution;
- OverPy type and contextual semantics;
- source diagnostics and source mapping;
- OPY HIR and source-aware tooling semantics;
- OverPy-specific lowering/compiler behavior;
- Workshop→OPY reconstruction.

These responsibilities remain usable independently from complete Workshop emission when the operation itself is Workshop-independent.

## Behavior and declarative data

Implement program behavior and semantic invariants in typed Rust. Examples include:

- receiver/member semantics;
- argument binding and contextual dispatch;
- macro and directive behavior;
- contextual evaluation/coercion rules;
- special lowering decisions;
- compile-time evaluation and control flow.

Validated/generated data is appropriate for large declarative inventories such as names, aliases that are purely identities, mechanical signatures, source attribution links, and completeness inventories when those records do not themselves become a language for programming semantics.

A manifest field that causes generic Rust to choose source-language behavior is not justified merely because an existing manifest already contains similar fields. When metadata drives receiver restrictions, argument semantics, contextual dispatch, special lowering, or other observable behavior, treat the placement as architecture debt to audit and prefer direct typed implementation unless a concrete domain reason justifies a declarative representation.

Existing manifest-driven semantics remain implementation reality until deliberately refactored; this contract does not silently change runtime behavior.

## Feature locality

A language feature should have a discoverable semantic home. Phase modules such as parser, lowerer, compiler, or generic registry are infrastructure boundaries, not default dumping grounds for unrelated source-language policy.

When implementing a feature in an already mixed responsibility, the smallest local extraction needed to keep the changed semantics cohesive is within scope. Do not use that permission for unrelated cleanup or speculative abstraction.

## Compatibility target

For the declared OverPy compiler surface, the pinned upstream OverPy compiler output is the correctness target. `opy-rs` compilation converges structurally on it, as defined by WrightKit goal principle 7.

The compared structure is the canonical Workshop program, and it must match upstream for:

- rule order;
- Workshop action/value/event/enum identities;
- control-flow structure;
- condition shape, including the number and order of rule conditions;
- string and value construction, including null and literal forms;
- variable names and indices, including compiler-generated helper variables and their initialization;
- element cost, including optimizer behavior that affects emitted structure.

Compatibility is measured by parsing both the upstream output and the `opy-rs` output with `workshop-rs` and comparing the canonical programs structurally. Text diffs, line counts, and text-pattern counts are not compatibility evidence. Formatting, whitespace, and comments are not criteria.

Structural rewrites are not accepted, even when they appear behaviorally equivalent or reduce element cost. Any structural difference is a defect unless it is a recorded exception approved by the owner, including a difference for an apparent upstream bug. Each exception records the upstream behavior, the `opy-rs` behavior, the approving decision, and the test that pins it.

### Approved exceptions

| Family | Pinned OverPy 9.7.10 | `opy-rs` | Decision | Pinning test |
| --- | --- | --- | --- | --- |
| Array in the `Object` text parameter of `logToInspector`, `bigMessage`, `smallMessage`, `setObjectiveDescription`, `progressBarHud` (and `printLog`) | Writes the array, with its type-check warning hidden. | Rejects at canonical validation (`semantic type 'Object'`). | workshop-rs ADR-0014 (no acceptance evidence), `wrightkit/opy-rs#372` | `structural_convergence::an_array_for_an_object_text_parameter_stays_rejected`; probe gap `validation-array-for-object` |
| `Number 0` in the `createEffect` position, from folding a vector `x - x` | Writes `Number 0`. | Rejects at canonical validation (`semantic type 'Vector|Player'`). | workshop-rs ADR-0014, `wrightkit/opy-rs#372` (`#366` item 3) | `structural_convergence::a_vector_that_folds_to_zero_in_a_vector_position_stays_rejected` |
| Optional or defaulted middle argument omitted | Shifts the remaining arguments left without checking types, writing a mistyped call. | Binds the shift identically; canonical validation rejects the mistyped call. | Project owner, `wrightkit/opy-rs#366`; canonical validation unchanged per `wrightkit/workshop-rs#357` | `structural_convergence::an_omitted_optional_argument_shifts_the_rest_but_stays_rejected`; probe gap `reference-binds-mistyped-optional` |
| `timeToString` padded slices | Writes `String Slice(Add(...), True, …)`: a Number in the `stringSlice` string position and a Boolean in the start-index position. | Writes `String Slice(Custom String("{0}", Add(...)), 1, …)`, the canonical form of the same formula; the slices stay unevaluated on constant input exactly as the reference emits them. | `wrightkit/opy-rs#434`; canonical validation rejects both upstream argument types | `structural_convergence::time_to_string_keeps_the_reference_shape_modulo_canonical_types` |
| `buttonToString` expansion | Writes `Mapped Array(Input Binding String(b), …)`, putting the binding `String` in the `Array` parameter. | Writes `Mapped Array(Array(Input Binding String(b)), …)` — the canonical form of the same formula, since the expansion selects the whole string. | `wrightkit/opy-rs#447`; canonical validation rejects `String` in the `mappedArray` `Array` parameter | `structural_convergence::button_to_string_expands_with_the_canonical_array_wrap` |
| Non-finite `log` folds (`log(0)`, `log(-1)`, `log(x, non-positive base)`) | Writes `-Infinity`/`NaN`, which the canonical grammar cannot parse. | Keeps the pre-fold `log`/`ln` power approximation so the call emits a valid program. | `wrightkit/opy-rs#435`; pending the number-spelling decision in `wrightkit/workshop-rs#358` | `structural_convergence::a_non_finite_log_fold_keeps_the_pre_fold_expansion` |
| Bare `Player` at the `EntityId` parameter of `destroyEffect`/`destroyIcon` | Writes `Destroy Effect(Event Player)`/`Destroy Icon(Event Player)` unchecked. | Rejects at canonical validation (`semantic type 'EntityId'`); the real-project fixtures pass player variables there, never a bare Player. | Project owner, `wrightkit/opy-rs#487`; workshop-rs ADR-0014 (no admission evidence) | `structural_convergence::a_bare_player_for_an_entity_parameter_stays_rejected`; probe gap `validation-player-for-entity-id` |
| `sorted` with a non-array first argument (`sorted(5, key=...)`, `sorted("bca", key=...)`) | Writes `Sorted Array(5, Current Array Element)` unchecked. | Rejects at canonical validation (`semantic type 'Array'`). | Project owner, `wrightkit/opy-rs#446`; workshop-rs ADR-0014 (no admission evidence) | `structural_convergence::a_non_array_sorted_argument_stays_rejected` |
| `ColorLiteral.LIGHT_*` (`LIGHT_RED`, `LIGHT_PURPLE`, `LIGHT_VIOLET`, `LIGHT_GRAY`) | Resolves the member through a display-name lookup that does not exist for `onlyInOverpy` members and splices the missing text into the argument list, writing an empty argument slot (`Set Global Variable(g, )`) the canonical grammar cannot parse. Through the `Color.` receiver the same member emits `Custom Color(255, 112, 122, 255)`. | Writes the canonical `Custom Color(255, 112, 122, 255)`, as the `Color.` receiver does; an empty argument slot has no Workshop meaning and is not representable in the canonical `Program` (workshop-rs ADR-0008 decision 7). | Project owner, `wrightkit/opy-rs#468` | `structural_convergence::literal_domain_members_emit_the_reference_display_names`; probe gap `literal-onlyinoverpy-empty-slot` |

`*Literal` member emission otherwise needs no exception: `opy-rs` reproduces
the reference's bare display-name emission (`Team 1`, `Assault`).

### Pending exceptions

This difference has no recorded owner approval. It stays rejected at
canonical validation, pending workshop-rs admission evidence for the position
(ADR-0014).

| Family | Pinned OverPy 9.7.10 | `opy-rs` | Status | Pinning test |
| --- | --- | --- | --- | --- |
| `x`/`y`/`z` component access on context-player receivers (`eventPlayer`, `hostPlayer`, `localPlayer`, `attacker`, `victim`) | Writes `X Component Of(Event Player)` and peers. | Rejects at canonical validation (`semantic type 'Vector'`). | pending workshop-rs admission evidence; `wrightkit/workshop-rs#336` named this outcome under option 1 and `#350` implemented it; a regression against `wrightkit/opy-rs#414`, which compiled `eventPlayer.x` until the workshop-rs 1.4 catalog | `structural_convergence::context_player_components_stay_rejected_under_canonical_validation`; `context-player-components`/`context-player-component-validation` fixtures |
| `settings` line inside an `enum` body (or the inline member tail, `enum E: settings {}`) | Absorbs the line as an enum member named `settings`, ignoring its object tail, and emits no settings section for it. | Extracts a line-start `settings` there as the settings block and emits it (the enum keeps its other members); the inline-tail form rejects when the enum is left memberless. | pending owner decision; tracked as `wrightkit/opy-rs#517` | `settings::tests::settings_inside_an_enum_body_is_extracted` |

`tools/overpy/probe-gaps.json` records the families the builtin probe can reach; the folded vector-zero form and the `sorted` special form are not generated by the probe and are pinned by their tests alone.

CI enforces this on pinned real projects (`tools/overpy/structural-gate.json`, currently Bastion's `src/main.opy` and `src/externalMain.opy`) and on the corpus; see `tools/overpy/README.md`. A gate exception is recorded in that file with the same fields as the table above.

Upstream internal architecture and IR remain non-contractual; only the emitted Workshop structure is.

`opy-rs` does not introduce a WrightKit-only OPY dialect.
