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
| Optional or defaulted middle argument omitted | Shifts the remaining arguments left without checking types, writing a mistyped call. | Rejects the call (`missing-argument`). | Project owner, `wrightkit/opy-rs#366` | `structural_convergence::an_omitted_optional_argument_does_not_shift_the_rest`; probe gap `reference-binds-mistyped-optional` |

`tools/overpy/probe-gaps.json` records the families the builtin probe can reach; the folded vector-zero form is not generated by the probe and is pinned by its test alone.

CI enforces this on pinned real projects (`tools/overpy/structural-gate.json`, currently Bastion's `src/main.opy` and `src/externalMain.opy`) and on the corpus; see `tools/overpy/README.md`. A gate exception is recorded in that file with the same fields as the table above.

Upstream internal architecture and IR remain non-contractual; only the emitted Workshop structure is.

`opy-rs` does not introduce a WrightKit-only OPY dialect.
