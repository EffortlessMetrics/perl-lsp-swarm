#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("run_parser_integration.py")
SPEC = importlib.util.spec_from_file_location("run_parser_integration", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MODULE_PATH}")
runner = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = runner
SPEC.loader.exec_module(runner)


class ParserIntegrationRunnerTests(unittest.TestCase):
    def write_json(self, payload: object) -> Path:
        temp = tempfile.NamedTemporaryFile(
            mode="w",
            suffix=".json",
            delete=False,
            encoding="utf-8",
        )
        with temp:
            json.dump(payload, temp)
        self.addCleanup(lambda: Path(temp.name).unlink(missing_ok=True))
        return Path(temp.name)

    @staticmethod
    def target(
        proof_id: str = "parser.example",
        *,
        package: str = "perl-parser",
        target: str = "semantic_smoke_tests",
        features: list[str] | None = None,
        cargo_args: list[str] | None = None,
        test_args: list[str] | None = None,
    ) -> dict[str, object]:
        return {
            "id": proof_id,
            "package": package,
            "target": target,
            "features": [] if features is None else features,
            "no_default_features": False,
            "cargo_args": [] if cargo_args is None else cargo_args,
            "test_args": ["--test-threads=4"] if test_args is None else test_args,
            "owner": "#6107",
            "reason": "Exercise one bounded parser integration proof.",
            "disposition": "execute",
            "boundedness": "focused",
        }

    def manifest(self, targets: list[dict[str, object]]) -> Path:
        return self.write_json(
            {"schema_version": runner.MANIFEST_SCHEMA_VERSION, "targets": targets}
        )

    def test_feature_gated_target_builds_one_manifest_owned_command(self) -> None:
        plan = runner.TargetPlan(
            proof_id="parser.incremental.integration",
            package="perl-parser",
            target="incremental_integration_test",
            features=("incremental",),
            no_default_features=False,
            cargo_args=(),
            test_args=("--test-threads=4",),
            owner="#2327",
            reason="Execute the feature-gated suite.",
            disposition="execute",
            boundedness="focused",
        )

        command = runner.cargo_command(plan)

        self.assertEqual(
            command,
            [
                "cargo",
                "test",
                "--locked",
                "--package",
                "perl-parser",
                "--features",
                "incremental",
                "--test",
                "incremental_integration_test",
                "--",
                "--test-threads=4",
            ],
        )

    def test_manifest_rejects_unknown_and_missing_fields(self) -> None:
        unknown = self.target()
        unknown["surprise"] = True
        with self.assertRaisesRegex(ValueError, "unknown fields"):
            runner.load_targets(self.manifest([unknown]))

        missing = self.target()
        del missing["reason"]
        with self.assertRaisesRegex(ValueError, "missing fields"):
            runner.load_targets(self.manifest([missing]))

    def test_manifest_rejects_empty_fields_and_invalid_owner(self) -> None:
        empty = self.target()
        empty["target"] = ""
        with self.assertRaisesRegex(ValueError, "non-empty string"):
            runner.load_targets(self.manifest([empty]))

        invalid_owner = self.target()
        invalid_owner["owner"] = "parser"
        with self.assertRaisesRegex(ValueError, "GitHub issue reference"):
            runner.load_targets(self.manifest([invalid_owner]))

    def test_manifest_rejects_duplicate_proof_id(self) -> None:
        with self.assertRaisesRegex(ValueError, "duplicate parser integration proof id"):
            runner.load_targets(
                self.manifest(
                    [
                        self.target(),
                        self.target(target="incremental_integration_test"),
                    ]
                )
            )

    def test_manifest_rejects_duplicate_invocation_under_another_id(self) -> None:
        with self.assertRaisesRegex(ValueError, "duplicate parser integration invocation"):
            runner.load_targets(
                self.manifest(
                    [
                        self.target("parser.one"),
                        self.target("parser.two"),
                    ]
                )
            )

    def test_manifest_rejects_feature_order_and_reserved_cargo_override(self) -> None:
        unsorted = self.target(features=["workspace", "incremental"])
        with self.assertRaisesRegex(ValueError, "unique and sorted"):
            runner.load_targets(self.manifest([unsorted]))

        override = self.target(cargo_args=["--test"])
        with self.assertRaisesRegex(ValueError, "override manifest-owned invocation identity"):
            runner.load_targets(self.manifest([override]))

    def test_lock_rejects_deleted_proof_even_when_an_unrelated_row_is_added(self) -> None:
        accepted = runner.load_targets(
            self.manifest(
                [
                    self.target("parser.one", target="semantic_smoke_tests"),
                    self.target(
                        "parser.two",
                        target="incremental_parser_accuracy",
                        features=["incremental"],
                    ),
                ]
            )
        )
        lock = {
            plan.proof_id: runner.invocation_digest(plan)
            for plan in accepted
        }
        changed = runner.load_targets(
            self.manifest(
                [
                    self.target(
                        "parser.two",
                        target="incremental_parser_accuracy",
                        features=["incremental"],
                    ),
                    self.target("parser.replacement", target="error_recovery_regression"),
                ]
            )
        )

        with self.assertRaisesRegex(ValueError, "missing=parser.one"):
            runner.validate_lock(changed, lock)

    def test_lock_rejects_behavior_bearing_feature_change(self) -> None:
        original = runner.load_targets(
            self.manifest(
                [
                    self.target(
                        "parser.incremental",
                        target="incremental_integration_test",
                        features=["incremental"],
                    )
                ]
            )
        )
        lock = {
            plan.proof_id: runner.invocation_digest(plan)
            for plan in original
        }
        changed = runner.load_targets(
            self.manifest(
                [
                    self.target(
                        "parser.incremental",
                        target="incremental_integration_test",
                    )
                ]
            )
        )

        with self.assertRaisesRegex(ValueError, "changed=parser.incremental"):
            runner.validate_lock(changed, lock)

    def test_lock_payload_is_deterministic_by_proof_id(self) -> None:
        plans = runner.load_targets(
            self.manifest(
                [
                    self.target("parser.z", target="semantic_smoke_tests"),
                    self.target("parser.a", target="error_recovery_regression"),
                ]
            )
        )

        payload = runner.lock_payload(plans)

        self.assertEqual(
            [row["id"] for row in payload["proofs"]],
            ["parser.a", "parser.z"],
        )

    @mock.patch.object(runner.subprocess, "run")
    def test_execute_plans_runs_complete_denominator_after_failure(
        self,
        run: mock.Mock,
    ) -> None:
        run.side_effect = [
            subprocess.CompletedProcess(["cargo"], 9),
            subprocess.CompletedProcess(["cargo"], 0),
        ]
        plans = runner.load_targets(
            self.manifest(
                [
                    self.target("parser.first", target="semantic_smoke_tests"),
                    self.target("parser.second", target="error_recovery_regression"),
                ]
            )
        )

        returncode, results = runner.execute_plans(plans)

        self.assertEqual(returncode, 9)
        self.assertEqual(run.call_count, 2)
        self.assertEqual(
            [item["result"] for item in results],
            ["failed", "passed"],
        )

    def test_checked_in_manifest_matches_exact_identity_lock(self) -> None:
        plans = runner.load_targets(runner.TARGETS_PATH)
        lock = runner.load_lock(runner.LOCK_PATH)

        runner.validate_lock(plans, lock)

        self.assertEqual(len(plans), len(lock))
        self.assertTrue(all(plan.disposition == "execute" for plan in plans))


if __name__ == "__main__":
    unittest.main()
