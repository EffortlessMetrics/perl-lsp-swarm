#!/usr/bin/env python3
"""Negative controls for exact-subject, one-proof Rust Small normalization."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from scripts.ci.rust_small_evidence import validate_evidence

ROOT = Path(__file__).resolve().parents[2]


class RustSmallEvidenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.evidence = {
            "schema": "em-ci-rust-result.v1", "repository": "owner/repository",
            "sha": "a" * 40, "run_id": "123", "run_attempt": "2",
            "profile": "standard", "route": "self_hosted", "result": "success",
            "infrastructure_failure": "", "proof_outcome": "success",
            "proof_entered_at": "2026-10-01T10:00:00Z",
        }
        self.environment = {
            "EM_CI_CALL_RESULT": "success", "EXPECTED_REPOSITORY": "owner/repository",
            "EXPECTED_SHA": "a" * 40, "EXPECTED_RUN_ID": "123", "EXPECTED_RUN_ATTEMPT": "2",
        }

    def validate(self, **changes: str) -> dict[str, str]:
        return validate_evidence({**self.environment, "EM_CI_EVIDENCE": json.dumps({**self.evidence, **changes})})

    def test_successful_primary_and_hosted_routes(self) -> None:
        for route in ("self_hosted", "hosted"):
            with self.subTest(route=route):
                self.assertEqual(self.validate(route=route)["route"], route)

    def test_only_typed_preproof_recovery_can_be_success(self) -> None:
        for reason in ("toolchain_unavailable", "disposable_paths_unavailable"):
            self.validate(route="hosted_fallback", infrastructure_failure=reason)
        for reason in ("", "timeout", "cancelled", "proof_failure", "runner_lost"):
            with self.subTest(reason=reason), self.assertRaises(ValueError):
                self.validate(route="hosted_fallback", infrastructure_failure=reason)

    def test_every_subject_field_is_bound(self) -> None:
        for key in ("schema", "repository", "sha", "run_id", "run_attempt", "profile"):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.validate(**{key: "different"})

    def test_missing_fields_are_not_evidence(self) -> None:
        for key in self.evidence:
            evidence = {k: v for k, v in self.evidence.items() if k != key}
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate_evidence({**self.environment, "EM_CI_EVIDENCE": json.dumps(evidence)})

    def test_call_failure_cancellation_skip_and_missing_stay_blocking(self) -> None:
        for result in ("failure", "cancelled", "skipped", "", "success "):
            self.environment["EM_CI_CALL_RESULT"] = result
            with self.subTest(result=result), self.assertRaises(ValueError):
                self.validate()

    def test_proof_failure_and_absence_stay_blocking(self) -> None:
        for key in ("result", "proof_outcome"):
            for result in ("failure", "cancelled", "skipped", ""):
                with self.subTest(key=key, result=result), self.assertRaises(ValueError):
                    self.validate(**{key: result})

    def test_entry_marker_must_be_valid_utc_timestamp(self) -> None:
        for entered in ("", "before", "2026-02-30T00:00:00Z", "2026-10-01T99:00:00Z", "2026-10-01T10:00:00+00:00"):
            with self.subTest(entered=entered), self.assertRaises(ValueError):
                self.validate(proof_entered_at=entered)

    def test_unknown_route_and_contradictory_primary_fail(self) -> None:
        for route in ("cx53", "unknown", "", "hosted_fallback\nproof_validated=true"):
            with self.subTest(route=route), self.assertRaises(ValueError):
                self.validate(route=route)
        with self.assertRaises(ValueError):
            self.validate(infrastructure_failure="toolchain_unavailable")

    def test_malformed_and_duplicate_json_fail(self) -> None:
        for raw in ("", "null", "[]", "true", "{", '{"schema":"wrong","schema":"em-ci-rust-result.v1"}'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                validate_evidence({**self.environment, "EM_CI_EVIDENCE": raw})

    def test_evidence_types_do_not_coerce(self) -> None:
        for key, value in (("run_id", 123), ("result", True), ("proof_entered_at", None)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate_evidence({**self.environment, "EM_CI_EVIDENCE": json.dumps({**self.evidence, key: value})})

    def test_missing_expected_identity_fails_closed(self) -> None:
        for key in self.environment:
            environment = {k: v for k, v in self.environment.items() if k != key}
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate_evidence({**environment, "EM_CI_EVIDENCE": json.dumps(self.evidence)})

    def test_workflow_wires_exact_subject_and_required_guard(self) -> None:
        import yaml
        workflow = yaml.safe_load((ROOT / ".github/workflows/em-ci-routed-rust.yml").read_text())
        steps = workflow["jobs"]["rust-small-result"]["steps"]
        guard = next(step for step in steps if step.get("id") == "evidence")
        self.assertEqual(guard, {
            "name": "Validate subject-bound governed proof", "id": "evidence",
            "if": "github.event_name != 'pull_request' || github.event.pull_request.draft != true",
            "shell": "bash", "run": "python3 scripts/ci/rust_small_evidence.py",
            "env": {
                "EM_CI_CALL_RESULT": "${{ needs.rust-small-proof.result }}",
                "EM_CI_EVIDENCE": "${{ needs.rust-small-proof.outputs.evidence }}",
                "EXPECTED_REPOSITORY": "${{ github.repository }}",
                "EXPECTED_SHA": "${{ github.sha }}",
                "EXPECTED_RUN_ID": "${{ github.run_id }}",
                "EXPECTED_RUN_ATTEMPT": "${{ github.run_attempt }}",
            },
        })
        names = [step.get("name") for step in steps]
        self.assertLess(names.index("Validate subject-bound governed proof"), names.index("Probe main-red refusal"))
        evaluate = next(step for step in steps if step.get("name") == "Evaluate routed result")
        self.assertNotIn("if", evaluate)
        self.assertNotIn("continue-on-error", evaluate)

    def test_cli_emits_validation_only_after_complete_success(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            environment = {**os.environ, **self.environment, "GITHUB_OUTPUT": str(output)}
            for evidence, expected_status in ((self.evidence, 0), ({**self.evidence, "proof_outcome": "failure"}, 1)):
                output.write_text("", encoding="utf-8")
                result = subprocess.run([sys.executable, str(ROOT / "scripts/ci/rust_small_evidence.py")], env={**environment, "EM_CI_EVIDENCE": json.dumps(evidence)}, capture_output=True, text=True, check=False)
                self.assertEqual(result.returncode, expected_status, result.stderr)
                self.assertEqual(output.read_text(), "proof_validated=true\n" if expected_status == 0 else "")


if __name__ == "__main__":
    unittest.main()
