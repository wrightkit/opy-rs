#!/usr/bin/env python3
"""Derive a stable identity for a fixture's resolved OPY source graph."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any


class InputIdentityError(ValueError):
    """A fixture source graph cannot be resolved into a stable identity."""


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _fixture_relative_path(fixture_dir: Path, path: Path) -> str:
    try:
        return path.resolve().relative_to(fixture_dir.resolve()).as_posix()
    except ValueError as error:
        raise InputIdentityError(
            f"source graph path escapes fixture directory: {path}"
        ) from error


def _resolve(fixture_dir: Path, relative: str) -> tuple[Path, str]:
    path = (fixture_dir / relative).resolve()
    relative_path = _fixture_relative_path(fixture_dir, path)
    if not path.is_file():
        raise InputIdentityError(f"source graph file does not exist: {relative_path}")
    return path, relative_path


def project_input(fixture_dir: Path, metadata: dict[str, Any]) -> dict[str, Any]:
    """Return the source project manifest and its canonical digest.

    A fixture's complete project input is every ``.opy`` source file under its
    directory, excluding explicitly listed minimized regression snippets. The
    manifest deliberately covers include closures and ``#!mainFile`` targets
    without reimplementing the compiler's path resolution rules.
    """

    source_value = metadata.get("source")
    if not isinstance(source_value, str) or not source_value:
        raise InputIdentityError("fixture source must be a non-empty string")

    fixture_dir = fixture_dir.resolve()
    _, source_relative = _resolve(fixture_dir, source_value)
    regression_paths = set()
    for regression in metadata.get("regressions", []):
        if not isinstance(regression, dict):
            raise InputIdentityError("fixture regression metadata must be objects")
        regression_source = regression.get("source")
        if not isinstance(regression_source, str) or not regression_source:
            raise InputIdentityError("fixture regression source must be a non-empty string")
        _, regression_relative = _resolve(fixture_dir, regression_source)
        regression_paths.add(regression_relative)

    files: list[dict[str, str]] = []

    source_paths = sorted(fixture_dir.rglob("*.opy"))
    for path in source_paths:
        relative = _fixture_relative_path(fixture_dir, path)
        if relative in regression_paths:
            continue
        content = path.read_bytes()
        try:
            content.decode("utf-8")
        except UnicodeDecodeError as error:
            raise InputIdentityError(f"source project file is not UTF-8: {relative}") from error
        files.append({"path": relative, "sha256": _sha256_bytes(content)})

    if not any(item["path"] == source_relative for item in files):
        raise InputIdentityError("fixture source is excluded from the source project")
    manifest = {"source": source_relative, "files": files}
    encoded = json.dumps(
        manifest,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return {**manifest, "sha256": _sha256_bytes(encoded)}
