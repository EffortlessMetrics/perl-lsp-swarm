#!/usr/bin/env python3
"""Falsifiers for the offline GitHub enforcement union model."""

from __future__ import annotations

import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name(
    "reconcile_github_enforcement_snapshot.py"
)
SPEC = importlib.util.spec_from_file_location(
    "github_enforcement_snapshot", MODULE_PATH
)
assert SPEC is not None and SPEC.loader is not None
model = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(model)

SHA = "a" * 40
POLICY = "b" * 64
SUBJECT = "c" * 64
CLASSIC_DIGEST = "d" * 64
RULESET_LIST_DIGEST = "e" * 64


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
                    "producer": "repository-job",
                },
                {
                    "name": "Ruleset Required",
                    "policy_role": "required",
                    "enforcement": "github-ruleset",
                    "producer": "repository-job",
                },
                {
                    "name": "Both Required",
                    "policy_role": "required",
                    "enforcement": (
                        "github-branch-protection+ruleset"
                    ),
                    "producer": "repository-job",
                },
                {
                    "name": "Advisory",
                    "policy_role": "advisory",
                    "enforcement": "neither",
                    "producer": "repository-job",
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
    include: list[str] | None = None,
    exclude: list[str] | None = None,
    bypass_actors: list[dict] | None = None,
) -> dict:
    return {
        "id": ruleset_id,
        "name": f"ruleset-{ruleset_id}",
        "target": "branch",
        "source_type": "Repository",
        "source": "EffortlessMetrics/perl-lsp-swarm",
        "enforcement": enforcement,
        "detail_response_sha256": f"{ruleset_id:064x}",
        "conditions": {
            "ref_name": {
                "include": (
                    ["~DEFAULT_BRANCH"] if include is None else include
                ),
                "exclude": [] if exclude is None else exclude,
            }
        },
        "bypass_actors": (
            [
                {
                    "actor_type": "OrganizationAdmin",
                    "actor_id": None,
                    "bypass_mode": "always",
                }
            ]
            if bypass_actors is None
            else bypass_actors
        ),
        "strict_required_status_checks_policy": False,
        "do_not_enforce_on_create": False,
        "required_status_checks": list(checks),
    }


def snapshot(*, source: str = "trusted_default_branch") -> dict:
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
            "source": source,
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
            "response_sha256": CLASSIC_DIGEST,
            "branch": "main",
            "strict": True,
            "required_status_checks": [
                check("Classic Required"),
                check("Both Required"),
            ],
        },
        "rulesets": {
            "instrument_state": "observed",
            "list_response_sha256": RULESET_LIST_DIGEST,
            "items": [
                ruleset(
                    16664791,
                    check("Ruleset Required"),
                    check("Both Required"),
                ),
                ruleset(
                    2,
                    check("Excluded Inactive", None),
                    enforcement="evaluate",
                ),
                ruleset(
                    3,
                    check("Excluded Untargeted", None),
                    include=["refs/heads/release"],
                ),
            ],
        },
    }


class EnforcementSnapshotTests(unittest.TestCase):
    def test_complete_matching_union(self) -> None:
        receipt = model.reconcile(snapshot(), static_receipt())
        self.assertEqual(receipt["status"], "MATCH")
        self.assertEqual(receipt["differences"], [])
        by_name = {
            row["context"]: row for row in receipt["live_union"]
        }
        self.assertEqual(
            by_name["Both Required"]["source_class"], "both"
        )
        self.assertEqual(
            [
                (row["id"], row["reason"])
                for row in receipt["excluded_rulesets"]
            ],
            [(2, "inactive"), (3, "untargeted")],
        )

    def test_fixture_source_cannot_establish_live_match(self) -> None:
        receipt = model.reconcile(
            snapshot(source="fixture"), static_receipt()
        )
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertTrue(
            any(
                row["code"] == "non_live_observation_source"
                for row in receipt["limitations"]
            )
        )

    def test_connector_source_can_feed_the_closed_contract(self) -> None:
        receipt = model.reconcile(
            snapshot(source="connector"), static_receipt()
        )
        self.assertEqual(receipt["status"], "MATCH")

    def test_classic_only_observation_is_not_proven(self) -> None:
        candidate = snapshot()
        candidate["rulesets"] = {
            "instrument_state": "unreadable",
            "list_response_sha256": None,
            "items": [],
        }
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
        candidate["classic_branch_protection"] = {
            "instrument_state": "unreadable",
            "response_sha256": None,
            "branch": "main",
            "strict": None,
            "required_status_checks": [],
        }
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertTrue(
            any(
                row["code"]
                == "classic_branch_protection_not_observed"
                for row in receipt["limitations"]
            )
        )

    def test_nonobserved_surface_cannot_carry_stale_rows(self) -> None:
        candidate = snapshot()
        candidate["rulesets"]["instrument_state"] = "unreadable"
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertEqual(
            receipt["limitations"][0]["code"], "invalid_input"
        )

    def test_observed_surface_requires_response_digest(self) -> None:
        candidate = snapshot()
        candidate["classic_branch_protection"][
            "response_sha256"
        ] = None
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertIn(
            "requires response_sha256",
            receipt["limitations"][0]["message"],
        )

    def test_missing_live_context_is_drift(self) -> None:
        candidate = snapshot()
        candidate["rulesets"]["items"][0][
            "required_status_checks"
        ] = [check("Both Required", None)]
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

    def test_extra_live_context_absent_from_policy_is_drift(self) -> None:
        candidate = snapshot()
        candidate["classic_branch_protection"][
            "required_status_checks"
        ].append(check("Unexpected Live"))
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "DRIFT")
        self.assertTrue(
            any(
                row["code"] == "live_context_missing_from_policy"
                and row["context"] == "Unexpected Live"
                for row in receipt["differences"]
            )
        )

    def test_live_advisory_context_is_role_mismatch_not_missing(self) -> None:
        candidate = snapshot()
        candidate["classic_branch_protection"][
            "required_status_checks"
        ].append(check("Advisory"))
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "DRIFT")
        self.assertTrue(
            any(
                row["code"] == "live_context_role_mismatch"
                and row["context"] == "Advisory"
                and row["expected"] == "advisory"
                for row in receipt["differences"]
            )
        )

    def test_wrong_enforcement_source_is_drift(self) -> None:
        candidate = snapshot()
        candidate["classic_branch_protection"][
            "required_status_checks"
        ] = [check("Both Required")]
        candidate["rulesets"]["items"][0][
            "required_status_checks"
        ].append(check("Classic Required", None))
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
        candidate["static_contract"]["policy_sha256"] = "f" * 64
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
        candidate["repository"]["branch_sha"] = "f" * 40
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertTrue(
            any(
                row["code"] == "branch_sha_mismatch"
                for row in receipt["limitations"]
            )
        )

    def test_same_context_in_both_surfaces_is_not_duplicate_error(
        self,
    ) -> None:
        receipt = model.reconcile(snapshot(), static_receipt())
        both = next(
            row
            for row in receipt["live_union"]
            if row["context"] == "Both Required"
        )
        self.assertEqual(
            both["sources"], ["classic", "ruleset"]
        )
        self.assertEqual(
            both["source_bindings"],
            [
                {
                    "source": "classic",
                    "app_ids": [15368],
                    "ruleset_ids": [],
                },
                {
                    "source": "ruleset",
                    "app_ids": [15368],
                    "ruleset_ids": [16664791],
                },
            ],
        )
        self.assertEqual(receipt["status"], "MATCH")

    def test_app_identity_is_compared_per_declared_source(self) -> None:
        candidate = snapshot()
        candidate["classic_branch_protection"][
            "required_status_checks"
        ][0] = check("Classic Required", None)
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "DRIFT")
        mismatch = next(
            row
            for row in receipt["differences"]
            if row["code"] == "app_identity_mismatch"
        )
        self.assertEqual(
            mismatch["observed"], {"classic": [None]}
        )

    def test_unknown_snapshot_field_fails_closed(self) -> None:
        candidate = snapshot()
        candidate["surprise"] = True
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertEqual(
            receipt["limitations"][0]["code"], "invalid_input"
        )

    def test_current_default_branch_selector_is_derived_by_p2(
        self,
    ) -> None:
        normalized = model.validate_snapshot(snapshot())
        active = normalized["rulesets"]["items"][0]
        self.assertEqual(active["targeting"]["status"], "TARGETED")
        self.assertEqual(
            active["targeting"]["matched_includes"],
            ["~DEFAULT_BRANCH"],
        )

    def test_active_unsupported_selector_is_not_proven(self) -> None:
        candidate = snapshot()
        candidate["rulesets"]["items"][0]["conditions"][
            "ref_name"
        ] = {
            "include": ["refs/heads/release/*"],
            "exclude": [],
        }
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertTrue(
            any(
                row["code"] == "ruleset_targeting_not_proven"
                for row in receipt["limitations"]
            )
        )

    def test_exact_nonmatching_ref_is_excluded_as_untargeted(
        self,
    ) -> None:
        candidate = snapshot()
        candidate["rulesets"]["items"][0]["conditions"][
            "ref_name"
        ] = {
            "include": ["refs/heads/release"],
            "exclude": [],
        }
        receipt = model.reconcile(candidate, static_receipt())
        self.assertEqual(receipt["status"], "DRIFT")
        self.assertTrue(
            any(
                row["id"] == 16664791
                and row["reason"] == "untargeted"
                for row in receipt["excluded_rulesets"]
            )
        )

    def test_active_ruleset_and_bypass_evidence_remains_in_receipt(
        self,
    ) -> None:
        receipt = model.reconcile(snapshot(), static_receipt())
        active = next(
            row
            for row in receipt["ruleset_inventory"]
            if row["id"] == 16664791
        )
        self.assertEqual(
            active["bypass_actors"][0]["actor_type"],
            "OrganizationAdmin",
        )
        self.assertEqual(
            receipt["evidence_digests"]["ruleset_details"][0][
                "sha256"
            ],
            f"{2:064x}",
        )
        self.assertIn(
            {
                "id": 16664791,
                "sha256": f"{16664791:064x}",
            },
            receipt["evidence_digests"]["ruleset_details"],
        )

    def test_input_order_and_equivalent_utc_time_are_semantic(
        self,
    ) -> None:
        left = snapshot()
        right = copy.deepcopy(left)
        right["repository"]["observed_at"] = (
            "2026-08-15T20:00:00-04:00"
        )
        right["classic_branch_protection"][
            "required_status_checks"
        ].reverse()
        right["rulesets"]["items"].reverse()
        right["rulesets"]["items"][2]["bypass_actors"].reverse()
        left_receipt = model.reconcile(left, static_receipt())
        right_receipt = model.reconcile(right, static_receipt())
        self.assertEqual(
            left_receipt["snapshot_sha256"],
            right_receipt["snapshot_sha256"],
        )
        self.assertEqual(
            left_receipt["live_union"],
            right_receipt["live_union"],
        )
        self.assertEqual(
            left_receipt["differences"],
            right_receipt["differences"],
        )

    def test_malformed_json_still_writes_not_proven_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            snapshot_path = root / "snapshot.json"
            static_path = root / "static.json"
            receipt_path = root / "receipt.json"
            snapshot_path.write_text("{", encoding="utf-8")
            static_path.write_text(
                json.dumps(static_receipt()), encoding="utf-8"
            )
            status = model.main(
                [
                    "reconcile",
                    "--snapshot",
                    str(snapshot_path),
                    "--static-receipt",
                    str(static_path),
                    "--receipt",
                    str(receipt_path),
                ]
            )
            receipt = json.loads(
                receipt_path.read_text(encoding="utf-8")
            )
        self.assertEqual(status, 1)
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertEqual(
            receipt["limitations"][0]["code"], "invalid_input"
        )


if __name__ == "__main__":
    unittest.main()
