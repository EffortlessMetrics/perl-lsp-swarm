#!/usr/bin/env python3
from __future__ import annotations

import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("check_release_settings_closeout.py")
SPEC = importlib.util.spec_from_file_location("check_release_settings_closeout", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"unable to load {MODULE_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

ROOT = Path(__file__).resolve().parents[2]
RECEIPT = ROOT / ".ci/security/release-settings-closeout.json"
MARKDOWN = ROOT / "docs/security/release-settings-closeout.md"
EVIDENCE_URL = (
    "https://github.com/EffortlessMetrics/perl-lsp-swarm/"
    "issues/4145#issuecomment-1"
)


class ReleaseSettingsCloseoutTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.valid = MODULE.load(RECEIPT)

    def assert_invalid(self, data: dict, needle: str) -> None:
        with self.assertRaisesRegex(MODULE.ReceiptError, needle):
            MODULE.validate(data)

    def test_checked_in_packet_is_truthful_and_current(self) -> None:
        self.assertEqual(MODULE.validate(self.valid), "not_proven")
        self.assertEqual(MODULE.render(self.valid), MARKDOWN.read_text(encoding="utf-8"))

    def test_primary_channel_cannot_be_silently_deferred(self) -> None:
        data = copy.deepcopy(self.valid)
        data["channel_dispositions"][0]["disposition"] = "deferred"
        data["channel_dispositions"][0]["limitation"] = "later"
        self.assert_invalid(data, "primary release channels")

    def test_conditional_channel_cannot_disappear(self) -> None:
        data = copy.deepcopy(self.valid)
        data["channel_dispositions"].pop()
        self.assert_invalid(data, "cover exactly")

    def test_checklist_without_evidence_cannot_be_proven(self) -> None:
        data = copy.deepcopy(self.valid)
        control = data["settings"]["immutable_releases"]
        control.update({"state": "proven", "enabled": True, "limitation": None})
        self.assert_invalid(data, "requires live evidence")

    def test_issue_reference_alone_is_not_live_settings_evidence(self) -> None:
        data = copy.deepcopy(self.valid)
        control = data["settings"]["immutable_releases"]
        control.update(
            {
                "state": "proven",
                "enabled": True,
                "evidence_refs": ["#4145"],
                "limitation": None,
            }
        )
        self.assert_invalid(data, "requires a durable GitHub URL")

    def test_tag_ruleset_proof_requires_active_enforcement_and_bypass_review(self) -> None:
        data = copy.deepcopy(self.valid)
        control = data["settings"]["tag_ruleset"]
        control.update(
            {
                "state": "proven",
                "enforcement": "disabled",
                "administrator_bypass_reviewed": False,
                "evidence_refs": [EVIDENCE_URL],
                "limitation": None,
            }
        )
        self.assert_invalid(data, "proven tag ruleset must be active")

    def test_environment_proof_requires_reviewer_policy_and_secret_scope(self) -> None:
        data = copy.deepcopy(self.valid)
        environment = data["settings"]["environments"][0]
        environment.update(
            {
                "state": "proven",
                "evidence_refs": [EVIDENCE_URL],
                "limitation": None,
            }
        )
        self.assert_invalid(data, "requires a human reviewer")

    def test_actions_proof_rejects_write_default_or_fork_write_tokens(self) -> None:
        data = copy.deepcopy(self.valid)
        control = data["settings"]["actions_policy"]
        control.update(
            {
                "state": "proven",
                "default_workflow_permissions": "write",
                "fork_pull_request_write_tokens": True,
                "workflow_pr_creation_and_approval": True,
                "evidence_refs": [EVIDENCE_URL],
                "limitation": None,
            }
        )
        self.assert_invalid(data, "read-only default permissions")

    def test_codeowners_proof_requires_complete_release_surface(self) -> None:
        data = copy.deepcopy(self.valid)
        control = data["settings"]["codeowners"]
        control.update(
            {
                "state": "proven",
                "covered_surfaces": ["release_workflows"],
                "evidence_refs": [EVIDENCE_URL],
                "limitation": None,
            }
        )
        self.assert_invalid(data, "proven CODEOWNERS must cover exactly")

    def test_declared_proven_cannot_outrun_computed_state(self) -> None:
        data = copy.deepcopy(self.valid)
        data["declared_overall_state"] = "proven"
        self.assert_invalid(data, "contradicts computed state")

    def test_failed_required_control_makes_packet_failed(self) -> None:
        data = copy.deepcopy(self.valid)
        control = data["settings"]["immutable_releases"]
        control.update({"state": "failed", "limitation": "live setting is disabled"})
        data["declared_overall_state"] = "failed"
        self.assertEqual(MODULE.validate(data), "failed")

    def test_write_check_round_trip_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            receipt = root / "receipt.json"
            markdown = root / "receipt.md"
            receipt.write_text(json.dumps(self.valid, indent=2) + "\n", encoding="utf-8")
            MODULE.check_or_write(receipt, markdown, True)
            first = markdown.read_bytes()
            MODULE.check_or_write(receipt, markdown, True)
            self.assertEqual(first, markdown.read_bytes())
            MODULE.check_or_write(receipt, markdown, False)


if __name__ == "__main__":
    unittest.main()
