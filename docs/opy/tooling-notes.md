# opy-rs Compatibility Tooling Notes

Small operational notes for the OverPy evidence harness; the full layout
contract is in [`tools/overpy/README.md`](../../tools/overpy/README.md).

## Prerequisites

* Python 3 (stdlib only, no pip dependencies).
* For oracle execution only: Node + pnpm
  (`pnpm install --dir tools/overpy/oracle`, which resolves the pinned
  `overpy@9.7.10` by integrity hash). No Node toolchain is needed to run
  the harness tests or the fixture snapshot checks, and the Rust crates are
  never required here.

## Commands

```sh
# Harness tests (no oracle needed)
python3 -m unittest discover -s tools/overpy/tests

# Regenerate snapshots from the pinned oracle (only when intentionally
# accepting a reference-behavior change; review the diff in the same commit)
python3 tools/overpy/run_oracle.py --update

# Verify snapshots still match the pinned oracle (fails on any mismatch)
python3 tools/overpy/run_oracle.py

# Compiler compatibility gate (public CLI plus internal evidence target)
cargo build --locked -p opy-cli --bin opy-cli
cargo build --locked -p opy-cli --features compatibility --bin opy-compat
python3 -B tools/overpy/run_native.py \
  --binary target/debug/opy-cli \
  --semantic-binary target/debug/opy-compat \
  --results target/opy-rs-compiler-results \
  --report target/opy-rs-compiler-report.json

# Differential report against an external producer's results (generic
# producer contract; the native Rust differential suite is the opy-rs
# producer side and runs in cargo test; see tools/overpy/README.md)
python3 tools/overpy/diff.py --results <results-root> --report target/opy-rs-differential-report.json
python3 tools/overpy/diff.py --producer-command '<cmd template>' --report target/opy-rs-differential-report.json
```

## Changing the pinned oracle

A pin change is an explicit, reviewed change: update
`tools/overpy/oracle/package.json` + `pnpm-lock.yaml` +
`oracle-metadata.json`, re-run `run_oracle.py --update`, review every snapshot
diff and fixture provenance note, and update the reference identity records in
`docs/compatibility/upstream-references.md` (policy: changed only on
demonstrated behavioral need, never on release recency).
