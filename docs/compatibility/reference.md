# Pinned OverPy reference

[← Compatibility index](README.md)

Status: accepted baseline (issue #2). The pinned compatibility oracle and the
clean-room/source-attribution policy for OPY compatibility tests.
Scope: project-level identity, licensing, and attribution for the OverPy
reference `opy-rs` studies or derives compatibility behavior from; the durable
record that lets `opy-rs` read and reference upstream source without per-symbol
or per-file attribution bureaucracy.

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
| Registry integrity | `sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw==` (recorded in `tools/overpy/oracle/oracle-metadata.json` and the lockfile) |
| Recorded `gitHead` | `1e2688954302a402d076944b46db07efb14d7b61`. npm's `gitHead` field lags the tarball content by one release; it is the `v9.7.9` tag commit and must **not** be treated as the content commit |
| License assumption | GPL-3.0-only (engineering assumption, not a legal conclusion; the npm `package.json` ships no `license` field, see `tools/overpy/oracle/oracle-metadata.json`) |
| Language | en-US (Workshop locale for reference tests) |

The integrity hash pins the content. Reproduction uses the recorded identity,
never `latest` or a range (see the pinning policy below).

### Acquisition and verification record

* The reference tree used for the support contract was acquired from the pinned npm
  tarball `overpy@9.7.10` and byte-verified against the repository content at
  the pinned content commit `889d9749d1def17f146548cbddb94ea1ab015847` (tag
  `v9.7.10`). The durable record is the tarball integrity hash and the content
  commit, not any machine-specific extraction path.
* The npm package is installed separately into `tools/overpy/oracle/` via the
  pinned `pnpm-lock.yaml`; `pnpm install` resolves `overpy@9.7.10` by its
  integrity hash.
* The compatibility corpus was re-run against a fresh install of the pinned
  package on 2026-08-17; the fixture snapshots present at that date
  (`crates/opy-rs/tests/fixtures/corpus/**/oracle.json`) matched byte-for-byte. Each current
  fixture carries its own oracle snapshot from the same pinned package; rerun
  `python3 tools/overpy/run_oracle.py` to re-verify the full corpus.
* The imported example fixtures were verified byte-identical to the pinned
  tree's `examples/` content (see `crates/opy-rs/tests/fixtures/corpus/README.md`).

### Oracle role

OverPy 9.7.10 (pinned content) is the compatibility **oracle** and **behavior
reference** for `opy-rs`'s `.opy` source implementation. It is not a production runtime
dependency of `opy-rs` and is never bundled into release artifacts. Concretely,
it serves as:

* the reference for S (syntax), D (diagnostic), and N (normalized-output)
  tests in the compatibility corpus (`crates/opy-rs/tests/fixtures/corpus/**`,
  `tools/overpy/oracle/`);
* the independent reference for support claims in
  [`docs/language-support.md`](../language-support.md), exercised through the
  corpus snapshots and differential harness;
* the reference for differential parity at the Opy HIR v2 boundary
  ([`docs/hir/opy-hir-v2.md`](../hir/opy-hir-v2.md)): the native differential
  suite (`crates/opy-rs/tests/differential.rs`, merged in PR #13) runs
  every corpus fixture through the native pipeline in `cargo test` and
  compares status, rule names, and diagnostics against the recorded
  oracle snapshots.

### Invocation records

The harness invokes the oracle only through documented, isolated entry points:

* **CLI** (`tools/overpy/run_oracle.py`, run with cwd = `tools/overpy/oracle/`):

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

### Upstream surfaces behind the support contract

The support contract in [`docs/language-support.md`](../language-support.md) is
grounded in the pinned tree, specifically:

* `README.md`: user-visible syntax tour (rules, annotations, subroutines,
  macros, enums, settings) and advertised feature surface;
* `examples/`: real-world OPY corpus (see `crates/opy-rs/tests/fixtures/corpus/README.md`
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

