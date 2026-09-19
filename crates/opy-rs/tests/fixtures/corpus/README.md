# OPY compatibility fixtures

This directory contains OPY inputs and pinned OverPy results used by the
source and compiler differential tests. Each fixture is ordinary test data:
the native implementation runs against the input, the pinned OverPy 9.7.10
result provides the reference behavior, and the runners assert the declared
relationship.

## Layout

```text
corpus/<category>/<name>/
  fixture.json   # input, attribution, and test expectations
  source.opy     # input, or the source path named by fixture.json
  oracle.json    # pinned reference result snapshot
```

`fixture.json` contains:

- `id`, `category`, optional `features`, and the fixture `source`;
- `attribution` with source kind, origin, license, and redistribution status;
- imported-source `sourceCommit`, `sourceUrl`, `licenseUrl`, and file hashes
  where attribution or reproducibility requires them;
- `tests.source` with the native source status, reference relationship, rule
  name comparison flag, and stable diagnostic code when applicable;
- `tests.compiler` with the native compiler status, relationship, comparison
  contract, and diagnostic details when applicable.

There is one manifest per fixture. The manifest does not contain Issue/PR
history, run timing, generic verification records, or generated reports.

`oracle.json` records the pinned oracle identity, the complete resolved OPY
source graph with per-file SHA-256 hashes, compile status, exit code,
diagnostics, normalized Workshop output, and output hash. It is the reference
test result, not a second expectation table. Run `python3 tools/overpy/run_oracle.py`
to verify the snapshots; use `--update` only when intentionally accepting a
new result from the pinned oracle.

## Fixture groups

Synthetic fixtures cover OPY syntax, preprocessing, diagnostics, source
semantics, and focused compiler lowering. Real-world fixtures retain complete
projects from the pinned OverPy examples or independently licensed projects;
their `regressions` entries only identify minimized `.opy` files excluded from
the full-project source digest. The complete project remains the integration
test.

`census/workshop-feature-census` contains opaque Workshop feature IDs used by
the OPY-side consumer test. Catalog definitions, validation, and emission
remain owned by `workshop-rs`.

Imported source files retain their applicable license, attribution, immutable
source revision, redistribution status, and file hashes. These concrete
records protect licensing and reproducibility; they do not define a generic
compatibility metadata model.
