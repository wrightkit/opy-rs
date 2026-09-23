# OPY source and licensing policy

[← Compatibility index](README.md)

## Clean-room and source-attribution policy

The policy below is the opy-rs-adapted summary of WrightKit's accepted
component boundary (wright `docs/licensing.md`) and reference pinning policy
(wright ADR-0007).

### Component boundary

| Component | May invoke or inspect the reference? | Boundary and distribution rule |
| --- | --- | --- |
| `opy-rs` core (lexer, preprocess, CST/parser, semantic resolution, HIR, diagnostics) | No | Independently implemented code. It must not link to the reference, copy its source, import its internal AST/types, or compile against its generated artifacts. |
| Compatibility harness / oracle tool | Yes, for isolated evaluation | It may invoke a separately installed/pinned reference and compare documented or generated results. It must remain separable from the core build and runtime distribution. |
| Compatibility fixtures (upstream example/test corpus) | Only after attribution review | Attributed, licensed, redistribution-reviewed upstream example and test fixture files (e.g. the GPL-3.0 OverPy `examples/*.opy` corpus) may be retained under `crates/opy-rs/tests/fixtures/corpus/` as ordinary test inputs, with per-file origin, license, redistribution status, byte identity against the pinned content commit, and SHA-256 records (see the fixture corpus policy below). They are never imported by core code and never bundled into core builds or release artifacts. |
| Generated reference artifacts (oracle snapshots, manifests) | Only after attribution review | Store identifiers, hashes, generators, or reviewable artifacts only when their license and redistribution status are recorded. Do not add reference implementation/data content or unclear third-party content. |
| CI and development scripts | Yes, when isolated | They may install or invoke a pinned external oracle for a compatibility check, but must not silently turn it into a core dependency or bundled release component. |

No allow-listed path may import reference implementation details into the
core. When a compatibility component is added, its ownership, license,
invocation method, and distribution status must be named in its own manifest
or README and linked from this document before it is used.

### Fixture corpus policy

`crates/opy-rs/tests/fixtures/corpus/` may retain attribution/license/redistribution-reviewed
upstream example and test fixture files, e.g. the GPL-3.0 OverPy
`examples/*.opy` corpus, as documented test inputs. Each
imported file carries its per-file record (origin, license, redistribution
status, byte-identity against the pinned content commit, SHA-256) in
`crates/opy-rs/tests/fixtures/corpus/README.md` and its `fixture.json`; that record is
authoritative and is not duplicated here. The fixture corpus is test data,
not a core input: core code never imports it, and it is never
bundled into core builds or release artifacts. Content with unclear
attribution or no license is prohibited, and OverPy implementation or data
(`src/` sources, `src/data/*` tables, internal AST/types, generated
artifacts) must not be imported into the core.

### Permitted inputs to the independent core

The core may be developed from:

* independently authored `opy-rs` code;
* public language or output specifications, subject to their own license;
* behavior observed through lawful, documented compatibility tests;
* a separately specified interchange format whose attribution and license are
  known; and
* third-party dependencies whose license and compatibility have been reviewed.

### Blizzard Global derived data

`crates/opy-rs/src/compiler/blizzard_global.rs` contains compact, opy-rs-owned
compatibility facts for Blizzard Global glyph widths, spacing characters, and
the cased-progress glyph substitutions. The facts are independently represented
from observed output of the pinned OverPy 9.7.10 oracle; they are not copied
from `src/data/opy/blizzardGlobal.ts` or another upstream source file. The
lowering algorithms remain native Rust code in `compiler/lowering.rs`.

The data file is part of the core's independently authored compatibility
implementation, not a generated upstream artifact or runtime dependency. Its
provenance is the pinned identity above, and its distribution decision is to
ship only the compact facts required for interoperable output, with this
attribution record. Changes to the table must be justified by an oracle
behavior comparison and reviewed against the clean-room boundary.

Observed behavior is an interoperability input, not permission to copy an
implementation. A test that passes only by importing a reference module or
reusing its internal representation belongs outside the core boundary.

### Clean-room expectations

Contributors working on the core must:

1. implement opy-rs-owned data structures and transformations rather than
   mechanically translating reference source or types;
2. keep source attribution for imported examples, fixtures, and generated
   artifacts;
3. record the reference version and acquisition method for compatibility
   tests; and
4. stop and request review when a proposed dependency, fixture, or code sample
   has unclear licensing or would place a reference implementation detail in a
   core API.

Process or JSON separation is an engineering isolation technique. It is not by
itself a legal determination that two works may be combined or distributed.

### Pinning policy

The oracle pin is **version-exact and content-pinned**, and it is changed only
on **demonstrated behavioral need**, never on release recency:

1. **Version-exact.** The pin is an exact npm version plus its integrity hash,
   recorded in `tools/overpy/oracle/package.json`,
   `tools/overpy/oracle/pnpm-lock.yaml`, and `oracle-metadata.json`. No range
   specifiers, no `latest`, no caret.
2. **Content-pinned.** The recorded identity includes the npm integrity hash
   and the byte-verified git content commit. A version bump alone is not an
   oracle change.
3. **Demonstrated need only.** "Demonstrated" means a version-sensitivity run
   (the minimal repro plus the comparison result against candidate versions)
   showing a different accept/reject outcome or a different normalized output
   for a construct the corpus needs. Absence of measured divergence is a
   no-change decision.
4. **Single reference by default.** A second reference is added only when a
   divergence is demonstrated and a single reference cannot represent it.
5. **Identity in every result.** Every compatibility result records the exact
   pinned identity (version, content commit, integrity) and complete source
   project input manifest, including per-file hashes and a canonical project
   digest, so historical claims remain interpretable after any future
   re-baseline.

A pin change follows the structured review path: `oracle-metadata.json`,
lockfile, `run_oracle.py --update` snapshot review, fixture attribution notes,
and the affected docs in one reviewed change.

### Distribution policy

The default distribution contains `opy-rs`'s independently implemented code
and its own documentation and tests. It does not bundle OverPy, its source
tree or internal libraries, or reference artifacts whose redistribution has
not been reviewed. The optional compatibility workflow requires users or CI to
provide an external reference installation; that workflow must identify the
exact version and must not prevent the core from building, testing, or running
when the oracle is absent.

## Durable reference limitations

* **Pinning policy.** The oracle is version-exact and content-pinned and is
  changed only on demonstrated behavioral need, never on release recency.
  A version bump alone is not an oracle change.
* **Measured stability.** Every accept/reject outcome and diagnostic in the
  ported fixture set is byte-identical across `9.7.10 → 9.7.13` (measured in
  the WrightKit Track B investigation); only hero/settings schema data
  differs. Historical claims stay interpretable because every result records
  the exact pinned identity.
* **Settings data newer than the pin.** Hero/settings schema data newer than
  the pin (e.g. dmon/domina/mizuki/vendetta) is unavailable to fixtures until
  a demonstrated need triggers the upgrade.
* **Unresolved upstream questions.** Whether a post-9.7.13 OverPy adds `"""`
  docstrings, `#!obfuscate`, custom `_hp_*` members, or inline `if` without
  `else` is unverified; the version-sensitivity matrix must be re-run before
  any new acceptance is claimed.
* **Round-trip boundary.** Emitted `settings` sections are deliberately not
  reparseable by the Workshop source implementation; a `.ws` decompiler is a non-goal for
  `opy-rs` (Workshop → OPY decompilation is deferred to the `workshop-rs`
  integration stage; see `../language-support.md`).

## Related documents

* [`docs/language-support.md`](../language-support.md): public support contract and current states
* [`docs/opy/tooling-notes.md`](../opy/tooling-notes.md): harness usage
* [`tools/overpy/README.md`](../../tools/overpy/README.md): oracle and fixture layout
* [`crates/opy-rs/tests/fixtures/corpus/README.md`](../../crates/opy-rs/tests/fixtures/corpus/README.md): fixture attribution and layout
* WrightKit's policy sources this document adapts: `wright/docs/licensing.md`, wright ADR-0004 (OverPy licensing and clean-room boundary), ADR-0007 (reference pinning policy)

