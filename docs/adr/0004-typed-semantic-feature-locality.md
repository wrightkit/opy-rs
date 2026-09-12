# ADR-0004: Typed semantic behavior and feature-local ownership

- Status: Accepted
- Date: 2026-09-12 (backfilled)
- Historical evidence: [Issue #202](https://github.com/wrightkit/opy-rs/issues/202), [Issue #203](https://github.com/wrightkit/opy-rs/issues/203), [Issue #207](https://github.com/wrightkit/opy-rs/issues/207), [Issue #209](https://github.com/wrightkit/opy-rs/issues/209), [Issue #221](https://github.com/wrightkit/opy-rs/issues/221), [PR #205](https://github.com/wrightkit/opy-rs/pull/205), [PR #210](https://github.com/wrightkit/opy-rs/pull/210), [PR #213](https://github.com/wrightkit/opy-rs/pull/213), [PR #214](https://github.com/wrightkit/opy-rs/pull/214), [PR #222](https://github.com/wrightkit/opy-rs/pull/222), [current language-core contract](../architecture/language-core.md)

## Context

The upstream OverPy implementation is the executable specification for the
declared core-language surface, but its internal organization is not an
architecture mandate. `opy-rs` needs to preserve observable source behavior
while keeping the semantic owner discoverable as parser, preprocessing,
resolution, HIR, lowering, compiler, or reconstruction responsibilities evolve.

Large inventories are useful for mechanical facts, yet a manifest or registry
can silently become a second programming language when it decides receiver
rules, contextual dispatch, argument semantics, or special lowering.

## Decision

Implement observable OverPy behavior and invariants in typed Rust close to the
owning language feature. Use validated or generated data for declarative facts
such as names, aliases that are identities, signatures, enum membership,
catalog links, and provenance. Existing metadata-driven behavior is treated as
architecture debt to audit, not as a reason to add more semantic control
fields.

Give durable parser, preprocessing, semantic, lowering, compiler-integration,
and reconstruction responsibilities discoverable feature or domain ownership.
Phase modules remain orchestration and genuinely shared-state boundaries; the
decision is about semantic locality, not file counts or a prescribed module
tree. Preserve shared cursor, preprocessing, lowering, and compiler state only
where the existing contract requires it.

## Alternatives and trade-offs

- Extend a generic manifest/registry: it is concise for new cases but hides
  behavior behind an implicit semantic DSL and weakens ownership.
- Keep all behavior in phase-wide modules: it minimizes file movement but
  makes unrelated features harder to locate and change safely.
- Translate upstream module structure directly: it may resemble the oracle but
  confuses compatibility evidence with an implementation boundary.

The chosen approach costs explicit Rust code and bounded internal module
organization, while keeping behavior reviewable, typed, and near its owner.

## Consequences

New language behavior should be placed by semantic responsibility and tested
through observable source, diagnostics, provenance, lowering, or tooling
contracts. Inventory completeness and conformance evidence remain important,
but neither narrows the upstream language scope nor authorizes implementation
behavior by itself.
