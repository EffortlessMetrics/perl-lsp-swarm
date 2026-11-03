#!/usr/bin/env python3
"""Native enqueue transaction controls; GitHub/authority observations are fixtures.

The process tests launch an actual fake gh executable through the production
transport. No test reaches GitHub or establishes installed admission authority.
"""
from __future__ import annotations

import copy
from dataclasses import replace
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from scripts.ci import native_queue as q

ROOT = Path(__file__).resolve().parents[2]
HEAD = "a" * 40
SUBJECT = q.Subject("owner/repo", "R_fixture", 17, HEAD, "main")


def response(*, entry=False, state="OPEN", auto=False, enabled=True, draft=False):
    return {"repository": {"id": "R_fixture", "nameWithOwner": "owner/repo", "pullRequest": {
        "id": "PR_fixture", "number": 17, "state": state, "isDraft": draft,
        "headRefOid": HEAD, "baseRefName": "main", "title": "A reviewed change", "body": "Claim",
        "reviewDecision": "APPROVED", "headRepository": {"id": "R_fixture"}, "stack": None,
        "isInMergeQueue": entry, "isMergeQueueEnabled": enabled,
        "mergeQueue": {"id": "MQ_fixture"} if enabled else None,
        "autoMergeRequest": {"enabledAt": "2026-11-02T00:00:00Z"} if auto else None,
        "mergeCommit": {"oid": "d" * 40} if state == "MERGED" else None,
        "mergeQueueEntry": {
            "id": "MQE_fixture", "state": "AWAITING_CHECKS", "headCommit": {"oid": "b" * 40},
            "baseCommit": {"oid": "c" * 40}, "mergeQueue": {"id": "MQ_fixture"},
            "pullRequest": {"id": "PR_fixture", "headRefOid": HEAD},
        } if entry else None,
    }}}


def pr(row):
    return row["repository"]["pullRequest"]


class Api:
    def __init__(self, *observations, uncertain=False, acknowledged="MQE_fixture"):
        self.observations = list(observations)
        self.reads = 0
        self.writes = []
        self.uncertain = uncertain
        self.acknowledged = acknowledged

    def observe(self, subject):
        self.reads += 1
        if not self.observations:
            raise AssertionError("unexpected extra read")
        value = self.observations.pop(0)
        if isinstance(value, Exception):
            raise value
        return copy.deepcopy(value)

    def enqueue(self, identity, head):
        self.writes.append((identity, head))
        if self.uncertain and len(self.writes) == 1:
            raise q.Uncertain("synthetic lost response")
        return self.acknowledged


def admitted(subject, observation):
    # This explicit fixture stands in for the still-uninstalled trusted owner.
    return subject == SUBJECT and observation["subject"] == as_subject()


def as_subject():
    return {"repository": "owner/repo", "repository_node_id": "R_fixture", "pr": 17,
            "expected_head": HEAD, "base_branch": "main"}


class ObservationTests(unittest.TestCase):
    def test_invalid_requested_identity_rejected(self):
        for changes in ({"repository": "owner/repo/other"}, {"repository_node_id": ""},
                        {"pr": True}, {"pr": 0}, {"expected_head": "main"},
                        {"expected_head": HEAD + "\n"}, {"base_branch": ""}):
            with self.subTest(changes=changes), self.assertRaises(q.Refused):
                replace(SUBJECT, **changes)

    def test_no_queue_entry_is_not_pending_qualification(self):
        result = q.observation(SUBJECT, response())
        self.assertEqual(result["status"], "NOT_QUEUED")
        self.assertEqual(result["qualification_result"], "NOT_PROVEN")
        self.assertEqual(result["landing_result"], "NOT_PROVEN")
        self.assertFalse(result["mutation_attempted"])

    def test_deferred_auto_merge_is_not_queue_custody(self):
        self.assertEqual(q.observation(SUBJECT, response(auto=True))["status"], "AUTO_MERGE_ARMED")

    def test_entry_integration_oid_is_not_confused_with_reviewed_head(self):
        result = q.observation(SUBJECT, response(entry=True))
        self.assertEqual(result["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertEqual(result["subject"]["expected_head"], HEAD)
        self.assertEqual(result["entry"]["head_oid"], "b" * 40)
        self.assertEqual(result["qualification_result"], "NOT_PROVEN")
        self.assertNotIn("members", result)

    def test_all_native_entry_states_are_observations_not_passes(self):
        for state in sorted(q.ENTRY_STATES):
            with self.subTest(state=state):
                data = response(entry=True)
                pr(data)["mergeQueueEntry"]["state"] = state
                result = q.observation(SUBJECT, data)
                self.assertEqual(result["entry"]["state"], state)
                self.assertEqual(result["qualification_result"], "NOT_PROVEN")
                self.assertEqual(result["landing_result"], "NOT_PROVEN")

    def test_queued_before_group_generation_can_have_null_commit_observations(self):
        data = response(entry=True)
        pr(data)["mergeQueueEntry"].update(state="QUEUED", headCommit=None, baseCommit=None)
        result = q.observation(SUBJECT, data)
        self.assertIsNone(result["entry"]["head_oid"])
        self.assertIsNone(result["entry"]["base_oid"])

    def test_wrong_repository_and_pr_identities_rejected(self):
        for field, value in (("number", 18), ("number", True), ("id", ""),
                             ("headRefOid", "b" * 40), ("baseRefName", "other"),
                             ("headRepository", {"id": "R_foreign"})):
            data = response()
            pr(data)[field] = value
            with self.subTest(field=field, value=value), self.assertRaises(q.Refused):
                q.observation(SUBJECT, data)
        for field, value in (("id", "R_foreign"), ("nameWithOwner", "other/repo")):
            data = response()
            data["repository"][field] = value
            with self.subTest(field=field), self.assertRaises(q.Refused):
                q.observation(SUBJECT, data)

    def test_incomplete_and_contradictory_observations_reject(self):
        variants = []
        for field in ("isDraft", "isInMergeQueue", "isMergeQueueEnabled", "mergeQueueEntry", "headRefOid"):
            data = response()
            del pr(data)[field]
            variants.append(data)
        for field, value in (("isInMergeQueue", True), ("isDraft", 0), ("state", "PENDING"),
                             ("mergeQueue", None), ("reviewDecision", "CLEAN")):
            data = response()
            pr(data)[field] = value
            variants.append(data)
        for data in variants + [{}, {"repository": None}]:
            with self.subTest(data=data), self.assertRaises(q.Refused):
                q.observation(SUBJECT, data)

    def test_wrong_entry_relationships_and_unknown_state_refuse(self):
        for field, value in (("id", ""), ("state", "SUCCESS"),
                             ("mergeQueue", {"id": "MQ_other"}),
                             ("pullRequest", {"id": "PR_other", "headRefOid": HEAD}),
                             ("pullRequest", {"id": "PR_fixture", "headRefOid": "f" * 40}),
                             ("headCommit", {"oid": "main"})):
            data = response(entry=True)
            pr(data)["mergeQueueEntry"][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(q.Refused):
                q.observation(SUBJECT, data)
        for changes in ({"isInMergeQueue": False}, {"isMergeQueueEnabled": False},
                        {"isDraft": True}, {"state": "CLOSED"}):
            data = response(entry=True)
            pr(data).update(changes)
            with self.subTest(changes=changes), self.assertRaises(q.Refused):
                q.observation(SUBJECT, data)

    def test_merged_observation_requires_oid_but_does_not_verify_landing(self):
        data = response(state="MERGED")
        result = q.observation(SUBJECT, data)
        self.assertEqual(result["status"], "MERGED_OBSERVED")
        self.assertEqual(result["merge_oid"], "d" * 40)
        self.assertEqual(result["landing_result"], "NOT_PROVEN")
        pr(data)["mergeCommit"] = None
        with self.assertRaises(q.Refused):
            q.observation(SUBJECT, data)


class TransactionTests(unittest.TestCase):
    def test_missing_false_and_truthy_authority_never_write(self):
        for authorize in (None, lambda *_: False, lambda *_: "READY", lambda *_: {"allowed": True}):
            api = Api(response(), response(), response(entry=True))
            with self.subTest(authorize=authorize), self.assertRaises(q.Refused):
                q.Adapter(SUBJECT, api).enqueue(authorize=authorize)
            self.assertEqual(api.writes, [])

    def test_disabled_queue_and_draft_do_not_fall_back_to_another_route(self):
        for data in (response(enabled=False), response(draft=True)):
            api = Api(data, data, response(entry=True))
            with self.assertRaises(q.Refused):
                q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
            self.assertEqual(api.writes, [])

    def test_stacked_pr_never_falls_back_to_a_broader_merge_transaction(self):
        data = response()
        pr(data)["stack"] = {"id": "STACK_fixture"}
        api = Api(data, data, response(entry=True))
        with self.assertRaisesRegex(q.Refused, "stack requires"):
            q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
        self.assertEqual(api.writes, [])

    def test_existing_entry_or_terminal_state_never_resubmits(self):
        for data in (response(entry=True), response(auto=True), response(state="MERGED"), response(state="CLOSED")):
            api = Api(data)
            result = q.Adapter(SUBJECT, api).enqueue(authorize=lambda *_: self.fail("not a new admission"))
            self.assertFalse(result["mutation_attempted"])
            self.assertEqual(api.writes, [])

    def test_success_requires_fresh_native_custody_readback(self):
        api = Api(response(), response(), response(entry=True))
        result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
        self.assertEqual(result["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertEqual(api.reads, 3)
        self.assertEqual(api.writes, [("PR_fixture", HEAD)])
        self.assertTrue(result["mutation_attempted"])
        self.assertEqual(result["qualification_result"], "NOT_PROVEN")

    def test_head_changed_during_authority_evaluation_refuses_before_mutation(self):
        moved = response()
        pr(moved)["headRefOid"] = "b" * 40
        api = Api(response(), moved, response(entry=True))
        with self.assertRaisesRegex(q.Refused, "head moved"):
            q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
        self.assertEqual(api.writes, [])

    def test_same_head_observed_metadata_change_requires_fresh_judgment(self):
        for changes in ({"title": "different"}, {"body": "different"},
                        {"reviewDecision": "CHANGES_REQUESTED"}, {"isDraft": True},
                        {"isMergeQueueEnabled": False, "mergeQueue": None}):
            moved = response()
            pr(moved).update(changes)
            api = Api(response(), moved, response(entry=True))
            with self.subTest(changes=changes), self.assertRaises(q.Refused):
                q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
            self.assertEqual(api.writes, [])

    def test_base_tip_change_is_not_itself_a_new_review_or_request(self):
        first, second = response(), response()
        # Deliberately not a pinned admission field: G is composed by GitHub.
        pr(first)["baseRefOid"] = "1" * 40
        pr(second)["baseRefOid"] = "2" * 40
        api = Api(first, second, response(entry=True))
        self.assertEqual(q.Adapter(SUBJECT, api).enqueue(authorize=admitted)["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertEqual(len(api.writes), 1)

    def test_authority_cannot_mutate_the_adapters_saved_native_snapshot(self):
        api = Api(response(), response(), response(entry=True))
        def authority(subject, copy_of_observation):
            copy_of_observation["pr_id"] = "PR_other"
            copy_of_observation["subject"]["expected_head"] = "e" * 40
            return True
        result = q.Adapter(SUBJECT, api).enqueue(authorize=authority)
        self.assertEqual(result["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertEqual(api.writes, [("PR_fixture", HEAD)])

    def test_concurrent_native_enqueue_observed_after_authority_does_not_duplicate(self):
        api = Api(response(), response(entry=True))
        result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
        self.assertEqual(result["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertFalse(result["mutation_attempted"])
        self.assertEqual(api.writes, [])

    def test_acknowledgment_with_no_entry_is_not_accepted_custody(self):
        for after in (response(), response(auto=True), response(state="CLOSED")):
            api = Api(response(), response(), after)
            result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
            self.assertEqual(result["status"], "ENQUEUE_NOT_CONFIRMED")
            self.assertTrue(result["mutation_may_have_occurred"])
            self.assertEqual(len(api.writes), 1)

    def test_lost_response_can_recover_custody_without_retry(self):
        api = Api(response(), response(), response(entry=True), uncertain=True)
        result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
        self.assertEqual(result["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertIsNone(result["acknowledged_entry_id"])
        self.assertEqual(len(api.writes), 1)

    def test_lost_response_and_absent_entry_remain_unknown(self):
        api = Api(response(), response(), response(), uncertain=True)
        result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
        self.assertEqual(result["status"], "ENQUEUE_NOT_CONFIRMED")
        self.assertTrue(result["mutation_may_have_occurred"])
        self.assertEqual(len(api.writes), 1)

    def test_readback_failure_does_not_erase_a_possible_mutation(self):
        for failure in (q.Uncertain("fixture unavailable"), q.Refused("fixture changed")):
            api = Api(response(), response(), failure)
            result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
            self.assertEqual(result["status"], "ENQUEUE_NOT_CONFIRMED")
            self.assertIsNone(result["observation"])
            self.assertTrue(result["mutation_may_have_occurred"])

    def test_changed_entry_after_acknowledgment_remains_unconfirmed(self):
        api = Api(response(), response(), response(entry=True), acknowledged="MQE_previous")
        result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
        self.assertEqual(result["status"], "ENQUEUE_NOT_CONFIRMED")
        self.assertEqual(result["observation"]["entry"]["id"], "MQE_fixture")

    def test_queue_generation_changed_after_mutation_requires_disposition(self):
        after = response(entry=True)
        pr(after)["mergeQueue"]["id"] = "MQ_new"
        pr(after)["mergeQueueEntry"]["mergeQueue"]["id"] = "MQ_new"
        api = Api(response(), response(), after, uncertain=True)
        result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
        self.assertEqual(result["status"], "ENQUEUE_NOT_CONFIRMED")
        self.assertEqual(result["observation"]["queue_id"], "MQ_new")
        self.assertIn("owner must withdraw or disposition", result["next_action"])
        self.assertEqual(len(api.writes), 1)

    def test_same_head_metadata_changed_after_mutation_is_not_cleared(self):
        for field, value in (("title", "Changed title"), ("body", "Changed claim"),
                             ("reviewDecision", "CHANGES_REQUESTED")):
            after = response(entry=True)
            pr(after)[field] = value
            api = Api(response(), response(), after)
            with self.subTest(field=field):
                result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
                self.assertEqual(result["status"], "ENQUEUE_NOT_CONFIRMED")
                self.assertIsNotNone(result["observation"]["entry"])
                self.assertIn("owner must withdraw or disposition", result["next_action"])
                self.assertEqual(len(api.writes), 1)

    def test_fast_native_landing_is_observed_not_declared_qualified(self):
        api = Api(response(), response(), response(state="MERGED"))
        result = q.Adapter(SUBJECT, api).enqueue(authorize=admitted)
        self.assertEqual(result["status"], "MERGED_OBSERVED")
        self.assertEqual(result["landing_result"], "NOT_PROVEN")
        self.assertEqual(result["qualification_result"], "NOT_PROVEN")

    def test_same_adapter_cannot_repeat_mutation_but_can_recover(self):
        api = Api(response(), response(), response(), response(entry=True))
        adapter = q.Adapter(SUBJECT, api)
        adapter.enqueue(authorize=admitted)
        with self.assertRaisesRegex(q.Refused, "already attempted"):
            adapter.enqueue(authorize=admitted)
        self.assertEqual(adapter.recover()["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertEqual(len(api.writes), 1)

    def test_fresh_recovery_is_read_only_even_without_an_entry(self):
        api = Api(response())
        self.assertEqual(q.Adapter(SUBJECT, api).recover()["status"], "NOT_QUEUED")
        self.assertEqual(api.writes, [])


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="queue-gh-")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.bin = self.directory / "bin"
        self.bin.mkdir()
        self.script = self.bin / "gh"
        self.script.write_text("#!" + sys.executable + "\n" + '''import json, os, pathlib, sys
root = pathlib.Path(os.environ["QUEUE_FIXTURE"])
request = json.load(sys.stdin)
with (root / "calls.jsonl").open("a") as stream:
    stream.write(json.dumps({"argv": sys.argv[1:], "request": request}) + "\\n")
items = json.loads((root / "responses.json").read_text())
if not items:
    sys.exit(97)
item = items.pop(0)
(root / "responses.json").write_text(json.dumps(items))
print(item["body"])
print(item.get("stderr", ""), file=sys.stderr)
sys.exit(item.get("exit", 0))
''')
        self.script.chmod(0o700)
        self.environment = {"PATH": str(self.bin), "HOME": self.temp.name,
                            "QUEUE_FIXTURE": self.temp.name, "LC_ALL": "C",
                            "GH_HOST": "not-github.example", "PYTHONDONTWRITEBYTECODE": "1"}
        self.enter = patch.dict(os.environ, self.environment, clear=True)
        self.enter.start()
        self.addCleanup(self.enter.stop)

    def supply(self, *responses):
        rows = [{"body": json.dumps({"data": row})} for row in responses]
        (self.directory / "responses.json").write_text(json.dumps(rows))

    def calls(self):
        path = self.directory / "calls.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_actual_gh_process_uses_only_exact_query_and_expected_head_mutation(self):
        self.supply(response(), response(), {"enqueuePullRequest": {"mergeQueueEntry": {"id": "MQE_fixture"}}},
                    response(entry=True))
        result = q.Adapter(SUBJECT).enqueue(authorize=admitted)
        calls = self.calls()
        self.assertEqual(result["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertEqual(len(calls), 4)
        for call in calls:
            self.assertEqual(call["argv"], ["api", "graphql", "--hostname", "github.com", "--input", "-"])
        self.assertEqual([c["request"]["query"] for c in calls], [q.OBSERVE, q.OBSERVE, q.ENQUEUE, q.OBSERVE])
        self.assertEqual(calls[2]["request"]["variables"], {"input": {
            "pullRequestId": "PR_fixture", "expectedHeadOid": HEAD, "jump": False}})
        self.assertNotIn("enablePullRequestAutoMerge", q.ENQUEUE)
        self.assertNotIn("mergePullRequest", q.ENQUEUE)
        self.assertNotIn("bypass", q.ENQUEUE)

    def test_transport_rejects_other_queries_before_process_launch(self):
        with self.assertRaises(q.Refused):
            q.GitHub()._graphql("mutation { mergePullRequest(input:{}) { clientMutationId } }", {})
        self.assertEqual(self.calls(), [])

    def test_graphql_errors_even_with_data_are_not_success(self):
        for body in ({"errors": [{"message": "PRIVATE"}], "data": response()},
                     {"errors": {}, "data": response()}, {"errors": False, "data": response()},
                     {"errors": "", "data": response()}, {"data": None}, {"data": []}, [], "text"):
            (self.directory / "responses.json").write_text(json.dumps([{"body": json.dumps(body)}]))
            with self.subTest(body=body), self.assertRaises(q.Uncertain) as seen:
                q.GitHub().observe(SUBJECT)
            self.assertNotIn("PRIVATE", str(seen.exception))

    def test_nonzero_gh_does_not_leak_stderr_or_retry(self):
        (self.directory / "responses.json").write_text(json.dumps([
            {"body": "{}", "exit": 1, "stderr": "PRIVATE_TOKEN"}]))
        with self.assertRaises(q.Uncertain) as seen:
            q.GitHub().enqueue("PR_fixture", HEAD)
        self.assertNotIn("PRIVATE_TOKEN", str(seen.exception))
        self.assertEqual(len(self.calls()), 1)

    def test_actual_lost_response_uses_one_mutation_then_readback(self):
        self.supply(response(), response(), {}, response(entry=True))
        path = self.directory / "responses.json"
        rows = json.loads(path.read_text())
        rows[2].update(exit=1, stderr="PRIVATE_TOKEN")
        path.write_text(json.dumps(rows))
        result = q.Adapter(SUBJECT).enqueue(authorize=admitted)
        self.assertEqual(result["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertIsNone(result["acknowledged_entry_id"])
        self.assertEqual(sum(c["request"]["query"] == q.ENQUEUE for c in self.calls()), 1)
        self.assertNotIn("PRIVATE_TOKEN", json.dumps(result))

    def test_timeout_is_unknown_and_has_one_process_attempt(self):
        with patch.object(q.subprocess, "run", side_effect=subprocess.TimeoutExpired("gh", 30)) as run:
            with self.assertRaises(q.Uncertain):
                q.GitHub().enqueue("PR_fixture", HEAD)
        self.assertEqual(run.call_count, 1)

    def test_malformed_duplicate_nonfinite_and_oversize_json_refuse(self):
        for body in ('{', '{"data":{},"data":{}}', '{"data":{"x":NaN}}', " " * (q.MAX_BYTES + 1)):
            (self.directory / "responses.json").write_text(json.dumps([{"body": body}]))
            with self.subTest(length=len(body)), self.assertRaises(q.Uncertain):
                q.GitHub().observe(SUBJECT)

    def test_malformed_acknowledgment_requires_readback(self):
        self.supply(response(), response(), {"enqueuePullRequest": None}, response())
        result = q.Adapter(SUBJECT).enqueue(authorize=admitted)
        self.assertEqual(result["status"], "ENQUEUE_NOT_CONFIRMED")
        self.assertEqual(len(self.calls()), 4)

    def test_cli_recovery_runs_real_transport_without_mutation(self):
        self.supply(response(entry=True))
        result = subprocess.run([sys.executable, "-B", "-m", "scripts.ci.native_queue", "recover",
            "--repository", SUBJECT.repository, "--repository-node-id", SUBJECT.repository_node_id,
            "--pr", str(SUBJECT.pr), "--expected-head", HEAD, "--base-branch", "main"],
            cwd=ROOT, text=True, capture_output=True, timeout=10, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["status"], "QUEUE_ENTRY_OBSERVED")
        self.assertEqual(len(self.calls()), 1)
        self.assertEqual(self.calls()[0]["request"]["query"], q.OBSERVE)

    def test_cli_cannot_install_mutation_authority(self):
        for operation in ("enqueue", "request", "merge", "--authorized"):
            result = subprocess.run([sys.executable, "-B", "-m", "scripts.ci.native_queue", operation],
                cwd=ROOT, text=True, capture_output=True, timeout=10, check=False)
            self.assertEqual(result.returncode, 2)
        self.assertEqual(self.calls(), [])


class WiringTests(unittest.TestCase):
    def test_existing_advisory_test_command_executes_native_queue_controls(self):
        source = (ROOT / ".github/workflows/gate-enforcement-contract.yml").read_text()
        command = source.split("python3 -m unittest", 1)[1].split("test_status=$?", 1)[0]
        self.assertIn("scripts/ci/test_native_queue.py", command)
        self.assertIn("scripts/ci/test_queue_qualification_source.py", command)
        self.assertIn("scripts/ci/test_qualification_cutover_docs.py", command)


if __name__ == "__main__":
    unittest.main()
