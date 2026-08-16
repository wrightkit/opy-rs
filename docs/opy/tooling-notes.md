# opy-rs Compatibility Tooling Notes

Small operational notes for the compatibility harness
(`compatibility/`); the full layout contract is in
[`compatibility/README.md`](../../compatibility/README.md).

## Prerequisites

* Python 3 (stdlib only — no pip dependencies).
* For oracle execution only: Node + pnpm
  (`pnpm install --dir compatibility/oracle`, which resolves the pinned
  `overpy@9.7.10` by integrity hash). No Node toolchain is needed to run
  the harness tests or the fixture snapshot checks, and the Rust crates are
  never required here.

## Commands

```sh
# Harness tests (no oracle needed)
python3 -m unittest discover -s compatibility/tests

# Regenerate snapshots from the pinned oracle (only when intentionally
# accepting a reference-behavior change; review the diff in the same commit)
python3 compatibility/run_oracle.py --update

# Verify snapshots still match the pinned oracle (fails on any mismatch)
python3 compatibility/run_oracle.py

# Differential report against an external producer's results (generic
# producer contract; the native Rust differential suite is the opy-rs
# producer side and runs in cargo test — see compatibility/README.md)
python3 compatibility/diff.py --results <results-root> --report compatibility/report.json
python3 compatibility/diff.py --producer-command '<cmd template>' --report compatibility/report.json
```

## Machine-readable support matrix

`compatibility/support-matrix.json` is the mechanically checkable state
tracking of the declared OverPy feature surface (states `planned`,
`frontend-supported`, `semantic-supported`, `lowering-dependent`,
`end-to-end-supported`; see [`docs/opy/support-matrix.md`](../opy/support-matrix.md)).
The consistency check lives in `compatibility/tests/test_support_matrix.py`
and runs as part of the harness test suite: every feature id is unique, every
state/category is from the declared domains, and every `fixtures:` evidence
path exists in the corpus.

## Changing the pinned oracle

A pin change is an explicit, reviewed change: update
`compatibility/oracle/package.json` + `pnpm-lock.yaml` +
`oracle-metadata.json`, re-run `run_oracle.py --update`, review every snapshot
diff and fixture provenance note, and update the reference identity records in
`docs/compatibility/upstream-references.md` (policy: changed only on
demonstrated behavioral need, never on release recency).
