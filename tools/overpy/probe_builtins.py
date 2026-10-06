#!/usr/bin/env python3
"""Compare every manifest builtin call with the pinned OverPy oracle.

Generates one probe per manifest function, trailing-default omission, argument
position and small literal, plus each value in a Boolean slot and in the
replacement slot of `.replace`, in default and `#!optimizeForSize` modes;
compiles each with the oracle and natively; and compares the parsed canonical
programs structurally.

A difference must belong to a gap recorded in `probe-gaps.json`, and a
recorded gap must still match something, so an unexplained or a stale
difference fails. The oracle rejecting a probe while the native compiler
accepts it is diagnostics parity, not structure, and is only counted.

Every accepted spelling — each manifest function id and alias source — is
also probed against the oracle's name resolution (a `:name` variant, and the
`base` calls for catalog-backed entries). An `unknown-spelling` finding means
the oracle reports the declared name as an unknown function; it fails the run
unless a recorded gap lists it as an approved exception with its decision.

    cargo build --locked -p opy-cli --features compatibility --bin opy-compat
    python3 tools/overpy/probe_builtins.py --binary target/debug/opy-compat
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "crates/opy-rs/src/manifest/data/manifest.json"
GAPS = ROOT / "tools/overpy/probe-gaps.json"


def run(command: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, text=True, encoding="utf-8", **kwargs)


def function_of(probe_id: str) -> str:
    return probe_id.split(":")[1]


def classify(findings: list[dict], gaps: list[dict]):
    """Split findings into explained by gap id, unexplained, and stale gaps."""
    explained: dict[str, list[dict]] = {gap["id"]: [] for gap in gaps}
    unexplained = []
    for finding in findings:
        for gap in gaps:
            functions = gap["functions"]
            if (
                finding["status"] == gap["status"]
                and (functions == "*" or function_of(finding["id"]) in functions)
                and gap.get("variant", "") in finding["id"]
                and gap.get("detail", "") in finding["detail"]
            ):
                explained[gap["id"]].append(finding)
                break
        else:
            unexplained.append(finding)
    stale = [gap_id for gap_id, matched in explained.items() if not matched]
    return explained, unexplained, stale


def compare_batch(binary: Path, selected: list[dict], scratch: Path) -> dict:
    scratch.mkdir()
    probes = scratch / "probes.json"
    references = scratch / "references.json"
    probes.write_text(json.dumps(selected), encoding="utf-8")
    batch = run(
        ["node", str(ROOT / "tools/overpy/probe_batch.cjs"), str(probes), str(references)],
        capture_output=True,
    )
    if batch.returncode != 0:
        raise RuntimeError(f"oracle probe batch failed: {batch.stderr}")
    compared = run(
        [str(binary), "probe-compare", str(probes), str(references)],
        capture_output=True,
    )
    if compared.returncode != 0:
        raise RuntimeError(f"native probe comparison failed: {compared.stderr}")
    sys.stderr.write(compared.stderr)
    return json.loads(compared.stdout)


def compare_probes(binary: Path, selected: list[dict], jobs: int) -> dict:
    scratch_root = ROOT / "target"
    scratch_root.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opy-probe-", dir=scratch_root) as scratch:
        batches = [selected[i::jobs] for i in range(min(jobs, len(selected)))]
        with ThreadPoolExecutor(max_workers=jobs) as pool:
            reports = list(pool.map(
                lambda item: compare_batch(binary, item[1], Path(scratch) / str(item[0])),
                enumerate(batches),
            ))
    counts = Counter()
    findings = []
    for report in reports:
        counts.update(report["counts"])
        findings.extend(report["findings"])
    order = {probe["id"]: i for i, probe in enumerate(selected)}
    findings.sort(key=lambda finding: order[finding["id"]])
    return {
        "probes": sum(report["probes"] for report in reports),
        "counts": dict(sorted(counts.items())),
        "findings": findings,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--binary", type=Path, default=ROOT / "target/debug/opy-compat"
    )
    parser.add_argument("--report", type=Path, help="write the JSON report here")
    parser.add_argument(
        "--functions",
        help="comma-separated function ids to probe instead of every function",
    )
    parser.add_argument("--jobs", type=int, default=1, help="independent probe workers (default: 1)")
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")

    generated = run([str(args.binary), "probe-generate"], capture_output=True)
    if generated.returncode != 0:
        sys.stderr.write(generated.stderr)
        return 2
    selected = json.loads(generated.stdout)
    if args.functions:
        wanted = set(args.functions.split(","))
        selected = [p for p in selected if function_of(p["id"]) in wanted]
    try:
        report = compare_probes(args.binary, selected, args.jobs)
    except RuntimeError as error:
        print(error, file=sys.stderr)
        return 2

    findings = [f for f in report["findings"] if f["status"] != "native-accepts"]
    gaps = json.loads(GAPS.read_text(encoding="utf-8"))["gaps"]
    explained, unexplained, stale = classify(findings, gaps)
    if args.functions:
        stale = []

    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))["functions"]
    # `:name` probes exercise name resolution only; they are not valid sample
    # calls and must not count toward call-shape coverage.
    probed = {
        function_of(p["id"]) for p in selected if p["id"].rsplit(":", 1)[-1] != "name"
    }
    unprobed = sorted({f["id"] for f in manifest} - probed)

    print(f"probes: {report['probes']}  {report['counts']}")
    for gap in gaps:
        print(f"  gap {gap['id']}: {len(explained[gap['id']])} probes ({gap['owner']}, {gap['decision']})")
    print(f"  unexplained: {len(unexplained)}")
    if not args.functions:
        print(f"  manifest functions without a valid sample call: {len(unprobed)}")
    for finding in unexplained:
        print(f"    {finding['status']} {finding['id']} {finding['detail'][:100]}")
    for gap_id in stale:
        print(f"    stale gap {gap_id}: it no longer matches any probe")
    if args.report:
        args.report.write_text(
            json.dumps({**report, "unprobedFunctions": unprobed}, indent=2),
            encoding="utf-8",
        )
    return 1 if unexplained or stale else 0


if __name__ == "__main__":
    sys.exit(main())
