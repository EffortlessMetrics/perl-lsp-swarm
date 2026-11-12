"""Discriminating native-subject and real Git controls for the G title leaf."""
from contextlib import redirect_stdout
from copy import deepcopy
from datetime import datetime, timedelta, timezone
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/ci"))
import native_queue_title as title

REPO = "EffortlessMetrics/perl-lsp-swarm"
B, G, H1, H2, T = (letter * 40 for letter in "abcd7")
NOW = datetime(2026, 11, 4, 12, tzinfo=timezone.utc)


def configured():
    result = json.loads(title.POLICY.read_text())
    result.update(enabled=True, publisher_app_id=98765, publisher_check_name="Hermes Queue Qualification")
    return title.policy(result)


def event_environment():
    ref = "refs/heads/gh-readonly-queue/main/native-observed-group"
    event = {"action": "checks_requested", "repository": {"id": 91, "node_id": "R_91", "full_name": REPO},
             "merge_group": {"base_sha": B, "head_sha": G, "head_ref": ref, "base_ref": "refs/heads/main"}}
    env = {"GITHUB_EVENT_NAME": "merge_group", "GITHUB_REPOSITORY": REPO, "GITHUB_SHA": G,
           "GITHUB_REF": ref, "GITHUB_RUN_ID": "501", "GITHUB_RUN_ATTEMPT": "2", "GITHUB_JOB": "validate-title"}
    return event, env


def member(position, head, group):
    entry = {"id": "entry-" + str(position), "position": position, "state": "AWAITING_CHECKS",
             "baseCommit": {"oid": B}, "headCommit": {"oid": group}}
    entry["pullRequest"] = {"id": "PR_" + str(position), "number": position, "state": "OPEN", "isDraft": False,
        "headRefOid": head, "baseRefName": "main", "title": "feat: native proof (#42)",
        "author": {"login": "contributor"}, "headRepository": {"id": "R_91", "nameWithOwner": REPO},
        "labels": {"totalCount": 0, "pageInfo": {"hasNextPage": False}, "nodes": []},
        "mergeQueueEntry": {**deepcopy(entry), "mergeQueue": {"id": "MQ_91"}}}
    return entry


class API:
    def __init__(self):
        self.subject = title.event_subject(*event_environment(), configured())
        nodes = [member(1, H1, "e" * 40), member(2, H2, G)]
        self.queue = {"repository": {"id": "R_91", "nameWithOwner": REPO, "mergeQueue": {
            "id": "MQ_91", "entries": {"totalCount": 2, "pageInfo": {"hasNextPage": False}, "nodes": nodes}}}}
        self.run = {"id": 501, "run_attempt": 2, "head_sha": G,
            "head_branch": self.subject["head_ref"].removeprefix("refs/heads/"), "event": "merge_group",
            "status": "in_progress", "workflow_id": 81, "check_suite_id": 71,
            "path": configured()["workflow_path"], "repository": {"id": 91, "full_name": REPO},
            "head_repository": {"id": 91}}
        self.attempt = None
        self.suite = {"app": {"id": 15368}, "head_sha": G}
        self.ref = {"object": {"sha": G}}
        self.checks = {}
        for number, head in [(1, H1), (2, H2)]:
            data = {"schema_id": "hermes.queue-intake.v2", "schema_version": 2, "repository": REPO,
                "pull_number": number, "head_sha": head, "entry_id": "entry-" + str(number),
                "admission_request_id": "admit-" + str(number), "enqueue_request_id": "enqueue-" + str(number),
                # H was admitted on an earlier base. It is not the G event B.
                "base_sha": "f" * 40, "expires_at": (NOW + timedelta(hours=1)).isoformat(),
                "authority_sha256": "1" * 64, "producer_generation_sha256": "5" * 64,
                "producer_closure": {path: {"kind": kind, "sha": "4" * 40,
                    "mode": "040000" if kind == "tree" else "100644"}
                    for path, kind in configured()["producer_closure"].items()}}
            self.checks[head] = {"total_count": 1, "check_runs": [{"id": 100 + number,
                "name": configured()["publisher_check_name"], "app": {"id": configured()["publisher_app_id"]},
                "head_sha": head, "external_id": "hermes-queue:admit-" + str(number) + ":H",
                "status": "completed", "conclusion": "success", "output": {"text": title.canonical(data)}}]}
        self.repository = {"id": 91, "node_id": "R_91", "full_name": REPO, "default_branch": "main"}
        self.branch = {"name": "main", "protected": True, "commit": {"sha": T}}
        self.main_ref = {"ref": "refs/heads/main", "object": {"type": "commit", "sha": T}}
        self.calls = []
        self.observations = 0
        self.on_observe = None

    @property
    def nodes(self):
        return self.queue["repository"]["mergeQueue"]["entries"]["nodes"]

    def observe_group(self, repository, branch):
        assert (repository, branch) == (REPO, "main")
        self.observations += 1
        if self.on_observe:
            self.on_observe(self)
        self.calls.append(("GRAPHQL_READ", branch))
        return deepcopy(self.queue)

    def get(self, repository, suffix):
        assert repository == REPO
        self.calls.append(("GET", suffix))
        if suffix == "":
            return deepcopy(self.repository)
        if suffix == "/branches/main":
            return deepcopy(self.branch)
        if suffix == "/git/ref/heads/main":
            return deepcopy(self.main_ref)
        if suffix == "/actions/runs/501":
            return deepcopy(self.run)
        if suffix == "/actions/runs/501/attempts/2":
            return deepcopy(self.attempt or self.run)
        if suffix == "/check-suites/71":
            return deepcopy(self.suite)
        if suffix.startswith("/git/ref/"):
            return deepcopy(self.ref)
        for head, data in self.checks.items():
            if suffix.startswith("/commits/" + head + "/check-runs?"):
                return deepcopy(data)
        raise AssertionError("Unexpected native operation: " + suffix)


class Proof:
    def __init__(self, repository):
        assert repository == REPO

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return None

    def prove(self, subject, members, policy):
        assert [member["head_sha"] for member in members] == [H1, H2]
        generation, closure = title.admitted_generation(members, policy)
        return {"tree_sha": "9" * 40, "composition": "ordered_source_tree_equivalence",
                "producer_generation_sha256": generation,
                "producer_closure": closure}


class NativeTitleSubjectTests(unittest.TestCase):
    def observe(self, api):
        return title.observe(api, api.subject, configured(), T, Proof, clock=lambda: NOW)

    def test_complete_native_group_requires_every_current_acknowledged_head(self):
        api = API()
        receipt = self.observe(api)
        self.assertEqual([m["pull_number"] for m in receipt["members"]], [1, 2])
        self.assertEqual(receipt["subject"]["base_sha"], B)
        self.assertEqual(receipt["members"][0]["admission"]["intake"]["base_sha"], "f" * 40)
        self.assertEqual(receipt["trusted_evaluator"]["sha"], T)
        self.assertNotEqual(T, B)
        self.assertNotIn("producer_generation", receipt)
        self.assertEqual(receipt["producer_generation_sha256"], "5" * 64)
        self.assertEqual(len(title.recheck(api, receipt, api.subject, configured(), T, clock=lambda: NOW)), 64)
        self.assertTrue(all(method in {"GET", "GRAPHQL_READ"} for method, _ in api.calls))

    def test_event_B_is_not_executable_authority_even_when_checkout_equals_B(self):
        api = API()
        # A speculative B passes the old HEAD == B gate but lacks independent
        # protected-main authority. T is deliberately different from B and G.
        with patch.object(title.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, B, "")):
            title.checkout_generation(B)
        with self.assertRaisesRegex(title.Refused, "protected main evaluator"):
            title.observe(api, api.subject, configured(), B, Proof, clock=lambda: NOW)
        self.assertEqual(self.observe(api)["trusted_evaluator"]["sha"], T)

    def test_protected_evaluator_requires_exact_repository_branch_ref_and_immutable_SHA(self):
        controls = [lambda a: a.repository.update(id=92), lambda a: a.repository.update(node_id="R_OTHER"),
            lambda a: a.repository.update(full_name="Other/repo"),
            lambda a: a.repository.update(default_branch="other"), lambda a: a.branch.update(name="other"),
            lambda a: a.branch.pop("protected"), lambda a: a.branch.update(protected=False),
            lambda a: a.branch.update(protected="true"), lambda a: a.branch["commit"].update(sha=B),
            lambda a: a.main_ref.update(ref="refs/heads/other"),
            lambda a: a.main_ref["object"].update(type="tag"),
            lambda a: a.main_ref["object"].update(sha=G)]
        for i, mutate in enumerate(controls):
            api = API(); mutate(api)
            with self.subTest(control=i), self.assertRaisesRegex(title.Refused, "protected main evaluator"):
                self.observe(api)
        for sha in (None, "main", "7" * 39, "A" * 40):
            with self.subTest(sha=sha), self.assertRaisesRegex(title.Refused, "immutable trusted"):
                title.trusted_evaluator(API(), API().subject, configured(), sha)

    def test_checkout_must_match_independent_selection_not_event_subject(self):
        for head, code, accepted in ((T, 0, True), (B, 0, False), (G, 0, False), (T, 1, False)):
            with self.subTest(head=head, code=code), patch.object(title.subprocess, "run",
                    return_value=subprocess.CompletedProcess([], code, head + "\n", "")) as run:
                if accepted:
                    title.checkout_generation(T)
                else:
                    with self.assertRaisesRegex(title.Refused, "independently selected"):
                        title.checkout_generation(T)
                self.assertEqual(run.call_args.kwargs["env"]["GIT_NO_REPLACE_OBJECTS"], "1")
        with patch.object(title.subprocess, "run") as run, self.assertRaises(title.Refused):
            title.checkout_generation(None)
        run.assert_not_called()

    def test_evaluator_movement_or_protection_loss_invalidates_observation_and_recheck(self):
        def move(api):
            api.branch["commit"]["sha"] = "8" * 40
            api.main_ref["object"]["sha"] = "8" * 40
        for mutate in (move, lambda a: a.branch.update(protected=False)):
            api = API(); receipt = self.observe(api); mutate(api)
            with self.assertRaisesRegex(title.Refused, "protected main evaluator"):
                title.recheck(api, receipt, api.subject, configured(), T, clock=lambda: NOW)
            api = API()
            def during(current):
                if current.observations == 1:
                    mutate(current)
            api.on_observe = during
            with self.assertRaisesRegex(title.Refused, "protected main evaluator"):
                self.observe(api)
        api = API(); receipt = self.observe(api)
        receipt["trusted_evaluator"]["sha"] = B
        with self.assertRaisesRegex(title.Refused, "changed before success"):
            title.recheck(api, receipt, api.subject, configured(), T, clock=lambda: NOW)

    def test_documentation_keeps_T_authority_distinct_from_B_G_and_installation(self):
        source = (ROOT / "docs/ci/native-queue-title.md").read_text()
        for retained in ("immutable evaluator **T**", "`trusted_evaluator`", "`producer_generation_sha256`",
                         "checked-in policy remains disabled", "full effective protection union",
                         "event B or G", "protected-main movement"):
            self.assertIn(retained, source)
        for retired in ("immutable protected B", "B supplies the executed", "`producer_generation=B`"):
            self.assertNotIn(retired, source)
        cutover = (ROOT / "docs/ci/qualification-cutover.md").read_text()
        self.assertIn("independently selected protected-main T execution", cutover)
        self.assertIn("event B and G remain subjects, never evaluator authority", cutover)

    def test_uninstalled_binding_cannot_green_the_title_leaf(self):
        with self.assertRaisesRegex(title.Refused, "not activated"):
            title.policy({**configured(), "enabled": False})
        for change in ({"publisher_app_id": 15368}, {"publisher_app_id": None},
                       {"publisher_check_name": ""}, {"producer_closure": {".github": "tree"}}):
            with self.subTest(change=change), self.assertRaises(title.Refused):
                title.policy({**configured(), **change})

    def test_event_head_base_ref_run_attempt_and_repository_are_not_caller_aliases(self):
        for field, value in [("GITHUB_EVENT_NAME", "workflow_dispatch"), ("GITHUB_SHA", H1),
                             ("GITHUB_REF", "refs/heads/main"), ("GITHUB_RUN_ATTEMPT", "0"),
                             ("GITHUB_REPOSITORY", "Other/repo"), ("GITHUB_JOB", "wrapper")]:
            event, env = event_environment()
            env[field] = value
            with self.subTest(field=field), self.assertRaises(title.Refused):
                title.event_subject(event, env, configured())

    def test_current_native_run_and_app_discriminators(self):
        controls = [lambda a: a.run.update(event="push"), lambda a: a.run.update(run_attempt=3),
            lambda a: a.run.update(head_sha=H1), lambda a: a.run.update(path=".github/workflows/other.yml"),
            lambda a: a.run.update(status="completed"), lambda a: a.run["repository"].update(id=92),
            lambda a: a.suite["app"].update(id=98765), lambda a: a.suite.update(head_sha=H1),
            lambda a: a.ref["object"].update(sha=H2),
            lambda a: setattr(a, "attempt", {**a.run, "workflow_id": 82})]
        for i, mutate in enumerate(controls):
            api = API(); mutate(api)
            with self.subTest(control=i), self.assertRaises(title.Refused):
                self.observe(api)

    def test_incomplete_ambiguous_or_H_valued_native_group_is_not_proven(self):
        def duplicate_anchor(api):
            api.nodes[0]["headCommit"] = {"oid": G}
            api.nodes[0]["pullRequest"]["mergeQueueEntry"]["headCommit"] = {"oid": G}
        controls = [
            lambda a: a.queue["repository"]["mergeQueue"]["entries"]["pageInfo"].update(hasNextPage=True),
            lambda a: a.queue["repository"]["mergeQueue"]["entries"].update(totalCount=3),
            lambda a: a.nodes[0].update(position=2), lambda a: a.nodes[0].update(id="entry-2"),
            lambda a: a.nodes[1]["headCommit"].update(oid=H2), duplicate_anchor,
            lambda a: a.nodes[1]["pullRequest"].update(isDraft=True),
            lambda a: a.nodes[1]["pullRequest"].update(state="CLOSED"),
            lambda a: a.nodes[1]["pullRequest"]["headRepository"].update(id="R_OTHER"),
            lambda a: a.nodes[1]["pullRequest"]["mergeQueueEntry"].update(id="replaced-entry"),
            lambda a: a.nodes[1]["pullRequest"]["labels"]["pageInfo"].update(hasNextPage=True),
        ]
        for i, mutate in enumerate(controls):
            api = API(); mutate(api)
            with self.subTest(control=i), self.assertRaises(title.Refused):
                self.observe(api)

    def test_same_tree_head_rewrite_still_needs_new_native_intake(self):
        api = API()
        changed = "8" * 40
        api.nodes[1]["pullRequest"]["headRefOid"] = changed
        # Even if a same-tree H2 has the old H check copied into its read result,
        # exact head metadata and the retained acknowledged intake must disagree.
        api.checks[changed] = deepcopy(api.checks[H2])
        with self.assertRaisesRegex(title.Refused, "exact H-to-entry"):
            self.observe(api)

    def test_intake_requires_authentic_app_current_H_exact_entry_and_unexpired_authority(self):
        controls = [lambda c, d: c["app"].update(id=123), lambda c, d: c.update(name="copied check"),
            lambda c, d: c.update(head_sha=H1), lambda c, d: c.update(external_id="hermes-queue:other:H"),
            lambda c, d: c.update(conclusion="failure"), lambda c, d: c.update(status="in_progress"),
            lambda c, d: d.update(head_sha=H1), lambda c, d: d.update(entry_id="unacknowledged-entry"),
            lambda c, d: d.update(pull_number=1), lambda c, d: d.update(repository="Other/repo"),
            lambda c, d: d.update(authority_sha256=""), lambda c, d: d.update(untrusted=True),
            lambda c, d: d.update(expires_at=NOW.isoformat()), lambda c, d: d.update(expires_at="2026-11-04T13:00:00"),
            lambda c, d: d.update(schema_version=True)]
        for i, mutate in enumerate(controls):
            api = API(); check = api.checks[H2]["check_runs"][0]
            data = title.decode(check["output"]["text"]); mutate(check, data)
            check["output"]["text"] = title.canonical(data)
            with self.subTest(control=i), self.assertRaises(title.Refused):
                self.observe(api)
        api = API(); api.checks[H2]["check_runs"] *= 2; api.checks[H2]["total_count"] = 2
        with self.assertRaisesRegex(title.Refused, "unique current"):
            self.observe(api)

    def test_legacy_or_absent_target_generation_never_authorizes_changed_source(self):
        controls = [lambda d: d.update(schema_id="hermes.queue-intake.v1", schema_version=1),
                    lambda d: d.pop("producer_closure"), lambda d: d.pop("producer_generation_sha256"),
                    lambda d: d.update(producer_generation_sha256="A" * 64)]
        for i, mutate in enumerate(controls):
            api = API(); check = api.checks[H2]["check_runs"][0]
            data = title.decode(check["output"]["text"]); mutate(data)
            check["output"]["text"] = title.canonical(data)
            with self.subTest(control=i), self.assertRaises(title.Refused):
                self.observe(api)

    def test_all_members_require_the_same_digest_and_exact_target_closure(self):
        controls = [lambda d: d.update(producer_generation_sha256="6" * 64),
                    lambda d: d["producer_closure"][".github"].update(sha="6" * 40)]
        for i, mutate in enumerate(controls):
            api = API(); check = api.checks[H2]["check_runs"][0]
            data = title.decode(check["output"]["text"]); mutate(data)
            check["output"]["text"] = title.canonical(data)
            with self.subTest(control=i), self.assertRaisesRegex(title.Refused, "mixed admitted"):
                self.observe(api)

    def test_target_closure_requires_complete_safe_exact_object_entries(self):
        path = "scripts/ci/native_queue_title.py"
        controls = [lambda c: c.pop(".github"), lambda c: c.pop(path),
            lambda c: c[".github"].update(kind="blob", mode="100644"),
            lambda c: c[path].update(kind="tree", mode="040000"),
            lambda c: c[path].update(sha="A" * 40), lambda c: c[path].update(mode="120000"),
            lambda c: c[path].update(mode="160000", kind="commit"),
            lambda c: c[path].update(mode=[]), lambda c: c[path].update(unreviewed=True),
            lambda c: c.update({"../outside": deepcopy(c[path])}),
            lambda c: c.update({".git/config": deepcopy(c[path])}),
            lambda c: c.update({"/absolute": deepcopy(c[path])})]
        for i, mutate in enumerate(controls):
            api = API(); check = api.checks[H2]["check_runs"][0]
            data = title.decode(check["output"]["text"]); mutate(data["producer_closure"])
            check["output"]["text"] = title.canonical(data)
            with self.subTest(control=i), self.assertRaisesRegex(title.Refused, "producer closure"):
                self.observe(api)

    def test_refreshed_target_generation_or_closure_invalidates_prior_observation(self):
        controls = [lambda d: d.update(producer_generation_sha256="6" * 64),
                    lambda d: d["producer_closure"][".github"].update(sha="6" * 40)]
        for i, mutate in enumerate(controls):
            api = API(); receipt = self.observe(api)
            # Both current H checks agree again, but this is not the generation
            # already proved. Agreement alone cannot refresh the old receipt.
            for row in api.checks.values():
                check = row["check_runs"][0]
                data = title.decode(check["output"]["text"]); mutate(data)
                check["output"]["text"] = title.canonical(data)
            with self.subTest(control=i), self.assertRaisesRegex(title.Refused, "changed before success"):
                title.recheck(api, receipt, api.subject, configured(), T, clock=lambda: NOW)

    def test_changed_title_author_skip_label_intake_or_authority_invalidates_observed_receipt(self):
        def label(api):
            api.nodes[0]["pullRequest"]["labels"] = {"totalCount": 1, "pageInfo": {"hasNextPage": False},
                                                       "nodes": [{"name": "skip-title-check"}]}
        def authority(api):
            check = api.checks[H1]["check_runs"][0]; data = title.decode(check["output"]["text"])
            data["authority_sha256"] = "2" * 64; check["output"]["text"] = title.canonical(data)
        controls = [lambda a: a.nodes[0]["pullRequest"].update(title="changed (#42)"),
                    lambda a: a.nodes[0]["pullRequest"]["author"].update(login="dependabot[bot]"),
                    label, authority, lambda a: a.checks[H1]["check_runs"][0].update(conclusion="cancelled")]
        for i, mutate in enumerate(controls):
            api = API(); receipt = self.observe(api); mutate(api)
            with self.subTest(control=i), self.assertRaises(title.Refused):
                title.recheck(api, receipt, api.subject, configured(), T, clock=lambda: NOW)
        api = API(); receipt = self.observe(api)
        with self.assertRaisesRegex(title.Refused, "expired"):
            title.recheck(api, receipt, api.subject, configured(), T, clock=lambda: NOW + timedelta(hours=2))

    def test_metadata_race_during_composition_cannot_pass(self):
        api = API()
        def mutate(current):
            if current.observations == 2:
                current.nodes[0]["pullRequest"]["title"] = "new title (#42)"
        api.on_observe = mutate
        with self.assertRaisesRegex(title.Refused, "changed during composition"):
            self.observe(api)

    def test_only_non_exemption_housekeeping_label_changes_are_outside_title_identity(self):
        api = API(); receipt = self.observe(api)
        api.nodes[0]["pullRequest"]["labels"] = {"totalCount": 1, "pageInfo": {"hasNextPage": False},
                                                   "nodes": [{"name": "needs-issue-link"}]}
        title.recheck(api, receipt, api.subject, configured(), T, clock=lambda: NOW)

    def test_native_membership_bound_accepts_100_and_refuses_101_or_incomplete_counts(self):
        def api_with(count):
            api = API()
            nodes = [member(i, format(i, "040x"), G if i == count else "e" * 40)
                     for i in range(1, count + 1)]
            api.queue["repository"]["mergeQueue"]["entries"].update(nodes=nodes, totalCount=count)
            return api
        api = api_with(100)
        result = title.group_members(api, api.subject, configured())
        self.assertEqual([m["pull_number"] for m in result["members"]], list(range(1, 101)))
        for api in [api_with(101), api_with(100)]:
            if len(api.nodes) == 100:
                api.queue["repository"]["mergeQueue"]["entries"]["totalCount"] = 101
            with self.assertRaisesRegex(title.Refused, "complete native queue membership"):
                title.group_members(api, api.subject, configured())

    def test_duplicate_JSON_is_not_an_authority_override(self):
        with self.assertRaisesRegex(title.Refused, "duplicate"):
            title.decode('{"head_sha":"old","head_sha":"new"}')

    def test_existing_native_transport_uses_only_fixed_repository_queue_read(self):
        response = subprocess.CompletedProcess([], 0, json.dumps(API().queue).encode(), b"")
        # The transport receives GraphQL's data envelope, not candidate JSON.
        response.stdout = json.dumps({"data": API().queue}).encode()
        with patch.object(title.queue.subprocess, "run", return_value=response) as run:
            result = title.GitHub().observe_group(REPO, "main")
        self.assertEqual(result, API().queue)
        request = json.loads(run.call_args.kwargs["input"])
        self.assertEqual(request["query"], title.queue.OBSERVE_GROUP)
        self.assertEqual(request["variables"], {"owner": "EffortlessMetrics", "name": "perl-lsp-swarm", "branch": "main"})
        self.assertNotIn("mutation", request["query"])
        response.stdout = b'{"errors":[{"message":"unsupported native field"}]}'
        with patch.object(title.queue.subprocess, "run", return_value=response), self.assertRaises(title.queue.Uncertain):
            title.GitHub().observe_group(REPO, "main")


class NativeTitleMainTests(unittest.TestCase):
    def run_main(self, operation, *, trusted_sha=T, checkout_sha=T, receipt=None):
        # Exercise main, custody, observe and recheck unchanged. Only external
        # service/custody observations and Git composition are fixture inputs;
        # the real Git composition has its separate production-path suite below.
        api = API()
        event, env = event_environment()
        with tempfile.TemporaryDirectory() as home:
            root = Path(home)
            policy_path, event_path, target = root / "policy.json", root / "event.json", root / "receipt.json"
            policy_path.write_text(title.canonical(configured()))
            event_path.write_text(title.canonical(event))
            if receipt is not None:
                target.write_text(title.canonical(receipt))
            env["GITHUB_EVENT_PATH"] = str(event_path)
            if trusted_sha is not None:
                env["TRUSTED_TITLE_SHA"] = trusted_sha
            real_run, custody_calls = subprocess.run, []
            def git_observation(command, **kwargs):
                if command == ["git", "-C", str(title.ROOT), "rev-parse", "HEAD"]:
                    custody_calls.append(command)
                    return subprocess.CompletedProcess(command, 0, checkout_sha + "\n", "")
                # GitProof initializes only a fresh local bare store; its
                # composition method is replaced below, so no fetch is possible.
                self.assertIn("init", command)
                self.assertIn("--bare", command)
                return real_run(command, **kwargs)
            output = io.StringIO()
            with patch.object(title, "POLICY", policy_path), patch.dict(os.environ, env, clear=True), \
                    patch.object(title, "GitHub", return_value=api), \
                    patch.object(title.GitProof, "prove", Proof.prove), \
                    patch.object(title, "datetime", wraps=datetime) as clock, \
                    patch.object(title.subprocess, "run", side_effect=git_observation), redirect_stdout(output):
                clock.now.return_value = NOW
                status = title.main([operation, "--receipt", str(target)])
            observed = title.decode(target.read_text()) if target.exists() else None
            return status, output.getvalue(), observed, api.calls, custody_calls

    def test_main_observe_and_recheck_use_independent_T_not_subject_B(self):
        self.assertNotEqual(T, B)
        status, output, receipt, calls, custody = self.run_main("observe")
        self.assertEqual(status, 0, output)
        self.assertEqual(receipt["subject"]["base_sha"], B)
        self.assertEqual(receipt["trusted_evaluator"]["sha"], T)
        self.assertEqual(len(custody), 1)
        self.assertIn(("GET", "/branches/main"), calls)
        status, output, retained, calls, custody = self.run_main("recheck", receipt=receipt)
        self.assertEqual(status, 0, output)
        self.assertIn("Current native title subject:", output)
        self.assertEqual(retained, receipt)
        self.assertEqual(len(custody), 1)
        self.assertIn(("GET", "/branches/main"), calls)

    def test_main_refuses_missing_mismatched_or_subject_selected_T(self):
        receipt = title.observe(API(), API().subject, configured(), T, Proof, clock=lambda: NOW)
        controls = [(None, T, "immutable trusted", False),
                    (T, B, "independently selected", False),
                    (T, G, "independently selected", False),
                    (B, B, "protected main evaluator", True)]
        for operation in ("observe", "recheck"):
            for selected, checkout, refusal, reads in controls:
                with self.subTest(operation=operation, selected=selected, checkout=checkout):
                    prior = receipt if operation == "recheck" else None
                    status, output, retained, calls, custody = self.run_main(
                        operation, trusted_sha=selected, checkout_sha=checkout, receipt=prior)
                    self.assertEqual(status, 1, output)
                    self.assertIn("NOT_PROVEN:", output)
                    self.assertIn(refusal, output)
                    self.assertEqual(retained, prior)
                    self.assertEqual(bool(calls), reads)
                    self.assertEqual(len(custody), int(selected is not None))


@unittest.skipUnless(shutil.which("git"), "native Git is required")
class NativeTitleCompositionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name); self.source = self.root / "source"; self.source.mkdir()
        self.git("init", "-q", "--template=")
        self.git("config", "user.name", "Fixture"); self.git("config", "user.email", "fixture@example.invalid")
        self.configured = configured()
        files = {p: "reviewed validator\n" for p, kind in self.configured["producer_closure"].items() if kind == "blob"}
        files.update({".github/workflows/proof.yml": "reviewed workflow\n", ".github/actions/child/action.yml": "reviewed action\n", "product.txt": "base\n"})
        self.base = self.commit(files); self.git("branch", "-M", "main")
        origin = self.source
        class LocalProof(title.GitProof):
            def origin(self): return str(origin)
            def environment(self): return {**super().environment(), "GIT_ALLOW_PROTOCOL": "file"}
        self.Proof = LocalProof

    def test_git_proof_accepts_utf8_temp_path_under_legacy_decoder(self):
        # Exercise actual Git output through the production subprocess boundary.
        with tempfile.TemporaryDirectory(prefix="native-title-", suffix="-\u2603") as home:
            with patch.object(tempfile, "tempdir", home), \
                    patch.object(subprocess, "_text_encoding", return_value="ascii"):
                with title.GitProof(REPO) as proof:
                    self.assertEqual(proof.run(["rev-parse", "--is-bare-repository"]), "true")

    def git(self, *args, expect_success=True):
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        result = subprocess.run(["git", *args], cwd=self.source, env=env, capture_output=True, text=True, encoding="utf-8")
        if expect_success: self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip() if expect_success else result.returncode

    def commit(self, files):
        for name, content in files.items():
            path = self.source / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_text(content)
        self.git("add", "--all"); self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def head(self, name, files):
        self.git("checkout", "-qB", name, self.base)
        return self.commit(files)

    def group(self, heads):
        self.git("checkout", "-qB", "native-group", self.base)
        for head in heads:
            self.git("merge", "--squash", head); self.git("commit", "-qm", "native squash fixture")
        return self.git("rev-parse", "HEAD")

    def closure(self, commit):
        result = {}
        for path in self.configured["producer_closure"]:
            metadata, actual_path = self.git("ls-tree", commit, "--", path).split("\t")
            mode, kind, sha = metadata.split()
            self.assertEqual(actual_path, path)
            result[path] = {"kind": kind, "sha": sha, "mode": mode}
        return result

    def admitted_members(self, heads, closure=None):
        target = self.closure(self.base) if closure is None else closure
        return [{"head_sha": head, "admission": {"intake": {
            "producer_generation_sha256": "5" * 64, "producer_closure": deepcopy(target)}}} for head in heads]

    def prove(self, heads, group, closure=None):
        with self.Proof(REPO) as proof:
            return proof.prove({"base_sha": self.base, "head_sha": group},
                               self.admitted_members(heads, closure), self.configured)

    def pipeline_api(self, heads, group):
        """Synthetic service observations, real local Git composition and workflow body.

        Protected-main observations are synthetic; these fixtures do not
        authenticate a live native queue or a deployed protection/App binding.
        """
        api = API()
        api.subject.update(base_sha=self.base, head_sha=group)
        api.run["head_sha"] = group
        api.suite["head_sha"] = group
        api.ref["object"]["sha"] = group
        nodes = [member(i, head, group if i == len(heads) else "e" * 40)
                 for i, head in enumerate(heads, 1)]
        for entry in nodes:
            entry["baseCommit"]["oid"] = self.base
            entry["pullRequest"]["mergeQueueEntry"]["baseCommit"]["oid"] = self.base
        connection = api.queue["repository"]["mergeQueue"]["entries"]
        connection.update(nodes=nodes, totalCount=len(nodes))
        template = deepcopy(api.checks[H1]["check_runs"][0])
        api.checks = {}
        for i, head in enumerate(heads, 1):
            check = deepcopy(template)
            check.update(id=100 + i, head_sha=head, external_id=f"hermes-queue:admit-{i}:H")
            intake = title.decode(check["output"]["text"])
            intake.update(pull_number=i, head_sha=head, entry_id=f"entry-{i}",
                          admission_request_id=f"admit-{i}", enqueue_request_id=f"enqueue-{i}",
                          producer_closure=self.closure(self.base))
            check["output"]["text"] = title.canonical(intake)
            api.checks[head] = {"total_count": 1, "check_runs": [check]}
        return api

    def workflow_titles(self, receipt, **options):
        self.assertIsNotNone(shutil.which("node"), "Node is required for actual workflow-body proof")
        process = subprocess.run(["node", str(ROOT / "scripts/ci/test_merge_group_title_workflow.js"),
                                  "--receipt-stdin"],
                                 input=json.dumps({"receipt": receipt, **options}),
                                 capture_output=True, text=True, encoding="utf-8", timeout=30)
        self.assertEqual(process.returncode, 0, process.stderr)
        result = json.loads(process.stdout)
        self.assertEqual([row["pull_number"] for row in result["results"]],
                         [row["pull_number"] for row in receipt["members"]])
        return result

    def pipeline_observe(self, api):
        return title.observe(api, api.subject, self.configured, T, self.Proof, clock=lambda: NOW)

    def assert_pipeline_current(self, api, receipt):
        self.assertEqual(len(title.recheck(api, receipt, api.subject, self.configured, T,
                                          clock=lambda: NOW)), 64)
        self.assertTrue(all(method in {"GET", "GRAPHQL_READ"} for method, _ in api.calls))
        self.assertFalse(any("/pulls" in target or "/compare/" in target for _, target in api.calls))

    def test_actual_pipeline_accepts_singleton_squash_without_commit_associations(self):
        # Live shape: #19257 review5854101886, RIPR G=71529dab, H=16f49ab.
        # The locally generated commits below are not a native-G qualification.
        head = self.head("pipeline-single", {"product.txt": "singleton\n"})
        group = self.group([head])
        self.assertNotEqual(self.git("merge-base", "--is-ancestor", head, group,
                                     expect_success=False), 0)
        api = self.pipeline_api([head], group)
        receipt = self.pipeline_observe(api)
        result = self.workflow_titles(receipt)
        self.assertEqual(result["failures"], [])
        self.assertEqual([row["verdict"] for row in result["results"]], ["success"])
        self.assert_pipeline_current(api, receipt)

    def test_actual_pipeline_requires_every_member_and_each_H_intake(self):
        first = self.head("pipeline-first", {"first.txt": "first\n"})
        second = self.head("pipeline-second", {"second.txt": "second\n"})
        group = self.group([first, second])
        api = self.pipeline_api([first, second], group)
        receipt = self.pipeline_observe(api)
        self.assertEqual(self.workflow_titles(receipt)["failures"], [])
        self.assert_pipeline_current(api, receipt)
        missing_intake = self.pipeline_api([first, second], group)
        missing_intake.checks[second] = {"total_count": 0, "check_runs": []}
        with self.assertRaisesRegex(title.Refused, "unique current"):
            self.pipeline_observe(missing_intake)
        # Even a syntactically complete singleton snapshot with valid H intake
        # cannot hide first's contribution to actual G.
        omitted = self.pipeline_api([second], group)
        with self.assertRaisesRegex(title.Refused, "differs from complete"):
            self.pipeline_observe(omitted)
        extra_group = self.commit({"unselected.txt": "extra\n"})
        extra = self.pipeline_api([first, second], extra_group)
        with self.assertRaisesRegex(title.Refused, "differs from complete"):
            self.pipeline_observe(extra)

    def test_actual_pipeline_ignores_unrelated_associations_and_later_queue_members(self):
        head = self.head("pipeline-unrelated", {"product.txt": "selected\n"})
        group = self.group([head])
        for change in ({"state": "CLOSED"}, {"baseRefName": "other"}):
            with self.subTest(change=change):
                api = self.pipeline_api([head], group)
                unrelated = member(2, "8" * 40, "9" * 40)
                unrelated["pullRequest"].update(title="unrelated title without an issue", **change)
                connection = api.queue["repository"]["mergeQueue"]["entries"]
                connection["nodes"].append(unrelated)
                connection["totalCount"] = 2
                original_get = api.get
                association_reads = []
                def get(repository, suffix):
                    if suffix.endswith("/pulls"):
                        association_reads.append(suffix)
                        return [{"number": 999, "title": "no issue", "state": "closed",
                                 "base": {"ref": "other"}}]
                    return original_get(repository, suffix)
                api.get = get
                receipt = self.pipeline_observe(api)
                result = self.workflow_titles(receipt)
                self.assertEqual(result["failures"], [])
                self.assertEqual([row["pull_number"] for row in result["results"]], [1])
                self.assert_pipeline_current(api, receipt)
                self.assertEqual(association_reads, [])
                # A selected constituent cannot use the same exclusion to pass.
                api.nodes[0]["pullRequest"].update(**change)
                with self.assertRaisesRegex(title.Refused, "current native PR"):
                    self.pipeline_observe(api)

    def test_actual_pipeline_uses_current_exact_author_and_rechecks_title_metadata(self):
        head = self.head("pipeline-metadata", {"product.txt": "metadata\n"})
        group = self.group([head])
        api = self.pipeline_api([head], group)
        receipt = self.pipeline_observe(api)
        self.assertEqual(self.workflow_titles(receipt)["failures"], [])
        self.assert_pipeline_current(api, receipt)
        api.nodes[0]["pullRequest"]["title"] = "no issue reference"
        with self.assertRaisesRegex(title.Refused, "changed before success"):
            self.assert_pipeline_current(api, receipt)
        for login, verdict in [("not-dependabot-person", "failure"), ("dependabot[bot]", "success")]:
            with self.subTest(login=login):
                api.nodes[0]["pullRequest"]["author"]["login"] = login
                current = self.pipeline_observe(api)
                result = self.workflow_titles(current)
                self.assertEqual([row["verdict"] for row in result["results"]], [verdict])
                self.assertEqual(len(result["failures"]), int(verdict == "failure"))
                # Current membership is distinct from a successful title verdict.
                self.assert_pipeline_current(api, current)

    def test_nonancestor_event_base_refuses_even_when_its_tree_equals_G(self):
        head = self.head("nonancestor-base", {"product.txt": "same resulting tree\n"})
        group = self.group([head])
        self.prove([head], group)
        self.assertEqual(self.git("rev-parse", head + "^{tree}"), self.git("rev-parse", group + "^{tree}"))
        with self.Proof(REPO) as proof, self.assertRaisesRegex(title.Refused, "conflicted or unavailable"):
            proof.prove({"base_sha": head, "head_sha": group},
                        self.admitted_members([head]), self.configured)

    def test_reviewed_new_producer_generation_can_differ_from_subject_B(self):
        head = self.head("reviewed-generation", {
            ".github/workflows/proof.yml": "reviewed new workflow\n",
            "scripts/ci/native_queue_title.py": "reviewed next evaluator; not executed for this G\n",
            ".ci/policies/native-queue-title.json": "reviewed next policy; current trust comes from independent protected main\n"})
        group = self.group([head])
        target = self.closure(group)
        self.assertNotEqual(target, self.closure(self.base))
        result = self.prove([head], group, target)
        self.assertNotIn("producer_generation", result)
        self.assertEqual(result["producer_closure"], target)
        self.assertEqual(result["producer_generation_sha256"], "5" * 64)
        original = self.Proof
        class FrozenBaseGeneration(original):
            def prove(self, subject, members, policy):
                result = super().prove(subject, members, policy)
                for path in policy["producer_closure"]:
                    title.require(self.run(["ls-tree", subject["base_sha"], "--", path]) ==
                                  self.run(["ls-tree", subject["head_sha"], "--", path]),
                                  "old B equality wrongly forbids the admitted target")
                return result
        with FrozenBaseGeneration(REPO) as mutant, self.assertRaisesRegex(title.Refused, "old B equality"):
            mutant.prove({"base_sha": self.base, "head_sha": group},
                         self.admitted_members([head], target), self.configured)

    def test_additional_admitted_producer_roots_are_verified_too(self):
        head = self.head("additional-owner", {"product.txt": "changed selected producer\n"})
        group = self.group([head])
        target = self.closure(self.base)
        target["product.txt"] = {"kind": "blob", "mode": "100644",
                                 "sha": self.git("rev-parse", self.base + ":product.txt")}
        with self.assertRaisesRegex(title.Refused, "producer closure"):
            self.prove([head], group, target)
        target["product.txt"]["sha"] = self.git("rev-parse", group + ":product.txt")
        self.prove([head], group, target)

    def test_squash_G_passes_without_H_ancestry_or_assumed_two_parents(self):
        head = self.head("first", {"product.txt": "changed\n"}); group = self.group([head])
        self.assertNotEqual(self.git("merge-base", "--is-ancestor", head, group, expect_success=False), 0)
        result = self.prove([head], group)
        self.assertEqual(result["tree_sha"], self.git("rev-parse", group + "^{tree}"))

    def test_complete_ordered_composition_and_omitted_extra_conflicted_members(self):
        first = self.head("first", {"first.txt": "first\n"}); second = self.head("second", {"second.txt": "second\n"})
        group = self.group([first, second]); self.prove([first, second], group)
        with self.assertRaisesRegex(title.Refused, "differs from complete"):
            self.prove([second], group)
        extra = self.commit({"extra.txt": "unselected\n"})
        with self.assertRaisesRegex(title.Refused, "differs from complete"):
            self.prove([first, second], extra)
        conflict1 = self.head("conflict1", {"product.txt": "one\n"})
        conflict2 = self.head("conflict2", {"product.txt": "two\n"})
        with self.assertRaisesRegex(title.Refused, "conflicted"):
            self.prove([conflict1, conflict2], group)

    def test_transitive_workflow_action_helper_or_policy_substitution_is_red(self):
        for i, path in enumerate([".github/workflows/proof.yml", ".github/actions/child/action.yml",
                                  "scripts/ci/native_queue.py", "scripts/ci/pr_title_validator.js",
                                  ".ci/policies/native-queue-title.json"]):
            with self.subTest(path=path):
                head = self.head("substitution" + str(i), {path: "forged green\n"}); group = self.group([head])
                with self.assertRaisesRegex(title.Refused, "producer closure"):
                    self.prove([head], group)

    def test_stacked_head_edit_and_revert_preserve_consumed_H_merge_base(self):
        for case in ("edit", "revert"):
            with self.subTest(case=case):
                first = self.head("stack-first-" + case, {"product.txt": "one\n", "stack.txt": "added\n"})
                # H2 descends from H1. Actual squash G need not retain that parent
                # chain, but the private composition must retain it for merge-base.
                self.git("checkout", "-qB", "stack-second-" + case, first)
                if case == "edit":
                    second = self.commit({"product.txt": "two\n"})
                else:
                    self.git("rm", "stack.txt")
                    second = self.commit({"product.txt": "base\n", "surviving.txt": "H2 addition\n"})
                group = self.group([second])
                self.prove([first, second], group)
                original = self.Proof
                class LostConsumedParent(original):
                    def run(self, args, **kwargs):
                        if "commit-tree" in args:
                            at = args.index("-p", args.index("-p") + 1)
                            args = args[:at] + args[at + 2:]
                        return super().run(args, **kwargs)
                with LostConsumedParent(REPO) as mutant, self.assertRaises(title.Refused):
                    mutant.prove({"base_sha": self.base, "head_sha": group},
                                 self.admitted_members([first, second]), self.configured)

    def test_environment_and_worker_git_config_cannot_import_hooks_or_replace_objects(self):
        head = self.head("first", {"product.txt": "changed\n"}); group = self.group([head])
        with self.Proof(REPO) as proof:
            env = proof.environment()
            self.assertEqual(env["GIT_NO_REPLACE_OBJECTS"], "1")
            self.assertEqual(env["GIT_CONFIG_GLOBAL"], os.devnull)
            self.assertNotIn("GIT_DIR", env); self.assertNotIn("GIT_CONFIG_COUNT", env)
            self.git("config", "core.hooksPath", str(self.root / "untrusted-hooks"))
            proof.prove({"base_sha": self.base, "head_sha": group}, self.admitted_members([head]), self.configured)

    def test_unchanged_helper_blob_does_not_hide_producer_mode_change(self):
        self.git("checkout", "-qB", "mode-change", self.base)
        path = "scripts/ci/pr_title_validator.js"
        (self.source / path).chmod(0o755)
        self.git("add", path)
        self.git("commit", "-qm", "changed helper mode")
        head = self.git("rev-parse", "HEAD"); group = self.group([head])
        self.assertEqual(self.git("rev-parse", self.base + ":" + path), self.git("rev-parse", group + ":" + path))
        with self.assertRaisesRegex(title.Refused, "producer closure"):
            self.prove([head], group)
        self.prove([head], group, self.closure(group))


if __name__ == "__main__":
    unittest.main()
