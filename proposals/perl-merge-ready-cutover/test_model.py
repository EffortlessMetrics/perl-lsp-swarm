import dataclasses
import json
import hashlib
from pathlib import Path
import subprocess
import unittest

from model import (Event, Subject, accepts, admit, group, material, result_name,
                   readiness, budget, remaining_budget)


class CutoverFixtures(unittest.TestCase):
    def setUp(self):
        self.event = Event("labeled", "merge-ready")
        self.subject = Subject("EffortlessMetrics/perl-lsp-swarm", 1,
                               "a" * 40, "b" * 40, "c" * 40,
                               "d" * 40, "e" * 40, 1, 1)
        self.args = dict(e=self.event, expected=self.subject, receipt=self.subject,
                         results={"compile": "success", "ripr": "success"},
                         required_workers=["compile", "ripr"],
                         merge_parents=(self.subject.base, self.subject.head),
                         checkout=(self.subject.merge, self.subject.tree))
        self.live = dict(live_head=self.subject.head, live_base=self.subject.base,
                         live_label=True)

    def test_control_qualifies(self):
        self.assertTrue(accepts(**self.args))

    def test_event_fixtures(self):
        for case in json.loads(Path(__file__).with_name("events.json").read_text()):
            with self.subTest(case=case["name"]):
                event = Event(**case["event"])
                self.assertEqual(admit(event), case["broad"])
                self.assertEqual(material(event), case["required_identity"])

    def test_ignored_labels_do_not_cancel_or_replace_pending(self):
        # Model native pending-slot replacement, even cancel-in-progress:false.
        pending = {group(self.event, "CI"): "qualification"}
        for action in ("labeled", "unlabeled"):
            other = Event(action, "documentation", run=2)
            pending[group(other, "CI")] = "ignored"
            self.assertEqual(pending[group(self.event, "CI")], "qualification")
            self.assertNotEqual(result_name(other, "Compile All Targets (bit-rot guard)"),
                                "Compile All Targets (bit-rot guard)")

    def test_material_events_share_cancellation_domain(self):
        for action in ("synchronize", "converted_to_draft", "unlabeled"):
            self.assertEqual(group(Event(action, "merge-ready"), "CI"),
                             group(self.event, "CI"))
        self.assertNotEqual(group(self.event, "CI"), group(self.event, "ripr"))

    def test_no_skipped_missing_or_partial_worker_can_pass(self):
        for result in ("skipped", "cancelled", "failure", "neutral", "timed_out", ""):
            self.assertFalse(accepts(**(self.args | {"results": {"compile": result,
                                                                "ripr": "success"}})))
        self.assertFalse(accepts(**(self.args | {"results": {}})))
        self.assertFalse(accepts(**(self.args | {"required_workers": []})))

    def test_source_binding_each_axis(self):
        for field in dataclasses.fields(self.subject):
            old = getattr(self.subject, field.name)
            value = old + 1 if isinstance(old, int) else "wrong"
            receipt = dataclasses.replace(self.subject, **{field.name: value})
            with self.subTest(axis=field.name):
                self.assertFalse(accepts(**(self.args | {"receipt": receipt})))

    def test_source_and_instrument_fail_closed(self):
        mutations = [dict(merge_parents=()),
                     dict(checkout=(self.subject.head, self.subject.tree)),
                     dict(checkout=(self.subject.merge, "wrong")),
                     dict(run_cancelled=True)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.assertFalse(accepts(**(self.args | mutation)))

    def test_final_live_state_even_when_head_base_label_unchanged(self):
        for mutation in (dict(live_draft=True), dict(live_state="closed"),
                         dict(live_base_ref="owner-branch"), dict(live_label=False),
                         dict(live_base_ref="master")):
            self.assertEqual(readiness(True, self.subject, **(self.live | mutation)),
                             "NOT_READY")
            self.assertTrue(accepts(**self.args))

    def test_base_advance_preserves_completed_evidence_for_affected_reassessment(self):
        self.assertTrue(accepts(**self.args))
        self.assertEqual(readiness(True, self.subject,
                                  **(self.live | dict(live_base="new"))),
                         "BASE_ADVANCED_REASSESS")
        self.assertEqual(readiness(True, self.subject, **self.live),
                         "READY_FOR_REMAINING_LIVE_GATES")

    def test_budget_failure_is_not_false_proof_or_automatic_repeat(self):
        for elapsed, verdict in ((1200, "WITHIN_BUDGET"), (1201, "BUDGET_EXCEEDED"),
                                 (-1, "NOT_MEASURED")):
            self.assertEqual(budget(elapsed), verdict)
            self.assertTrue(accepts(**self.args))
        self.assertEqual(remaining_budget(1201), 0)
        self.assertEqual(remaining_budget(1190), 10)

    def test_event_cannot_borrow_another_run_or_pr(self):
        for field in ("pr", "run", "attempt"):
            event = dataclasses.replace(self.event, **{field: 2})
            self.assertFalse(accepts(**(self.args | {"e": event})))
        for field, value in (("repository", "other/repo"), ("base_ref", "master")):
            event = dataclasses.replace(self.event, **{field: value})
            self.assertFalse(accepts(**(self.args | {"e": event})))

    def test_malformed_expected_source_is_not_exact_identity(self):
        for field in ("head", "base", "merge", "tree", "workflow"):
            subject = dataclasses.replace(self.subject, **{field: "main"})
            self.assertFalse(accepts(**(self.args | {"expected": subject,
                                                       "receipt": subject})))

    def test_inventory_pins_all_five_result_owners(self):
        inventory = json.loads(Path(__file__).with_name("inventory.json").read_text())
        self.assertEqual(len(inventory["workflows"]), 103)
        self.assertEqual({r["name"] for r in inventory["required_results"]}, {
            "Perl LSP Rust Small Result", "ripr+ New Gap Gate",
            "Compile All Targets (bit-rot guard)", "Conflict marker check", "validate-title"})

    def test_inventory_matches_pinned_source_bytes(self):
        inventory = json.loads(Path(__file__).with_name("inventory.json").read_text())
        for workflow in inventory["workflows"]:
            source = subprocess.check_output(
                ["git", "show", inventory["source_commit"] + ":" + workflow["path"]])
            self.assertEqual(hashlib.sha256(source).hexdigest(), workflow["sha256"])


if __name__ == "__main__":
    unittest.main()
