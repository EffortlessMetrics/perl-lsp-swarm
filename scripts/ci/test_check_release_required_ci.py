#!/usr/bin/env python3
"""Required release CI laws and the actual orchestration caller (#6059)."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

if __package__:
    from . import check_release_required_ci as checker
    from . import observe_github_enforcement as observer
else:
    import check_release_required_ci as checker
    import observe_github_enforcement as observer


ROOT = Path(__file__).resolve().parents[2]
REPOSITORY = "EffortlessMetrics/perl-lsp-swarm"
SHA = "1f973039d812fe080abddc44e461112d7b95a6fa"
APP = 15368
PREFIX = f"repos/{REPOSITORY}/commits/{SHA}"
ALL = f"{PREFIX}/check-runs?filter=all&per_page=100&page=1"
LATEST = f"{PREFIX}/check-runs?filter=latest&per_page=100&page=1"
STATUSES = f"{PREFIX}/statuses?per_page=100&page=1"
CLASSIC = f"repos/{REPOSITORY}/branches/main/protection"
RULESETS = f"repos/{REPOSITORY}/rulesets?includes_parents=true&per_page=100"


def check_run(*, conclusion="success", status="completed", sha=SHA, app=APP, id=1):
    return {"id": id, "name": "Required Actions", "status": status,
            "conclusion": conclusion, "head_sha": sha, "app": {"id": app}}


def responses(*, conclusion="success"):
    return {
        f"repos/{REPOSITORY}": {"id": 1, "full_name": REPOSITORY, "default_branch": "main"},
        f"repos/{REPOSITORY}/git/ref/heads/main": {
            "ref": "refs/heads/main", "object": {"type": "commit", "sha": SHA}},
        f"repos/{REPOSITORY}/branches/main/protection": {
            "required_status_checks": {"strict": True, "contexts": ["Required Actions"],
                                       "checks": [{"context": "Required Actions", "app_id": APP}]}},
        f"repos/{REPOSITORY}/rulesets?includes_parents=true&per_page=100": [],
        f"repos/{REPOSITORY}/commits/{SHA}/check-suites?per_page=100": {"total_count": 1},
        f"repos/{REPOSITORY}/commits/{SHA}/check-runs?filter=latest&per_page=100&page=1": {
            "total_count": 1, "check_runs": [check_run(conclusion=conclusion)]},
        f"repos/{REPOSITORY}/commits/{SHA}/check-runs?filter=all&per_page=100&page=1": {
            "total_count": 1, "check_runs": [check_run(conclusion=conclusion)]},
        f"repos/{REPOSITORY}/commits/{SHA}/statuses?per_page=100&page=1": [],
    }


def add_ruleset(fixture, *, app=APP, context="Required Actions", enforcement="active", include=None):
    fixture[RULESETS] = [{"id": 17, "target": "branch"}]
    fixture[f"repos/{REPOSITORY}/rulesets/17"] = {
        "id": 17, "name": "Release policy", "target": "branch",
        "source_type": "Repository", "source": REPOSITORY,
        "enforcement": enforcement, "bypass_actors": [],
        "conditions": {"ref_name": {"include": include or ["~DEFAULT_BRANCH"], "exclude": []}},
        "rules": [{"type": "required_status_checks", "parameters": {
            "strict_required_status_checks_policy": True, "do_not_enforce_on_create": False,
            "required_status_checks": [{"context": context, "integration_id": app}]}}],
    }


class FixtureTransport:
    def __init__(self, fixture):
        self.fixture = fixture
        self.calls = []

    def __call__(self, path):
        self.calls.append(path)
        if path not in self.fixture:
            return observer.ApiResult(404, b'{"message":"Not Found"}')
        value = self.fixture[path]
        return value if isinstance(value, observer.ApiResult) else observer.ApiResult(200, json.dumps(value).encode())


class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


def validation_step():
    text = (ROOT / ".github/workflows/release-orchestration.yml").read_text()
    match = re.search(r"      - name: Check default branch and CI status\n.*?        run: \|\n"
                      r"(?P<body>(?:          .*\n|\n)+)", text, re.DOTALL)
    if match is None:
        raise AssertionError("production release validation step is missing")
    # Stop at the next step; only run the read-only validation, never dispatch.
    body = match.group("body").split("      - name:", 1)[0]
    return "\n".join(line[10:] if line.startswith("          ") else line
                     for line in body.splitlines()).replace("${{ github.repository }}", REPOSITORY)


class WorkflowTests(unittest.TestCase):
    def run_step(self, fixture):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "responses.json").write_text(json.dumps(fixture))
            # Legacy caller's gh endpoint deliberately remains green. Only a
            # real required-check evaluation can detect the failing fixture.
            (directory / "gh").write_text("#!/bin/sh\nprintf 'success\\n'\n")
            (directory / "git").write_text(f"#!/bin/sh\nprintf '{SHA}\\n'\n")
            launcher = f"""#!{sys.executable}
import json, os, sys
from pathlib import Path
if sys.argv[1] == '-B':
    del sys.argv[1]
if sys.argv[1] != 'scripts/ci/check_release_required_ci.py':
    raise SystemExit('unexpected production Python command')
sys.path.insert(0, str(Path(sys.argv[1]).resolve().parent))
import check_release_required_ci as checker
import observe_github_enforcement as observer
fixture = json.loads(Path(os.environ['RELEASE_CI_FIXTURE']).read_text())
def get(path):
    with open(os.environ['RELEASE_CI_CALLS'], 'a') as log:
        log.write(path + '\\n')
    if path not in fixture:
        return observer.ApiResult(404, b'{{"message":"Not Found"}}')
    return observer.ApiResult(200, json.dumps(fixture[path]).encode())
raise SystemExit(checker.main(sys.argv[2:], transport=get))
"""
            (directory / "python3").write_text(launcher)
            for name in ("gh", "git", "python3"):
                (directory / name).chmod(0o700)
            env = dict(os.environ, PATH=f"{directory}:{os.environ['PATH']}",
                       PYTHONDONTWRITEBYTECODE="1",
                       GITHUB_SHA=SHA, GITHUB_REPOSITORY=REPOSITORY, DEFAULT_BRANCH="main",
                       VALIDATE_RELEASE_CI_TIMEOUT_SECONDS="1",
                       VALIDATE_RELEASE_CI_POLL_INTERVAL_SECONDS="0.01",
                       RELEASE_CI_FIXTURE=str(directory / "responses.json"),
                       RELEASE_CI_CALLS=str(directory / "calls.log"))
            result = subprocess.run(["bash", "-e", "-c", validation_step()], cwd=ROOT,
                                    env=env, capture_output=True, text=True, timeout=5)
            calls = (directory / "calls.log").read_text().splitlines() if (directory / "calls.log").exists() else []
            return result, calls

    def test_actual_step_rejects_green_legacy_status_with_failed_required_actions(self):
        result, calls = self.run_step(responses(conclusion="failure"))
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["verdict"], "FAIL")
        self.assertIn(ALL, calls)
        self.assertIn(LATEST, calls)

    def test_actual_step_accepts_required_actions_without_legacy_statuses(self):
        result, _ = self.run_step(responses())
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["subject_sha"], SHA)

    def test_actual_step_refuses_observed_check_without_latest_identity(self):
        for conclusion in ("failure", "success"):
            with self.subTest(conclusion=conclusion):
                fixture = responses(conclusion=conclusion)
                fixture[CLASSIC]["required_status_checks"]["checks"][0]["app_id"] = -1
                fixture[LATEST] = {"total_count": 0, "check_runs": []}
                fixture[STATUSES] = [{"id": 2, "context": "Required Actions", "state": "success"}]
                result, calls = self.run_step(fixture)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                packet = json.loads(result.stdout)
                self.assertEqual(packet["verdict"], "NOT_PROVEN")
                self.assertEqual(packet["checks"][0]["result"], "NOT_PROVEN")
                self.assertIn(ALL, calls)
                self.assertIn(LATEST, calls)

    def test_actual_step_preserves_genuine_legacy_only_success(self):
        fixture = responses()
        fixture[CLASSIC]["required_status_checks"]["checks"][0]["app_id"] = -1
        fixture[ALL] = fixture[LATEST] = {"total_count": 0, "check_runs": []}
        fixture[f"{PREFIX}/check-suites?per_page=100"] = {"total_count": 0}
        fixture[STATUSES] = [{"id": 2, "context": "Required Actions", "state": "success"}]
        result, _ = self.run_step(fixture)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        packet = json.loads(result.stdout)
        self.assertEqual(packet["verdict"], "PASS")
        self.assertIsNone(packet["checks"][0]["check_run_id"])
        self.assertEqual(packet["checks"][0]["status_id"], 2)

    def test_release_and_hosted_contract_callers_are_wired(self):
        workflow = (ROOT / ".github/workflows/release-orchestration.yml").read_text()
        validate_job = workflow.split("  validate:\n", 1)[1].split("  orchestrate:", 1)[0]
        self.assertIn("      checks: read", validate_job)
        self.assertNotIn("/commits/${HEAD_SHA}/status", validate_job)
        self.assertIn('--sha "$GITHUB_SHA"', validation_step())
        self.assertIn('python3 -B scripts/ci/check_release_required_ci.py', validation_step())
        self.assertIn('git rev-parse HEAD', validation_step())
        hosted = (ROOT / ".github/workflows/gate-enforcement-contract.yml").read_text()
        self.assertIn("scripts/ci/test_check_release_required_ci.py", hosted)


class RequiredCITests(unittest.TestCase):
    def policy(self, fixture):
        return checker.required_policy(FixtureTransport(fixture), REPOSITORY, "main", SHA)

    def verdict(self, fixture):
        clock = Clock()
        transport = FixtureTransport(fixture)
        return checker.wait_for_required_ci(REPOSITORY, "main", SHA, timeout=1, interval=0.1,
                                            transport=transport, clock=clock, sleep=clock.sleep)

    def evaluate(self, fixture):
        return checker.evaluate_required(self.policy(fixture), fixture[LATEST]["check_runs"],
                                         fixture[ALL]["check_runs"], fixture[STATUSES], SHA)

    def test_actions_success_without_legacy_status_is_non_vacuous(self):
        packet = self.verdict(responses())
        self.assertEqual(packet["verdict"], "PASS")
        self.assertEqual(len(packet["checks"]), 1)
        self.assertEqual(packet["checks"][0]["check_run_id"], 1)

    def test_unbound_legacy_status_only_can_satisfy_its_required_context(self):
        fixture = responses()
        fixture[CLASSIC]["required_status_checks"]["checks"][0]["app_id"] = -1
        fixture[ALL] = fixture[LATEST] = {"total_count": 0, "check_runs": []}
        fixture[f"{PREFIX}/check-suites?per_page=100"] = {"total_count": 0}
        fixture[STATUSES] = [{"id": 1, "context": "Required Actions", "state": "success"}]
        self.assertEqual(self.verdict(fixture)["verdict"], "PASS")

    def test_status_cannot_substitute_for_required_app(self):
        fixture = responses()
        fixture[ALL] = fixture[LATEST] = {"total_count": 0, "check_runs": []}
        fixture[STATUSES] = [{"id": 1, "context": "Required Actions", "state": "success"}]
        self.assertEqual(self.evaluate(fixture)[0]["result"], "MISSING")
        packet = self.verdict(fixture)
        self.assertEqual(packet["verdict"], "NOT_PROVEN")
        self.assertEqual(packet["checks"][0]["result"], "MISSING")

    def test_wrong_app_cannot_satisfy_required_name(self):
        fixture = responses()
        fixture[ALL]["check_runs"][0]["app"]["id"] = APP + 1
        fixture[LATEST]["check_runs"][0]["app"]["id"] = APP + 1
        self.assertEqual(self.evaluate(fixture)[0]["result"], "MISSING")

    def test_wrong_sha_never_passes(self):
        fixture = responses()
        fixture[LATEST]["check_runs"][0]["head_sha"] = "a" * 40
        self.assertEqual(self.verdict(fixture)["verdict"], "NOT_PROVEN")
        self.assertEqual(self.evaluate(fixture)[0]["result"], "STALE")

    def test_unfinished_rerun_vetoes_old_green_completion(self):
        fixture = responses()
        pending = check_run(id=2, status="queued", conclusion=None)
        pending["started_at"] = None
        fixture[ALL] = {"total_count": 2, "check_runs": [check_run(), pending]}
        self.assertEqual(self.evaluate(fixture)[0]["result"], "PENDING")

    def test_new_success_can_replace_historical_failure(self):
        fixture = responses()
        fixture[ALL] = {"total_count": 2, "check_runs": [check_run(id=100, conclusion="failure"), check_run(id=1)]}
        self.assertEqual(self.verdict(fixture)["verdict"], "PASS")

    def test_latest_failed_attempt_vetoes_historical_success(self):
        fixture = responses(conclusion="failure")
        fixture[ALL] = {"total_count": 2, "check_runs": [check_run(id=100), check_run(id=1, conclusion="failure")]}
        self.assertEqual(self.verdict(fixture)["verdict"], "FAIL")

    def test_ambiguous_latest_identity_refuses(self):
        fixture = responses()
        fixture[LATEST] = {"total_count": 2, "check_runs": [check_run(), check_run(id=2)]}
        self.assertEqual(self.verdict(fixture)["verdict"], "NOT_PROVEN")

    def test_population_moving_between_all_and_latest_does_not_pass(self):
        fixture = responses()
        fixture[LATEST]["check_runs"][0]["id"] = 2
        self.assertEqual(self.evaluate(fixture)[0]["result"], "PENDING")

    def test_pending_attempt_can_complete_within_the_same_bound(self):
        fixture = responses()
        clock = Clock()
        transport = FixtureTransport(fixture)
        def get(path):
            if clock.now == 0 and path in (ALL, LATEST):
                return observer.ApiResult(200, json.dumps({"total_count": 1, "check_runs": [
                    check_run(status="queued", conclusion=None)]}).encode())
            return transport(path)
        packet = checker.wait_for_required_ci(REPOSITORY, "main", SHA, timeout=1, interval=.1,
                                              transport=get, clock=clock, sleep=clock.sleep)
        self.assertEqual(packet["verdict"], "PASS")
        self.assertGreater(clock.now, 0)

    def test_cancel_skip_unknown_and_neutral_remain_distinct(self):
        for conclusion, expected in (("cancelled", "NOT_PROVEN"), ("skipped", "NOT_PROVEN"),
                                     ("future-state", "NOT_PROVEN"), ("neutral", "PASS")):
            with self.subTest(conclusion=conclusion):
                self.assertEqual(self.verdict(responses(conclusion=conclusion))["verdict"], expected)

    def test_classic_and_rulesets_are_additive_with_multiple_app_bindings(self):
        fixture = responses()
        add_ruleset(fixture, app=APP + 1)
        union = self.policy(fixture)
        self.assertEqual(union[0]["app_ids"], [APP, APP + 1])
        self.assertEqual([row["result"] for row in self.evaluate(fixture)], ["SUCCESS", "MISSING"])

    def test_inactive_and_other_branch_rulesets_do_not_add_requirements(self):
        for options in ({"enforcement": "disabled"}, {"enforcement": "evaluate"},
                        {"include": ["refs/heads/other"]}):
            with self.subTest(options=options):
                fixture = responses()
                add_ruleset(fixture, app=APP + 1, **options)
                self.assertEqual(self.verdict(fixture)["verdict"], "PASS")

    def test_unproven_active_selector_refuses(self):
        fixture = responses()
        add_ruleset(fixture, include=["refs/heads/*"])
        with self.assertRaisesRegex(checker.NotProven, "union is unproven"):
            self.policy(fixture)

    def test_unreadable_enforcement_is_not_an_empty_policy(self):
        for path in (CLASSIC, RULESETS):
            for code in (401, 403, 500):
                with self.subTest(path=path, code=code):
                    fixture = responses()
                    fixture[path] = observer.ApiResult(code, b'{}')
                    with self.assertRaisesRegex(checker.NotProven, "policy is unreadable"):
                        self.policy(fixture)

    def test_generic_404_is_not_classic_absence(self):
        fixture = responses()
        fixture[CLASSIC] = observer.ApiResult(404, b'{"message":"Not Found"}')
        with self.assertRaisesRegex(checker.NotProven, "absence is not corroborated"):
            self.policy(fixture)

    def test_explicit_classic_absence_preserves_ruleset_requirements(self):
        fixture = responses()
        fixture[CLASSIC] = observer.ApiResult(404, b'{"message":"Branch not protected"}')
        add_ruleset(fixture)
        self.assertEqual(self.verdict(fixture)["verdict"], "PASS")

    def test_empty_policy_cannot_pass_vacuously(self):
        fixture = responses()
        fixture[CLASSIC]["required_status_checks"] = None
        with self.assertRaisesRegex(checker.NotProven, "union is unproven or empty"):
            self.policy(fixture)

    def test_unreadable_or_truncated_ruleset_details_refuse(self):
        fixture = responses()
        add_ruleset(fixture)
        fixture[f"repos/{REPOSITORY}/rulesets/17"] = observer.ApiResult(403)
        with self.assertRaisesRegex(checker.NotProven, "policy is unreadable"):
            self.policy(fixture)
        fixture = responses()
        fixture[RULESETS] = observer.ApiResult(200, b'[]', link='<next>; rel="next"')
        with self.assertRaisesRegex(checker.NotProven, "policy is unreadable"):
            self.policy(fixture)

    def test_wrong_repository_branch_and_tip_refuse_before_check_collection(self):
        for field, value in (("full_name", "other/repo"), ("default_branch", "other")):
            fixture = responses()
            fixture[f"repos/{REPOSITORY}"][field] = value
            with self.subTest(field=field), self.assertRaises(checker.NotProven):
                self.policy(fixture)
        fixture = responses()
        fixture[f"repos/{REPOSITORY}/git/ref/heads/main"]["object"]["sha"] = "b" * 40
        with self.assertRaisesRegex(checker.NotProven, "current default-branch tip"):
            self.policy(fixture)

    def test_tip_moving_after_check_read_refuses(self):
        fixture = responses()
        transport = FixtureTransport(fixture)
        def get(path):
            if path.endswith("/git/ref/heads/main") and path in transport.calls:
                return observer.ApiResult(200, json.dumps({"ref": "refs/heads/main", "object": {
                    "type": "commit", "sha": "b" * 40}}).encode())
            return transport(path)
        with self.assertRaisesRegex(checker.NotProven, "current default-branch tip"):
            checker.wait_for_required_ci(REPOSITORY, "main", SHA, timeout=1, interval=.1, transport=get)

    def test_app_policy_changing_after_check_read_refuses(self):
        fixture = responses()
        transport = FixtureTransport(fixture)
        def get(path):
            if path == CLASSIC and path in transport.calls:
                changed = responses()[CLASSIC]
                changed["required_status_checks"]["checks"][0]["app_id"] = APP + 1
                return observer.ApiResult(200, json.dumps(changed).encode())
            return transport(path)
        with self.assertRaisesRegex(checker.NotProven, "policy changed"):
            checker.wait_for_required_ci(REPOSITORY, "main", SHA, timeout=1, interval=.1, transport=get)

    def test_required_result_on_second_page_is_consumed(self):
        fixture = responses()
        first = [dict(check_run(id=i + 1), name=f"Advisory {i}") for i in range(100)]
        for filter in ("all", "latest"):
            fixture[f"{PREFIX}/check-runs?filter={filter}&per_page=100&page=1"] = {
                "total_count": 101, "check_runs": first}
            fixture[f"{PREFIX}/check-runs?filter={filter}&per_page=100&page=2"] = {
                "total_count": 101, "check_runs": [check_run(id=101)]}
        self.assertEqual(self.verdict(fixture)["verdict"], "PASS")
        fixture[f"{PREFIX}/check-runs?filter=latest&per_page=100&page=2"]["check_runs"][0]["conclusion"] = "failure"
        self.assertEqual(self.verdict(fixture)["verdict"], "FAIL")

    def test_incomplete_changed_and_duplicate_pages_refuse(self):
        first = [dict(check_run(id=i + 1), name=f"Advisory {i}") for i in range(100)]
        for second in ({"total_count": 101, "check_runs": []},
                       {"total_count": 102, "check_runs": [check_run(id=101)]},
                       {"total_count": 101, "check_runs": [check_run(id=1)]}):
            fixture = responses()
            fixture[LATEST] = {"total_count": 101, "check_runs": first}
            fixture[f"{PREFIX}/check-runs?filter=latest&per_page=100&page=2"] = second
            with self.subTest(second=second), self.assertRaises(checker.NotProven):
                checker.collect_pages(FixtureTransport(fixture), REPOSITORY, SHA, "check-runs")

    def test_latest_legacy_order_is_not_numeric_id_order(self):
        fixture = responses()
        fixture[CLASSIC]["required_status_checks"]["checks"][0]["app_id"] = -1
        fixture[ALL] = fixture[LATEST] = {"total_count": 0, "check_runs": []}
        fixture[STATUSES] = [{"id": 1, "context": "Required Actions", "state": "success"},
                             {"id": 100, "context": "Required Actions", "state": "failure"}]
        self.assertEqual(self.verdict(fixture)["verdict"], "PASS")
        fixture[STATUSES][0]["state"] = "failure"
        self.assertEqual(self.verdict(fixture)["verdict"], "FAIL")

    def test_legacy_status_pagination_retains_the_documented_order(self):
        fixture = responses()
        fixture[CLASSIC]["required_status_checks"]["checks"][0]["app_id"] = -1
        first = [{"id": i + 1, "context": f"Advisory {i}", "state": "success"} for i in range(100)]
        fixture[STATUSES] = first
        fixture[f"{PREFIX}/statuses?per_page=100&page=2"] = [
            {"id": 101, "context": "Required Actions", "state": "failure"}]
        self.assertEqual(self.verdict(fixture)["verdict"], "FAIL")

    def test_ref_api_suite_limit_cannot_hide_active_attempts(self):
        for count in (-1, True, 1000, 1001):
            fixture = responses()
            fixture[f"{PREFIX}/check-suites?per_page=100"] = {"total_count": count}
            with self.subTest(count=count), self.assertRaisesRegex(checker.NotProven, "ref API limit"):
                self.verdict(fixture)

    def test_malformed_app_and_result_values_do_not_pass(self):
        fixture = responses()
        fixture[LATEST]["check_runs"][0]["app"]["id"] = True
        with self.assertRaisesRegex(checker.NotProven, "app identity is malformed"):
            self.verdict(fixture)
        for value in (None, [], {}, 7):
            fixture = responses()
            fixture[LATEST]["check_runs"][0]["conclusion"] = value
            with self.subTest(value=value):
                self.assertEqual(self.verdict(fixture)["verdict"], "NOT_PROVEN")

    def test_legacy_failure_is_not_hidden_by_green_unbound_check_run(self):
        fixture = responses()
        fixture[CLASSIC]["required_status_checks"]["checks"][0]["app_id"] = -1
        fixture[STATUSES] = [{"id": 1, "context": "Required Actions", "state": "failure"}]
        self.assertEqual(self.verdict(fixture)["verdict"], "FAIL")

    def test_http_deadline_is_passed_to_existing_transport(self):
        with mock.patch.object(observer.urllib.request, "build_opener") as build:
            response = build.return_value.open.return_value.__enter__.return_value
            response.status = 200
            response.read.return_value = b'{}'
            response.headers.get.return_value = ""
            self.assertTrue(observer.http_transport("fake-token", timeout=0.25)("repos/example/repo").ok)
            self.assertEqual(build.return_value.open.call_args.kwargs["timeout"], 0.25)

    def test_response_arriving_after_deadline_cannot_pass(self):
        clock = Clock()
        def get(path):
            clock.now = 2
            return observer.ApiResult(200, b'{}')
        with self.assertRaisesRegex(checker.NotProven, "deadline expired"):
            checker.wait_for_required_ci(REPOSITORY, "main", SHA, timeout=1, interval=.1,
                                         transport=get, clock=clock, sleep=clock.sleep)

    def test_cli_rejects_invalid_subject_and_unbounded_polling_without_network(self):
        base = ["--repository", REPOSITORY, "--branch", "main", "--sha", SHA]
        cases = [base[:-1] + ["main"], base + ["--timeout-seconds", "nan"],
                 base + ["--timeout-seconds", "0"], base + ["--timeout-seconds", "601"],
                 base + ["--poll-interval-seconds", "0"], base + ["--poll-interval-seconds", "inf"]]
        for argv in cases:
            with self.subTest(argv=argv), mock.patch("builtins.print"), mock.patch.object(checker, "wait_for_required_ci") as wait:
                self.assertEqual(checker.main(argv), 1)
                wait.assert_not_called()


if __name__ == "__main__":
    unittest.main()
