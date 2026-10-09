#!/usr/bin/env python3
"""Execute the production RIPR router with offline inventory and clock fixtures."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = Path(os.environ.get("RIPR_ROUTER_WORKFLOW", ROOT / ".github/workflows/ripr.yml"))


def production_script():
    section = WORKFLOW.read_text().split("      - name: Decide target runner\n", 1)[1]
    lines = section.split("        run: |\n", 1)[1].splitlines()
    script = []
    for line in lines:
        if line.strip() and not line.startswith("          "):
            break
        script.append(line[10:] if line.strip() else "")
    return "\n".join(script) + "\n"


def runner(online=True, busy=False, labels=None, identity=1):
    return {"id": identity, "status": "online" if online else "offline", "busy": busy,
            "labels": [{"name": label} for label in
                       (labels if labels is not None else ["EM-CI", "Rust-Standard", "trusted-pr"])]}


def page(runners, total=None):
    return {"total_count": len(runners) if total is None else total, "runners": runners}


FAKE_CURL = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
from urllib.parse import urlparse, parse_qs
args = sys.argv[1:]
out = args[args.index('-o') + 1]
url = next(a for a in args if a.startswith('https://'))
query = parse_qs(urlparse(url).query)
assert urlparse(url).path == '/orgs/fixture/actions/runners'
assert query.get('per_page') == ['100']
call = {'page': int(query.get('page', ['1'])[0]),
        'max_time': float(args[args.index('--max-time') + 1]) if '--max-time' in args else None,
        'connect_timeout': float(args[args.index('--connect-timeout') + 1]) if '--connect-timeout' in args else None}
calls = pathlib.Path(os.environ['CALLS'])
previous = calls.read_text().splitlines() if calls.exists() else []
with calls.open('a') as f: f.write(json.dumps(call) + '\n')
fixtures = json.loads(os.environ['RESPONSES'])
fixture = fixtures[min(len(previous), len(fixtures)-1)]
clock = pathlib.Path(os.environ['CLOCK'])
clock.write_text(str(int(clock.read_text()) + fixture.get('elapsed', 0)))
body = fixture.get('body', {'total_count': 0, 'runners': []})
pathlib.Path(out).write_text(body if isinstance(body, str) else json.dumps(body))
print(fixture.get('status', '200'), end='')
sys.exit(fixture.get('exit', 0))
'''
FAKE_DATE = '''#!/usr/bin/env python3
import os, pathlib
print(pathlib.Path(os.environ['CLOCK']).read_text())
'''
FAKE_SLEEP = '''#!/usr/bin/env python3
import os, pathlib, sys
clock = pathlib.Path(os.environ['CLOCK'])
clock.write_text(str(int(clock.read_text()) + int(sys.argv[1])))
'''


class RouterTests(unittest.TestCase):
    def route(self, responses, probe=60, **overrides):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, contents in (("curl", FAKE_CURL), ("date", FAKE_DATE), ("sleep", FAKE_SLEEP)):
                executable = root / name
                executable.write_text(contents)
                executable.chmod(0o755)
            (root / "clock").write_text("1000")
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ["PATH"],
                       GH_TOKEN="fixture-only", ORG="fixture", IS_FORK_PR="false",
                       PR_AUTHOR_LOGIN="human", PR_AUTHOR_TYPE="User",
                       RIPR_ROUTER_PROBE_SECONDS=str(probe), RESPONSES=json.dumps(responses),
                       CALLS=str(root / "calls"), CLOCK=str(root / "clock"),
                       GITHUB_OUTPUT=str(root / "out"), GITHUB_STEP_SUMMARY=str(root / "summary"),
                       TMPDIR=str(root))
            env.update(overrides)
            result = subprocess.run(["bash", "--noprofile", "--norc", "-c", production_script()],
                                    env=env, capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            outputs = dict(line.split("=", 1) for line in (root / "out").read_text().splitlines())
            calls = [json.loads(line) for line in (root / "calls").read_text().splitlines()] if (root / "calls").exists() else []
            return outputs, calls, int((root / "clock").read_text()) - 1000

    def assert_hosted(self, outputs, reason="runner_api_failed"):
        self.assertEqual(outputs, {"target": "github", "reason": reason, "fallback_allowed": "true"})

    def test_empty_snapshot_is_refetched_and_new_capacity_routes_selfhosted(self):
        outputs, calls, _ = self.route([{"body": page([])}, {"body": page([runner()])}])
        self.assertEqual(outputs["target"], "selfhosted")
        self.assertEqual(outputs["fallback_allowed"], "false")
        self.assertEqual([call["page"] for call in calls], [1, 1])

    def test_pagination_finds_eligible_runner_on_second_page(self):
        first = [runner(online=False, identity=i) for i in range(100)]
        outputs, calls, _ = self.route([{"body": page(first, 101)}, {"body": page([runner(identity=100)], 101)}], probe=0)
        self.assertEqual(outputs["target"], "selfhosted")
        self.assertEqual([call["page"] for call in calls], [1, 2])

    def test_busy_online_case_insensitive_pool_member_remains_eligible(self):
        outputs, calls, _ = self.route([{"body": page([runner(busy=True)])}], probe=0)
        self.assertEqual(outputs["target"], "selfhosted")
        self.assertEqual(len(calls), 1)

    def test_offline_or_wrong_capability_is_not_eligible(self):
        for member in (runner(online=False), runner(labels=["em-ci", "rust-light", "trusted-pr"]),
                       runner(labels=["em-ci", "rust-standard"])):
            with self.subTest(member=member):
                outputs, _, _ = self.route([{"body": page([member])}], probe=0)
                self.assert_hosted(outputs, "no_rust_standard_capacity_online")

    def test_every_request_has_connect_and_total_time_limits(self):
        _, calls, _ = self.route([{"body": page([runner()])}], probe=0)
        self.assertGreater(len(calls), 0)
        for call in calls:
            self.assertEqual(call["connect_timeout"], 5)
            self.assertGreater(call["max_time"], 0)
            self.assertLessEqual(call["max_time"], 30)

    def test_http_auth_failure_and_transport_timeout_fall_back(self):
        for fixture in ({"status": "401"}, {"status": "503"}, {"status": "000", "exit": 28},
                        {"status": "200", "exit": 28, "body": page([runner()])}):
            with self.subTest(fixture=fixture):
                outputs, calls, _ = self.route([fixture])
                self.assert_hosted(outputs)
                self.assertEqual(len(calls), 1)

    def test_malformed_http_200_is_uncertainty_not_capacity(self):
        for body in ("broken", {}, {"runners": []}, page([], -1), page([], 0.5), page([], 2**63),
                     {"total_count": 0, "runners": None}, page([{"status": "online"}]),
                     {"total_count": "0", "runners": []},
                     page([{**runner(), "labels": [{"name": 1}]}]),
                     page([{**runner(), "status": None}])):
            with self.subTest(body=body):
                outputs, _, _ = self.route([{"body": body}], probe=0)
                self.assert_hosted(outputs)

    def test_integral_json_number_is_canonicalized_before_shell_arithmetic(self):
        outputs, _, _ = self.route([{"body": page([runner()], 1.0)}], probe=0)
        self.assertEqual(outputs["target"], "selfhosted")

    def test_multiple_json_documents_are_not_a_single_inventory(self):
        body = json.dumps(page([runner()])) + "\n" + json.dumps(page([]))
        outputs, _, _ = self.route([{"body": body}], probe=0)
        self.assert_hosted(outputs)

    def test_partial_or_changing_pagination_never_asserts_capacity(self):
        first = [runner(identity=i) for i in range(100)]
        for second in ({"status": "401"}, {"body": page([], 101)},
                       {"body": page([runner()], 102)}, {"body": "broken"}):
            with self.subTest(second=second):
                outputs, calls, _ = self.route([{"body": page(first, 101)}, second], probe=0)
                self.assert_hosted(outputs)
                self.assertEqual(len(calls), 2)

    def test_refresh_failure_does_not_reuse_previous_snapshot(self):
        outputs, calls, _ = self.route([{"body": page([])}, {"status": "401"}])
        self.assert_hosted(outputs)
        self.assertEqual(len(calls), 2)

    def test_probe_exhaustion_is_bounded_and_reports_no_capacity(self):
        outputs, calls, elapsed = self.route([{"body": page([])}], probe=35)
        self.assert_hosted(outputs, "no_rust_standard_capacity_online")
        self.assertEqual([call["page"] for call in calls], [1, 1])
        self.assertEqual(elapsed, 35)

    def test_refresh_request_is_limited_by_remaining_probe_time(self):
        outputs, calls, _ = self.route([{"body": page([])}, {"body": page([runner()])}], probe=35)
        self.assertEqual(outputs["target"], "selfhosted")
        self.assertEqual([call["max_time"] for call in calls], [30, 5])

    def test_pagination_requests_share_snapshot_deadline(self):
        first = [runner(online=False, identity=i) for i in range(100)]
        outputs, calls, elapsed = self.route([{"body": page(first, 201), "elapsed": 30}], probe=0)
        self.assert_hosted(outputs)
        self.assertEqual(len(calls), 2)
        self.assertEqual(elapsed, 60)

    def test_forks_bots_and_missing_token_do_not_query_inventory(self):
        for override, reason in (({"IS_FORK_PR": "true"}, "fork_pr"),
                                 ({"PR_AUTHOR_TYPE": "Bot"}, "bot_pr_github_hosted"),
                                 ({"PR_AUTHOR_LOGIN": "dependabot[bot]"}, "bot_pr_github_hosted"),
                                 ({"GH_TOKEN": ""}, "runner_token_missing")):
            with self.subTest(override=override):
                outputs, calls, _ = self.route([{}], **override)
                self.assert_hosted(outputs, reason)
                self.assertEqual(calls, [])


if __name__ == "__main__":
    unittest.main()
