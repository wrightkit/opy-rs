#!/usr/bin/env python3
"""Compare opy-rs (producer) result records with pinned OverPy oracle snapshots."""

from __future__ import annotations

import argparse
import json
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_FIXTURES = ROOT / "compatibility" / "fixtures"
DEFAULT_REPORT = ROOT / "compatibility" / "report.json"
DEFAULT_EXPECTATIONS = ROOT / "compatibility" / "differential-expectations.json"

EXPECTED_NATIVE_STATUSES = {"success", "failure"}
EXPECTED_CLASSIFICATIONS = {"match", "known-gap", "unsupported"}


class DiffError(RuntimeError):
    """A malformed result or differential-runner configuration error."""


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise DiffError(f"cannot read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise DiffError(f"JSON root must be an object: {path}")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def compact(value: Any) -> Any:
    """Return a JSON-safe value suitable for a report detail."""

    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    if isinstance(value, list):
        return [compact(item) for item in value]
    if isinstance(value, dict):
        return {str(key): compact(item) for key, item in value.items()}
    return repr(value)


def fixture_ids(fixtures_root: Path) -> list[str]:
    ids = []
    for metadata_path in sorted(fixtures_root.glob("**/fixture.json")):
        metadata = load_json(metadata_path)
        fixture_id = metadata.get("id")
        if not isinstance(fixture_id, str) or not fixture_id:
            raise DiffError(f"fixture id is missing or invalid: {metadata_path}")
        ids.append(fixture_id)
    if not ids:
        raise DiffError(f"no fixtures found under {fixtures_root}")
    if len(ids) != len(set(ids)):
        raise DiffError("duplicate fixture id in corpus")
    return ids


def load_expectations(path: Path = DEFAULT_EXPECTATIONS) -> dict[str, dict[str, Any]]:
    data = load_json(path)
    if data.get("schemaVersion") != 1:
        raise DiffError(f"unsupported differential expectation schema: {path}")
    cases = data.get("cases")
    if not isinstance(cases, list) or not cases:
        raise DiffError(f"differential expectations must contain cases: {path}")

    by_fixture: dict[str, dict[str, Any]] = {}
    for case in cases:
        if not isinstance(case, dict):
            raise DiffError(f"differential expectation must be an object: {path}")
        fixture = case.get("fixture")
        if not isinstance(fixture, str) or not fixture:
            raise DiffError(f"differential expectation fixture is invalid: {path}")
        if fixture in by_fixture:
            raise DiffError(f"duplicate differential expectation: {fixture}")
        native_status = case.get("nativeStatus")
        if native_status not in EXPECTED_NATIVE_STATUSES:
            raise DiffError(f"{fixture}: nativeStatus must be success or failure")
        classification = case.get("classification")
        if classification not in EXPECTED_CLASSIFICATIONS:
            raise DiffError(
                f"{fixture}: classification must be match, known-gap, or unsupported"
            )
        evidence = case.get("evidence")
        if not isinstance(evidence, list) or not evidence or not all(
            isinstance(item, str) and item for item in evidence
        ):
            raise DiffError(f"{fixture}: evidence must be a non-empty string array")
        note = case.get("note")
        if not isinstance(note, str) or not note:
            raise DiffError(f"{fixture}: note must be a non-empty string")
        if not isinstance(case.get("ruleNames"), bool):
            raise DiffError(f"{fixture}: ruleNames must be boolean")
        by_fixture[fixture] = case
    return by_fixture


def require_result_shape(result: dict[str, Any], label: str) -> None:
    if result.get("schemaVersion") != 1:
        raise DiffError(f"{label}: unsupported or missing schemaVersion")
    if not isinstance(result.get("fixture"), str):
        raise DiffError(f"{label}: fixture must be a string")
    compile_result = result.get("compile")
    if not isinstance(compile_result, dict):
        raise DiffError(f"{label}: compile must be an object")
    if compile_result.get("status") not in ("success", "failure"):
        raise DiffError(f"{label}: compile.status must be success or failure")
    for key in ("diagnostics", "workshop"):
        if key not in compile_result:
            raise DiffError(f"{label}: compile.{key} is required")
    if not isinstance(compile_result["diagnostics"], list):
        raise DiffError(f"{label}: compile.diagnostics must be an array")
    if not isinstance(compile_result["workshop"], str):
        raise DiffError(f"{label}: compile.workshop must be a string")


def result_path(results_root: Path, fixture_id: str) -> Path:
    return results_root / fixture_id / "result.json"


def run_producer(
    command_template: str,
    fixture_id: str,
    source: Path,
    output_path: Path,
) -> None:
    try:
        command = [
            argument.format(
                fixture_id=fixture_id,
                source=str(source),
                result=str(output_path),
            )
            for argument in shlex.split(command_template)
        ]
    except (ValueError, KeyError) as error:
        raise DiffError(f"invalid --producer-command template: {error}") from error
    if not command:
        raise DiffError("--producer-command cannot be empty")

    completed = subprocess.run(command, cwd=ROOT, check=False)
    if completed.returncode != 0:
        raise DiffError(
            f"producer failed for {fixture_id} with exit code {completed.returncode}"
        )
    if not output_path.is_file():
        raise DiffError(f"producer did not write {output_path}")


def stage(name: str, outcome: str, **details: Any) -> dict[str, Any]:
    return {"name": name, "outcome": outcome, **details}


def compare_stage(oracle: dict[str, Any], producer: dict[str, Any]) -> list[dict[str, Any]]:
    oracle_compile = oracle["compile"]
    producer_compile = producer["compile"]
    stages = [
        stage(
            "compile-status",
            "match" if oracle_compile["status"] == producer_compile["status"] else "regression",
            oracle=oracle_compile["status"],
            producer=producer_compile["status"],
        ),
        stage(
            "diagnostics",
            "match"
            if oracle_compile["diagnostics"] == producer_compile["diagnostics"]
            else "regression",
            oracle=compact(oracle_compile["diagnostics"]),
            producer=compact(producer_compile["diagnostics"]),
        ),
    ]

    oracle_exact = oracle_compile.get("workshopExact")
    producer_exact = producer_compile.get("workshopExact")
    if isinstance(oracle_exact, str) and isinstance(producer_exact, str):
        stages.append(
            stage(
                "exact-output",
                "match" if oracle_exact == producer_exact else "difference",
                oracleSha256=_sha256(oracle_exact),
                producerSha256=_sha256(producer_exact),
            )
        )
    else:
        stages.append(stage("exact-output", "inconclusive", reason="exact output is absent"))

    stages.append(
        stage(
            "normalized-output",
            "match"
            if oracle_compile["workshop"] == producer_compile["workshop"]
            else "regression",
            oracleSha256=_sha256(oracle_compile["workshop"]),
            producerSha256=_sha256(producer_compile["workshop"]),
        )
    )

    oracle_semantic = oracle.get("semantic")
    producer_semantic = producer.get("semantic")
    if oracle_semantic is None or producer_semantic is None:
        stages.append(
            stage(
                "semantic",
                "inconclusive",
                reason="a semantic result was not produced by both sides",
            )
        )
    else:
        stages.append(
            stage(
                "semantic",
                "match" if oracle_semantic == producer_semantic else "regression",
                oracle=compact(oracle_semantic),
                producer=compact(producer_semantic),
            )
        )
    return stages


def _sha256(value: str) -> str:
    import hashlib

    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def compare_fixture(
    fixtures_root: Path,
    fixture_id: str,
    results_root: Path | None,
    command_template: str | None,
) -> dict[str, Any]:
    metadata_path = fixtures_root / fixture_id / "fixture.json"
    oracle_path = fixtures_root / fixture_id / "oracle.json"
    metadata = load_json(metadata_path)
    expectations_path = fixtures_root.parent / "differential-expectations.json"
    expectations = load_expectations(
        expectations_path if expectations_path.is_file() else DEFAULT_EXPECTATIONS
    )
    expectation = expectations.get(fixture_id)
    if expectation is None:
        raise DiffError(f"missing differential expectation: {fixture_id}")
    oracle = load_json(oracle_path)
    require_result_shape(oracle, f"oracle {fixture_id}")
    if oracle["fixture"] != fixture_id:
        raise DiffError(f"oracle {oracle_path}: fixture id does not match path")

    source = (metadata_path.parent / metadata["source"]).resolve()
    if command_template:
        with tempfile.TemporaryDirectory(prefix="opy-diff-") as temporary:
            path = Path(temporary) / "result.json"
            run_producer(command_template, fixture_id, source, path)
            producer = load_json(path)
    elif results_root:
        path = result_path(results_root, fixture_id)
        if not path.is_file():
            return {
                "fixture": fixture_id,
                "category": metadata.get("category", "unknown"),
                "status": "inconclusive",
                "expectedNativeStatus": expectation["nativeStatus"],
                "reason": f"missing producer result: {path}",
                "stages": [],
            }
        producer = load_json(path)
    else:
        return {
            "fixture": fixture_id,
            "category": metadata.get("category", "unknown"),
            "status": "inconclusive",
            "expectedNativeStatus": expectation["nativeStatus"],
            "reason": "no producer result root or producer command was provided",
            "stages": [],
        }

    require_result_shape(producer, f"producer {fixture_id}")
    if producer["fixture"] != fixture_id:
        raise DiffError(f"producer result for {fixture_id} has fixture {producer['fixture']!r}")
    oracle_input = oracle.get("input")
    producer_input = producer.get("input")
    if not isinstance(oracle_input, dict) or not isinstance(producer_input, dict):
        raise DiffError(f"{fixture_id}: both results must include input metadata")
    if oracle_input.get("sha256") != producer_input.get("sha256"):
        raise DiffError(f"{fixture_id}: producer input hash does not match oracle input")

    stages = compare_stage(oracle, producer)
    regression_stages = [item["name"] for item in stages if item["outcome"] == "regression"]
    differences = [item["name"] for item in stages if item["outcome"] == "difference"]
    inconclusive = [item["name"] for item in stages if item["outcome"] == "inconclusive"]
    native_status = producer["compile"]["status"]
    oracle_status = oracle["compile"]["status"]
    native_status_mismatch = native_status != expectation["nativeStatus"]
    reference_gap = oracle_status != native_status
    declared_reference_gap = oracle_status != expectation["nativeStatus"]
    if native_status_mismatch:
        status = "unexpected-divergence"
    elif expectation["classification"] in {"known-gap", "unsupported"}:
        if not declared_reference_gap:
            raise DiffError(
                f"{fixture_id}: {expectation['classification']} must differ from oracle status"
            )
        status = expectation["classification"]
    elif oracle_status != native_status:
        status = "unexpected-divergence"
    elif regression_stages:
        status = "regression"
    elif inconclusive:
        status = "inconclusive"
    else:
        status = "match"
    return {
        "fixture": fixture_id,
        "category": metadata.get("category", "unknown"),
        "status": status,
        "expectedClassification": expectation["classification"],
        "expectedNativeStatus": expectation["nativeStatus"],
        "referenceStatus": oracle_status,
        "referenceGap": reference_gap,
        "declaredReferenceGap": declared_reference_gap,
        "evidence": expectation["evidence"],
        "note": expectation["note"],
        "regressionStages": regression_stages,
        "differenceStages": differences,
        "inconclusiveStages": inconclusive,
        "stages": stages,
    }


def build_report(results: list[dict[str, Any]]) -> dict[str, Any]:
    by_stage: dict[str, dict[str, int]] = {}
    by_category: dict[str, dict[str, int]] = {}
    for result in results:
        category = result["category"]
        by_category.setdefault(category, {})[result["status"]] = (
            by_category.setdefault(category, {}).get(result["status"], 0) + 1
        )
        for item in result.get("stages", []):
            outcomes = by_stage.setdefault(item["name"], {})
            outcomes[item["outcome"]] = outcomes.get(item["outcome"], 0) + 1
    counts = {}
    for result in results:
        counts[result["status"]] = counts.get(result["status"], 0) + 1
    return {
        "schemaVersion": 1,
        "comparison": {
            "oracle": "fixture oracle snapshots",
            "producer": "opy-rs result producer contract",
            "stages": [
                "compile-status",
                "diagnostics",
                "exact-output",
                "normalized-output",
                "semantic",
            ],
        },
        "summary": {
            "fixtures": len(results),
            "counts": counts,
            "byCategory": by_category,
            "byStage": by_stage,
        },
        "results": results,
    }


def run(
    fixtures_root: Path,
    report_path: Path,
    results_root: Path | None,
    command_template: str | None,
    selected_ids: set[str],
    allow_inconclusive: bool,
) -> int:
    all_ids = fixture_ids(fixtures_root)
    expectations_path = fixtures_root.parent / "differential-expectations.json"
    expectations = load_expectations(
        expectations_path if expectations_path.is_file() else DEFAULT_EXPECTATIONS
    )
    missing = sorted(set(all_ids) - set(expectations))
    extra = sorted(set(expectations) - set(all_ids))
    if missing or extra:
        detail = []
        if missing:
            detail.append(f"missing expectations: {', '.join(missing)}")
        if extra:
            detail.append(f"expectations for unknown fixtures: {', '.join(extra)}")
        raise DiffError("; ".join(detail))
    ids = [fixture_id for fixture_id in all_ids if not selected_ids or fixture_id in selected_ids]
    unknown = sorted(selected_ids - set(all_ids))
    if unknown:
        raise DiffError(f"fixture not found: {', '.join(unknown)}")
    if results_root and command_template:
        raise DiffError("provide only one of --results or --producer-command")

    results = [
        compare_fixture(fixtures_root, fixture_id, results_root, command_template)
        for fixture_id in ids
    ]
    report = build_report(results)
    write_json(report_path, report)
    print(json.dumps(report["summary"], indent=2, sort_keys=True))
    regressions = [
        result
        for result in results
        if result["status"] in {"regression", "unexpected-divergence"}
    ]
    inconclusive = [result for result in results if result["status"] == "inconclusive"]
    if regressions:
        for result in regressions:
            stages = ", ".join(result.get("regressionStages", [])) or "native outcome"
            print(f"REGRESSION {result['fixture']}: {stages}", file=sys.stderr)
        return 1
    if inconclusive and not allow_inconclusive:
        for result in inconclusive:
            print(f"INCONCLUSIVE {result['fixture']}: {result['reason']}", file=sys.stderr)
        return 2
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", type=Path, default=DEFAULT_FIXTURES)
    parser.add_argument("--results", type=Path, help="directory with producer results")
    parser.add_argument(
        "--producer-command",
        help=(
            "command template that writes {result}; receives {fixture_id} and {source} "
            "placeholders"
        ),
    )
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--fixture", action="append", dest="fixture_ids", default=[])
    parser.add_argument(
        "--allow-inconclusive",
        action="store_true",
        help="return success when the producer has not produced a result",
    )
    args = parser.parse_args(argv)
    try:
        return run(
            args.fixtures.resolve(),
            args.report.resolve(),
            args.results.resolve() if args.results else None,
            args.producer_command,
            set(args.fixture_ids),
            args.allow_inconclusive,
        )
    except DiffError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
