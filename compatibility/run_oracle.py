#!/usr/bin/env python3
"""Run the pinned OverPy oracle against the opy-rs compatibility corpus."""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

import input_identity


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_FIXTURES = ROOT / "compatibility" / "fixtures"
DEFAULT_ORACLE = ROOT / "compatibility" / "oracle"
SNAPSHOT_NAME = "oracle.json"


class RunnerError(RuntimeError):
    """A fixture or oracle execution error that should be shown to a user."""


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise RunnerError(f"cannot read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise RunnerError(f"JSON root must be an object: {path}")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def normalize_text(value: str) -> str:
    """Normalize line endings and presentation-only trailing whitespace."""

    if not value:
        return ""
    lines = value.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    lines = [line.rstrip(" \t") for line in lines]
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines) + ("\n" if lines else "")


def normalize_diagnostics(stderr: str) -> list[dict[str, str]]:
    """Keep diagnostics structured while preserving oracle wording and locations."""

    text = normalize_text(stderr).rstrip("\n")
    if not text:
        return []

    diagnostics = []
    for paragraph in re.split(r"\n{2,}", text):
        first_line = paragraph.splitlines()[0]
        if first_line.startswith("Error:"):
            severity = "error"
        elif first_line.startswith("Warning:"):
            severity = "warning"
        else:
            severity = "info"
        diagnostics.append({"severity": severity, "text": paragraph})
    return diagnostics


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_text(value: str) -> str:
    return sha256_bytes(value.encode("utf-8"))


def _inside(path: Path, directory: Path) -> bool:
    try:
        path.relative_to(directory)
    except ValueError:
        return False
    return True


def validate_fixture(path: Path) -> tuple[Path, dict[str, Any]]:
    metadata = load_json(path)
    required = ("schemaVersion", "id", "category", "source", "expectedStatus", "provenance")
    missing = [key for key in required if key not in metadata]
    if missing:
        raise RunnerError(f"{path}: missing fields: {', '.join(missing)}")
    if metadata["schemaVersion"] != 1:
        raise RunnerError(f"{path}: unsupported schemaVersion {metadata['schemaVersion']!r}")
    if metadata["expectedStatus"] not in ("success", "failure"):
        raise RunnerError(f"{path}: expectedStatus must be success or failure")
    if not isinstance(metadata["id"], str) or not metadata["id"]:
        raise RunnerError(f"{path}: id must be a non-empty string")
    if not isinstance(metadata["category"], str) or not metadata["category"]:
        raise RunnerError(f"{path}: category must be a non-empty string")

    provenance = metadata["provenance"]
    if not isinstance(provenance, dict):
        raise RunnerError(f"{path}: provenance must be an object")
    for key in ("kind", "origin", "license", "redistributable"):
        if key not in provenance:
            raise RunnerError(f"{path}: provenance missing {key}")
    if not isinstance(provenance["redistributable"], bool):
        raise RunnerError(f"{path}: provenance.redistributable must be boolean")

    fixture_dir = path.parent.resolve()
    source_value = metadata["source"]
    if not isinstance(source_value, str) or not source_value:
        raise RunnerError(f"{path}: source must be a non-empty string")
    source = (fixture_dir / source_value).resolve()
    if not _inside(source, fixture_dir):
        raise RunnerError(f"{path}: source must stay inside the fixture directory")
    if not source.is_file():
        raise RunnerError(f"{path}: source does not exist: {source_value}")
    return source, metadata


def discover_fixtures(fixtures_root: Path) -> list[tuple[Path, dict[str, Any]]]:
    paths = sorted(fixtures_root.glob("**/fixture.json"))
    if not paths:
        raise RunnerError(f"no fixtures found under {fixtures_root}")

    fixtures = []
    seen_ids: set[str] = set()
    for path in paths:
        source, metadata = validate_fixture(path)
        fixture_id = metadata["id"]
        if fixture_id in seen_ids:
            raise RunnerError(f"duplicate fixture id: {fixture_id}")
        seen_ids.add(fixture_id)
        fixtures.append((path, metadata | {"_source": source}))
    return fixtures


def oracle_identity(metadata: dict[str, Any]) -> dict[str, Any]:
    required = (
        "schemaVersion",
        "name",
        "version",
        "gitHead",
        "repository",
        "registryTarball",
        "integrity",
        "license",
        "language",
    )
    missing = [key for key in required if key not in metadata]
    if missing:
        raise RunnerError(f"oracle metadata is missing: {', '.join(missing)}")
    if metadata["schemaVersion"] != 1:
        raise RunnerError(f"unsupported oracle metadata schemaVersion: {metadata['schemaVersion']!r}")
    return {key: metadata[key] for key in required if key != "schemaVersion"}


def run_fixture(
    fixture_path: Path,
    fixture: dict[str, Any],
    oracle_dir: Path,
    oracle: dict[str, Any],
) -> dict[str, Any]:
    source = fixture["_source"]
    fixture_dir = fixture_path.parent.resolve()
    with tempfile.TemporaryDirectory(prefix="opy-overpy-") as temporary:
        output_path = Path(temporary) / "workshop.txt"
        command = [
            "pnpm",
            "exec",
            "overpy",
            "compile",
            "--input",
            str(source),
            "--output",
            str(output_path),
            "--language",
            oracle["language"],
            "--root",
            str(fixture_dir),
            "--main-file",
            source.name,
        ]
        try:
            completed = subprocess.run(
                command,
                cwd=oracle_dir,
                capture_output=True,
                check=False,
                text=True,
                encoding="utf-8",
            )
        except FileNotFoundError as error:
            raise RunnerError(
                "pnpm is unavailable; install the pinned oracle dependencies with "
                "pnpm install --dir compatibility/oracle"
            ) from error

        # Recorded diagnostics must be checkout-path-independent: replace the
        # absolute fixture-directory path with a stable placeholder so oracle
        # snapshots do not embed local state (COMPATIBILITY.md).
        stderr = completed.stderr.replace(str(fixture_dir), "<fixture-dir>")
        stdout = completed.stdout.replace(str(fixture_dir), "<fixture-dir>")

        workshop_exact = ""
        if output_path.is_file():
            workshop_exact = output_path.read_text(encoding="utf-8")
        workshop = normalize_text(workshop_exact)

    status = "success" if completed.returncode == 0 else "failure"
    try:
        project = input_identity.project_input(fixture_dir, fixture)
    except input_identity.InputIdentityError as error:
        raise RunnerError(f"invalid source project identity: {error}") from error
    return {
        "schemaVersion": 1,
        "fixture": fixture["id"],
        "oracle": oracle,
        "input": project,
        "compile": {
            "status": status,
            "exitCode": completed.returncode,
            "diagnostics": normalize_diagnostics(stderr),
            "stdout": normalize_text(stdout),
            "workshopExact": workshop_exact,
            "workshop": workshop,
            "workshopSha256": sha256_text(workshop),
        },
    }


def snapshot_diff(expected: dict[str, Any], actual: dict[str, Any]) -> str:
    expected_text = json.dumps(expected, indent=2, sort_keys=True).splitlines()
    actual_text = json.dumps(actual, indent=2, sort_keys=True).splitlines()
    return "\n".join(
        difflib.unified_diff(
            expected_text,
            actual_text,
            fromfile="expected oracle.json",
            tofile="actual oracle.json",
            lineterm="",
        )
    )


def run(
    fixtures_root: Path,
    oracle_dir: Path,
    update: bool,
    selected_ids: set[str],
) -> int:
    oracle = oracle_identity(load_json(oracle_dir / "oracle-metadata.json"))
    fixtures = discover_fixtures(fixtures_root)
    failures = []

    for fixture_path, fixture in fixtures:
        if selected_ids and fixture["id"] not in selected_ids:
            continue
        try:
            actual = run_fixture(fixture_path, fixture, oracle_dir, oracle)
            expected_status = fixture["expectedStatus"]
            actual_status = actual["compile"]["status"]
            if actual_status != expected_status:
                raise RunnerError(
                    f"expected {expected_status}, oracle returned {actual_status}"
                )

            snapshot_path = fixture_path.parent / SNAPSHOT_NAME
            if update:
                write_json(snapshot_path, actual)
                print(f"UPDATED {fixture['id']} ({actual_status})")
            elif not snapshot_path.is_file():
                raise RunnerError(f"missing snapshot: {snapshot_path}")
            else:
                expected = load_json(snapshot_path)
                if expected != actual:
                    raise RunnerError(
                        f"snapshot differs:\n{snapshot_diff(expected, actual)}"
                    )
                print(f"PASS {fixture['id']} ({actual_status})")
        except RunnerError as error:
            failures.append(f"FAIL {fixture['id']}: {error}")

    if selected_ids:
        discovered_ids = {fixture["id"] for _, fixture in fixtures}
        unknown = sorted(selected_ids - discovered_ids)
        failures.extend(f"FAIL {fixture_id}: fixture not found" for fixture_id in unknown)

    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", type=Path, default=DEFAULT_FIXTURES)
    parser.add_argument("--oracle-dir", type=Path, default=DEFAULT_ORACLE)
    parser.add_argument("--fixture", action="append", dest="fixture_ids", default=[])
    parser.add_argument(
        "--update",
        action="store_true",
        help="rewrite oracle.json snapshots from the pinned oracle",
    )
    args = parser.parse_args(argv)
    return run(
        args.fixtures.resolve(),
        args.oracle_dir.resolve(),
        args.update,
        set(args.fixture_ids),
    )


if __name__ == "__main__":
    raise SystemExit(main())
