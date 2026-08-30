#!/usr/bin/env python3
"""Focused cache-writer authority contract for issue #13924."""

from __future__ import annotations

import os
import re
import unittest
from pathlib import Path

REPO_ROOT = Path(os.environ.get("A3_REPO_ROOT", Path(__file__).resolve().parents[2]))
WORKFLOW = REPO_ROOT / ".github/workflows/ci.yml"
TRUSTED_SAVE = (
    "${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"
)
ACTION = "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6"
EXPECTED_EVENTS = {"pull_request", "merge_group", "push", "workflow_dispatch"}
EXPECTED_CACHES = {
    "platform-overrides": {
        "shared-key": "ci-platform-scope-${{ hashFiles('Cargo.lock') }}",
        "cache-on-failure": "true",
        "cache-all-crates": "true",
    },
    "repository-contract": {
        "shared-key": "ci-contract-${{ hashFiles('Cargo.lock') }}",
        "cache-on-failure": "true",
        "cache-all-crates": "true",
    },
    "public-api-pr": {
        "key": "public-api-${{ hashFiles('Cargo.lock') }}",
        "cache-on-failure": "true",
    },
    "semver-pr": {
        "key": "semver-${{ hashFiles('Cargo.lock') }}",
        "cache-on-failure": "true",
    },
}
EXPECTED_JOB_IF = {
    "platform-overrides": (
        "needs.draft-pr-check.outputs.run_ci == 'true' && "
        "needs.preflight-latest-check.outputs.is_latest == 'true'"
    ),
    "repository-contract": (
        "(github.event_name == 'pull_request' || github.event_name == 'merge_group') && "
        "needs.draft-pr-check.outputs.run_ci == 'true' && "
        "needs.preflight-latest-check.outputs.is_latest == 'true'"
    ),
    "public-api-pr": (
        "github.event_name == 'pull_request' && "
        "needs.draft-pr-check.outputs.run_ci == 'true' && "
        "needs.preflight-latest-check.outputs.is_latest == 'true'"
    ),
    "semver-pr": (
        "github.event_name == 'pull_request' && "
        "needs.draft-pr-check.outputs.run_ci == 'true' && "
        "needs.preflight-latest-check.outputs.is_latest == 'true'"
    ),
}
EXPECTED_STEP_IF = {
    "platform-overrides": None,
    "repository-contract": None,
    "public-api-pr": "needs.draft-pr-check.outputs.api_scope == 'true'",
    "semver-pr": "needs.draft-pr-check.outputs.api_scope == 'true'",
}


def indented_block(lines: list[str], marker: str, indent: int) -> list[str]:
    """Return one YAML mapping entry without depending on its next sibling's name."""
    prefix = " " * indent + marker + ":"
    matches = [index for index, line in enumerate(lines) if line == prefix]
    if len(matches) != 1:
        raise AssertionError(f"expected one {marker!r} entry, found {len(matches)}")
    start = matches[0]
    end = next(
        (
            index
            for index in range(start + 1, len(lines))
            if lines[index]
            and not lines[index].startswith(" " * (indent + 1))
        ),
        len(lines),
    )
    return lines[start:end]


def scalar_field(block: list[str], name: str, indent: int) -> str | None:
    prefix = " " * indent + name + ":"
    matches = [index for index, line in enumerate(block) if line.startswith(prefix)]
    if not matches:
        return None
    if len(matches) != 1:
        raise AssertionError(f"expected at most one {name!r} field, found {len(matches)}")
    start = matches[0]
    value = block[start][len(prefix) :].strip()
    if value not in {">", ">-", "|", "|-"}:
        return value
    continuation: list[str] = []
    for line in block[start + 1 :]:
        if line.strip() and len(line) - len(line.lstrip()) <= indent:
            break
        if line.strip() and not line.lstrip().startswith("#"):
            continuation.append(line.strip())
    return " ".join(continuation)


def cache_step(
    job: list[str], expected_key: tuple[str, str]
) -> tuple[dict[str, str], dict[str, str]]:
    """Find the one pinned cache step with the expected identity in a job."""
    starts = [index for index, line in enumerate(job) if line.startswith("      - ")]
    steps = [
        job[start : starts[pos + 1] if pos + 1 < len(starts) else len(job)]
        for pos, start in enumerate(starts)
    ]
    key_name, key_value = expected_key
    matches: list[tuple[list[str], dict[str, str], dict[str, str]]] = []
    for step in steps:
        fields: dict[str, str] = {}
        unrecognized_fields: list[str] = []
        for line in step:
            parsed = re.fullmatch(r"        ([a-z-]+):\s*(.*)", line)
            if parsed:
                fields[parsed.group(1)] = parsed.group(2).split("  #", 1)[0].rstrip()
            elif (
                line.strip()
                and not line.lstrip().startswith("#")
                and len(line) - len(line.lstrip()) == 8
            ):
                unrecognized_fields.append(line)
        with_indexes = [
            index for index, line in enumerate(step) if line == "        with:"
        ]
        if len(with_indexes) > 1:
            raise AssertionError("cache step contains more than one with mapping")
        inputs: dict[str, str] = {}
        unrecognized_inputs: list[str] = []
        if with_indexes:
            for line in step[with_indexes[0] + 1 :]:
                if not line.strip() or line.lstrip().startswith("#"):
                    continue
                indentation = len(line) - len(line.lstrip())
                if indentation <= 8:
                    break
                parsed = re.fullmatch(r"          ([a-z-]+):\s*(.*)", line)
                if not parsed:
                    unrecognized_inputs.append(line)
                    continue
                inputs[parsed.group(1)] = parsed.group(2)
        if fields.get("uses") == ACTION and inputs.get(key_name) == key_value:
            if unrecognized_fields or unrecognized_inputs:
                raise AssertionError(
                    "cache step contains unrecognized YAML fields: "
                    f"{unrecognized_fields + unrecognized_inputs}"
                )
            matches.append((step, fields, inputs))
    if len(matches) != 1:
        raise AssertionError(
            f"expected one pinned cache step for {key_name}: {key_value}, found {len(matches)}"
        )
    step, fields, inputs = matches[0]
    if not inputs:
        raise AssertionError(f"cache step for {key_value} has no with mapping")
    return inputs, fields


class CacheWriterAuthorityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        if not WORKFLOW.is_file():
            raise unittest.SkipTest("ci.yml not present in this checkout")
        cls.lines = WORKFLOW.read_text(encoding="utf-8").splitlines()

    def test_exact_cache_steps_preserve_identity_restore_options_and_save_guard(
        self,
    ) -> None:
        for job_name, expected in EXPECTED_CACHES.items():
            with self.subTest(job=job_name):
                key = next(
                    (item for item in expected.items() if item[0] in {"key", "shared-key"}),
                    None,
                )
                self.assertIsNotNone(key)
                job = indented_block(self.lines, job_name, 2)
                inputs, fields = cache_step(job, key)
                self.assertEqual(
                    expected | {"save-if": TRUSTED_SAVE},
                    inputs,
                    f"{job_name} changed cache behavior",
                )
                self.assertEqual(EXPECTED_JOB_IF[job_name], scalar_field(job, "if", 4))
                self.assertEqual(EXPECTED_STEP_IF[job_name], fields.get("if"))

    def test_ref_only_guard_is_bound_to_current_trigger_authority(self) -> None:
        trigger = indented_block(self.lines, "on", 0)
        events: set[str] = set()
        for line in trigger[1:]:
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            if len(line) - len(line.lstrip()) != 2:
                continue
            match = re.fullmatch(r"  ([a-z_]+):.*", line)
            if not match:
                self.fail(f"unrecognized workflow trigger entry: {line!r}")
            events.add(match.group(1))
        self.assertEqual(EXPECTED_EVENTS, events)
        self.assertNotIn("pull_request_target", events)

    def test_guard_evaluates_only_default_branch_refs_as_writers(self) -> None:
        allowed_refs = set(re.findall(r"github\.ref == '([^']+)'", TRUSTED_SAVE))
        self.assertEqual({"refs/heads/main", "refs/heads/master"}, allowed_refs)
        cases = {
            "refs/heads/main": True,
            "refs/heads/master": True,
            "refs/pull/14062/merge": False,
            "refs/heads/gh-readonly-queue/main/pr-14062-deadbeef": False,
            "refs/heads/cache-experiment": False,
        }
        for ref, expected in cases.items():
            with self.subTest(ref=ref):
                self.assertEqual(expected, ref in allowed_refs)


if __name__ == "__main__":
    unittest.main()
