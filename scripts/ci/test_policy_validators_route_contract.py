#!/usr/bin/env python3
"""Route-shape contract for Policy Validators hosted fallback.

`Policy Validators` was pinned directly to `em-ci-nano` while the lane
registry marks it blocking. Its work is portable stdlib-only Python, so
self-hosting is an optimization, not proof semantics. This workflow routes
trusted same-repository events to `em-ci-nano` only when a matching runner is
online and idle, and falls back to `ubuntu-24.04` otherwise:

- `route` (GitHub-hosted) emits target/reason/error/fallback_allowed
- `validate` runs only when routed to `nano`
- `validate-hosted` runs only when routed to `github`, with step bodies
  identical to `validate`
- `validate-result` (display name `Validate CI policy ledgers`, the stable
  required identity) aggregates fail-closed: the selected route must pass,
  the unselected route must skip, and a validator failure on either route
  remains a real failure.

Red-first contract: mutating ANY single implementation job's copy of a
validator step — argument drift, commenting out, echo decoy — must fail this
contract WITH THE SITE NAMED, so a silent revert fails the required aggregate
instead of drifting back to per-runner copies. Fork and bot PRs must stay off
trusted self-hosted capacity.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "policy-validators.yml"

IMPL_JOBS = ("validate", "validate-hosted")
RESULT_JOB = "validate-result"
ROUTE_JOB = "route"
STABLE_DISPLAY_NAME = "Validate CI policy ledgers"
CONTRACT_TEST_FILE = "scripts/ci/test_policy_validators_route_contract.py"

# Every validator step the two implementation jobs must carry identically.
# These are (name, first command line) pairs; the parity test compares full
# bodies, this list names the sites in failure messages.
VALIDATOR_STEPS = (
    "Validate risk packs",
    "Validate trust lanes",
    "Validate gate -> lane mapping",
    "Validate policy_checks inventory",
    "Validate policy TOML parses",
    "Validate provider fact-read inventory",
    "Validate Cargo.lock conflict-repair policy",
    "Validate Dependabot source contract",
    "Validate Dependabot cooldown contract",
    "Validate exposed-surface disposition contract",
    "Validate bounded-result overflow invariants",
    "Validate parser facade authority",
    "Validate Homebrew formula digest binding",
)

# Router fallback reasons that must each route to hosted capacity.
HOSTED_REASONS = (
    "fork_pr",
    "bot_pr_github_hosted",
    "runner_token_missing_github_hosted",
    "runner_api_unavailable_github_hosted",
    "runner_group_absent_github_hosted",
    "no_idle_runner_github_hosted",
)


def read_workflow() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


def job_block(text: str, job: str) -> str:
    """Return the raw YAML block for a top-level job id."""
    pattern = re.compile(rf"^  {re.escape(job)}:\n((?:  .*\n|\n)*?)(?=^  \S|\Z)", re.M)
    match = pattern.search(text)
    assert match, f"job {job!r} not found in {WORKFLOW_PATH}"
    return match.group(0)


def step_bodies(job_text: str) -> dict[str, str]:
    """Map step name -> normalized run body for `- name:` steps with `run:`."""
    steps: dict[str, str] = {}
    current: str | None = None
    collecting = False
    buf: list[str] = []
    for line in job_text.splitlines():
        name_match = re.match(r"\s+- name: (.*)$", line)
        if name_match:
            if current is not None and buf:
                steps[current] = "\n".join(buf).strip()
            current = name_match.group(1).strip()
            buf = []
            collecting = False
            continue
        if re.match(r"\s+run: \|$", line):
            collecting = True
            continue
        if collecting:
            if re.match(r"\s+- name: ", line) or re.match(r"  \S", line):
                collecting = False
            else:
                buf.append(line.strip())
    if current is not None and buf:
        steps[current] = "\n".join(buf).strip()
    return steps


class ParityTest(unittest.TestCase):
    def test_implementation_step_bodies_identical(self):
        text = read_workflow()
        bodies = {job: step_bodies(job_block(text, job)) for job in IMPL_JOBS}
        for step in VALIDATOR_STEPS:
            for job in IMPL_JOBS:
                self.assertIn(
                    step, bodies[job], f"step {step!r} missing from job {job!r}"
                )
            self.assertEqual(
                bodies["validate"][step],
                bodies["validate-hosted"][step],
                f"validator step {step!r} drifted between validate and "
                "validate-hosted: command parity violated",
            )

    def test_step_order_identical(self):
        text = read_workflow()
        orders = [
            [name for name in step_bodies(job_block(text, job)) if name in VALIDATOR_STEPS]
            for job in IMPL_JOBS
        ]
        self.assertEqual(
            orders[0], orders[1], "validator step order drifted between routes"
        )


class RouterTest(unittest.TestCase):
    def test_router_emits_all_outputs(self):
        route = job_block(read_workflow(), ROUTE_JOB)
        for output in ("target=", "reason=", "error=", "fallback_allowed="):
            self.assertIn(
                output, route, f"router never emits {output.rstrip('=')}"
            )

    def test_router_runs_hosted(self):
        route = job_block(read_workflow(), ROUTE_JOB)
        self.assertIn("ubuntu-latest", route)

    def test_every_fallback_reason_routes_hosted(self):
        route = job_block(read_workflow(), ROUTE_JOB)
        for reason in HOSTED_REASONS:
            self.assertIn(
                reason, route, f"fallback reason {reason!r} missing from router"
            )
        # Each emit with one of these reasons must select the github target.
        for match in re.finditer(r'emit "(\w+)" "(\w+)"', route):
            target, reason = match.groups()
            if reason in HOSTED_REASONS:
                self.assertEqual(
                    target, "github", f"reason {reason!r} must route to github"
                )

    def test_fork_and_bot_off_self_hosted(self):
        route = job_block(read_workflow(), ROUTE_JOB)
        self.assertIn('IS_FORK_PR', route)
        self.assertIn('is_bot_pr="true"', route)

    def test_implementation_jobs_key_off_router(self):
        text = read_workflow()
        validate = job_block(text, "validate")
        hosted = job_block(text, "validate-hosted")
        self.assertIn("needs: route", validate)
        self.assertIn("needs: route", hosted)
        self.assertIn("outputs.target == 'nano'", validate)
        self.assertIn("outputs.target == 'github'", hosted)

    def test_self_hosted_job_keeps_nano_placement(self):
        validate = job_block(read_workflow(), "validate")
        self.assertIn("group: em-ci-nano", validate)
        self.assertIn("workflow-nano", validate)


class AggregateTest(unittest.TestCase):
    def test_stable_display_name(self):
        result = job_block(read_workflow(), RESULT_JOB)
        self.assertIn(f"name: {STABLE_DISPLAY_NAME}", result)

    def test_aggregate_always_runs(self):
        result = job_block(read_workflow(), RESULT_JOB)
        self.assertRegex(result, r"if:\s*always\(\)")

    def test_aggregate_runs_contract(self):
        result = job_block(read_workflow(), RESULT_JOB)
        self.assertIn(CONTRACT_TEST_FILE, result)

    def test_aggregate_fail_closed(self):
        result = job_block(read_workflow(), RESULT_JOB)
        # Selected route must succeed...
        self.assertIn("!= \"success\"", result)
        # ...unselected route must skip...
        self.assertIn("!= \"skipped\"", result)
        # ...and every violation path exits nonzero.
        self.assertGreaterEqual(result.count("exit 1"), 3)


class RedFirstTest(unittest.TestCase):
    """Mutations that must fail the parity contract with the site named."""

    def mutate_bodies(self, job: str, old: str, new: str) -> dict[str, dict[str, str]]:
        text = read_workflow()
        bodies = {j: step_bodies(job_block(text, j)) for j in IMPL_JOBS}
        for step, body in bodies[job].items():
            if old in body:
                bodies[job][step] = body.replace(old, new)
                return bodies
        raise AssertionError(f"mutation anchor {old!r} not found in {job}")

    def assert_parity_fails(self, bodies: dict[str, dict[str, str]], site: str):
        diffs = [
            step
            for step in VALIDATOR_STEPS
            if bodies["validate"].get(step) != bodies["validate-hosted"].get(step)
        ]
        self.assertTrue(diffs, f"mutation at {site} did not break parity")
        self.assertIn(site, diffs, f"parity failure must name {site}")

    def test_strict_flag_drift_fails(self):
        bodies = self.mutate_bodies(
            "validate-hosted",
            "validate_risk_packs.py --strict",
            "validate_risk_packs.py",
        )
        self.assert_parity_fails(bodies, "Validate risk packs")

    def test_commented_step_fails(self):
        bodies = self.mutate_bodies(
            "validate",
            "check_bounded_result_overflow.py",
            "# check_bounded_result_overflow.py",
        )
        self.assert_parity_fails(bodies, "Validate bounded-result overflow invariants")


if __name__ == "__main__":
    unittest.main()
