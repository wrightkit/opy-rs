# Compatibility Fixture Corpus — Provenance

This directory is the opy-rs compatibility corpus: OPY sources with their
pinned-oracle snapshots (`oracle.json`), ported from the WrightKit project's
evidence base (wright `compatibility/fixtures/`) and re-verified against the
pinned OverPy 9.7.10 oracle (see
[`docs/compatibility/upstream-references.md`](../../docs/compatibility/upstream-references.md)
for the dated verification record).

Corpus policy: every fixture records provenance in its `fixture.json`
(`kind`, `origin`, `license`, `redistributable`, and — for imported
fixtures — `sourceCommit`, `sourceUrl`, `licenseUrl`, `modifications`).
No fixture is committed without a clear provenance record. The pinned
reference identity and the clean-room policy are in
[`docs/compatibility/upstream-references.md`](../../docs/compatibility/upstream-references.md).
These files are oracle evidence, not core inputs: the core never imports
them, and they are never bundled into core builds or release artifacts.

## Layout

```text
fixtures/<category>/<name>/
  fixture.json   # metadata and expected compile status
  source.opy     # input (or the source path named by fixture.json; multi-file
                 # fixtures name their main file via fixture.json "source")
  oracle.json    # normalized pinned-oracle snapshot (compile status, exit code,
                 # diagnostics, normalized Workshop text, output hash)
```

## Synthetic fixtures (WrightKit-authored)

`fixtures/synthetic/` — fixtures authored for the WrightKit compatibility
corpus (AGPL-3.0-or-later, `kind: original`): an initial set ported unchanged
from the wright repository corpus, extended by fixtures added in this
repository:

| Fixture | Covers |
| --- | --- |
| `basic-rule` | minimal rule with `@Event global`, `disableInspector()` |
| `control-flow` | `if`/`elif`/`else`, `for … in range`, `while`, `pass` |
| `declarations-rules` | `globalvar`/`playervar`/`subroutine`/`def`/`enum`, rule headers |
| `declarations-numbers` | numeric literal forms and variable-index declarations |
| `expressions-values` | expressions, arrays, strings, vectors, calls, `.format` |
| `preprocessing` | `#!include` (with `shared.opy`), `#!define` object/function-like, `#!undef` |
| `issue-31-positive` | Pinned positive probe for rule-prefix templates, include prefix restoration, macro/enum redeclaration, and normalized translations |
| `issue-31-negative` | Pinned negative probe for a translation code outside the oracle's exact set |
| `issue-31-nested-scope` | Pinned nested-include probe for observable optimization state transitions |
| `diagnostics` | expected-failure fixture with a syntax diagnostic |
| `settings` | top-of-file `settings { … }` JSONC block |
| `receiver-calls` | receiver/member call forms (derived from the real-world overpy-meipocalypse corpus; see its `fixture.json` provenance note) |
| `chase-enums` | `ChaseTimeReeval`/`ChaseRateReeval` enum domains |
| `chase-condition-agentlab` | `chaseOverTime(...)` in rule conditions (agent-lab regression) |
| `chase-keywords` | named/keyword arguments and the `chase`/`ChaseReeval` contextual forms |
| `for-range-agentlab` | `for` with implicit default-variable binder (agent-lab regression, `kind: derived`) |
| `issue-28-*` | pure OPY syntax probes for switch, do-while, hex, membership, modifiers, dicts, comprehensions, lambda, and negative diagnostics |
| `issue-35-integration` | minimal OPY HIR to canonical Workshop WIR validation and deterministic emission slice |
| `issue-46-primitives` | #46 oracle-backed primitive lowering probe: assignments and modifications (including `**=`), expressions, indexing, format, initializers, implicit default variables at fixed slots; the snapshot constrains the native compiler through structural equivalence |
| `issue-46-unsupported` | #46 negative probe: a dict-indexed assignment the compiler rejects with the stable source-attributed diagnostic while the oracle compiles it |
| `issue-59-postfix-assignment` | #59 oracle-backed postfix `++`/`--` assignment probe for global, player, and single-level indexed variables; the snapshot constrains native lowering through canonical WIR equivalence |
| `issue-59-postfix-negative` | #59 pinned negative probe for rejected prefix and embedded postfix forms with a stable source-attributed parse diagnostic |
| `issue-47-control-flow` | #47 pinned oracle-backed control-flow lowering probe: if/elif/else, while, range-for, do-while expansion, switch fallthrough/default, and direct break |
| `issue-47-unsupported` | #47 negative probe: a break hidden inside a conditional switch arm is accepted by the source implementation/oracle but rejected by the compiler with a stable source-attributed diagnostic |
| `issue-47-switch-order` | #47 pinned oracle probe for a default arm before later case arms and source-order fallthrough |
| `issue-47-switch-structured-target` | #47 pinned oracle probe for nested if/while structure in an earlier arm and later case/default targets |
| `issue-47-switch-multiple-break` | #47 pinned oracle probe for multiple direct breaks; the source implementation preserves the source while the compiler reports the canonical multi-target WIR gap |
| `issue-47-do-while-shapes` | #47 pinned oracle probe for direct, conditional, and nested do-while break lowering |
| `issue-47-do-while-invalid-placement` | #47 pinned negative probe for the stable do-while placement diagnostic |
| `issue-29-*` | directive/include/main-file preprocessing probes |
| `issue-33-*` | switch break/fallthrough, f-string interpolation, and lambda negative probes |
| `receiver-playervar` | bare variable member expression `A = B.C` with preserved receiver/member provenance |

## Real-world fixtures

### Derived from upstream OverPy `examples/` (GPL-3.0-only)

Fixtures whose OPY sources are byte-identical to the pinned reference tree's
`examples/` content (verified by `diff` against the pinned content
commit on acquisition; the fixture `fixture.json` files additionally record
the example-capture commit `eea67adbcf6926c4004e35e25ab4be072624a44e` used
by the original WrightKit acquisition pipeline — both identities describe
the same bytes). Mapping to the pinned tree (content commit
`889d9749d1def17f146548cbddb94ea1ab015847`):

| Fixture | Pinned-tree source | `expectedStatus` |
| --- | --- | --- |
| `overpy-cake` | `examples/cake.opy` | success |
| `overpy-pixelart` | `examples/pixelart.opy` | success |
| `overpy-santa` | `examples/santa.opy` | success |
| `overpy-cronch` | `examples/cronch.opy` | success |
| `overpy-broken-weapons` | `examples/broken_weapons.opy` | success |
| `overpy-client-to-server` | `examples/clientToServer.opy` | success |
| `overpy-crosshair` | `examples/crosshair.opy` | success |
| `overpy-inputhud` | `examples/inputhud.opy` | success |
| `overpy-parabola` | `examples/parabola.opy` | success |
| `overpy-meipocalypse` | `examples/meipocalypse/*.opy` (the non-OPY generators `generateWalls.js`, `generateZoneVariables.js`, `elements.md`, `todo.md` are not ported) | failure (reference rejects part of the project; recorded in the snapshot) |
| `overpy-zencopter` | `examples/Zencopter/heli.opy` (`heliturrets.js` is not ported) | failure |

License note: the upstream `examples/` are part of the GPL-3.0-only OverPy
repository. These fixture OPY sources are retained **as provenance-recorded
evidence** for oracle evaluation: they are not redistributed as library code
or imported into the opy-rs core, and the repository's AGPL-3.0-or-later
license does not relicense them. `oracle.json` snapshots record observed
oracle behavior (accept/reject, diagnostics, normalized Workshop text) for
compatibility evidence. See the clean-room policy in
`docs/compatibility/upstream-references.md`.

Current real-world reference-success/native-gap cases also keep a
minimized regression snippet in the parent fixture's `regressions` metadata.
Those snippets retain a link to the full-project oracle evidence; they are not
standalone replacement expectations.

`census/workshop-feature-census` is the OPY consumer-side census fixture. Its
`workshopFeatureIds` are opaque IDs reserved for the `workshop-rs#10` contract;
this repository does not copy Workshop catalog definitions or signatures.

### Independent third-party projects (BSD-2-Clause)

| Fixture | Origin | `expectedStatus` |
| --- | --- | --- |
| `ow1-emulator` | [Overwatch-1-Emulator/ow1-emulator](https://github.com/Overwatch-1-Emulator/ow1-emulator) `src/1v1_main.opy`, full include closure, pinned commit `25cd6ce8d4acdd64b66c862a55c7ed66c8e50af1`, BSD-2-Clause | failure (recorded reference diagnostics) |
| `6v6-adjustments` | [6v6-Adjustments/6v6-adjustments](https://github.com/6v6-Adjustments/6v6-adjustments) `src/main.opy`, full include closure, pinned dev-branch commit `624480db6b7494f8bd5f3ab68fbb7e96a7726702`, BSD-2-Clause | failure (recorded reference diagnostics) |

Both are committed unchanged from their pinned commits with per-file SHA-256
records in `fixture.json` (`files` map). The multi-file fixtures exercise
large include closures, macros, subroutines, settings blocks, and Workshop
enum/action surfaces; their snapshots keep `expectedStatus: failure` with the
reference diagnostics, exactly like the pinned oracle behaves.

> Note on `acquisitionMethod` in imported `fixture.json` files: imported
> fixtures preserve `scripts/acquire-corpus.py` as the **historical** Wright
> acquisition method. That one-off M11 helper and its corpus manifest were
> retired after live OPY compatibility ownership moved to opy-rs. The field is
> retained only as provenance and is not an executable instruction. Current
> corpus/oracle acquisition and re-validation are owned by this repository;
> do not reintroduce the old Wright helper unless new evidence demonstrates a
> concrete need for an equivalent owner-side acquisition workflow.

## Not ported / dropped

* **No fixture was dropped for provenance reasons**: fixtures in the WrightKit
  corpus carried complete, reviewed provenance and are ported.
* Upstream `examples/` not ported (candidates for later expansion once a
  demonstrated need exists): `lucioball_all_heroes.opy`, `skirmish_elim.opy`,
  `settings.opy.json` (settings schema data, not an OPY source), and the
  non-OPY generator files listed above.
* The full-gamemode list in the upstream `examples/README.md` (OverWordle,
  Riptire Racing, Conquest, …) points to third-party repositories; they stay
  out of the corpus until their licenses are reviewed.
