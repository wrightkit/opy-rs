#!/usr/bin/env python3
"""Gate compilation on structural convergence with pinned OverPy for real projects.

Each project in `structural-gate.json` is fetched at its pinned commit,
compiled with the pinned OverPy oracle and with `opy-rs`, and the two outputs
are compared as canonical programs parsed by `workshop-rs`. A difference that is
not a recorded exception, or a recorded exception that matches nothing, fails
the run. Differences are reported by rule and canonical path, not as text diffs.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / "tools" / "overpy"
CONFIG = TOOLS / "structural-gate.json"
ORACLE = TOOLS / "oracle"
DEFAULT_WORKDIR = ROOT / "target" / "structural-gate"
DEFAULT_REPORT = ROOT / "target" / "opy-rs-structural-gate-report.json"
EXCEPTION_KEYS = (
    "id", "project", "entry", "section", "name", "path",
    "upstream", "opyRs", "decision", "pinningTest",
)


class GateError(RuntimeError):
    """A configuration or execution error that should be shown to a user."""


def load_config(path: Path) -> dict[str, Any]:
    config = json.loads(path.read_text(encoding="utf-8"))
    if config.get("schemaVersion") != 1:
        raise GateError(f"{path}: unsupported schemaVersion")
    for project in config["projects"]:
        if not re.fullmatch(r"[0-9a-f]{40}", project["commit"]):
            raise GateError(f"{project['id']}: commit must be a full SHA, never a ref")
    for exception in config["exceptions"]:
        missing = [key for key in EXCEPTION_KEYS if not exception.get(key)]
        if missing:
            raise GateError(f"exception {exception.get('id')}: missing {', '.join(missing)}")
    return config


def matches(exception: dict[str, Any], project: str, entry: str, difference: dict[str, Any]) -> bool:
    return (
        exception["project"] == project
        and exception["entry"] == entry
        and exception["section"] == difference["section"]
        and exception["name"] == difference["name"]
        and exception["path"] == difference["path"]
    )


def classify(
    project: str,
    entry: str,
    differences: list[dict[str, Any]],
    exceptions: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], set[str]]:
    """Split differences into recorded and unrecorded; return the used exception ids."""

    recorded, unrecorded, used = [], [], set()
    for difference in differences:
        exception = next((e for e in exceptions if matches(e, project, entry, difference)), None)
        if exception is None:
            unrecorded.append(difference)
        else:
            recorded.append(difference)
            used.add(exception["id"])
    return recorded, unrecorded, used


def describe(difference: dict[str, Any]) -> str:
    return (
        f"{difference['section']}[{difference['index']}] {difference['name']}: "
        f"{difference['path'] or '<item>'}\n"
        f"    opy-rs:   {difference['native']}\n"
        f"    OverPy:   {difference['reference']}"
    )


def run(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command, text=True, encoding="utf-8", capture_output=True, check=False, **kwargs
    )


def checkout(project: dict[str, Any], workdir: Path) -> Path:
    directory = workdir / project["id"]
    commit = project["commit"]
    if not (directory / ".git").is_dir():
        directory.mkdir(parents=True, exist_ok=True)
        for command in (
            ["git", "init", "-q"],
            ["git", "remote", "add", "origin", project["repository"]],
        ):
            done = run(command, cwd=directory)
            if done.returncode:
                raise GateError(done.stderr.strip())
    head = run(["git", "rev-parse", "HEAD"], cwd=directory)
    if head.stdout.strip() != commit:
        for command in (
            ["git", "fetch", "-q", "--depth", "1", "origin", commit],
            ["git", "checkout", "-q", "--force", "FETCH_HEAD"],
        ):
            done = run(command, cwd=directory)
            if done.returncode:
                raise GateError(f"{project['id']}: {done.stderr.strip()}")
        head = run(["git", "rev-parse", "HEAD"], cwd=directory)
    if head.stdout.strip() != commit:
        raise GateError(f"{project['id']}: checked out {head.stdout.strip()}, not {commit}")
    return directory


def oracle_identity() -> dict[str, Any]:
    metadata = json.loads((ORACLE / "oracle-metadata.json").read_text(encoding="utf-8"))
    installed = json.loads(
        (ORACLE / "node_modules" / "overpy" / "package.json").read_text(encoding="utf-8")
    )
    if installed["version"] != metadata["version"]:
        raise GateError(
            f"installed OverPy {installed['version']} is not the pinned {metadata['version']}"
        )
    return metadata


def compare_entry(
    binary: Path, project_dir: Path, entry: str, language: str, workdir: Path
) -> list[dict[str, Any]]:
    source = (project_dir / entry).resolve()
    reference = workdir / f"{project_dir.name}-{source.stem}.reference.ow"
    compiled = run(
        [
            "pnpm", "exec", "overpy", "compile",
            "--input", str(source),
            "--output", str(reference),
            "--language", language,
            "--root", str(source.parent),
            "--main-file", source.name,
        ],
        cwd=ORACLE,
    )
    if compiled.returncode:
        raise GateError(f"pinned OverPy rejects {entry}:\n{compiled.stderr.strip()}")
    compared = run(
        [
            str(binary), "project-compare",
            "--source", source.name,
            "--root", ".",
            "--reference", str(reference),
        ],
        cwd=source.parent,
    )
    if compared.returncode:
        raise GateError(f"opy-rs cannot compare {entry}: {compared.stderr.strip()}")
    return json.loads(compared.stdout)["differences"]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", type=Path, required=True, help="opy-compat binary")
    parser.add_argument("--config", type=Path, default=CONFIG)
    parser.add_argument("--workdir", type=Path, default=DEFAULT_WORKDIR)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--project", action="append", help="limit to a project id")
    args = parser.parse_args()

    try:
        config = load_config(args.config)
        oracle = oracle_identity()
        args.workdir.mkdir(parents=True, exist_ok=True)
        binary = args.binary.resolve()
        failed = False
        used: set[str] = set()
        report: dict[str, Any] = {
            "oracle": {key: oracle[key] for key in ("name", "version", "gitHead", "contentCommit")},
            "projects": [],
        }
        for project in config["projects"]:
            if args.project and project["id"] not in args.project:
                continue
            directory = checkout(project, args.workdir.resolve())
            entries = []
            for entry in project["entries"]:
                differences = compare_entry(
                    binary, directory, entry, oracle["language"], args.workdir.resolve()
                )
                recorded, unrecorded, ids = classify(
                    project["id"], entry, differences, config["exceptions"]
                )
                used |= ids
                failed |= bool(unrecorded)
                status = "FAIL" if unrecorded else "ok"
                print(f"{status} {project['id']} {entry}: {len(recorded)} recorded, "
                      f"{len(unrecorded)} unrecorded")
                for difference in unrecorded:
                    print("  " + describe(difference).replace("\n", "\n  "))
                entries.append(
                    {"entry": entry, "recorded": recorded, "unrecorded": unrecorded}
                )
            report["projects"].append(
                {"id": project["id"], "commit": project["commit"], "entries": entries}
            )
        if not args.project:
            stale = [e["id"] for e in config["exceptions"] if e["id"] not in used]
            for identifier in stale:
                print(f"FAIL recorded exception matches no difference: {identifier}")
            failed |= bool(stale)
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        return 1 if failed else 0
    except GateError as error:
        print(f"structural gate: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
