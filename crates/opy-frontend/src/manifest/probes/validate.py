#!/usr/bin/env python3
"""Deterministic oracle validation for the OPY semantic compatibility manifest.

Runs every probe recorded in `probes.json` against the pinned OverPy 9.7.10
oracle and compares, at the S/D level:

* source identity: each probe file's SHA-256 must match `probes.json`;
* accept/reject: the oracle's compile exit status must match `expect`;
* normalized emission: for accepted probes, the SHA-256 of the oracle's
  emitted Workshop text must match `outputSha256`;
* diagnostics: for rejected probes, the oracle's stderr must contain the
  recorded `diagnosticContains` fragment (the D-level category).

This is the reference-validated evidence for the Wright-authored manifest
data (`../data/manifest.json`): every manifest entry records the probe (or
probe batch) that validates it. A changed oracle pin or a behavioral drift
fails here deterministically and requires a reviewed data update, mirroring
the `compatibility/` harness rules (`python3 compatibility/run_oracle.py`).

Stdlib-only; run from anywhere:

    python3 crates/wright-opy/src/manifest/probes/validate.py

Exit code 0 only when every probe matches the recorded oracle evidence.
"""

import hashlib
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
# crates/wright-opy/src/manifest/probes -> workspace root (5 levels up).
WORKSPACE = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", ".."))
ORACLE = os.path.join(
    WORKSPACE, "compatibility", "oracle", "node_modules", "overpy", "cli.js"
)
PROBES_JSON = os.path.join(HERE, "probes.json")
LANGUAGE = "en-US"


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_probe(probe):
    name = probe["id"]
    source_path = os.path.join(HERE, probe["source"])
    if not os.path.isfile(source_path):
        return (name, f"missing probe source {probe['source']}")
    if sha256_file(source_path) != probe["sha256"]:
        return (name, f"probe source hash mismatch for {probe['source']}")
    output_path = source_path + ".ws"
    try:
        result = subprocess.run(
            [
                "node", ORACLE, "compile",
                "-i", source_path, "-l", LANGUAGE, "-o", output_path,
            ],
            capture_output=True,
            text=True,
            timeout=120,
        )
    except FileNotFoundError:
        return (name, f"node or the pinned oracle is unavailable at {ORACLE}")
    except subprocess.TimeoutExpired:
        return (name, "oracle timed out")
    emitted = ""
    if os.path.exists(output_path):
        with open(output_path, "rb") as handle:
            emitted = handle.read()
        os.unlink(output_path)
    expected = probe["expect"]
    if expected == "success":
        if result.returncode != 0:
            stderr = (result.stderr or "").strip().replace("\n", " | ")
            return (name, f"expected success, oracle rejected: {stderr[:200]}")
        emitted_sha = hashlib.sha256(emitted).hexdigest()
        if emitted_sha != probe["outputSha256"]:
            return (
                name,
                f"emission hash mismatch: expected {probe['outputSha256']}, "
                f"got {emitted_sha}",
            )
    else:
        if result.returncode == 0:
            return (name, "expected failure, oracle accepted")
        fragment = probe.get("diagnosticContains")
        if fragment and fragment not in (result.stderr or ""):
            stderr = (result.stderr or "").strip().replace("\n", " | ")
            return (
                name,
                f"diagnostic category mismatch: missing "
                f"{fragment!r} in: {stderr[:200]}",
            )
    return (name, None)


def main():
    if not os.path.isfile(ORACLE):
        print(
            f"oracle not installed at {ORACLE}; run "
            f"`pnpm install --dir compatibility/oracle` first",
            file=sys.stderr,
        )
        return 2
    with open(PROBES_JSON, encoding="utf-8") as handle:
        data = json.load(handle)
    if data.get("schemaVersion") != 1:
        print("unsupported probes schemaVersion", file=sys.stderr)
        return 2
    failures = []
    for probe in data["probes"]:
        name, error = run_probe(probe)
        if error is None:
            print(f"OK   {name}")
        else:
            print(f"FAIL {name}: {error}")
            failures.append(f"{name}: {error}")
    if failures:
        print(f"\n{len(failures)} probe(s) failed against the pinned oracle")
        return 1
    print(f"\nall {len(data['probes'])} probes match the pinned oracle evidence")
    return 0


if __name__ == "__main__":
    sys.exit(main())
