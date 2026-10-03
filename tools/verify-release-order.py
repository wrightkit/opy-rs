#!/usr/bin/env python3
"""Assert the opy-rs release promotion order (#236).

A public opy-rs GitHub Release must represent a complete first-party release:
release-plz creates the release as a draft, the provider matrix artifacts are
staged once, the same staged bytes go to the draft and to immutable
`opy-rs/releases/<version>/` R2 objects, the R2 set is publicly verified, and
only then does promote-release publish the draft and advance-latest move the
`opy-rs/latest/version` pointer. GitHub gates promotion through `needs` edges,
so this script fails when the workflow's declared edges or the release-plz
draft flag would let a promotion happen before its prerequisites.

Usage: python3 tools/verify-release-order.py
"""

import re
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def fail(message: str) -> None:
    raise SystemExit(f"release-order validation failed: {message}")


def job_lines() -> dict[str, list[str]]:
    parts = (
        (REPO_ROOT / ".github/workflows/release-plz.yml")
        .read_text()
        .split("\njobs:", 1)
    )
    if len(parts) != 2:
        fail("release-plz.yml has no jobs section")
    # Comments are stripped so `# needs: x` cannot fabricate or hide an edge.
    lines = [re.sub(r"(\s|^)#.*$", "", line).rstrip() for line in parts[1].splitlines()]
    # The jobs section ends at the next top-level key.
    end = next((i for i, line in enumerate(lines) if line[:1].strip()), len(lines))
    lines = lines[:end]
    starts: dict[str, int] = {}
    for i, line in enumerate(lines):
        head = re.match(r"^  ([a-zA-Z0-9_-]+):$", line)
        if head:
            starts[head.group(1)] = i
    jobs: dict[str, list[str]] = {}
    markers = sorted(starts.items(), key=lambda kv: kv[1])
    for i, (job, start) in enumerate(markers):
        stop = markers[i + 1][1] if i + 1 < len(markers) else len(lines)
        jobs[job] = lines[start:stop]
    return jobs


def needs_of(job: str, jobs: dict[str, list[str]]) -> list[str]:
    needs, collecting = [], False
    for line in jobs[job]:
        head = re.match(r"^    needs:\s*(.*)$", line)
        if head:
            needs.extend(re.findall(r"[a-zA-Z0-9_-]+", head.group(1)))
            collecting = not head.group(1).strip()
            continue
        item = re.match(r"^      -\s*([a-zA-Z0-9_-]+)\s*$", line)
        if collecting and item:
            needs.append(item.group(1))
        elif line.strip():
            collecting = False
    return needs


def job_gate(job: str, jobs: dict[str, list[str]]) -> str:
    """The job-level `if:` expression, including `>-` folded continuations."""
    gate, lines = [], jobs[job]
    for i, line in enumerate(lines):
        head = re.match(r"^    if:\s*(.*)$", line)
        if not head:
            continue
        gate.append(head.group(1))
        j = i + 1
        while j < len(lines) and re.match(r"^      [^\s-]", lines[j]) and ":" not in lines[j]:
            gate.append(lines[j].strip())
            j += 1
    return " ".join(gate)


def require_draft_release() -> None:
    config = "\n".join(
        re.sub(r"(\s|^)#.*$", "", line)
        for line in (REPO_ROOT / "release-plz.toml").read_text().splitlines()
    )
    for block in re.split(r"(?m)^\[\[package\]\]\s*$", config):
        if re.search(r'(?m)^name\s*=\s*"opy-rs"\s*$', block):
            if not re.search(r"(?m)^git_release_draft\s*=\s*true\s*$", block):
                fail(
                    "release-plz.toml does not set git_release_draft = true on opy-rs: "
                    "the GitHub Release would be public before provider staging"
                )
            return
    fail("release-plz.toml has no opy-rs package block")


def main() -> None:
    require_draft_release()
    jobs = job_lines()
    for job in (
        "provider",
        "publish-provider-github",
        "publish-provider-r2",
        "promote-release",
        "advance-latest",
    ):
        if job not in jobs:
            fail(f"release-plz.yml is missing the {job} job")
    for job in ("publish-provider-github", "publish-provider-r2"):
        if "provider" not in needs_of(job, jobs):
            fail(f"{job} does not need provider: it cannot reuse the staged artifacts")
    for job in jobs:
        # A status-check function would let the job run despite failed needs.
        if re.search(r"(always|failure|cancelled)\s*\(", job_gate(job, jobs)):
            fail(f"{job} gates on a status-check function: failed prerequisites no longer block it")
    github_body = "\n".join(jobs["publish-provider-github"])
    if "gh release upload" not in github_body:
        fail("publish-provider-github does not upload the artifacts to the release")
    r2_body = "\n".join(jobs["publish-provider-r2"])
    if "curl" not in r2_body or "sha256sum --check" not in r2_body:
        fail("publish-provider-r2 does not publicly verify the R2 objects")
    r2_needs = needs_of("publish-provider-r2", jobs)
    for later in ("publish-provider-github", "promote-release", "advance-latest"):
        if later in r2_needs:
            fail(f"publish-provider-r2 waits on {later}: R2 verification is not before promotion")
    promote_needs = needs_of("promote-release", jobs)
    for gate in ("publish-provider-github", "publish-provider-r2"):
        if gate not in promote_needs:
            fail(f"promote-release does not need {gate}: the release can go public without it")
    if "promote-release" not in needs_of("advance-latest", jobs):
        fail("advance-latest does not need promote-release: the pointer can outrun the release")
    if "gh release edit" not in "\n".join(jobs["promote-release"]) or \
            "--draft=false" not in "\n".join(jobs["promote-release"]):
        fail("promote-release does not publish the draft release")
    if "latest/version" in r2_body:
        fail("publish-provider-r2 still advances the version pointer inside the R2 job")
    if "latest/version" not in "\n".join(jobs["advance-latest"]):
        fail("advance-latest does not write the opy-rs/latest/version pointer")
    print("ok: opy-rs release promotes only after verified provider R2 staging")


if __name__ == "__main__":
    main()
