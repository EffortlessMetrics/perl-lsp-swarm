from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
CLIENT_ROOT = PACKAGE_ROOT.parent
VALIDATOR_PATH = CLIENT_ROOT / "validate_sublime_command_receipt.py"
SCHEMA_PATH = CLIENT_ROOT / "sublime-command-receipt.v1.schema.json"


def load_validator():
    spec = importlib.util.spec_from_file_location("sublime_command_receipt_validator", VALIDATOR_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("unable to load Sublime command receipt validator")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sample_receipt() -> dict:
    validator = load_validator()
    return {
        "schema_version": 1,
        "stage": "exact_source_local",
        "source_sha": "a" * 40,
        "recorded_at": "2026-08-13T11:00:00+00:00",
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
        "command": {
            "action": "workspace_trust_report",
            "id": "perl.workspaceTrustReport",
            "session": "LSP-perllsp",
            "result_surface": "output.perllsp",
        },
        "assertions": {name: True for name in validator.REQUIRED_ASSERTIONS},
    }


class SublimeCommandReceiptTests(unittest.TestCase):
    def test_validator_accepts_complete_receipt(self) -> None:
        load_validator().validate(sample_receipt())

    def test_validator_rejects_wrong_session_and_public_overclaim(self) -> None:
        wrong_session = sample_receipt()
        wrong_session["command"]["session"] = "another-server"
        with self.assertRaisesRegex(ValueError, "wrong session"):
            load_validator().validate(wrong_session)

        public = sample_receipt()
        public["stage"] = "package_control_public"
        with self.assertRaisesRegex(ValueError, "exact_source_local"):
            load_validator().validate(public)

    def test_validator_rejects_receipt_from_failed_command(self) -> None:
        # The output panel renders the same caption for a served report and for
        # a JSON-RPC or application failure, so a receipt whose journey observed
        # a failure must not validate.
        failed = sample_receipt()
        failed["assertions"]["command_reported_success"] = False
        with self.assertRaisesRegex(ValueError, "command_reported_success"):
            load_validator().validate(failed)

        missing = sample_receipt()
        del missing["assertions"]["command_reported_success"]
        with self.assertRaisesRegex(ValueError, "command_reported_success"):
            load_validator().validate(missing)

    def test_schema_and_validator_require_the_same_assertions(self) -> None:
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        schema_assertions = schema["properties"]["assertions"]
        self.assertEqual(
            set(schema_assertions["required"]),
            set(load_validator().REQUIRED_ASSERTIONS),
        )
        self.assertEqual(
            set(schema_assertions["properties"]),
            set(load_validator().REQUIRED_ASSERTIONS),
        )

    def test_schema_is_pinned_to_preview_safe_command_boundary(self) -> None:
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        self.assertEqual(schema["properties"]["stage"]["const"], "exact_source_local")
        command = schema["properties"]["command"]["properties"]
        self.assertEqual(command["id"]["const"], "perl.workspaceTrustReport")
        self.assertNotIn("perl.safeDeleteSymbol", SCHEMA_PATH.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
