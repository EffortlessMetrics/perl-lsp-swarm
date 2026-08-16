#!/usr/bin/env python3
"""Falsifiers for the offline GitHub enforcement union model."""

from __future__ import annotations

import copy
import importlib.util
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("reconcile_github_enforcement_snapshot.py")
SPEC = importlib.util.spec_from_file_location("github_enforcement_snapshot", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
model = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(model)

SHA = "a" * 40
POLICY = "b" * 64
SUBJECT = "c" * 64


def static_receipt() -> dict:
    return {
        "schema_version": 2,
        "status": "SUCCESS",
        "subject_sha256": SUBJECT,
        "subjects": {
            "repository_sha": SHA,
            "policy": {"sha256": POLICY},
            "contexts": [
                {
                    "name": "Classic Required",
                    "policy_role": "required",
                    "enforcement": "github-branch-protection",
                },
                {
                    "name": "Ruleset Required",
                    "policy_role": "required",
                    "enforcement": "github-ruleset",
                },
                {
                    "name": "Both Required",
                    "policy_role": "required",
                    "enforcement": "github-branch-protection+ruleset",
                },
                {
                    "name": "Advisory",
                    "policy_role": "advisory",
                    "enforcement": "neither",
                },
            ],
        },
    }


def check(context: str, app_id: int | None = 15368) -> dict:
    return {"context": context, "app_id": app_id}


def ruleset(
    ruleset_id: int,
    *checks: dict,
    enforcement: str = "active",
    targets_default_branch: bool = True,
) -> dict:
    return {
        "id": ruleset_id,
        "name": f"ruleset-{ruleset_id}",
        "target": "branch",
        "enforcement": enforcement,
        "targets_default_branch": targets_default_branch,
        "bypass_actors": [
            {
                "actor_type": "OrganizationAdmin",
                "actor_id": None,
                "bypass_mode": "always",
            }
        ],
        "required_status_checks": list(checks),
    }


def snapshot() -> dict:
    return {
        "schema_version": 1,
        "repository": {
            "full_name": "EffortlessMetrics/perl-lsp-swarm",
            "repository_id": 1244101844,
            "default_branch": "main",
            "branch_sha": SHA,
            "observed_at": "2026-08-16T00:00:00Z",
        },
        "observation": {
            "source": "fixture",
            "permission": "complete",
            "limitations": [],
        },
        "static_contract": {
            "subject_sha256": SUBJECT,
            "policy_sha256": POLICY,
            "repository_sha": SHA,
        },
        "classic_branch_protection": {
            "instrument_state": "observed",
            "branch": "main",
            "required_status_checks": [
                check("Classic Required"),
                check("Both Required"),
            ],
        },
        "rulesets": {
            "instrument_state": "observed",
            "items": [
                ruleset(
                    16664791,
                    check("Ruleset Required"),
                    check("Both Required"),
                ),
                ruleset(
                    2,
                    check("Excluded Inactive"),
                    enforcement="evaluate",
                ),
                ruleset(
                    3,
                    check("Excluded Untargeted"),
                    targets_default_branch=False,
                ),
            ],
        },
    }


class EnforcementSnapshotTests(unittest.TestCase):
    def test_complete_matching_union(self) -> None:
        receipt = model.reconcile(snapshot(), static_receipt())
        self.assertEqual(receipt["status"], "MATCH")
        self.assertEqual(receipt["differences"], [])
        by_name = {row["context"]: row for row in receipt["live_union"]}
        self.assertEqual(by_name["Both Required"]["source_class"], "both")
        self.assertEqual(
            [row["id"] for row in receipt["excluded_rulesets"]],
            [2, 3],
        )

    def test_classic_only_observation_is_not_proven(self) -> None:
        candidate = snapshot()
        candidate["rulesets"]["instrument_state"] = "unreadable"
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertTrue(
            any(
                row["code"] == "rulesets_not_observed"
                for row in receipt["limitations"]
            )
        )

    def test_ruleset_only_observation_is_not_proven(self) -> None:
        candidate = snapshot()
        candidate["classic_branch_protection"]["instrument_state"] = "unreadable"
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertTrue(
            any(
                row["code"] == "classic_branch_protection_not_observed"
                for row in receipt["limitations"]
            )
        )

    def test_missing_live_context_is_drift(self) -> None:
        candidate = snapshot()
        candidate["rulesets"]["items"][0]["required_status_checks"] = [
            check("Both Required")
        ]
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "DRIFT")
        self.assertIn(
            "Ruleset Required",
            {
                row["context"]
                for row in receipt["differences"]
                if row["code"] == "policy_context_missing_live"
            },
        )

    def test_extra_live_context_is_drift(self) -> None:
        candidate = snapshot()
        candidate["classic_branch_protection"]["required_status_checks"].append(
            check("Unexpected Live")
        )
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "DRIFT")
        self.assertTrue(
            any(
                row["code"] == "live_context_missing_from_policy"
                and row["context"] == "Unexpected Live"
                for row in receipt["differences"]
            )
        )

    def test_wrong_enforcement_source_is_drift(self) -> None:
        candidate = snapshot()
        candidate["classic_branch_protection"]["required_status_checks"] = [
            check("Both Required")
        ]
        candidate["rulesets"]["items"][0]["required_status_checks"].append(
            check("Classic Required")
        )
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "DRIFT")
        self.assertTrue(
            any(
                row["code"] == "enforcement_source_mismatch"
                and row["context"] == "Classic Required"
                for row in receipt["differences"]
            )
        )

    def test_stale_policy_digest_is_not_proven(self) -> None:
        candidate = snapshot()
        candidate["static_contract"]["policy_sha256"] = "d" * 64
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertTrue(
            any(
                row["code"] == "policy_digest_mismatch"
                for row in receipt["limitations"]
            )
        )

    def test_cross_sha_snapshot_is_not_proven(self) -> None:
        candidate = snapshot()
        candidate["repository"]["branch_sha"] = "d" * 40
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertTrue(
            any(
                row["code"] == "branch_sha_mismatch"
                for row in receipt["limitations"]
            )
        )

    def test_same_context_in_both_surfaces_is_not_duplicate_error(self) -> None:
        receipt = model.reconcile(snapshot(), static_receipt())
        both = next(
            row for row in receipt["live_union"] if row["context"] == "Both Required"
        )
        self.assertEqual(both["sources"], ["classic", "ruleset"])
        self.assertEqual(receipt["status"], "MATCH")

    def test_app_identity_is_retained_and_compared_when_declared(self) -> None:
        static = static_receipt()
        static["subjects"]["contexts"][0]["app_id"] = 42
        receipt = model.reconcile(snapshot(), static)
        self.assertEqual(receipt["status"], "DRIFT")
        self.assertTrue(
            any(
                row["code"] == "app_identity_mismatch"
                and row["context"] == "Classic Required"
                for row in receipt["differences"]
            )
        )

    def test_unknown_snapshot_field_fails_closed(self) -> None:
        candidate = snapshot()
        candidate["surprise"] = True
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertEqual(receipt["limitations"][0]["code"], "invalid_input")

    def test_input_order_does_not_change_semantic_receipt(self) -> None:
        left = snapshot()
        right = copy.deepcopy(left)
        right["classic_branch_protection"]["required_status_checks"].reverse()
        right["rulesets"]["items"].reverse()
        left_receipt = model.reconcile(left, static_receipt())
        right_receipt = model.reconcile(right, static_receipt())
        self.assertEqual(
            left_receipt["snapshot_sha256"],
            right_receipt["snapshot_sha256"],
        )
        self.assertEqual(left_receipt["live_union"], right_receipt["live_union"])
        self.assertEqual(left_receipt["differences"], right_receipt["differences"])


if __name__ == "__main__":
    unittest.main()
