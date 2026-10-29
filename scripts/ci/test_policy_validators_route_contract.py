#!/usr/bin/env python3
"""Fallback-shape contract for Policy Validators hosted failover.

`Policy Validators` was pinned directly to `em-ci-nano` while the lane
registry marks it blocking, so a runner outage queued the lane forever. Its
work is portable stdlib-only Python, so self-hosting is an optimization, not
proof semantics. This workflow runs trusted same-repository events on
`em-ci-nano` and fails over to `ubuntu-24.04` whenever the trusted lane does
not succeed (outage timeout, failure, cancellation, or a fork/bot skip):

- `validate` runs on trusted capacity for same-repo human PRs (and dispatch)
- `validate-hosted` always runs in parallel with step bodies identical to
  `validate`
- `validate-result` (display name `Validate CI policy ledgers`, the stable
  required identity) runs the contract and shared decision table after only
  the fallback. Every pair passes exactly when hosted succeeded, so no nano
  observation is necessary to decide the required verdict.
- `validate-diagnostics` independently polls nano with the same deadline
  and reports the full agreement/divergence/failover/failure comparison,
  including outage diagnostics and the scheduled reaper's role.

No secrets and no expressions embedded in run source: the router that needed
a runner-inventory token cannot satisfy the workflow security ratchet, and a
needs gate cannot fail over a lane that queues forever (queue time ignores
timeout-minutes), so only the diagnostic job polls native job conclusions.

Red-first contract: mutating ANY single implementation job's copy of a
validator step — argument drift, commenting out, echo decoy — must fail this
contract WITH THE SITE NAMED, so a silent revert fails the required aggregate
instead of drifting back to per-runner copies. Fork and bot PRs must stay off
trusted self-hosted capacity. Reverting trusted-failure to a hard fail must
fail the decision table even when hosted succeeded.
"""

from __future__ import annotations

import importlib.util
import io
import os
import re
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "policy-validators.yml"
AGGREGATE_PATH = ROOT / "scripts" / "ci" / "policy_validators_aggregate.py"

IMPL_JOBS = ("validate", "validate-hosted")
RESULT_JOB = "validate-result"
DIAGNOSTIC_JOB = "validate-diagnostics"
STABLE_DISPLAY_NAME = "Validate CI policy ledgers"
CONTRACT_TEST_FILE = "scripts/ci/test_policy_validators_route_contract.py"
AGGREGATE_SCRIPT = "scripts/ci/policy_validators_aggregate.py"


def load_aggregate():
    spec = importlib.util.spec_from_file_location(
        "policy_validators_aggregate", AGGREGATE_PATH
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


aggregate = load_aggregate()

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
    "Validate Cargo feature roles",
    "Validate Dependabot source contract",
    "Validate Dependabot cooldown contract",
    "Validate exposed-surface disposition contract",
    "Validate bounded-result overflow invariants",
    "Validate parser facade authority",
    "Validate Homebrew formula digest binding",
)

# Fork/bot markers that must each keep the trusted lane skipped.
UNTRUSTED_MARKERS = (
    "head.repo.full_name",
    "user.type != 'Bot'",
    "dependabot",
    "app/",
    "[bot]",
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
            if line.strip() and not line.startswith("          "):
                collecting = False
            else:
                # Remove only YAML's run-body indentation. Internal Python
                # heredoc and shell indentation is part of the executable.
                buf.append(line[10:])
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


class FallbackRoutingTest(unittest.TestCase):
    def test_no_router_job(self):
        text = read_workflow()
        self.assertNotRegex(text, r"(?m)^  route:\n")

    def test_no_secret_references(self):
        text = read_workflow()
        self.assertNotIn("secrets.", text)

    def test_no_expressions_in_run_source(self):
        text = read_workflow()
        for job in IMPL_JOBS + (RESULT_JOB, DIAGNOSTIC_JOB):
            for step, body in step_bodies(job_block(text, job)).items():
                self.assertNotIn(
                    "${{", body, f"expression embedded in {job} step {step!r}"
                )

    def test_fork_and_bot_off_self_hosted(self):
        validate = job_block(read_workflow(), "validate")
        for marker in UNTRUSTED_MARKERS:
            self.assertIn(
                marker, validate, f"untrusted marker {marker!r} missing from validate guard"
            )

    def test_hosted_always_runs_unblocked(self):
        hosted = job_block(read_workflow(), "validate-hosted")
        # No needs gate: a queued-forever trusted lane must not block the
        # fallback, and queue time ignores timeout-minutes.
        self.assertNotIn("needs:", hosted)
        self.assertIn("ubuntu-24.04", hosted)

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
        needs = re.search(r"(?m)^    needs:\n((?:      .*\n)+)", result)
        self.assertIsNotNone(needs, "required result must depend on hosted validation")
        self.assertEqual(needs.group(1).strip(), "- validate-hosted")
        self.assertIn("HOSTED_RESULT: ${{ needs.validate-hosted.result }}", result)
        self.assertNotIn("continue-on-error:", result)
        self.assertEqual(re.findall(r"(?m)^\s+if:.*$", result), ["    if: always()"])
        bodies = step_bodies(result)
        self.assertEqual(
            list(bodies),
            ["Prove policy-validators fallback contract", "Evaluate routed result"],
        )
        self.assertEqual(
            bodies["Prove policy-validators fallback contract"],
            "set -euo pipefail\n"
            f"python3 -m unittest {CONTRACT_TEST_FILE}",
        )
        verdict = bodies["Evaluate routed result"]
        self.assertIn("set -euo pipefail", verdict)
        self.assertEqual(
            verdict.splitlines()[-1],
            f'python3 {AGGREGATE_SCRIPT} "unobserved" "$HOSTED_RESULT"',
        )
        for forbidden in ("gh api", "sleep ", "POLL_", "GH_TOKEN", "|| true", "exit 0"):
            self.assertNotIn(forbidden, result, f"required result contains {forbidden!r}")

    def test_required_identity_is_unique(self):
        self.assertEqual(
            len(re.findall(rf"(?m)^    name: {re.escape(STABLE_DISPLAY_NAME)}$", read_workflow())),
            1,
        )

    def test_diagnostics_are_independent_and_bounded(self):
        diagnostics = job_block(read_workflow(), DIAGNOSTIC_JOB)
        self.assertEqual(re.findall(r"(?m)^\s+if:.*$", diagnostics), ["    if: always()"])
        self.assertNotIn("continue-on-error:", diagnostics)
        needs = re.search(r"(?m)^    needs:\n((?:      .*\n)+)", diagnostics)
        self.assertIsNotNone(needs)
        self.assertEqual(needs.group(1).strip(), "- validate-hosted")
        self.assertIn("actions: read", diagnostics)
        self.assertIn("actions/runs/", diagnostics)
        self.assertIn("POLL_DEADLINE_SECONDS: 900", diagnostics)
        self.assertIn("POLL_INTERVAL_SECONDS: 30", diagnostics)
        self.assertIn(AGGREGATE_SCRIPT, diagnostics)


class RequiredVerdictExecutionTest(unittest.TestCase):
    """Execute the workflow's Bash with forbidden diagnostic tools as tripwires."""

    def test_hosted_result_alone_decides_without_api_or_sleep(self):
        body = step_bodies(job_block(read_workflow(), RESULT_JOB))["Evaluate routed result"]
        with tempfile.TemporaryDirectory() as directory:
            tripwire = Path(directory) / "called"
            for command in ("gh", "sleep"):
                script = Path(directory) / command
                script.write_text('#!/bin/sh\necho "$0" >> "$TRIPWIRE"\nexit 83\n')
                script.chmod(0o755)
            for hosted in ("success", "failure", "skipped", "cancelled", "pending", "timed_out", "", "unknown"):
                with self.subTest(hosted=hosted):
                    env = dict(os.environ, PATH=f"{directory}:{os.environ['PATH']}",
                               TRIPWIRE=str(tripwire), HOSTED_RESULT=hosted,
                               REPO="fixture/repo", RUN_ID="123", NANO_JOB="nano",
                               POLL_DEADLINE_SECONDS="900", POLL_INTERVAL_SECONDS="30")
                    completed = subprocess.run(
                        ["bash", "-c", body], cwd=ROOT, env=env,
                        capture_output=True, text=True, timeout=3,
                    )
                    self.assertEqual(completed.returncode, 0 if hosted == "success" else 1,
                                     completed.stdout + completed.stderr)
                    self.assertEqual("::error::" in completed.stdout, hosted != "success")
                    self.assertFalse(tripwire.exists(), "required verdict called a diagnostic tool")


class RequiredPathNegativeTest(unittest.TestCase):
    def assert_contract_rejects(self, old: str, new: str):
        text = read_workflow()
        result = job_block(text, RESULT_JOB)
        self.assertIn(old, result)
        mutated = text.replace(result, result.replace(old, new, 1), 1)
        with patch(__name__ + ".read_workflow", return_value=mutated):
            with self.assertRaises(AssertionError):
                AggregateTest("test_aggregate_fail_closed").test_aggregate_fail_closed()

    def test_waiting_on_nano_or_diagnostics_is_rejected(self):
        for dependency in ("validate", DIAGNOSTIC_JOB):
            with self.subTest(dependency=dependency):
                self.assert_contract_rejects("- validate-hosted", f"- validate-hosted\n      - {dependency}")

    def test_sleeping_in_verdict_is_rejected(self):
        self.assert_contract_rejects("set -euo pipefail", "set -euo pipefail\n          sleep 900")

    def test_skipped_contract_cannot_receive_credit(self):
        self.assert_contract_rejects(f"python3 -m unittest {CONTRACT_TEST_FILE}",
                                     f"echo python3 -m unittest {CONTRACT_TEST_FILE}")
        self.assert_contract_rejects("- name: Prove policy-validators fallback contract",
                                     "- name: Prove policy-validators fallback contract\n        if: false")

    def test_hardcoded_green_and_ignored_errors_are_rejected(self):
        command = f'python3 {AGGREGATE_SCRIPT} "unobserved" "$HOSTED_RESULT"'
        for replacement in ("true", command + " || true", command.replace("$HOSTED_RESULT", "success")):
            with self.subTest(replacement=replacement):
                self.assert_contract_rejects(command, replacement)

    def test_continue_on_error_is_rejected(self):
        self.assert_contract_rejects("if: always()", "if: always()\n    continue-on-error: true")


class DiagnosticExecutionTest(unittest.TestCase):
    """Run the actual diagnostic body with bounded fake native job reads."""

    def run_diagnostics(self, nano: str, hosted: str, *, api_exit: int = 0):
        body = step_bodies(job_block(read_workflow(), DIAGNOSTIC_JOB))["Report lane diagnostics"]
        with tempfile.TemporaryDirectory() as directory:
            summary = Path(directory) / "summary"
            calls = Path(directory) / "calls"
            gh = Path(directory) / "gh"
            gh.write_text(
                '#!/bin/sh\necho api >> "$CALLS"\n'
                '[ "$API_EXIT" = 0 ] || exit "$API_EXIT"\n'
                'if [ "$NANO" = delayed ] && [ "$(wc -l < "$CALLS")" = 1 ]; then\n'
                '  echo pending\n'
                'elif [ "$NANO" = delayed ]; then\n'
                '  echo success\n'
                'else\n'
                '  printf "%s\\n" "$NANO"\n'
                'fi\n'
            )
            gh.chmod(0o755)
            sleep = Path(directory) / "sleep"
            sleep.write_text('#!/bin/sh\necho sleep >> "$SLEEPS"\n')
            sleep.chmod(0o755)
            sleeps = Path(directory) / "sleeps"
            env = dict(
                os.environ, PATH=f"{directory}:{os.environ['PATH']}",
                HOSTED_RESULT=hosted, NANO=nano, API_EXIT=str(api_exit),
                REPO="fixture/repo", RUN_ID="123", NANO_JOB="nano",
                GITHUB_STEP_SUMMARY=str(summary), CALLS=str(calls), SLEEPS=str(sleeps),
                POLL_DEADLINE_SECONDS="0" if nano in ("", "pending") else "900",
                POLL_INTERVAL_SECONDS="30",
            )
            completed = subprocess.run(
                ["bash", "-c", body], cwd=ROOT, env=env,
                capture_output=True, text=True, timeout=3,
            )
            return (completed, summary.read_text() if summary.exists() else "",
                    calls.read_text().splitlines() if calls.exists() else [],
                    sleeps.read_text().splitlines() if sleeps.exists() else [])

    def test_complete_comparison_matrix_in_logs_and_summary(self):
        statuses = ("success", "failure", "skipped", "cancelled", "pending", "timed_out", "")
        for nano in statuses:
            for hosted in statuses:
                with self.subTest(nano=nano, hosted=hosted):
                    completed, summary, calls, sleeps = self.run_diagnostics(nano, hosted)
                    self.assertEqual(completed.returncode, 0 if hosted == "success" else 1,
                                     completed.stdout + completed.stderr)
                    if nano == hosted == "success":
                        message = "both lanes agree"
                    elif nano == "success":
                        message = "lane divergence"
                    elif hosted == "success":
                        message = "hosted passed"
                    else:
                        message = "Policy Validators failed"
                    self.assertIn(message, completed.stdout)
                    self.assertIn(message, summary)
                    self.assertIn(f"nano={nano or 'pending'} hosted={hosted}", summary)
                    self.assertEqual(sleeps, [], "terminal conclusions must not wait")
                    self.assertEqual(calls, [] if nano in ("", "pending") else ["api"])

    def test_deadline_preserves_actionable_outage_diagnostics(self):
        completed, summary, _, _ = self.run_diagnostics("pending", "success")
        self.assertEqual(completed.returncode, 0)
        self.assertIn("::warning::", completed.stdout)
        for marker in ("em-ci-nano", "nano-reaper", "#15900", "registration and connectivity"):
            self.assertIn(marker, completed.stdout + summary)

    def test_pending_then_terminal_result_is_observed(self):
        completed, summary, calls, sleeps = self.run_diagnostics("delayed", "success")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(calls, ["api", "api"])
        self.assertEqual(sleeps, ["sleep"])
        self.assertIn("both lanes agree", summary)

    def test_api_failure_is_not_green_diagnostic_credit(self):
        completed, summary, calls, sleeps = self.run_diagnostics("success", "success", api_exit=83)
        self.assertEqual(completed.returncode, 83)
        self.assertEqual(calls, ["api"])
        self.assertEqual(sleeps, [])
        self.assertNotIn("hosted passed", summary)
        self.assertNotIn("both lanes agree", summary)

    def test_skipped_or_ignored_diagnostics_are_rejected(self):
        text = read_workflow()
        diagnostics = job_block(text, DIAGNOSTIC_JOB)
        for addition in ("if: false", "continue-on-error: true"):
            with self.subTest(addition=addition):
                mutated = text.replace(
                    diagnostics,
                    diagnostics.replace("- name: Report lane diagnostics",
                                        f"- name: Report lane diagnostics\n        {addition}"),
                )
                with patch(__name__ + ".read_workflow", return_value=mutated):
                    with self.assertRaises(AssertionError):
                        AggregateTest("test_diagnostics_are_independent_and_bounded").test_diagnostics_are_independent_and_bounded()


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


class DeadlineCancelTest(unittest.TestCase):
    """A queued-forever trusted lane must reach an explicit terminal state.

    Job queue time ignores timeout-minutes, so a trusted lane that never
    receives a runner keeps the run and its check pending forever (#15900).
    The aggregate (PR-triggered, no write authority per the workflow
    security ratchet) must emit actionable deadline diagnostics; the
    scheduled policy-validators-nano-reaper workflow owns the explicit
    cancel.
    """

    def read_result_block(self) -> str:
        return job_block(read_workflow(), DIAGNOSTIC_JOB)

    def test_aggregate_reports_deadline_diagnostics(self):
        result = self.read_result_block()
        self.assertIn("::warning::", result)
        self.assertIn("GITHUB_STEP_SUMMARY", result)
        self.assertIn("em-ci-nano", result)
        self.assertIn("#15900", result)
        self.assertIn("nano-reaper", result)

    def test_aggregate_holds_no_write_authority(self):
        text = read_workflow()
        self.assertNotIn("actions: write", text)
        self.assertNotIn("/cancel", text)
        self.assertNotIn("secrets.", text)

    def test_reaper_owns_the_cancel(self):
        reaper = (
            ROOT / ".github" / "workflows" / "policy-validators-nano-reaper.yml"
        ).read_text(encoding="utf-8")
        self.assertIn("schedule:", reaper)
        self.assertNotIn("pull_request", reaper)
        self.assertIn("actions: write", reaper)
        self.assertNotIn("actions/checkout", reaper)
        self.assertIn("/cancel", reaper)
        self.assertIn("em-ci-nano", reaper)
        self.assertIn("#15900", reaper)
        self.assertIn("MAX_QUEUE_SECONDS", reaper)
        self.assertNotIn("secrets.", reaper)


class DecisionTableTest(unittest.TestCase):
    """Executable #15854 matrix: failover on every trusted non-success."""

    CASES = (
        ("success", "success", 0),
        ("success", "failure", 1),
        ("success", "cancelled", 1),
        ("failure", "success", 0),
        ("failure", "failure", 1),
        ("skipped", "success", 0),
        ("cancelled", "success", 0),
        ("pending", "success", 0),
        ("pending", "failure", 1),
        ("", "success", 0),
        ("failure", "", 1),
    )

    def test_decision_table(self):
        for nano, hosted, code in self.CASES:
            with self.subTest(nano=nano or "empty", hosted=hosted or "empty"):
                got_code, message = aggregate.decide(nano, hosted)
                self.assertEqual(got_code, code, message)
                if code == 0 and nano == "success":
                    self.assertIn("both lanes agree", message)
                elif code == 0:
                    self.assertIn("failover", message)
                    self.assertIn("hosted passed", message)
                elif nano == "success":
                    self.assertIn("lane divergence", message)
                else:
                    self.assertIn("failed", message)

    def test_trusted_failure_failovers_when_hosted_succeeds(self):
        code, message = aggregate.decide("failure", "success")
        self.assertEqual(code, 0, "trusted failure must not block a hosted success")
        self.assertIn("failover", message)
        self.assertNotIn("trusted lane failed", message)

    def test_hosted_failure_fail_closes_regardless_of_trusted(self):
        for nano in ("success", "failure", "skipped", "cancelled", "pending"):
            with self.subTest(nano=nano):
                code, message = aggregate.decide(nano, "failure")
                self.assertEqual(code, 1)
                self.assertTrue(
                    "divergence" in message or "failed" in message, message
                )

    def test_cli_fail_closed_writes_error_annotation(self):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            code = aggregate.main(["failure", "failure"])
        self.assertEqual(code, 1)
        self.assertIn("::error::", stdout.getvalue())
        self.assertEqual(stderr.getvalue(), "")

    def test_cli_failover_is_quiet_success(self):
        stdout = io.StringIO()
        with redirect_stdout(stdout):
            code = aggregate.main(["failure", "success"])
        self.assertEqual(code, 0)
        self.assertIn("failover", stdout.getvalue())
        self.assertNotIn("::error::", stdout.getvalue())

    def test_workflow_invokes_decision_table(self):
        result = job_block(read_workflow(), DIAGNOSTIC_JOB)
        self.assertIn(AGGREGATE_SCRIPT, result)
        self.assertIn('"$nano"', result)
        self.assertIn('"$HOSTED_RESULT"', result)

    def test_workflow_paths_cover_decision_table(self):
        text = read_workflow()
        self.assertIn(f"- '{AGGREGATE_SCRIPT}'", text)
        self.assertIn(f"- '{CONTRACT_TEST_FILE}'", text)

    def test_full_truth_table_verdict_depends_only_on_hosted_success(self):
        statuses = ("success", "failure", "skipped", "cancelled", "pending", "timed_out", "")
        for nano in statuses:
            for hosted in statuses:
                with self.subTest(nano=nano, hosted=hosted):
                    code, _ = aggregate.decide(nano, hosted)
                    self.assertEqual(code, 0 if hosted == "success" else 1)
                    self.assertEqual(code, aggregate.decide("unobserved", hosted)[0])


if __name__ == "__main__":
    unittest.main()
