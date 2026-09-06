#!/usr/bin/env python3
"""Run the independent, stage-aware offline OverPy conformance baseline."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

import diff
import input_identity


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "compatibility" / "fixtures"
MANIFEST = ROOT / "compatibility" / "conformance-manifest.json"
INVENTORY = ROOT / "compatibility" / "feature-contracts.json"
PINNED_AUDIT = ROOT / "compatibility" / "pinned-overpy-audit.json"
DEFAULT_REPORT = ROOT / "target" / "opy-rs-conformance-report.json"
FRONTEND_CODES = {
    "lex-error": "lex",
    "parse-error": "parse",
    "lambda-context": "parse",
    "translations-invalid": "preprocess",
    "script-not-found": "preprocess",
    "w_already_imported": "preprocess",
    "do-while-placement": "semantic",
    "invalid-range-binder": "semantic",
    "four-dimensional-assignment": "semantic",
    "duplicate-rule-name": "semantic",
    "unknown-member": "semantic",
}
FRONTIER_CONSTRUCTS = {
    "lambda-context": "parse-error",
}
PROBE_KINDS = {"positive", "negative", "contextual", "composition"}
INVENTORY_STATUSES = {"implemented", "partial", "unsupported", "external-owner", "unclear"}
INVENTORY_COVERAGE = {"covered", "partial", "uncovered"}
TOOLING_CONTRACTS = {
    "tooling.decompile",
    "tooling.javascript-macros",
    "tooling.metadata",
    "tooling.post-compile-hook",
}


class ConformanceError(RuntimeError):
    """A malformed baseline, fixture, or producer response."""


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ConformanceError(f"cannot read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise ConformanceError(f"JSON root must be an object: {path}")
    return value


def load_manifest(path: Path = MANIFEST) -> dict[str, Any]:
    manifest = load_json(path)
    if manifest.get("schemaVersion") != 1:
        raise ConformanceError("conformance manifest schemaVersion must be 1")
    if manifest.get("contract") != "offline-overpy-conformance":
        raise ConformanceError("unsupported conformance manifest contract")
    if not manifest.get("reference", {}).get("contentCommit"):
        raise ConformanceError("conformance manifest has no pinned reference")
    categories = manifest.get("categories")
    if not isinstance(categories, list) or not categories:
        raise ConformanceError("conformance manifest must declare categories")
    for category in categories:
        if not isinstance(category, dict) or not category.get("id"):
            raise ConformanceError("each conformance category needs an id")
        if (
            not category.get("authority")
            or not category.get("contracts")
            or not category.get("probeFixtures")
            or not category.get("owner")
        ):
            raise ConformanceError(
                f"{category.get('id', '<unknown>')}: authority, contracts, and probes are required"
            )
    return manifest


def load_inventory(path: Path = INVENTORY) -> dict[str, Any]:
    inventory = load_json(path)
    if inventory.get("schemaVersion") != 1:
        raise ConformanceError("feature contract inventory schemaVersion must be 1")
    if inventory.get("contract") != "pinned-overpy-feature-contract-inventory":
        raise ConformanceError("unsupported feature contract inventory")
    if not isinstance(inventory.get("reference"), dict):
        raise ConformanceError("feature contract inventory has no pinned reference")
    status_vocabulary = inventory.get("statusVocabulary")
    if not isinstance(status_vocabulary, dict) or set(status_vocabulary) != INVENTORY_STATUSES:
        raise ConformanceError("feature contract inventory status vocabulary is incomplete")
    coverage_vocabulary = inventory.get("coverageVocabulary")
    if not isinstance(coverage_vocabulary, dict) or set(coverage_vocabulary) != INVENTORY_COVERAGE:
        raise ConformanceError("feature contract inventory coverage vocabulary is incomplete")
    for key in ("registries", "branches"):
        if not isinstance(inventory.get(key), list) or not inventory[key]:
            raise ConformanceError(f"feature contract inventory needs {key}")
    for key in ("requiredRegistries", "requiredBranches"):
        values = inventory.get(key)
        if (
            not isinstance(values, list)
            or not values
            or any(not isinstance(value, str) or not value for value in values)
            or len(values) != len(set(values))
        ):
            raise ConformanceError(f"feature contract inventory needs distinct {key}")
    return inventory


def load_pinned_audit(path: Path = PINNED_AUDIT) -> dict[str, Any]:
    audit = load_json(path)
    if audit.get("schemaVersion") != 1:
        raise ConformanceError("pinned OverPy audit schemaVersion must be 1")
    if audit.get("contract") != "pinned-overpy-source-audit":
        raise ConformanceError("unsupported pinned OverPy audit")
    if not isinstance(audit.get("reference"), dict):
        raise ConformanceError("pinned OverPy audit has no reference")
    for key in ("registries", "branches"):
        values = audit.get(key)
        if (
            not isinstance(values, list)
            or not values
            or any(not isinstance(value, dict) for value in values)
            or any(not isinstance(value.get("id"), str) or not value["id"] for value in values)
            or len({value["id"] for value in values}) != len(values)
        ):
            raise ConformanceError(f"pinned OverPy audit needs distinct {key}")
    for registry in audit["registries"]:
        source = registry.get("source")
        if (
            not isinstance(source, dict)
            or not isinstance(source.get("path"), str)
            or not source["path"]
            or not isinstance(source.get("objectPath"), str)
            or not source["objectPath"]
        ):
            raise ConformanceError(f"{registry['id']}: pinned audit source is incomplete")
    for branch in audit["branches"]:
        if not isinstance(branch.get("source"), str) or not branch["source"]:
            raise ConformanceError(f"{branch['id']}: pinned audit source is incomplete")
    return audit


def _validate_evidence(
    evidence: Any, fixture_set: set[str], leaf_id: str
) -> None:
    if not isinstance(evidence, list):
        raise ConformanceError(f"{leaf_id}: evidence must be a list")
    for item in evidence:
        if not isinstance(item, str) or not item:
            raise ConformanceError(f"{leaf_id}: evidence entries must be non-empty strings")
        if item.startswith("fixture:"):
            fixture_id = item.removeprefix("fixture:")
            if not fixture_id or fixture_id not in fixture_set:
                raise ConformanceError(f"{leaf_id}: evidence fixture does not exist: {item}")
        elif item.startswith("upstream:"):
            if not item.removeprefix("upstream:"):
                raise ConformanceError(f"{leaf_id}: upstream evidence path is empty")
        else:
            raise ConformanceError(f"{leaf_id}: unsupported evidence reference: {item}")


def _validate_production(production: Any, leaf_id: str) -> None:
    if not isinstance(production, list):
        raise ConformanceError(f"{leaf_id}: production must be a list")
    for item in production:
        if not isinstance(item, str) or not item:
            raise ConformanceError(f"{leaf_id}: production entries must be non-empty strings")
        if item != "workshop-rs" and not (ROOT / item).is_file():
            raise ConformanceError(f"{leaf_id}: production path does not exist: {item}")


def inventory_leaves(inventory: dict[str, Any]) -> list[dict[str, Any]]:
    leaves: list[dict[str, Any]] = []
    for registry in inventory["registries"]:
        defaults = registry["defaults"]
        overrides = registry.get("overrides", {})
        for key in registry["keys"]:
            leaf = dict(defaults)
            leaf.update(overrides.get(key, {}))
            leaf["id"] = f"{registry['id']}/{key}"
            leaf["registry"] = registry["id"]
            leaf["upstreamKey"] = key
            leaf["contract"] = registry["contract"]
            leaves.append(leaf)
    leaves.extend(inventory["branches"])
    return leaves


def _direct_object_properties(text: str, opening: int) -> dict[str, int | None]:
    properties: dict[str, int | None] = {}
    depth = 0
    quote: str | None = None
    escaped = False
    comment: str | None = None
    i = opening
    while i < len(text):
        char = text[i]
        following = text[i + 1] if i + 1 < len(text) else ""
        if comment == "line":
            if char == "\n":
                comment = None
            i += 1
            continue
        if comment == "block":
            if char == "*" and following == "/":
                comment = None
                i += 2
                continue
            i += 1
            continue
        if depth == 1:
            match = re.match(r"\s*([\"'])([^\"']+)\1\s*:", text[i:])
            if match:
                key = match.group(2)
                value_start = i + match.end()
                while value_start < len(text) and text[value_start].isspace():
                    value_start += 1
                properties[key] = value_start if value_start < len(text) and text[value_start] == "{" else None
                i += match.end()
                continue
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            i += 1
            continue
        if char in "'\"`":
            quote = char
            i += 1
            continue
        if char == "/" and following == "/":
            comment = "line"
            i += 2
            continue
        if char == "/" and following == "*":
            comment = "block"
            i += 2
            continue
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                break
        i += 1
    return properties


def _upstream_object_keys(source: Path, object_path: str) -> set[str]:
    text = source.read_text(encoding="utf-8")
    parts = object_path.split(".")
    declaration = re.search(rf"export\s+const\s+{re.escape(parts[0])}\b.*?=", text, re.DOTALL)
    if declaration is None:
        raise ConformanceError(f"upstream object is missing: {source}#{object_path}")
    opening = text.find("{", declaration.end())
    if opening < 0:
        raise ConformanceError(f"upstream object has no body: {source}#{object_path}")
    for part in parts[1:]:
        child = _direct_object_properties(text, opening).get(part)
        if child is None:
            raise ConformanceError(f"upstream object is missing: {source}#{object_path}")
        opening = child
    return set(_direct_object_properties(text, opening))


def validate_inventory(
    inventory: dict[str, Any],
    manifest: dict[str, Any],
    fixtures_root: Path = FIXTURES,
    upstream_root: Path | None = None,
    audit: dict[str, Any] | None = None,
) -> list[dict[str, Any]]:
    audit = load_pinned_audit() if audit is None else audit
    if inventory["reference"] != manifest["reference"]:
        raise ConformanceError("feature contract inventory reference pin disagrees with conformance manifest")
    if audit["reference"] != manifest["reference"]:
        raise ConformanceError("pinned OverPy audit reference pin disagrees with conformance manifest")
    fixture_set = fixture_ids(fixtures_root)
    category_contracts = {
        f"{category['id']}/{contract['id']}"
        for category in manifest["categories"]
        for contract in category["contracts"]
    }
    surfaces = inventory["registries"]
    if any(not isinstance(surface, dict) for surface in surfaces):
        raise ConformanceError("feature contract registries must be objects")
    surface_ids = [surface.get("id") for surface in surfaces]
    if any(not isinstance(item, str) or not item for item in surface_ids) or len(set(surface_ids)) != len(surface_ids):
        raise ConformanceError("feature contract registry ids must be unique")
    required_surfaces = set(inventory["requiredRegistries"])
    if required_surfaces != set(surface_ids):
        raise ConformanceError(
            "feature contract registry catalog differs from required registries: "
            f"missing={sorted(required_surfaces - set(surface_ids))}, "
            f"extra={sorted(set(surface_ids) - required_surfaces)}"
        )
    audit_surfaces = {surface["id"]: surface for surface in audit["registries"]}
    if required_surfaces != set(audit_surfaces):
        raise ConformanceError(
            "feature contract registry catalog differs from pinned audit: "
            f"missing={sorted(set(audit_surfaces) - required_surfaces)}, "
            f"extra={sorted(required_surfaces - set(audit_surfaces))}"
        )
    if any(not isinstance(branch, dict) for branch in inventory["branches"]):
        raise ConformanceError("feature contract branches must be objects")
    leaves = inventory_leaves(inventory)
    leaf_ids = [leaf.get("id") for leaf in leaves]
    if any(not isinstance(item, str) or not item for item in leaf_ids) or len(set(leaf_ids)) != len(leaf_ids):
        raise ConformanceError("feature contract leaf ids must be unique")
    branch_ids = [branch.get("id") for branch in inventory["branches"]]
    required_branches = set(inventory["requiredBranches"])
    if required_branches != set(branch_ids):
        raise ConformanceError(
            "feature contract branch catalog differs from required branches: "
            f"missing={sorted(required_branches - set(branch_ids))}, "
            f"extra={sorted(set(branch_ids) - required_branches)}"
        )
    audit_branches = {branch["id"]: branch for branch in audit["branches"]}
    if required_branches != set(audit_branches):
        raise ConformanceError(
            "feature contract branch catalog differs from pinned audit: "
            f"missing={sorted(set(audit_branches) - required_branches)}, "
            f"extra={sorted(required_branches - set(audit_branches))}"
        )
    for branch in inventory["branches"]:
        if (
            not isinstance(branch, dict)
            or branch.get("kind") != "compiler-branch"
            or not isinstance(branch.get("id"), str)
            or not branch["id"]
            or not isinstance(branch.get("authority"), str)
            or not branch["authority"]
        ):
            raise ConformanceError("feature contract branches need compiler kind, id, and authority")
    for registry in surfaces:
        registry_id = registry["id"]
        if registry.get("kind") != "registry" or not isinstance(registry.get("authority"), str):
            raise ConformanceError(f"{registry_id}: malformed registry surface")
        if (
            not isinstance(registry.get("source"), dict)
            or not isinstance(registry["source"].get("path"), str)
            or not registry["source"].get("path")
            or not isinstance(registry["source"].get("objectPath"), str)
            or not registry["source"].get("objectPath")
        ):
            raise ConformanceError(f"{registry_id}: source path and object path are required")
        if registry["source"] != audit_surfaces[registry_id]["source"]:
            raise ConformanceError(f"{registry_id}: source differs from pinned audit")
        keys = registry.get("keys")
        if not isinstance(keys, list) or not keys or len(keys) != len(set(keys)) or any(not isinstance(key, str) or not key for key in keys):
            raise ConformanceError(f"{registry_id}: registry keys must be distinct non-empty strings")
        defaults = registry.get("defaults")
        if not isinstance(defaults, dict):
            raise ConformanceError(f"{registry_id}: defaults are required")
        overrides = registry.get("overrides", {})
        if not isinstance(overrides, dict) or set(overrides) - set(keys):
            raise ConformanceError(f"{registry_id}: overrides must name declared registry keys")
        if upstream_root is not None:
            source = upstream_root / registry["source"]["path"]
            actual = _upstream_object_keys(source, registry["source"]["objectPath"])
            if actual != set(keys):
                missing = sorted(actual - set(keys))
                extra = sorted(set(keys) - actual)
                raise ConformanceError(f"{registry_id}: upstream key set differs (missing={missing}, extra={extra})")
    if upstream_root is not None:
        for branch in audit["branches"]:
            source = upstream_root / branch["source"]
            if not source.exists():
                raise ConformanceError(f"{branch['id']}: pinned audit source does not exist: {source}")
    for leaf in leaves:
        leaf_id = leaf["id"]
        status = leaf.get("status")
        coverage = leaf.get("coverage")
        if status not in INVENTORY_STATUSES:
            raise ConformanceError(f"{leaf_id}: invalid inventory status")
        if coverage not in INVENTORY_COVERAGE:
            raise ConformanceError(f"{leaf_id}: invalid inventory coverage")
        if not isinstance(leaf.get("contract"), str) or not leaf["contract"]:
            raise ConformanceError(f"{leaf_id}: contract is required")
        if leaf["contract"] not in category_contracts and leaf["contract"] not in TOOLING_CONTRACTS:
            raise ConformanceError(f"{leaf_id}: contract is not declared by conformance inventory")
        if coverage != "covered" or status != "implemented":
            if not isinstance(leaf.get("limits"), str) or not leaf["limits"]:
                raise ConformanceError(f"{leaf_id}: bounded or unresolved claims need limits")
        if status == "implemented" and coverage == "covered":
            if not isinstance(leaf.get("production"), list) or not leaf["production"]:
                raise ConformanceError(f"{leaf_id}: covered implementation needs production evidence")
            if not isinstance(leaf.get("evidence"), list) or not leaf["evidence"]:
                raise ConformanceError(f"{leaf_id}: covered implementation needs executable evidence")
        if "production" in leaf:
            _validate_production(leaf["production"], leaf_id)
        _validate_evidence(leaf.get("evidence", []), fixture_set, leaf_id)
    return leaves


def fixture_ids(fixtures_root: Path = FIXTURES) -> set[str]:
    ids = set()
    for path in sorted(fixtures_root.glob("**/fixture.json")):
        metadata = load_json(path)
        fixture_id = metadata.get("id")
        if not isinstance(fixture_id, str) or not fixture_id:
            raise ConformanceError(f"fixture id is missing: {path}")
        if fixture_id in ids:
            raise ConformanceError(f"duplicate fixture id: {fixture_id}")
        ids.add(fixture_id)
    if not ids:
        raise ConformanceError(f"no fixtures found under {fixtures_root}")
    return ids


def validate_manifest(manifest: dict[str, Any], fixtures_root: Path = FIXTURES) -> None:
    actual = fixture_ids(fixtures_root)
    stage_ids = {stage.get("id") for stage in manifest.get("stages", [])}
    category_stages = {
        stage
        for category in manifest["categories"]
        for stage in category.get("stages", [category.get("stage")])
    }
    if not stage_ids or None in category_stages or not category_stages <= stage_ids:
        raise ConformanceError("conformance categories must use declared stages")
    declared: list[str] = []
    category_ids = set()
    for category in manifest["categories"]:
        category_id = category["id"]
        if category_id in category_ids:
            raise ConformanceError(f"duplicate conformance category: {category_id}")
        category_ids.add(category_id)
        contracts = category["contracts"]
        if not isinstance(contracts, list) or not contracts:
            raise ConformanceError(f"{category_id}: contracts are required")
        contract_ids = set()
        contract_probes: list[str] = []
        for contract in contracts:
            if not isinstance(contract, dict):
                raise ConformanceError(f"{category_id}: contract must be an object")
            contract_id = contract.get("id")
            if not isinstance(contract_id, str) or not contract_id:
                raise ConformanceError(f"{category_id}: contract needs an id")
            if contract_id in contract_ids:
                raise ConformanceError(f"{category_id}: duplicate contract: {contract_id}")
            contract_ids.add(contract_id)
            if not isinstance(contract.get("claim"), str) or not contract["claim"]:
                raise ConformanceError(f"{category_id}/{contract_id}: claim is required")
            kinds = contract.get("probeKinds")
            if (
                not isinstance(kinds, list)
                or not kinds
                or any(kind not in PROBE_KINDS for kind in kinds)
                or len(set(kinds)) != len(kinds)
            ):
                raise ConformanceError(
                    f"{category_id}/{contract_id}: probeKinds must be distinct known kinds"
                )
            probes = contract.get("probes")
            if not isinstance(probes, list) or not probes:
                raise ConformanceError(f"{category_id}/{contract_id}: probes are required")
            for fixture_id in probes:
                if fixture_id not in actual:
                    raise ConformanceError(
                        f"{category_id}/{contract_id}: probe fixture does not exist: {fixture_id}"
                    )
                contract_probes.append(fixture_id)
        if not set(contract_probes) <= set(category["probeFixtures"]):
            raise ConformanceError(
                f"{category_id}: contract probes must be declared category probes"
            )
        for fixture_id in category["probeFixtures"]:
            if fixture_id not in actual:
                raise ConformanceError(
                    f"{category_id}: probe fixture does not exist: {fixture_id}"
                )
            declared.append(fixture_id)
    missing = sorted(actual - set(declared))
    if missing:
        raise ConformanceError(f"fixtures missing from conformance inventory: {missing}")
    frontiers = manifest.get("referenceFrontiers", {})
    if not isinstance(frontiers, dict):
        raise ConformanceError("referenceFrontiers must be an object")
    for fixture_id in sorted(actual):
        oracle_path = fixtures_root / fixture_id / "oracle.json"
        oracle = load_json(oracle_path)
        diff.require_result_shape(oracle, f"oracle {fixture_id}")
        if oracle["fixture"] != fixture_id:
            raise ConformanceError(f"{fixture_id}: oracle fixture id does not match path")
        metadata = load_json(fixtures_root / fixture_id / "fixture.json")
        try:
            expected_input = input_identity.project_input(fixtures_root / fixture_id, metadata)
        except input_identity.InputIdentityError as error:
            raise ConformanceError(f"{fixture_id}: invalid source graph: {error}") from error
        if oracle.get("input") != expected_input:
            raise ConformanceError(f"{fixture_id}: oracle input source graph is stale")
        oracle_identity = oracle.get("oracle", {})
        reference = manifest["reference"]
        if any(
            oracle_identity.get(key) != reference[key]
            for key in ("name", "version", "integrity")
        ):
            raise ConformanceError(
                f"{fixture_id}: oracle identity does not match the manifest pin"
            )
        status = oracle.get("compile", {}).get("status")
        if status != metadata.get("expectedStatus"):
            raise ConformanceError(
                f"{fixture_id}: fixture expectedStatus disagrees with oracle snapshot"
            )
        frontier = frontiers.get(fixture_id)
        if status == "failure":
            if not isinstance(frontier, dict) or not all(
                isinstance(frontier.get(key), str) and frontier[key]
                for key in ("stage", "construct", "diagnosticContains")
            ):
                raise ConformanceError(
                    f"{fixture_id}: reference failure needs an audited frontier"
                )
            diagnostics = oracle["compile"].get("diagnostics", [])
            text = "\n".join(item.get("text", "") for item in diagnostics)
            if frontier["diagnosticContains"] not in text:
                raise ConformanceError(
                    f"{fixture_id}: frontier is not supported by oracle diagnostics"
                )
        elif status == "success":
            if frontier is not None:
                raise ConformanceError(
                    f"{fixture_id}: successful reference input cannot have a frontier"
                )
        else:
            raise ConformanceError(f"{fixture_id}: oracle has invalid compile status")


def native_frontier(result: dict[str, Any]) -> dict[str, str] | None:
    compile_result = result.get("compile", {})
    if compile_result.get("status") != "failure":
        return None
    diagnostics = compile_result.get("diagnostics", [])
    first = next(
        (item for item in diagnostics if item.get("severity") == "error"),
        diagnostics[0] if diagnostics else {},
    )
    code = first.get("code")
    if not isinstance(code, str) or not code:
        return None
    stage = FRONTEND_CODES.get(code)
    if stage is None:
        failure_class = compile_result.get("failureClass")
        stage = "lowering" if failure_class == "integration" else "semantic"
    return {"stage": stage, "construct": FRONTIER_CONSTRUCTS.get(code, code)}


def run_compile(binary: Path, directory: Path, metadata: dict[str, Any]) -> dict[str, Any]:
    source = directory / metadata["source"]
    completed = subprocess.run(
        [
            str(binary),
            "compile",
            "--format",
            "json",
            "--language",
            "en-US",
            source.name,
        ],
        cwd=directory,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise ConformanceError(
            f"{metadata['id']}: compile did not produce JSON (exit {completed.returncode}): "
            f"{completed.stderr.strip()}"
        ) from error
    if not isinstance(result, dict):
        raise ConformanceError(f"{metadata['id']}: compile result is not an object")
    result["fixture"] = metadata["id"]
    result["input"] = input_identity.project_input(directory, metadata)
    result.setdefault("compile", {})["processExitCode"] = completed.returncode
    diff.require_result_shape(result, f"native {metadata['id']}")
    return result


def run_semantic(
    binary: Path,
    directory: Path,
    metadata: dict[str, Any],
    project: dict[str, Any],
    reference_sha256: str,
) -> dict[str, Any]:
    source = directory / metadata["source"]
    completed = subprocess.run(
        [
            str(binary),
            "--source",
            source.name,
            "--root",
            ".",
            "--oracle",
            "oracle.json",
            "--input-sha256",
            project["sha256"],
        ],
        cwd=directory,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if completed.returncode != 0:
        return {"status": "inconclusive", "reason": completed.stderr.strip()}
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError:
        return {
            "status": "inconclusive",
            "reason": "semantic evidence was not JSON",
        }
    semantic = result.get("semanticWIR") if isinstance(result, dict) else None
    if not isinstance(semantic, dict):
        return {
            "status": "inconclusive",
            "reason": "semantic-WIR evidence is missing",
        }
    valid = (
        result.get("schemaVersion") == 1
        and semantic.get("schemaVersion") == 1
        and semantic.get("algorithm") == "workshop-rs::roundtrip::equivalent"
        and semantic.get("inputSha256") == project["sha256"]
        and semantic.get("referenceInputSha256") == reference_sha256
        and isinstance(semantic.get("equivalent"), bool)
    )
    if not valid:
        return {
            "status": "inconclusive",
            "reason": "semantic-WIR evidence failed validation",
            "evidence": semantic,
        }
    return {
        "status": "match" if semantic["equivalent"] else "divergence",
        "evidence": semantic,
    }


def compare_case(
    oracle: dict[str, Any],
    native: dict[str, Any],
    reference_frontier: dict[str, str] | None,
    frontier: dict[str, str] | None,
    semantic: dict[str, Any] | None,
) -> dict[str, Any]:
    reference_compile = oracle["compile"]
    native_compile = native["compile"]
    reference_status = reference_compile["status"]
    native_status = native_compile["status"]
    result: dict[str, Any] = {
        "fixture": native["fixture"],
        "referenceStatus": reference_status,
        "nativeStatus": native_status,
        "status": "inconclusive",
        "referenceDiagnostics": reference_compile.get("diagnostics", []),
        "nativeDiagnostics": native_compile.get("diagnostics", []),
        "referenceFrontier": reference_frontier,
        "nativeFrontier": frontier,
    }
    if reference_status == "success" and native_status == "success":
        if semantic is None:
            result["reason"] = "canonical-WIR evidence was not requested"
        elif semantic["status"] == "match":
            result["status"] = "match"
        else:
            result["status"] = semantic["status"]
            result["reason"] = semantic.get("reason") or (
                "canonical WIR is not equivalent"
                if semantic["status"] == "divergence"
                else None
            )
        if semantic and "evidence" in semantic:
            result["semanticWIR"] = semantic["evidence"]
        return result
    if reference_status != native_status:
        result["status"] = "divergence"
        result["reason"] = "reference and native compile statuses differ"
    elif reference_status == "failure":
        if frontier is None or reference_frontier is None:
            result["status"] = "inconclusive"
            result["reason"] = "a failure frontier is unavailable"
        else:
            result["status"] = (
                "match"
                if frontier["stage"] == reference_frontier["stage"]
                and frontier["construct"] == reference_frontier["construct"]
                else "divergence"
            )
            if result["status"] == "divergence":
                result["reason"] = "failure frontier differs"
    return result


def run(args: argparse.Namespace) -> int:
    manifest = load_manifest(args.manifest)
    validate_manifest(manifest, args.fixtures)
    inventory = load_inventory(args.inventory)
    audit = load_pinned_audit()
    leaves = validate_inventory(inventory, manifest, args.fixtures, args.upstream_root, audit)
    frontiers = manifest["referenceFrontiers"]
    selected = set(args.fixture)
    discovered = fixture_ids(args.fixtures)
    unknown = selected - discovered
    if unknown:
        raise ConformanceError(f"fixture not found: {sorted(unknown)}")
    results = []
    category_by_fixture: dict[str, list[dict[str, str]]] = {}
    for category in manifest["categories"]:
        for contract in category["contracts"]:
            for fixture_id in contract["probes"]:
                category_by_fixture.setdefault(fixture_id, []).append(
                    {
                        "id": category["id"],
                        "contract": contract["id"],
                        "owner": category["owner"],
                    }
                )
    for manifest_path in sorted(args.fixtures.glob("**/fixture.json")):
        metadata = load_json(manifest_path)
        fixture_id = metadata["id"]
        if selected and fixture_id not in selected:
            continue
        directory = manifest_path.parent
        oracle = load_json(directory / "oracle.json")
        native = run_compile(args.binary, directory, metadata)
        reference_frontier = frontiers.get(fixture_id)
        semantic = None
        project = native["input"]
        if (
            oracle["compile"]["status"] == "success"
            and native["compile"]["status"] == "success"
        ):
            semantic = run_semantic(
                args.semantic_binary,
                directory,
                metadata,
                project,
                oracle["input"]["sha256"],
            )
        result = compare_case(
            oracle,
            native,
            reference_frontier,
            native_frontier(native),
            semantic,
        )
        result["fixtureCategory"] = metadata["category"]
        result["rootCapabilities"] = category_by_fixture[fixture_id]
        result["owners"] = sorted(
            {item["owner"] for item in category_by_fixture[fixture_id]}
        )
        results.append(result)
        print(f"{result['status'].upper():13} {fixture_id}")
    counts: dict[str, int] = {}
    by_capability: dict[str, dict[str, int]] = {}
    for result in results:
        counts[result["status"]] = counts.get(result["status"], 0) + 1
        for capability in result["rootCapabilities"]:
            statuses = by_capability.setdefault(capability["id"], {})
            statuses[result["status"]] = statuses.get(result["status"], 0) + 1
    report = {
        "schemaVersion": 1,
        "artifact": "opy-rs offline OverPy conformance report",
        "generatedBy": "compatibility/conformance.py",
        "contract": manifest["contract"],
        "reference": manifest["reference"],
        "featureInventory": {
            "contract": inventory["contract"],
            "registries": len(inventory["registries"]),
            "registryLeaves": sum(len(registry["keys"]) for registry in inventory["registries"]),
            "branches": len(inventory["branches"]),
            "byStatus": {
                status: sum(1 for leaf in leaves if leaf["status"] == status)
                for status in sorted(INVENTORY_STATUSES)
            },
            "byCoverage": {
                coverage: sum(1 for leaf in leaves if leaf["coverage"] == coverage)
                for coverage in sorted(INVENTORY_COVERAGE)
            },
            "gaps": [
                leaf["id"]
                for leaf in leaves
                if leaf["coverage"] != "covered" or leaf["status"] != "implemented"
            ],
        },
        "comparison": {
            "stages": [stage["id"] for stage in manifest["stages"]],
            "success": "canonical WIR equivalence via workshop-rs::roundtrip::equivalent",
            "failure": "reference and native stage plus first construct frontier",
        },
        "summary": {
            "total": len(results),
            "byStatus": counts,
            "byCapability": by_capability,
        },
        "divergences": [item for item in results if item["status"] == "divergence"],
        "inconclusive": [item for item in results if item["status"] == "inconclusive"],
        "fixtures": results,
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"report written to {args.report}")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target" / "debug" / "opy-cli")
    parser.add_argument(
        "--semantic-binary",
        type=Path,
        default=ROOT / "target" / "debug" / "opy-compat",
    )
    parser.add_argument("--fixtures", type=Path, default=FIXTURES)
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    parser.add_argument("--inventory", type=Path, default=INVENTORY)
    parser.add_argument(
        "--upstream-root",
        type=Path,
        help="optional checkout of the pinned OverPy content for registry key-set verification",
    )
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--fixture", action="append", default=[])
    args = parser.parse_args(argv)
    args.binary = args.binary.resolve()
    args.semantic_binary = args.semantic_binary.resolve()
    args.fixtures = args.fixtures.resolve()
    args.manifest = args.manifest.resolve()
    args.inventory = args.inventory.resolve()
    if args.upstream_root is not None:
        args.upstream_root = args.upstream_root.resolve()
    args.report = args.report.resolve()
    if not args.binary.is_file():
        parser.error(f"compiler binary does not exist: {args.binary}")
    if not args.semantic_binary.is_file():
        parser.error(f"semantic binary does not exist: {args.semantic_binary}")
    try:
        return run(args)
    except (ConformanceError, diff.DiffError, input_identity.InputIdentityError) as error:
        print(f"conformance: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
