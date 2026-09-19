# OverPy compatibility test harness

This directory contains the pinned OverPy reference runner and the scripts
that execute ordinary OPY source and compiler differential tests. It is
evaluation tooling, not a dependency of the `opy-rs` core.

## Pinned oracle

`oracle/package.json` pins OverPy `9.7.10`; `oracle/pnpm-lock.yaml` pins its
integrity. `oracle-metadata.json` records the package identity, repository,
license assumption, and Workshop language. The oracle is installed separately
and is never bundled into `opy-rs` or imported by the Rust core.

```sh
pnpm install --dir tools/overpy/oracle
```

## Fixtures and tests

Each fixture under
`crates/opy-rs/tests/fixtures/corpus/<category>/<name>/` contains a
`fixture.json` manifest, OPY input, and `oracle.json` result snapshot. The
manifest is the only per-fixture expectation record. It keeps concrete source
attribution/licensing and reproducibility data, plus the source and compiler
test declarations. The snapshot records the pinned reference output and the
complete source-project digest.

The Rust differential test runs the native frontend without Node or OverPy:

```sh
cargo test -p opy-rs --test differential
```

The Python harness drives the public CLI and, for semantic-WIR comparisons,
the feature-gated `opy-compat` comparison target:

```sh
python3 -m unittest discover -s tools/overpy/tests
python3 tools/overpy/run_oracle.py
cargo build --locked -p opy-cli --bin opy-cli
cargo build --locked -p opy-cli --features compatibility --bin opy-compat
python3 tools/overpy/run_native.py \
  --binary target/debug/opy-cli \
  --semantic-binary target/debug/opy-compat
```

Use `run_oracle.py --update` only when intentionally accepting a changed
result from the pinned reference. Snapshot changes must be reviewed with the
fixture input and concrete attribution/reproducibility data.

`run_native.py` writes producer results and reports under `target/`. The
comparison stages are compile status, diagnostics, exact/normalized output,
failure frontier, semantic WIR, diagnostic code, and explicitly declared
compiler contracts. Missing producer output or unavailable semantic-WIR data
is `inconclusive` and blocks the normal command.

`diff.py` can also compare any producer that writes the compile-result schema:

```sh
python3 tools/overpy/diff.py \
  --producer-command 'your-producer --fixture {fixture_id} --input {source} --output {result}' \
  --report target/opy-rs-differential-report.json
```

The placeholder command is tokenized without a shell. The public compiler
report does not contain oracle input or semantic-WIR comparison data; those
belong to this isolated harness.
