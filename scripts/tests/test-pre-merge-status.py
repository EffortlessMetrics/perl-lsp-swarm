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
        "pr": {"headRefOid": HEAD, "statusCheckRollup": [
            {"name": "Required A", "conclusion": "SUCCESS", "detailsUrl": "https://example.test/required"}]},
        "rules": [{"type": "required_status_checks", "parameters": {
            "required_status_checks": [{"context": "Required A"}]}}],
        "classic": {"contexts": [], "checks": []},
        "required_checks": [{"name": "Required A", "state": "SUCCESS"}],
    }


def red(data, name="PR Smoke (Fast Feedback, advisory)", url=URL):
    data["pr"]["statusCheckRollup"].append(
        {"name": name, "conclusion": "FAILURE", "detailsUrl": url})
    return {"name": name, "detailsUrl": url, "classification": "inherited",
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
        data["pr"]["statusCheckRollup"] = []
        self.assertIn("required context missing", run(data).stderr)

    def test_failed_required_context_blocks_even_with_advisory_evidence(self):
        data = fixture()
        data["pr"]["statusCheckRollup"][0]["conclusion"] = "FAILURE"
        data["required_checks"][0]["state"] = "FAILURE"
        self.assertIn("required context is not successful", run(data).stderr)

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
        inherited = red(data)
        stable_url = "https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/36551987082/job/109352098183"
        uncertain = red(data, "Current-source Linux smoke (stable)", stable_url)
        uncertain.pop("mergeBaseRunUrl")
        uncertain["classification"] = "not_proven_nonmaterial"
        uncertain["discriminator"] = "stable VS Code host-resolution lane has no matching merge-base proof; changed launcher paths do not reach this lane"
        uncertain["nonmaterialReason"] = "stable VS Code host resolution is outside the four changed CLI launcher paths"
        data["evidence"] = {"headRefOid": HEAD, "advisories": [inherited, uncertain]}
        self.assertEqual(run(data).returncode, 0)
        bad = copy.deepcopy(data)
        bad["evidence"]["advisories"][1].pop("nonmaterialReason")
        self.assertNotEqual(run(bad).returncode, 0)


if __name__ == "__main__":
    unittest.main()
