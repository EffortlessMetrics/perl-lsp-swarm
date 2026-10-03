#!/usr/bin/env python3
"""Contract for late PR qualification of the required Rust Small context.

Draft activity must not create `Perl LSP Rust Small Result`, including as a
successful skipped aggregate. The native Draft -> Ready transition is the only
pull-request admission event. Merge-group, landed-main, and explicit manual
authorities remain separate.
"""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml"
CONTRACT_TEST_FILE = "scripts/ci/test_late_ci_required_context.py"
EXPECTED_PR_DECLARATIONS = (
    "    branches: [main]",
    "    types: [ready_for_review]",
)
EXPECTED_CONCURRENCY = (
    "concurrency:",
    "  group: em-ci-routed-rust-${{ github.repository }}-${{ github.event.pull_request.number || github.event_name == 'push' && github.sha || github.ref }}-${{ github.event_name }}",
    "  cancel-in-progress: ${{ github.event_name == 'pull_request' }}",
)
REQUIRED_CONTEXT = "Perl LSP Rust Small Result"


def load_workflow_text() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


def active_lines(text: str) -> list[str]:
    return [
        line.rstrip()
        for line in text.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]


def mapping_block(text: str, header: str, next_header: str) -> tuple[str, ...]:
    try:
        body = text.split(header, 1)[1].split(next_header, 1)[0]
    except IndexError as error:
        raise AssertionError(f"missing mapping boundary: {header!r} -> {next_header!r}") from error
    return tuple(active_lines(body))


def job_bodies(workflow_text: str) -> dict[str, str]:
    """Return indent-2 GitHub Actions job bodies keyed by job id."""
    bodies: dict[str, list[str]] = {}
    current: str | None = None
    in_jobs = False
    for raw_line in workflow_text.splitlines():
        line = raw_line.rstrip()
        if line == "jobs:":
            in_jobs = True
            current = None
            continue
        if in_jobs and line and not line.startswith((" ", "\t")):
            break
        if not in_jobs:
            continue
        if (
            line.startswith("  ")
            and not line.startswith("   ")
            and line.endswith(":")
            and not line.lstrip().startswith("-")
        ):
            current = line.strip()[:-1]
            bodies[current] = [line]
        elif current is not None:
            bodies[current].append(line)
    return {job_id: "\n".join(lines) for job_id, lines in bodies.items()}


def direct_job_fields(job_body: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in job_body.splitlines()[1:]:
        if not line.startswith("    ") or line.startswith("     "):
            continue
        key, separator, value = line.strip().partition(":")
        if separator:
            fields[key] = value.strip()
    return fields


def validate_late_ci_contract(workflow_text: str) -> None:
    pr_declarations = mapping_block(
        workflow_text,
        "\n  pull_request:\n",
        "\n  merge_group:\n",
    )
    if pr_declarations != EXPECTED_PR_DECLARATIONS:
        raise AssertionError(
            "pull_request admission must be exactly main + ready_for_review; "
            f"observed {pr_declarations!r}"
        )

    header = workflow_text.split("\npermissions:\n", 1)[0]
    if "\n  workflow_dispatch:\n" not in header:
        raise AssertionError("workflow_dispatch authority disappeared")
    if "\n  merge_group:\n" not in header:
        raise AssertionError("merge_group authority disappeared")
    if "\n  push:\n    branches: [main, master]\n" not in header:
        raise AssertionError("main/master push authority changed")

    concurrency = ("concurrency:",) + mapping_block(
        workflow_text,
        "\nconcurrency:\n",
        "\nenv:\n",
    )
    if concurrency != EXPECTED_CONCURRENCY:
        raise AssertionError(
            "qualification concurrency must isolate event authorities and let a "
            f"new Ready transaction replace the prior one; observed {concurrency!r}"
        )

    jobs = job_bodies(workflow_text)
    route = jobs.get("route-rust-small")
    if route is None:
        raise AssertionError("route-rust-small job disappeared")
    route_fields = direct_job_fields(route)
    if "if" in route_fields:
        raise AssertionError(
            "route-rust-small must not use a Draft job guard; Draft withholding "
            "belongs at the workflow event boundary"
        )

    result = jobs.get("rust-small-result")
    if result is None:
        raise AssertionError("rust-small-result job disappeared")
    result_fields = direct_job_fields(result)
    if result_fields.get("name") != REQUIRED_CONTEXT:
        raise AssertionError(
            "rust-small-result must keep the static required context name; "
            f"observed {result_fields.get('name')!r}"
        )
    if CONTRACT_TEST_FILE not in "\n".join(active_lines(result)):
        raise AssertionError(
            "rust-small-result must execute the late-CI contract so event-boundary "
            "drift fails the required result"
        )


class LateCiRequiredContextTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow_text = load_workflow_text()

    def test_checked_in_workflow_matches_late_ci_contract(self) -> None:
        validate_late_ci_contract(self.workflow_text)

    def test_synchronize_admission_fails(self) -> None:
        broken = self.workflow_text.replace(
            "types: [ready_for_review]",
            "types: [ready_for_review, synchronize]",
        )
        self.assertNotEqual(broken, self.workflow_text)
        with self.assertRaisesRegex(AssertionError, "pull_request admission"):
            validate_late_ci_contract(broken)

    def test_draft_job_guard_fails(self) -> None:
        marker = "    timeout-minutes: 5\n"
        broken = self.workflow_text.replace(
            marker,
            marker + "    if: github.event.pull_request.draft != true\n",
            1,
        )
        self.assertNotEqual(broken, self.workflow_text)
        with self.assertRaisesRegex(AssertionError, "Draft job guard"):
            validate_late_ci_contract(broken)

    def test_static_required_context_rename_fails(self) -> None:
        broken = self.workflow_text.replace(
            "    name: Perl LSP Rust Small Result",
            "    name: Perl LSP Rust Small Draft Result",
            1,
        )
        self.assertNotEqual(broken, self.workflow_text)
        with self.assertRaisesRegex(AssertionError, "static required context"):
            validate_late_ci_contract(broken)

    def test_event_concurrency_collapse_fails(self) -> None:
        broken = self.workflow_text.replace("-${{ github.event_name }}\n", "\n", 1)
        self.assertNotEqual(broken, self.workflow_text)
        with self.assertRaisesRegex(AssertionError, "qualification concurrency"):
            validate_late_ci_contract(broken)

    def test_contract_suite_removal_fails(self) -> None:
        marker = f"            {CONTRACT_TEST_FILE}"
        self.assertIn(marker, self.workflow_text)
        broken = self.workflow_text.replace(
            marker,
            "            scripts/ci/removed-late-ci-contract.py",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "must execute the late-CI contract"):
            validate_late_ci_contract(broken)


if __name__ == "__main__":
    unittest.main()
