#!/usr/bin/env python3
"""Fail closed when release publishers can bypass common eligibility.

The checker intentionally uses only the Python standard library. It validates
the small load-bearing workflow fragments directly, then mutates each required
edge/identity token to prove the check rejects the old race and common bypasses.
"""

from __future__ import annotations

import pathlib
import re
import sys

WORKFLOW = pathlib.Path(".github/workflows/release-orchestration.yml")
PUBLISHERS = {
    "publish-crates": "publish-crates.yml",
    "publish-extension": "publish-extension.yml",
    "publish-docker": "docker-publish.yml",
}


class GraphError(RuntimeError):
    """The checked release graph permits an ordering or identity bypass."""


def _jobs_text(workflow: str) -> str:
    marker = "\njobs:\n"
    if marker not in workflow:
        raise GraphError("workflow has no jobs mapping")
    return workflow.split(marker, 1)[1]


def _job_block(workflow: str, job_name: str) -> str:
    jobs = _jobs_text(workflow)
    pattern = re.compile(
        rf"(?ms)^  {re.escape(job_name)}:\n(?P<body>.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)"
    )
    match = pattern.search(jobs)
    if match is None:
        raise GraphError(f"workflow misses required job {job_name}")
    return match.group(0)


def _require(block: str, token: str, message: str) -> None:
    if token not in block:
        raise GraphError(message)


def validate_graph(workflow: str) -> None:
    for job_name in (
        "validate",
        "create-tag",
        "build-release",
        "publication-eligibility",
        *PUBLISHERS,
        "summary",
    ):
        _job_block(workflow, job_name)

    if re.search(r"(?m)^  trigger-release:\s*$", _jobs_text(workflow)):
        raise GraphError("legacy fire-and-forget trigger-release job remains")

    create_tag = _job_block(workflow, "create-tag")
    _require(
        create_tag,
        "needs: validate",
        "create-tag must depend on validated release inputs",
    )

    release = _job_block(workflow, "build-release")
    _require(
        release,
        "needs: [validate, create-tag]",
        "build-release must depend on validation and immutable tag creation",
    )
    _require(
        release,
        "--workflow release.yml",
        "build-release does not gate the exact release workflow",
    )
    _require(
        release,
        '--expected-sha "$SOURCE_SHA"',
        "build-release does not bind the exact expected source SHA",
    )
    for forbidden in PUBLISHERS.values():
        if forbidden in release:
            raise GraphError(
                f"build-release still dispatches publisher {forbidden} beside the predecessor"
            )

    eligibility = _job_block(workflow, "publication-eligibility")
    _require(
        eligibility,
        "needs: [validate, create-tag, build-release]",
        "publication-eligibility must consume validation, tag, and release predecessor",
    )
    for token in (
        "release-prerequisite-manifest.json",
        "EXPECTED_SHA",
        "EXPECTED_DIGEST",
        "SHA256SUMS",
        "sbom-spdx.json",
    ):
        _require(
            eligibility,
            token,
            f"publication-eligibility lacks required proof token {token}",
        )

    for job_name, workflow_name in PUBLISHERS.items():
        publisher = _job_block(workflow, job_name)
        _require(
            publisher,
            "needs: [validate, publication-eligibility]",
            f"{job_name} bypasses common publication eligibility",
        )
        _require(
            publisher,
            "release_workflow_gate.py",
            f"{job_name} does not use the exact-run gate",
        )
        _require(
            publisher,
            f"--workflow {workflow_name}",
            f"{job_name} dispatches the wrong workflow",
        )
        _require(
            publisher,
            '--expected-sha "$SOURCE_SHA"',
            f"{job_name} does not bind the expected source SHA",
        )

    summary = _job_block(workflow, "summary")
    for job_name in ("build-release", "publication-eligibility", *PUBLISHERS):
        _require(summary, f"      - {job_name}\n", f"summary omits terminal job {job_name}")


def _replace_once(text: str, old: str, new: str, mutation: str) -> str:
    if text.count(old) != 1:
        raise GraphError(
            f"negative-control setup for {mutation} expected one {old!r}, got {text.count(old)}"
        )
    return text.replace(old, new, 1)


def _expect_failure(workflow: str, mutation: str) -> None:
    if mutation == "publisher_bypass":
        mutated = _replace_once(
            workflow,
            "  publish-crates:\n    name: Publish crates after eligibility\n    needs: [validate, publication-eligibility]",
            "  publish-crates:\n    name: Publish crates after eligibility\n    needs: [validate]",
            mutation,
        )
    elif mutation == "release_race":
        mutated = _replace_once(
            workflow,
            "--workflow release.yml",
            "--workflow release.yml\n            # forbidden sibling dispatch: publish-extension.yml",
            mutation,
        )
    elif mutation == "missing_manifest":
        mutated = _replace_once(workflow, '"sbom-spdx.json"', '""', mutation)
    elif mutation == "wrong_publisher":
        marker = "  publish-extension:\n"
        prefix, suffix = workflow.split(marker, 1)
        suffix = _replace_once(
            suffix,
            "--workflow publish-extension.yml",
            "--workflow release.yml",
            mutation,
        )
        mutated = prefix + marker + suffix
    elif mutation == "legacy_trigger":
        mutated = workflow.replace(
            "  build-release:\n",
            "  trigger-release:\n    runs-on: ubuntu-24.04\n    steps: []\n\n  build-release:\n",
            1,
        )
    else:
        raise GraphError(f"unknown negative-control mutation {mutation}")

    try:
        validate_graph(mutated)
    except GraphError:
        return
    raise GraphError(f"negative control did not fail: {mutation}")


def main() -> int:
    try:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        validate_graph(workflow)
        for mutation in (
            "publisher_bypass",
            "release_race",
            "missing_manifest",
            "wrong_publisher",
            "legacy_trigger",
        ):
            _expect_failure(workflow, mutation)
    except (GraphError, OSError) as error:
        print(f"release prerequisite graph check failed: {error}", file=sys.stderr)
        return 1

    print(
        "release prerequisite graph: exact release predecessor, common eligibility, "
        "and three downstream publisher gates are structurally enforced"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
