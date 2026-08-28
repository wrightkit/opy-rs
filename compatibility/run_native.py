#!/usr/bin/env python3
"""Compile the compatibility corpus and write an explicit native report."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from collections import Counter
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "compatibility" / "fixtures"
DEFAULT_RESULTS = ROOT / "target" / "opy-compiler-results"
DEFAULT_REPORT = ROOT / "target" / "opy-compiler-report.json"


class NativeError(RuntimeError):
    """A corpus execution or result-contract error."""


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise NativeError(f"cannot read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise NativeError(f"JSON root must be an object: {path}")
    return value


def fixtures() -> list[tuple[Path, dict[str, Any]]]:
    found = []
    for metadata_path in sorted(FIXTURES.glob("**/fixture.json")):
        metadata = read_json(metadata_path)
        source = metadata_path.parent / metadata["source"]
        if not source.is_file():
            raise NativeError(f"source does not exist: {source}")
        found.append((metadata_path.parent, metadata))
    if not found:
        raise NativeError(f"no fixtures found under {FIXTURES}")
    return found


def run_fixture(binary: Path, directory: Path, metadata: dict[str, Any]) -> dict[str, Any]:
    source = directory / metadata["source"]
    completed = subprocess.run(
        [
            str(binary),
            "compile",
            "--format",
            "json",
            "--language",
            "en-US",
            str(source),
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise NativeError(
            f"{metadata['id']}: compile did not produce JSON "
            f"(exit {completed.returncode}): {completed.stderr.strip()}"
        ) from error
    if not isinstance(result, dict) or result.get("schemaVersion") != 1:
        raise NativeError(f"{metadata['id']}: invalid compile report schema")
    if result.get("compile", {}).get("status") not in {"success", "failure"}:
        raise NativeError(f"{metadata['id']}: invalid compile status")
    result["fixture"] = metadata["id"]
    result["input"] = {
        "source": metadata["source"],
        "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
    }
    result["compile"]["processExitCode"] = completed.returncode
    if completed.stderr:
        result["compile"]["stderr"] = completed.stderr
    return result


def compare(
    directory: Path, metadata: dict[str, Any], native: dict[str, Any]
) -> dict[str, Any]:
    oracle = read_json(directory / "oracle.json")
    oracle_compile = oracle["compile"]
    native_compile = native["compile"]
    oracle_status = oracle_compile["status"]
    native_status = native_compile["status"]
    if oracle_status == native_status == "success":
        output = (
            "match"
            if oracle_compile["workshop"] == native_compile["workshop"]
            else "normalized-output-difference"
        )
        status = "match" if output == "match" else "inconclusive"
    elif oracle_status == native_status == "failure":
        output = "not-applicable"
        status = "expected-failure"
    elif oracle_status == "success" and native_status == "failure":
        output = "not-applicable"
        status = "known-gap"
    else:
        output = "not-applicable"
        status = "unexpected-success"
    return {
        "fixture": metadata["id"],
        "expectedStatus": metadata["expectedStatus"],
        "oracleStatus": oracle_status,
        "nativeStatus": native_status,
        "status": status,
        "output": output,
        "failureClass": native_compile.get("failureClass"),
        "diagnostics": native_compile.get("diagnostics", []),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        type=Path,
        default=ROOT / "target" / "debug" / "opy-cli",
        help="path to a built opy-cli binary",
    )
    parser.add_argument("--results", type=Path, default=DEFAULT_RESULTS)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument(
        "--allow-inconclusive",
        action="store_true",
        help="return success even when normalized output needs semantic WIR review",
    )
    args = parser.parse_args(argv)
    binary = args.binary.resolve()
    if not binary.is_file():
        raise NativeError(f"opy-cli binary does not exist: {binary}")
    results_root = args.results.resolve()
    report_path = args.report.resolve()
    results_root.mkdir(parents=True, exist_ok=True)

    comparisons = []
    for directory, metadata in fixtures():
        native = run_fixture(binary, directory, metadata)
        result_path = results_root / metadata["id"] / "result.json"
        result_path.parent.mkdir(parents=True, exist_ok=True)
        result_path.write_text(json.dumps(native, indent=2, sort_keys=True) + "\n")
        comparisons.append(compare(directory, metadata, native))

    counts = Counter(item["status"] for item in comparisons)
    report = {
        "schemaVersion": 1,
        "comparison": {
            "oracle": "compatibility/fixtures/**/oracle.json",
            "producer": "opy-cli compile --format json",
            "contract": "normalized Workshop text; semantic differences require canonical WIR evidence",
        },
        "summary": {"fixtures": len(comparisons), "counts": dict(sorted(counts.items()))},
        "results": comparisons,
    }
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report["summary"], indent=2, sort_keys=True))
    blocking = {"unexpected-success"}
    if not args.allow_inconclusive:
        blocking.add("inconclusive")
    return 1 if any(item["status"] in blocking for item in comparisons) else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except NativeError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        raise SystemExit(2)
