#!/usr/bin/env python3
"""Execute the native-G producer binding and fail-closed Rust Small result.

The PR supersession resolver is retired with PR-triggered Rust qualification.
Native queue cancellation remains absence of proof, never a green result or a
claimed product failure. These controls execute production shell with no Cargo,
API calls, tokens, or hosted runs. They do not prove GitHub queue activation.
"""
from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/em-ci-routed-rust.yml"
G = "a" * 40


def workflow_step(job: str, name: str) -> dict:
    source = yaml.load(WORKFLOW.read_text(), Loader=yaml.BaseLoader)
    return next(step for step in source["jobs"][job]["steps"] if step.get("name") == name)


def run_shell(script: str, overrides: dict[str, str]) -> subprocess.CompletedProcess:
    env = {"PATH": os.environ["PATH"], "EVENT_NAME": "merge_group", "MERGE_GROUP_SHA": G,
           "GITHUB_SHA": G, "PROOF_SHA": G, "GITHUB_RESULT": "success", "CHECKOUT_SHA": G,
           "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2",
           "PROOF_RUN_ID": "123", "PROOF_RUN_ATTEMPT": "2"}
    env.update(overrides)
    with tempfile.TemporaryDirectory() as directory:
        env.update(GITHUB_STEP_SUMMARY=str(Path(directory) / "summary"),
                   GITHUB_OUTPUT=str(Path(directory) / "outputs"))
        return subprocess.run(["bash", "--noprofile", "--norc", "-c",
            'gh() { echo "unexpected API call" >&2; return 97; };\n'
            'git() { test "$*" = "rev-parse HEAD" || return 98; printf "%s\\n" "$CHECKOUT_SHA"; };\n'
            + script], env=env, capture_output=True, text=True, timeout=5)


class NativeGroupResultTests(unittest.TestCase):
    def setUp(self):
        self.step = workflow_step("rust-small-result", "Evaluate routed result")
        self.script = self.step["run"]

    def test_result_consumes_only_this_run_producer_subject(self):
        self.assertEqual(self.step["env"], {
            "EVENT_NAME": "${{ github.event_name }}",
            "MERGE_GROUP_SHA": "${{ github.event.merge_group.head_sha }}",
            "PROOF_SHA": "${{ needs.rust-small-github.outputs.subject_sha }}",
            "PROOF_RUN_ID": "${{ needs.rust-small-github.outputs.run_id }}",
            "PROOF_RUN_ATTEMPT": "${{ needs.rust-small-github.outputs.run_attempt }}",
            "GITHUB_RESULT": "${{ needs.rust-small-github.result }}",
        })

    def test_exact_g_success_is_green(self):
        result = run_shell(self.script, {})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Perl LSP Rust Small Result: success", result.stdout)

    def test_finite_current_run_attempt_tuples_pass(self):
        for run_id in ("1", "123456789"):
            for attempt in ("1", "2", "12"):
                with self.subTest(run_id=run_id, attempt=attempt):
                    result = run_shell(self.script, {
                        "GITHUB_RUN_ID": run_id, "GITHUB_RUN_ATTEMPT": attempt,
                        "PROOF_RUN_ID": run_id, "PROOF_RUN_ATTEMPT": attempt,
                    })
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_stale_wrong_or_missing_producer_run_attempt_stays_blocking(self):
        for key, values in {
            "PROOF_RUN_ID": ("", "124", "bad", "0", "123 "),
            "PROOF_RUN_ATTEMPT": ("", "1", "3", "bad", "0", "2 "),
        }.items():
            for value in values:
                with self.subTest(key=key, value=value):
                    result = run_shell(self.script, {key: value})
                    self.assertEqual(result.returncode, 1)
                    self.assertIn("not from this exact run/attempt: NOT_PROVEN", result.stdout)
                    self.assertNotIn("Perl LSP Rust Small Result: success", result.stdout)

    def test_missing_or_malformed_current_identity_cannot_match_its_way_to_green(self):
        for current, proof in (("GITHUB_RUN_ID", "PROOF_RUN_ID"),
                               ("GITHUB_RUN_ATTEMPT", "PROOF_RUN_ATTEMPT")):
            for value in ("", "0", "-1", "01", "bad", "2 "):
                with self.subTest(current=current, value=value):
                    result = run_shell(self.script, {current: value, proof: value})
                    self.assertEqual(result.returncode, 1)
                    self.assertNotIn("Perl LSP Rust Small Result: success", result.stdout)

    def test_partial_rerun_false_green_mutant_is_distinguished(self):
        before = '[ "$GITHUB_RUN_ATTEMPT" != "${PROOF_RUN_ATTEMPT:-}" ]'
        mutant = self.script.replace(before, "false")
        self.assertNotEqual(mutant, self.script)
        stale = {"GITHUB_RUN_ATTEMPT": "2", "PROOF_RUN_ATTEMPT": "1"}
        self.assertEqual(run_shell(self.script, stale).returncode, 1)
        self.assertEqual(run_shell(mutant, stale).returncode, 0)
        before = '[ "$GITHUB_RUN_ID" != "${PROOF_RUN_ID:-}" ]'
        mutant = self.script.replace(before, "false")
        self.assertNotEqual(mutant, self.script)
        wrong_run = {"PROOF_RUN_ID": "124"}
        self.assertEqual(run_shell(self.script, wrong_run).returncode, 1)
        self.assertEqual(run_shell(mutant, wrong_run).returncode, 0)

    def test_every_absent_or_non_success_result_is_blocking(self):
        for status in ("", "skipped", "failure", "cancelled", "neutral", "timed_out", "unknown", "success "):
            with self.subTest(status=status):
                result = run_shell(self.script, {"GITHUB_RESULT": status})
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertNotIn("Perl LSP Rust Small Result: success", result.stdout)

    def test_cancellation_without_output_is_not_a_product_failure(self):
        result = run_shell(self.script, {"GITHUB_RESULT": "cancelled", "PROOF_SHA": ""})
        self.assertEqual(result.returncode, 1)
        self.assertIn("RUST_SMALL_GATE_VERDICT=cancelled-no-verdict", result.stdout)
        self.assertIn("This is NOT a test failure", result.stdout)
        self.assertNotIn("superseded", result.stdout)

    def test_wrong_event_or_stale_malformed_missing_subject_never_passes(self):
        for key, values in {
            "EVENT_NAME": ("pull_request", "push", "workflow_dispatch", ""),
            "MERGE_GROUP_SHA": ("", "bad", "A" * 40, "b" * 40),
            "GITHUB_SHA": ("", "b" * 40),
            "PROOF_SHA": ("", "bad", "b" * 40),
        }.items():
            for value in values:
                with self.subTest(key=key, value=value):
                    result = run_shell(self.script, {key: value})
                    self.assertEqual(result.returncode, 1)
                    self.assertNotIn("Perl LSP Rust Small Result: success", result.stdout)

    def test_executable_false_green_mutants_are_distinguished(self):
        for before, after, inputs in (
            ('if [ "${GITHUB_RESULT:-}" != success ]; then',
             'if false; then', {"GITHUB_RESULT": "skipped"}),
            ('if [ "$MERGE_GROUP_SHA" != "${PROOF_SHA:-}" ]; then',
             'if false; then', {"PROOF_SHA": "b" * 40}),
        ):
            mutant = self.script.replace(before, after)
            self.assertNotEqual(mutant, self.script)
            self.assertEqual(run_shell(self.script, inputs).returncode, 1)
            self.assertEqual(run_shell(mutant, inputs).returncode, 0)


class NativeGroupCheckoutTests(unittest.TestCase):
    def test_subject_exports_this_exact_run_attempt(self):
        step = workflow_step("rust-small-github", "Verify merge-group source")
        result = run_shell(step["run"] + '\ncat "$GITHUB_OUTPUT"\n', {})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ["subject_sha=" + G, "run_id=123", "run_attempt=2"])

    def test_subject_rejects_missing_or_malformed_run_attempt_before_export(self):
        step = workflow_step("rust-small-github", "Verify merge-group source")
        for key in ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"):
            for value in ("", "0", "-1", "01", "bad", "2 "):
                with self.subTest(key=key, value=value):
                    result = run_shell(step["run"], {key: value})
                    self.assertEqual(result.returncode, 1)
                    self.assertIn("no valid run/attempt identity: NOT_PROVEN", result.stdout)

    def test_actual_checkout_must_match_g_before_any_rust_proof(self):
        step = workflow_step("rust-small-github", "Verify merge-group source")
        self.assertEqual(step["env"], {"MERGE_GROUP_SHA": "${{ github.event.merge_group.head_sha }}"})
        self.assertEqual(run_shell(step["run"], {}).returncode, 0)
        for values in ({"CHECKOUT_SHA": "b" * 40}, {"GITHUB_SHA": "b" * 40},
                       {"MERGE_GROUP_SHA": ""}, {"MERGE_GROUP_SHA": "bad"}):
            with self.subTest(values=values):
                self.assertEqual(run_shell(step["run"], values).returncode, 1)


if __name__ == "__main__":
    unittest.main()
