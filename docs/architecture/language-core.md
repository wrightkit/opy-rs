# OverPy Language Core Contract

`opy-rs` is an independently usable Rust implementation of the OverPy language. This document defines the current semantic ownership and implementation model; it does not claim that every core feature is already implemented.

## Upstream core is the executable specification

For the declared OverPy core-language surface, the established upstream OverPy implementation is the executable specification. Core behavior is presumptively in scope unless it is explicitly excluded as editor/browser/integration functionality or is demonstrated to be a non-contractual implementation artifact.

Compatibility inventory, probes, differential tests, corpus fixtures, and real projects verify completeness and observable compatibility. They do not decide feature-by-feature whether established core language behavior belongs in `opy-rs`.

Upstream implementation structure is not an architecture mandate. `opy-rs` should understand the source behavior and implement it directly in clear Rust rather than mechanically translating upstream internals.

## Semantic ownership

`opy-rs` owns:

- lexical and source syntax;
- preprocessing, includes, defines, macros, and source directives;
- source-language name/member/signature resolution;
- OverPy type and contextual semantics;
- source diagnostics and provenance;
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

Validated/generated data is appropriate for large declarative inventories such as names, aliases that are purely identities, mechanical signatures, provenance links, and completeness inventories when those records do not themselves become a language for programming semantics.

A manifest field that causes generic Rust to choose source-language behavior is not justified merely because an existing manifest already contains similar fields. When metadata drives receiver restrictions, argument semantics, contextual dispatch, special lowering, or other observable behavior, treat the placement as architecture debt to audit and prefer direct typed implementation unless a concrete domain reason justifies a declarative representation.

Existing manifest-driven semantics remain implementation reality until deliberately refactored; this contract does not silently change runtime behavior.

## Feature locality

A language feature should have a discoverable semantic home. Phase modules such as parser, lowerer, compiler, or generic registry are infrastructure boundaries, not default dumping grounds for unrelated source-language policy.

When implementing a feature in an already mixed responsibility, the smallest local extraction needed to keep the changed semantics cohesive is within scope. Do not use that permission for unrelated cleanup or speculative abstraction.

## Compatibility target

Target observable semantic compatibility: accepted/rejected source, meaningful diagnostics/provenance, source tooling behavior, lowering semantics, and declared round-trip/reconstruction contracts.

Compiler-output identity, optimizer shape, temporary names, formatting, or internal upstream IR are not contracts unless independently required for observable behavior.

`opy-rs` does not introduce a WrightKit-only OPY dialect.