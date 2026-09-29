#!/usr/bin/env python3
"""Fixture tests for the required/advisory half of the pre-merge guard."""

import copy
import json
import subprocess
import sys
import unittest
from pathlib import Path


CHECKER = Path(__file__).resolve().parents[1] / "ci" / "classify-pre-merge-status.py"
HEAD = "6129bf77dddf9ccc4b483936cb18d74bd0d64433"
URL = "https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/36551986994/job/109352822130"


def fixture():
    return {
        "pr": {"headRefOid": HEAD},
        "checks": [{"name": "Required A", "state": "SUCCESS", "link": "https://example.test/required"}],
        "rules": [{"type": "required_status_checks", "parameters": {
            "required_status_checks": [{"context": "Required A"}]}}],
        "classic": {"contexts": [], "checks": []},
        "required_checks": [{"name": "Required A", "state": "SUCCESS", "link": "https://example.test/required"}],
    }


def red(data, name="PR Smoke (Fast Feedback, advisory)", url=URL):
    data["checks"].append(
        {"name": name, "state": "FAILURE", "link": url})
    return {"name": name, "link": url, "classification": "inherited",
            "discriminator": "same gate and failure signature on merge-base run",
            "evidenceUrl": url,
            "mergeBaseRunUrl": "https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/36545615698"}


def run(data):
    return subprocess.run([sys.executable, str(CHECKER)], input=json.dumps(data),
                          text=True, capture_output=True, check=False)


class StatusFixtures(unittest.TestCase):
    def test_all_green_current_contexts(self):
        self.assertEqual(run(fixture()).returncode, 0)

    def test_missing_required_context_fails_closed(self):
        data = fixture()
        data["checks"] = []
        self.assertIn("required context missing", run(data).stderr)

    def test_failed_required_context_blocks_even_with_advisory_evidence(self):
        data = fixture()
        data["checks"][0]["state"] = "FAILURE"
        data["required_checks"][0]["state"] = "FAILURE"
        self.assertIn("required context is not successful", run(data).stderr)

    def test_conflicting_required_results_cannot_hide_failure(self):
        data = fixture()
        data["checks"].append({"name": "Required A", "state": "FAILURE", "link": "https://example.test/older"})
        self.assertIn("ambiguous current results", run(data).stderr)

    def test_running_check_is_controlled_not_proven(self):
        data = fixture()
        data["checks"].append({"name": "Running advisory", "state": None, "link": "https://example.test/running"})
        result = run(data)
        self.assertEqual(result.returncode, 1)
        self.assertIn("advisory result pending or unknown", result.stderr)
        self.assertNotIn("Traceback", result.stderr)

    def test_classic_policy_is_additive(self):
        data = fixture()
        data["classic"]["contexts"] = ["Classic B"]
        self.assertNotEqual(run(data).returncode, 0)

    def test_candidate_owned_advisory_regression_blocks(self):
        data = fixture()
        entry = red(data)
        entry["classification"] = "candidate-owned"
        data["evidence"] = {"headRefOid": HEAD, "advisories": [entry]}
        self.assertIn("candidate-owned", run(data).stderr)

    def test_inherited_advisory_with_merge_base_evidence_passes(self):
        data = fixture()
        data["evidence"] = {"headRefOid": HEAD, "advisories": [red(data)]}
        self.assertEqual(run(data).returncode, 0)

    def test_unclassified_advisory_and_stale_head_fail(self):
        data = fixture()
        entry = red(data)
        entry["classification"] = "unclassified"
        data["evidence"] = {"headRefOid": HEAD, "advisories": [entry]}
        self.assertNotEqual(run(data).returncode, 0)
        entry["classification"] = "inherited"
        data["evidence"]["headRefOid"] = "older-head"
        self.assertNotEqual(run(data).returncode, 0)

    def test_each_live_advisory_red_requires_one_exact_identity(self):
        data = fixture()
        entry = red(data)
        data["evidence"] = {"headRefOid": HEAD, "advisories": [entry, copy.deepcopy(entry)]}
        self.assertIn("duplicate advisory evidence", run(data).stderr)
        data["evidence"]["advisories"] = []
        self.assertIn("advisory evidence identities differ", run(data).stderr)

    def test_16939_mixed_advisory_evidence_preserves_not_proven(self):
        data = fixture()
        required = [
            ("ripr+ New Gap Gate", "36551986972/job/109396113490"),
            ("validate-title", "36558049300/job/109371886643"),
            ("Perl LSP Rust Small Result", "36551986899/job/109356771370"),
            ("Compile All Targets (bit-rot guard)", "36551986994/job/109352822178"),
            ("Conflict marker check", "36551986994/job/109352822207"),
        ]
        base = "https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/"
        data["rules"][0]["parameters"]["required_status_checks"] = [{"context": name} for name, _ in required]
        data["checks"] = [{"name": name, "state": "SUCCESS", "link": base + path} for name, path in required]
        data["required_checks"] = copy.deepcopy(data["checks"])
        # Current gh pr checks snapshot from merged #16939, head 6129bf77.
        # Superseded failed/cancelled attempts in statusCheckRollup are absent.
        failed = [
            ("CI Gate (Advisory Aggregate)", "36551986994/job/109361447358", "inherited"),
            ("CI Gate shard (lsp)", "36551986994/job/109352894172", "inherited"),
            ("CI Gate shard (meta)", "36551986994/job/109352894241", "inherited"),
            ("CI Gate shard (policy)", "36551986994/job/109352894182", "inherited"),
            ("Public API Surface (facade PR)", "36551986994/job/109352822167", "inherited"),
            ("PR Smoke (Fast Feedback, advisory)", "36551986994/job/109352822130", "not_proven_nonmaterial"),
            ("Current-source Linux smoke (stable)", "36551987082/job/109352098183", "not_proven_nonmaterial"),
        ]
        entries = []
        for name, path, classification in failed:
            entry = red(data, name, base + path)
            entry["classification"] = classification
            if classification == "not_proven_nonmaterial":
                entry.pop("mergeBaseRunUrl")
                entry["nonmaterialReason"] = (
                    "PR Smoke has different first unit failures at head and merge base, both outside four changed launcher paths"
                    if name.startswith("PR Smoke") else
                    "stable VS Code host-resolution failure has no matching merge-base proof and is outside four changed launcher paths"
                )
                entry["discriminator"] = "changed-path and production-route comparison for four CLI launcher paths"
            entries.append(entry)
        self.assertEqual(len(data["checks"]), 12)
        data["evidence"] = {"headRefOid": HEAD, "advisories": entries}
        self.assertEqual(run(data).returncode, 0)
        bad = copy.deepcopy(data)
        bad["evidence"]["advisories"][-1].pop("nonmaterialReason")
        self.assertNotEqual(run(bad).returncode, 0)


if __name__ == "__main__":
    unittest.main()
