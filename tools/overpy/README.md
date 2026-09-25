# OverPy compatibility test harness

This directory contains the pinned OverPy reference runner and the scripts
that execute ordinary OPY source and compiler differential tests. It is
evaluation tooling, not a dependency of the `opy-rs` core.

## Pinned oracle

`oracle/package.json` pins OverPy `9.7.10`; `oracle/pnpm-lock.yaml` pins its
integrity. `oracle-metadata.json` records the package `gitHead`, the
byte-verified `contentCommit`, repository, license assumption, and Workshop
language. The oracle is installed separately and is never bundled into
`opy-rs` or imported by the Rust core.

```sh
pnpm install --dir tools/overpy/oracle
```

## Fixtures and tests

Each fixture under
`crates/opy-rs/tests/fixtures/corpus/<category>/<name>/` contains a
`fixture.json` manifest, OPY input, and `oracle.json` result snapshot. The
manifest is the only per-fixture expectation record. When needed, it keeps
concrete source attribution/licensing and reproducibility data, plus the source
and compiler test declarations. The snapshot records the pinned reference output
and the complete source-project digest.

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

## Builtin-call probe

`probe_builtins.py` compares every catalog-backed manifest function with the
pinned oracle. For each function it compiles the base call, each trailing or
single omission of a defaulted argument, each argument position replaced by
each small literal the position accepts, and the call as a Boolean argument
and as the replacement of `.replace`, in default and `#!optimizeForSize`
modes. The oracle compiles them all in one process; the native compiler
compiles the same programs, and both outputs are parsed by `workshop-rs` and
compared structurally.

```sh
cargo build --locked -p opy-cli --features compatibility --bin opy-compat
python3 tools/overpy/probe_builtins.py --binary target/debug/opy-compat
```

`--functions a,b` probes only those functions. `probe-gaps.json` records the
differences that remain, each with its cause and owner; a difference outside
it, or a recorded gap that matches nothing, fails the run. The report lists
the manifest functions without a valid sample call, which are the special-
lowering entries and those whose arguments are ids created by another call.
A probe the oracle rejects and the native compiler accepts is diagnostics
parity and is only counted.

## Structural convergence gate

`structural_gate.py` enforces the structural compatibility contract for real
projects. `structural-gate.json` pins each project by repository and full
commit SHA (never a ref) and lists its entry files. The gate fetches the
commit into `target/structural-gate/`, compiles each entry with the pinned
OverPy oracle and with `opy-rs`, parses both outputs with `workshop-rs`, and
compares the canonical programs. Projects whose license does not allow
redistribution, such as Bastion, are fetched at test time and never vendored;
redistributable projects stay in the corpus, where the semantic-WIR stage
applies the same comparison.

```sh
pnpm install --dir tools/overpy/oracle
cargo build --locked -p opy-cli --features compatibility --bin opy-compat
python3 tools/overpy/structural_gate.py --binary target/debug/opy-compat
```

A difference is reported by section, item name, and canonical path of the
first diverging node with both sides' values, never as a text diff. Every
difference fails unless `exceptions` in `structural-gate.json` records it by
project, entry, section, item name, and path, together with the upstream
behavior, the `opy-rs` behavior, the approving decision, and the pinning test.
A recorded exception that matches no difference fails the full run as stale.
Approved exceptions and their decisions are listed in
[`language-core.md`](../../docs/architecture/language-core.md#approved-exceptions).

`run_native.py` writes producer results and reports under `target/`. The
comparison stages are compile status, diagnostics, exact/normalized output,
failure frontier, semantic WIR, source-visible projections, diagnostic code,
and explicitly declared compiler contracts. A semantic-WIR fixture may declare
a named source-visible projection to compare selected pinned exact-output
comments in addition to its secondary semantic check. Missing producer output
or unavailable semantic-WIR data is `inconclusive` and blocks the normal
command.

`diff.py` can also compare any producer that writes the compile-result schema:

```sh
python3 tools/overpy/diff.py \
  --producer-command 'your-producer --fixture {fixture_id} --input {source} --output {result}' \
  --report target/opy-rs-differential-report.json
```

The placeholder command is tokenized without a shell. The public compiler
report does not contain oracle input or semantic-WIR comparison data; those
belong to this isolated harness.
