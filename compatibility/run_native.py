#!/usr/bin/env python3
"""Compile the compatibility corpus and run the shared differential contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path
from typing import Any

import diff


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "compatibility" / "fixtures"
COMPILER_EXPECTATIONS = ROOT / "compatibility" / "compiler-expectations.json"
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
        help="return success for inconclusive comparisons after shared checks pass",
    )
    args = parser.parse_args(argv)
    binary = args.binary.resolve()
    if not binary.is_file():
        raise NativeError(f"opy-cli binary does not exist: {binary}")
    results_root = args.results.resolve()
    report_path = args.report.resolve()
    results_root.mkdir(parents=True, exist_ok=True)

    for directory, metadata in fixtures():
        native = run_fixture(binary, directory, metadata)
        result_path = results_root / metadata["id"] / "result.json"
        result_path.parent.mkdir(parents=True, exist_ok=True)
        result_path.write_text(json.dumps(native, indent=2, sort_keys=True) + "\n")

    # Keep expectation loading, input-hash validation, stage comparison, and
    # blocking status classification in the existing independent comparator.
    try:
        return diff.run_compiler(
            FIXTURES,
            report_path,
            results_root,
            COMPILER_EXPECTATIONS,
            allow_inconclusive=args.allow_inconclusive,
        )
    except diff.DiffError as error:
        raise NativeError(str(error)) from error


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except NativeError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        raise SystemExit(2)
