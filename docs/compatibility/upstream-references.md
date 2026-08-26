# Upstream Reference and Provenance: OverPy

Status: accepted baseline (issue #2). The pinned compatibility oracle and the
clean-room/provenance policy for the opy-rs evidence base
Scope: project-level provenance for the OverPy reference `opy-rs` studies or
derives compatibility knowledge from; the durable record that lets `opy-rs`
read and reference upstream source without per-symbol or per-file provenance
bureaucracy

`opy-rs` is independently implemented Rust software (AGPL-3.0-or-later) that
reimplements OverPy source-language semantics. Reimplementing compatible
language semantics without inspecting the reference implementation is neither
required nor desirable, but the reference is an **oracle and behavior
reference only**, never a runtime dependency, and never a source of copied
implementation.

This document is the opy-rs-owned record of the reference identity, invocation
records, and the clean-room policy. It is adapted from the WrightKit project's
accepted policy ([`wright/docs/licensing.md`](https://github.com/wrightkit/wright/blob/main/docs/licensing.md)
and wright ADR-0004/ADR-0007), summarized below to fit this repository. It is
not legal advice and does not settle questions that require a qualified lawyer.

## OverPy reference

### Identity

| Field | Value |
| --- | --- |
| Project | OverPy: high-level language for the Overwatch Workshop |
| Repository | <https://github.com/Zezombye/overpy> |
| Pinned reference | npm `overpy@9.7.10` |
| Content commit | `889d9749d1def17f146548cbddb94ea1ab015847` (git tag `v9.7.10`; byte-verified) |
| Registry integrity | `sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw==` (recorded in `compatibility/oracle/oracle-metadata.json` and the lockfile) |
| Recorded `gitHead` | `1e2688954302a402d076944b46db07efb14d7b61`. npm's `gitHead` field lags the tarball content by one release; it is the `v9.7.9` tag commit and must **not** be treated as the content commit |
| License assumption | GPL-3.0-only (engineering assumption, not a legal conclusion; the npm `package.json` ships no `license` field, see `compatibility/oracle/oracle-metadata.json`) |
| Language | en-US (Workshop locale for reference evidence) |

The integrity hash pins the content. Reproduction uses the recorded identity,
never `latest` or a range (see the pinning policy below).

### Acquisition and verification record

* The reference tree used for the inventory was acquired from the pinned npm
  tarball `overpy@9.7.10` and byte-verified against the repository content at
  the pinned content commit `889d9749d1def17f146548cbddb94ea1ab015847` (tag
  `v9.7.10`). The durable record is the tarball integrity hash and the content
  commit, not any machine-specific extraction path.
* The npm package is installed separately into `compatibility/oracle/` via the
  pinned `pnpm-lock.yaml`; `pnpm install` resolves `overpy@9.7.10` by its
  integrity hash.
* The compatibility corpus was re-run against a fresh install of the pinned
  package on 2026-08-17; the fixture snapshots present at that date
  (`compatibility/fixtures/**/oracle.json`) matched byte-for-byte. Each current
  fixture carries its own oracle snapshot from the same pinned package; rerun
  `python3 compatibility/run_oracle.py` to re-verify the full corpus.
* The imported example fixtures were verified byte-identical to the pinned
  tree's `examples/` content (see `compatibility/fixtures/README.md`).

### Oracle role

OverPy 9.7.10 (pinned content) is the compatibility **oracle** and **behavior
reference** for `opy-rs`'s `.opy` frontend. It is not a production runtime
dependency of `opy-rs` and is never bundled into release artifacts. Concretely,
it serves as:

* the reference for S (syntax), D (diagnostic), and N (normalized-output)
  evidence in the compatibility corpus (`compatibility/fixtures/**`,
  `compatibility/oracle/`);
* the source of systematic probe validation for the proactive compatibility
  baseline (see [`docs/opy/compatibility-baseline.md`](../opy/compatibility-baseline.md)
  and [`docs/opy/compat-manifest-spec.md`](../opy/compat-manifest-spec.md));
* the reference for differential parity at the Opy HIR v2 boundary
  ([`docs/hir/opy-hir-v2.md`](../hir/opy-hir-v2.md)): the native differential
  suite (`crates/opy-frontend/tests/differential.rs`, merged in PR #13) runs
  every corpus fixture through the native pipeline in `cargo test` and
  compares status, rule-name, and diagnostic evidence against the recorded
  oracle snapshots.

### Invocation records

The harness invokes the oracle only through documented, isolated entry points:

* **CLI** (`compatibility/run_oracle.py`, run with cwd = `compatibility/oracle/`):

  ```sh
  pnpm exec overpy compile --input <source.opy> --output <workshop.txt> \
      --language en-US --root <fixture-dir> --main-file <source-name>
  ```

  Flags verified against the pinned tree's `src/cli.ts`: `-i/--input`,
  `-o/--output`, `-l/--language` (default `en-US`), `--root`, `--main-file`,
  `--ignore-variable-index` / `--ignore-subroutine-index` (decompile only),
  `-V/--version`. `compile` with no input exits 2 with `No input provided`
  (upstream `runCliTests.mjs`). `--help`/`--version` are read-only queries.
* **Library API** (npm `overpy` module, `overpy.d.ts`): `compile(content,
  language?, rootPath?, mainFileName?) → Promise<CompileResult>`,
  `decompileAllRules(content, language?, {ignoreVariableIndex?,
  ignoreSubroutineIndex?})`, `decompileActions`, `decompileConditions`,
  `astToOpy`, `readyPromise`. Upstream test entry points: `runTests.mjs`
  (compile tests against `src/tests/results`, decompiler tests, and the
  QuickJS `__script__` probes), `runCliTests.mjs` (CLI behavior), jest via
  `jest.config.cjs` for `src/test/*.test.ts`.

### Upstream surfaces inspected for the inventory

The feature inventory in [`docs/language-support.md`](../language-support.md) and
its linked component inventories is
grounded in the pinned tree, specifically:

* `README.md`: user-visible syntax tour (rules, annotations, subroutines,
  macros, enums, settings) and advertised feature surface;
* `examples/`: real-world OPY corpus (see `compatibility/fixtures/README.md`
  for the ported subset and the per-file mapping);
* `src/tests/`: 60 `.opy` compile tests with 50 pinned result files plus 17
  decompiler inputs (16 pinned results) covering arrays, macros, enums,
  dicts, gotos, includes, loops, operators, rule prefixes, strings,
  translations, custom game settings, and full gamemode `z_*` programs;
* `runTests.mjs` / `runCliTests.mjs` / `jest.config.cjs`: upstream test
  entry points (compile, decompile, CLI, QuickJS macro runtime);
* `src/compiler/{tokenizer.ts,parser.ts,astParser.ts,astToWorkshop.ts,
  compiler.ts,translations.ts}` and `src/decompiler/`: behavior reference
  for grammar, preprocessing, lowering, and decompilation semantics (read
  for observation only);
* `src/data/{actions.ts,values.ts,constants.ts,customGameSettings.ts,
  gamemodes.ts,heroes.ts,maps.ts,localizedStrings.ts,other.ts}` and
  `src/data/opy/{annotations,blizzardGlobal,constants,functions,
  internalFunctions,keywords,macros,memberFunctions,modules,preprocessing}.ts`,
  the upstream data surface (action/value/member/enum/hero/map domains).
  These files are GPL-3.0 data and are **not** imported into `opy-rs`; the
  opy-rs-owned semantic manifest records only oracle-validated facts
  (see `compat-manifest-spec.md`).

## Clean-room and provenance policy

The policy below is the opy-rs-adapted summary of WrightKit's accepted
component boundary (wright `docs/licensing.md`) and reference pinning policy
(wright ADR-0007).

### Component boundary

| Component | May invoke or inspect the reference? | Boundary and distribution rule |
| --- | --- | --- |
| `opy-rs` core (lexer, preprocess, CST/parser, semantic resolution, HIR, diagnostics) | No | Independently implemented code. It must not link to the reference, copy its source, import its internal AST/types, or compile against its generated artifacts. |
| Compatibility harness / oracle tool | Yes, for isolated evaluation | It may invoke a separately installed/pinned reference and compare documented or generated results. It must remain separable from the core build and runtime distribution. |
| Compatibility fixtures (upstream example/test corpus) | Only after provenance review | Provenance/license/redistribution-reviewed upstream example and test fixture files (e.g. the GPL-3.0 OverPy `examples/*.opy` corpus) may be retained under `compatibility/fixtures/` as documented, isolated oracle evidence, with per-file origin, license, redistribution status, byte-identity against the pinned content commit, and SHA-256 records (see the fixture corpus policy below). They are never imported by core code and never bundled into core builds or release artifacts. |
| Generated reference artifacts (oracle snapshots, manifests) | Only after provenance review | Store identifiers, hashes, generators, or reviewable artifacts only when their license and redistribution status are recorded. Do not add reference implementation/data content or unclear third-party content. |
| CI and development scripts | Yes, when isolated | They may install or invoke a pinned external oracle for a compatibility check, but must not silently turn it into a core dependency or bundled release component. |

No allow-listed path may import reference implementation details into the
core. When a compatibility component is added, its ownership, license,
invocation method, and distribution status must be named in its own manifest
or README and linked from this document before it is used.

### Fixture corpus policy

`compatibility/fixtures/` may retain provenance/license/redistribution-reviewed
upstream example and test fixture files, e.g. the GPL-3.0 OverPy
`examples/*.opy` corpus, as documented, isolated oracle evidence. Each
imported file carries its per-file record (origin, license, redistribution
status, byte-identity against the pinned content commit, SHA-256) in
`compatibility/fixtures/README.md` and its `fixture.json`; that record is
authoritative and is not duplicated here. The fixture corpus is oracle
evidence, not a core input: core code never imports it, and it is never
bundled into core builds or release artifacts. Content with unclear
provenance or no license is prohibited, and OverPy implementation or data
(`src/` sources, `src/data/*` tables, internal AST/types, generated
artifacts) must not be imported into the core.

### Permitted inputs to the independent core

The core may be developed from:

* independently authored `opy-rs` code;
* public language or output specifications, subject to their own license;
* behavior observed through lawful, documented compatibility tests;
* a separately specified interchange format whose provenance and license are
  known; and
* third-party dependencies whose license and compatibility have been reviewed.

Observed behavior is an interoperability input, not permission to copy an
implementation. A test that passes only by importing a reference module or
reusing its internal representation belongs outside the core boundary.

### Clean-room expectations

Contributors working on the core must:

1. implement opy-rs-owned data structures and transformations rather than
   mechanically translating reference source or types;
2. keep source provenance for imported examples, fixtures, and generated
   artifacts;
3. record the reference version and acquisition method for compatibility
   evidence; and
4. stop and request review when a proposed dependency, fixture, or code sample
   has unclear licensing or would place a reference implementation detail in a
   core API.

Process or JSON separation is an engineering isolation technique. It is not by
itself a legal determination that two works may be combined or distributed.

### Pinning policy

The oracle pin is **version-exact and content-pinned**, and it is changed only
on **demonstrated behavioral need**, never on release recency:

1. **Version-exact.** The pin is an exact npm version plus its integrity hash,
   recorded in `compatibility/oracle/package.json`,
   `compatibility/oracle/pnpm-lock.yaml`, and `oracle-metadata.json`. No range
   specifiers, no `latest`, no caret.
2. **Content-pinned.** The recorded identity includes the npm integrity hash
   and the byte-verified git content commit. A version bump alone is not an
   oracle change.
3. **Demonstrated need only.** "Demonstrated" means a version-sensitivity run
   (the minimal repro plus the evidence source against candidate versions)
   showing a different accept/reject outcome or a different normalized output
   for a construct the corpus needs. Absence of measured divergence is a
   no-change decision.
4. **Single reference by default.** A second reference is added only when a
   divergence is demonstrated and a single reference cannot represent it.
5. **Identity in every result.** Every compatibility result records the exact
   pinned identity (version, content commit, integrity), so historical claims
   remain interpretable after any future re-baseline.

A pin change follows the structured review path: `oracle-metadata.json`,
lockfile, `run_oracle.py --update` snapshot review, fixture provenance notes,
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
  ported evidence set is byte-identical across `9.7.10 → 9.7.13` (measured in
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
  reparseable by the Workshop frontend; a `.ws` decompiler is a non-goal for
  `opy-rs` (Workshop → OPY decompilation is deferred to the `workshop-rs`
  integration stage; see `support-matrix.md`).

## Related documents

* [`docs/language-support.md`](../language-support.md): audited public support contract and current states
* [`docs/opy/compatibility-baseline.md`](../opy/compatibility-baseline.md): tiered planning baseline
* [`docs/opy/compat-manifest-spec.md`](../opy/compat-manifest-spec.md): machine-readable semantic manifest specification
* [`docs/opy/tooling-notes.md`](../opy/tooling-notes.md): harness usage
* [`compatibility/README.md`](../../compatibility/README.md): oracle and fixture layout
* [`compatibility/fixtures/README.md`](../../compatibility/fixtures/README.md): corpus provenance
* WrightKit's policy sources this document adapts: `wright/docs/licensing.md`, wright ADR-0004 (OverPy licensing and clean-room boundary), ADR-0007 (reference pinning policy)
