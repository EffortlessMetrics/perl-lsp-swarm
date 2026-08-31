#!/usr/bin/env python3
"""Fail closed when release publishers can bypass common eligibility."""

from __future__ import annotations

import copy
import pathlib
import sys
from collections.abc import Mapping
from typing import Any

import yaml

WORKFLOW = pathlib.Path(".github/workflows/release-orchestration.yml")
PUBLISHERS = ("publish-crates", "publish-extension", "publish-docker")


class GraphError(RuntimeError):
    """The checked release graph permits an ordering or identity bypass."""


def _needs(job: Mapping[str, Any]) -> set[str]:
    raw = job.get("needs", [])
    if isinstance(raw, str):
        return {raw}
    if isinstance(raw, list) and all(isinstance(item, str) for item in raw):
        return set(raw)
    raise GraphError(f"invalid needs declaration: {raw!r}")


def _run_text(job: Mapping[str, Any]) -> str:
    steps = job.get("steps")
    if not isinstance(steps, list):
        raise GraphError("dispatch job has no step list")
    runs = [step.get("run", "") for step in steps if isinstance(step, dict)]
    return "\n".join(value for value in runs if isinstance(value, str))


def validate_graph(document: Mapping[str, Any]) -> None:
    jobs = document.get("jobs")
    if not isinstance(jobs, dict):
        raise GraphError("workflow has no jobs mapping")

    required = {
        "validate",
        "create-tag",
        "build-release",
        "publication-eligibility",
        *PUBLISHERS,
        "summary",
    }
    missing = sorted(required - set(jobs))
    if missing:
        raise GraphError(f"workflow misses required jobs: {missing}")
    if "trigger-release" in jobs:
        raise GraphError("legacy fire-and-forget trigger-release job remains")

    if _needs(jobs["create-tag"]) != {"validate"}:
        raise GraphError("create-tag must depend exactly on validate")
    if not {"validate", "create-tag"}.issubset(_needs(jobs["build-release"])):
        raise GraphError("build-release must depend on validated tag creation")
    if not {"validate", "create-tag", "build-release"}.issubset(
        _needs(jobs["publication-eligibility"])
    ):
        raise GraphError(
            "publication-eligibility must consume validation, tag, and release predecessor"
        )

    release_text = _run_text(jobs["build-release"])
    if "--workflow release.yml" not in release_text:
        raise GraphError("build-release does not use the exact-run gate for release.yml")
    for forbidden in ("publish-crates.yml", "publish-extension.yml", "docker-publish.yml"):
        if forbidden in release_text:
            raise GraphError(
                f"build-release still dispatches publisher {forbidden} beside the predecessor"
            )

    expected_workflows = {
        "publish-crates": "publish-crates.yml",
        "publish-extension": "publish-extension.yml",
        "publish-docker": "docker-publish.yml",
    }
    for job_name, workflow_name in expected_workflows.items():
        job = jobs[job_name]
        if "publication-eligibility" not in _needs(job):
            raise GraphError(f"{job_name} bypasses common publication eligibility")
        run_text = _run_text(job)
        if "release_workflow_gate.py" not in run_text:
            raise GraphError(f"{job_name} does not use the exact-run gate")
        if f"--workflow {workflow_name}" not in run_text:
            raise GraphError(f"{job_name} dispatches the wrong workflow")
        if '--expected-sha "$SOURCE_SHA"' not in run_text:
            raise GraphError(f"{job_name} does not bind the expected source SHA")

    eligibility_text = _run_text(jobs["publication-eligibility"])
    for required_token in (
        "release-prerequisite-manifest.json",
        "EXPECTED_SHA",
        "EXPECTED_DIGEST",
        "SHA256SUMS",
        "sbom-spdx.json",
    ):
        if required_token not in eligibility_text:
            raise GraphError(
                f"publication-eligibility lacks required proof token {required_token}"
            )

    summary_needs = _needs(jobs["summary"])
    for job_name in ("build-release", "publication-eligibility", *PUBLISHERS):
        if job_name not in summary_needs:
            raise GraphError(f"summary omits terminal job {job_name}")


def _load(path: pathlib.Path) -> Mapping[str, Any]:
    parsed = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(parsed, dict):
        raise GraphError(f"{path} did not parse as a workflow mapping")
    return parsed


def _expect_failure(document: Mapping[str, Any], mutation: str) -> None:
    mutated = copy.deepcopy(document)
    jobs = mutated["jobs"]
    if mutation == "publisher_bypass":
        jobs["publish-crates"]["needs"] = ["validate"]
    elif mutation == "release_race":
        steps = jobs["build-release"]["steps"]
        steps.append({"run": "gh workflow run publish-extension.yml"})
    elif mutation == "missing_manifest":
        for step in jobs["publication-eligibility"]["steps"]:
            if isinstance(step, dict) and isinstance(step.get("run"), str):
                step["run"] = step["run"].replace("sbom-spdx.json", "")
    elif mutation == "wrong_publisher":
        for step in jobs["publish-extension"]["steps"]:
            if isinstance(step, dict) and isinstance(step.get("run"), str):
                step["run"] = step["run"].replace(
                    "--workflow publish-extension.yml", "--workflow release.yml"
                )
    elif mutation == "legacy_trigger":
        jobs["trigger-release"] = {"runs-on": "ubuntu-latest", "steps": []}
    else:
        raise AssertionError(f"unknown mutation {mutation}")

    try:
        validate_graph(mutated)
    except GraphError:
        return
    raise GraphError(f"negative control did not fail: {mutation}")


def main() -> int:
    try:
        document = _load(WORKFLOW)
        validate_graph(document)
        for mutation in (
            "publisher_bypass",
            "release_race",
            "missing_manifest",
            "wrong_publisher",
            "legacy_trigger",
        ):
            _expect_failure(document, mutation)
    except (GraphError, OSError, yaml.YAMLError) as error:
        print(f"release prerequisite graph check failed: {error}", file=sys.stderr)
        return 1

    print(
        "release prerequisite graph: exact release predecessor, common eligibility, "
        "and three downstream publisher gates are structurally enforced"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
