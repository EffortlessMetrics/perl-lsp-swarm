#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("run_parser_integration.py")
SPEC = importlib.util.spec_from_file_location("run_parser_integration", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {MODULE_PATH}")
runner = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = runner
SPEC.loader.exec_module(runner)


class ParserIntegrationRunnerTests(unittest.TestCase):
    def write_manifest(self, payload: object) -> Path:
        temp = tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False, encoding="utf-8")
        with temp:
            json.dump(payload, temp)
        self.addCleanup(lambda: Path(temp.name).unlink(missing_ok=True))
        return Path(temp.name)

    def test_command_contains_every_manifest_target(self) -> None:
        targets = [
            ("perl-ast", "ast_behavior_spec_tests"),
            ("perl-parser", "semantic_smoke_tests"),
            ("perl-parser-core", "pir_a_loop_body_test"),
            ("perl-parser-core", "pir_a_branch_body_test"),
            ("perl-parser-core", "error_recovery_regression"),
            ("perl-parser-core", "fix_incomplete_brace_recovery_marker_1911"),
            ("perl-parser", "incremental_integration_test"),
        ]
        command = runner.cargo_command(targets)
        self.assertEqual(command.count("--test"), len(targets))
        for _, target in targets:
            self.assertIn(target, command)

    def test_manifest_rejects_empty_target_fields(self) -> None:
        for key in ("package", "target"):
            manifest = self.write_manifest(
                {
                    "schema_version": 1,
                    "targets": [
                        {
                            "package": "" if key == "package" else "perl-parser",
                            "target": "" if key == "target" else "semantic_smoke_tests",
                        }
                    ],
                }
            )
            with self.assertRaisesRegex(ValueError, "non-empty string"):
                runner.load_targets(manifest)

    def test_manifest_rejects_duplicate_target(self) -> None:
        manifest = self.write_manifest(
            {"schema_version": 1, "targets": [{"package": "perl-parser", "target": "semantic_smoke_tests"}] * 7}
        )
        with self.assertRaisesRegex(ValueError, "duplicate"):
            runner.load_targets(manifest)

    def test_manifest_rejects_duplicate_target_name_across_packages(self) -> None:
        manifest = self.write_manifest(
            {
                "schema_version": 1,
                "targets": [
                    {"package": "perl-parser", "target": "shared_test"},
                    {"package": "perl-parser-core", "target": "shared_test"},
                ],
            }
        )
        with self.assertRaisesRegex(ValueError, "duplicate parser integration target name"):
            runner.load_targets(manifest)

    def test_manifest_rejects_shrink(self) -> None:
        manifest = self.write_manifest(
            {
                "schema_version": 1,
                "targets": [
                    {"package": "perl-parser", "target": target}
                    for target in [
                        "semantic_smoke_tests",
                        "incremental_integration_test",
                        "incremental_edge_cases_test",
                        "incremental_parsing_tests",
                        "incremental_regression_slices",
                        "incremental_comprehensive_test",
                    ]
                ],
            }
        )
        with self.assertRaisesRegex(ValueError, "shrank"):
            runner.load_targets(manifest)


if __name__ == "__main__":
    unittest.main()
