from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
CLIENT = PACKAGE.parent
REPO = Path(__file__).resolve().parents[4]
VALIDATOR_PATH = CLIENT / "validate_sublime_dap_receipt.py"
WORKFLOW_PATH = REPO / ".github" / "workflows" / "sublime-dap-real-host.yml"


def load_validator():
    spec = importlib.util.spec_from_file_location("sublime_dap_validator", VALIDATOR_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("unable to load Sublime DAP receipt validator")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def binary() -> dict:
    return {
        "path": "/tmp/perl-dap",
        "sha256": "b" * 64,
        "version": "perl-dap 0.17.0",
        "command": ["/tmp/perl-dap", "--stdio"],
    }


def runtime_receipt() -> dict:
    validator = load_validator()
    return {
        "schema_version": 1,
        "kind": "perl_dap_runtime",
        "stage": "exact_source_local",
        "source_sha": "a" * 40,
        "recorded_at": "2026-08-13T11:30:00+00:00",
        "binary": binary(),
        "fixture": {"path": "/tmp/debug_current.pl", "sha256": "c" * 64},
        "tests": ["stdio", "breakpoint", "step", "restart"],
        "assertions": {name: True for name in validator.RUNTIME_ASSERTIONS},
    }


def host_receipt() -> dict:
    validator = load_validator()
    return {
        "schema_version": 1,
        "kind": "sublime_debugger_host",
        "stage": "exact_source_local",
        "source_sha": "a" * 40,
        "recorded_at": "2026-08-13T11:31:00+00:00",
        "host": {
            "name": "Sublime Text",
            "version": "4200",
            "platform": "linux",
            "arch": "x64",
        },
        "debugger": {
            "repository": "daveleroy/SublimeDebugger",
            "version": "0.11.6",
            "ref": "58ed02acb8c06759445be62b63aef071462e0349",
        },
        "adapter": {
            "type": "perl",
            "module": "LSP-perllsp.debugger_adapter",
            "class": "PerlDapAdapter",
            "transport": "stdio",
        },
        "binary": binary(),
        "fixture": {"path": "/tmp/debug_current.pl", "sha256": "c" * 64},
        "runtime_receipt": {
            "kind": "perl_dap_runtime",
            "path": "/tmp/runtime.json",
            "sha256": "d" * 64,
            "source_sha": "a" * 40,
            "binary_sha256": "b" * 64,
        },
        "assertions": {name: True for name in validator.HOST_ASSERTIONS},
    }


class SublimeDapReceiptContractTests(unittest.TestCase):
    def test_validator_accepts_runtime_and_host_receipts(self) -> None:
        validator = load_validator()
        validator.validate(runtime_receipt())
        validator.validate(host_receipt())

    def test_validator_rejects_stage_identity_and_false_green_drift(self) -> None:
        validator = load_validator()

        public = host_receipt()
        public["stage"] = "package_control_public"
        with self.assertRaisesRegex(ValueError, "exact_source_local"):
            validator.validate(public)

        wrong_debugger = host_receipt()
        wrong_debugger["debugger"]["ref"] = "e" * 40
        with self.assertRaisesRegex(ValueError, "exact Debugger"):
            validator.validate(wrong_debugger)

        wrong_binary = host_receipt()
        wrong_binary["runtime_receipt"]["binary_sha256"] = "f" * 64
        with self.assertRaisesRegex(ValueError, "binary identity drifted"):
            validator.validate(wrong_binary)

        failed = runtime_receipt()
        failed["assertions"]["breakpoint_verified_hit"] = False
        with self.assertRaisesRegex(ValueError, "breakpoint_verified_hit"):
            validator.validate(failed)

    def test_dap_host_configuration_and_workflow_pin_exact_sources(self) -> None:
        config = json.loads((CLIENT / "dap-unittesting.json").read_text(encoding="utf-8"))
        self.assertEqual(config["pattern"], "test_sublime_debugger_adapter.py")
        self.assertTrue(config["deferred"])

        workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
        self.assertIn("58ed02acb8c06759445be62b63aef071462e0349", workflow)
        self.assertIn("cc9f5201d9f053d9ab67aa0ea575b494fd133803", workflow)
        self.assertIn("cargo build --locked --release -p perl-dap", workflow)
        self.assertIn("dap_stdio_transport_e2e", workflow)
        self.assertIn("test_e2e_single_breakpoint_hit_inspect_continue", workflow)
        self.assertIn("test_e2e_step_over_changes_execution", workflow)
        self.assertNotIn("Package Control: Install Package", workflow)


if __name__ == "__main__":
    unittest.main()
