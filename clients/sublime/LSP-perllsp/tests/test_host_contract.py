from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path


PACKAGE_ROOT = Path(__file__).resolve().parents[1]
CLIENT_ROOT = PACKAGE_ROOT.parent
VALIDATOR_PATH = CLIENT_ROOT / "validate_sublime_host_receipt.py"
SCHEMA_PATH = CLIENT_ROOT / "sublime-host-receipt.v1.schema.json"
WORKFLOW_PATH = Path(__file__).resolve().parents[4] / ".github" / "workflows" / "sublime-real-host.yml"


def load_validator():
    spec = importlib.util.spec_from_file_location("sublime_host_receipt_validator", VALIDATOR_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("unable to load Sublime host receipt validator")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sample_receipt() -> dict:
    assertions = {name: True for name in load_validator().REQUIRED_ASSERTIONS}
    return {
        "schema_version": 1,
        "stage": "exact_source_local",
        "source_sha": "a" * 40,
        "recorded_at": "2026-08-13T10:30:00+00:00",
        "host": {
            "name": "Sublime Text",
            "version": "4200",
            "platform": "linux",
            "arch": "x64",
        },
        "lsp_package": {
            "repository": "sublimelsp/LSP",
            "ref": "cc9f5201d9f053d9ab67aa0ea575b494fd133803",
        },
        "helper_package": {
            "name": "LSP-perllsp",
            "source": "clients/sublime/LSP-perllsp",
        },
        "binary": {
            "path": "/tmp/perllsp",
            "sha256": "b" * 64,
            "command": ["/tmp/perllsp", "--stdio"],
        },
        "fixtures": {
            "pl": "app.pl",
            "pm": "customlib/Greeting.pm",
            "t": "t/greeting.t",
        },
        "assertions": assertions,
    }


class SublimeHostContractTests(unittest.TestCase):
    def test_validator_accepts_complete_exact_source_receipt(self) -> None:
        load_validator().validate(sample_receipt())

    def test_validator_rejects_public_stage_overclaim(self) -> None:
        payload = sample_receipt()
        payload["stage"] = "package_control_public"
        with self.assertRaisesRegex(ValueError, "exact_source_local"):
            load_validator().validate(payload)

    def test_schema_and_unittesting_configuration_are_valid_json(self) -> None:
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        config = json.loads((PACKAGE_ROOT / "unittesting.json").read_text(encoding="utf-8"))
        self.assertEqual(schema["properties"]["stage"]["const"], "exact_source_local")
        self.assertEqual(config["tests_dir"], "host_tests")
        self.assertTrue(config["deferred"])
        self.assertGreaterEqual(config["condition_timeout"], 120_000)

    def test_workflow_pins_lsp_2_13_source_and_all_three_host_os_families(self) -> None:
        workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
        self.assertIn("cc9f5201d9f053d9ab67aa0ea575b494fd133803", workflow)
        self.assertIn("ubuntu-latest", workflow)
        self.assertIn("macos-latest", workflow)
        self.assertIn("windows-latest", workflow)
        self.assertNotIn("Package Control: Install Package", workflow)


if __name__ == "__main__":
    unittest.main()
