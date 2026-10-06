#!/usr/bin/env python3
"""Execute the nightly alert's real YAML shell in an empty, non-checkout cwd.

Run with Python, PyYAML, Bash, and jq. All gh calls use a recording fake;
neither GitHub access nor benchmark/Rust builds are needed.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from bash_binary import bash_binary, bash_path

WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/perf-nightly-alert.yml"
REPOSITORY = "fixture-owner/nightly-project"
TITLE = "[perf] nightly benchmark regressions (rolling)"
REGRESSION = "### 🔴 Critical Regressions\n| parse | +25% |\n"
FAKE_GH = r'''
import json
import os
import sys
from pathlib import Path

args = sys.argv[1:]
with open(os.environ["FAKE_GH_LOG"], "a", encoding="utf-8") as log:
    log.write(json.dumps({"argv": args, "GH_REPO": os.environ.get("GH_REPO"),
                          "cwd_files": sorted(p.name for p in Path.cwd().iterdir())}) + "\n")
if os.environ.get("GH_REPO") != os.environ["FAKE_REPOSITORY"]:
    sys.exit("fake gh: missing or wrong GH_REPO outside a Git checkout")
for index, arg in enumerate(args):
    explicit = None
    if arg in ("--repo", "-R"):
        explicit = args[index + 1]
    elif arg.startswith("--repo="):
        explicit = arg.split("=", 1)[1]
    elif arg.startswith("-R"):
        explicit = arg[2:]
    if explicit is not None and explicit != os.environ["FAKE_REPOSITORY"]:
        sys.exit("fake gh: wrong explicit repository")
state_path = Path(os.environ["FAKE_GH_STATE"])
state = json.loads(state_path.read_text(encoding="utf-8"))
def flag(name):
    return args[args.index(name) + 1]
if args[:2] == ["issue", "list"]:
    print(json.dumps(state))
elif args[:2] == ["issue", "create"]:
    state.append({"number": 42, "title": flag("--title"), "body": flag("--body"), "comments": []})
    print("https://github.com/fixture-owner/nightly-project/issues/42")
elif args[:2] in (["issue", "view"], ["issue", "comment"]):
    issue = next(i for i in state if str(i["number"]) == args[2])
    if args[1] == "view":
        print("\n".join([issue["body"], *[c["body"] for c in issue["comments"]]]))
    else:
        issue["comments"].append({"body": flag("--body")})
else:
    sys.exit("fake gh: unexpected command: " + repr(args))
state_path.write_text(json.dumps(state), encoding="utf-8")
'''


class NightlyAlertTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="perl-nightly-alert-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.cwd = self.root / "empty-cwd"
        self.cwd.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.bench = self.root / "nightly-bench"
        self.bench.mkdir()
        self.summary = self.root / "summary.md"
        self.log = self.root / "gh.jsonl"
        self.state = self.root / "issues.json"
        self.state.write_text("[]", encoding="utf-8")
        self.fake_script = self.root / "fake_gh.py"
        self.fake_script.write_text(FAKE_GH, encoding="utf-8", newline="\n")
        fake = self.bin / "gh"
        fake.write_text('#!/usr/bin/env bash\nexec "$FAKE_PYTHON" "$FAKE_GH_SCRIPT" "$@"\n', encoding="utf-8", newline="\n")
        fake.chmod(0o755)
        workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
        self.step = next(s for s in workflow["jobs"]["alert"]["steps"] if s.get("name") == "Upsert rolling regression issue")
        self.script = self.root / "step.sh"
        self.script.write_text(self.step["run"], encoding="utf-8", newline="\n")

    def calls(self) -> list[dict]:
        return [json.loads(line) for line in self.log.read_text(encoding="utf-8").splitlines()] if self.log.exists() else []

    def issues(self) -> list[dict]:
        return json.loads(self.state.read_text(encoding="utf-8"))

    def seed_issue(self, body: str = "An earlier nightly", comments: list[str] | None = None) -> None:
        self.state.write_text(json.dumps([{"number": 42, "title": TITLE, "body": body,
                                           "comments": [{"body": c} for c in comments or []]}]), encoding="utf-8")

    def run_step(self, *, alert: str | None = REGRESSION, outcome: str = "success",
                 run_id: str = "1001", attempt: str = "1", omit_repo: bool = False) -> subprocess.CompletedProcess:
        alert_path = self.bench / "alert.md"
        if alert is not None:
            alert_path.write_text(alert, encoding="utf-8")
        else:
            alert_path.unlink(missing_ok=True)
        # Resolve only expressions actually present in the YAML env. Injecting
        # GH_REPO here unconditionally would hide the original workflow defect.
        context = {
            "github.repository": REPOSITORY,
            "secrets.GITHUB_TOKEN": "fixture-token",
            "github.event.workflow_run.html_url": f"https://github.com/{REPOSITORY}/actions/runs/{run_id}",
            "github.event.workflow_run.id": run_id,
            "github.event.workflow_run.run_attempt": attempt,
            "github.event.workflow_run.head_sha": "a" * 40,
            "runner.temp": bash_path(self.root),
            "steps.dl.outcome": outcome,
        }
        env = {key: os.environ[key] for key in ("PATH", "SystemRoot", "WINDIR", "ProgramFiles", "TEMP", "TMP") if key in os.environ}
        for name, value in self.step["env"].items():
            env[name] = re.sub(r"\$\{\{\s*(.*?)\s*\}\}", lambda m: context[m[1]], str(value))
        if omit_repo:
            env.pop("GH_REPO", None)
        env.update({
            "GITHUB_STEP_SUMMARY": bash_path(self.summary), "FAKE_BIN": bash_path(self.bin),
            "FAKE_PYTHON": bash_path(Path(sys.executable)), "FAKE_GH_SCRIPT": bash_path(self.fake_script),
            "FAKE_GH_LOG": bash_path(self.log), "FAKE_GH_STATE": bash_path(self.state),
            # Byte matching avoids Git/MSYS's non-BMP regex locale behavior;
            # this harness makes no runner locale or installed-platform claim.
            "FAKE_REPOSITORY": REPOSITORY, "STEP_SCRIPT": bash_path(self.script), "LC_ALL": "C", "PYTHONUTF8": "1",
        })
        # A drive colon is a PATH separator inside Git Bash. Native paths work
        # for file arguments, but the fake's PATH directory must use /c/... .
        if sys.platform == "win32":
            env["FAKE_BIN"] = "/" + self.bin.drive[0].lower() + self.bin.as_posix()[2:]
        self.assertEqual(list(self.cwd.iterdir()), [])
        result = subprocess.run([bash_binary(), "--noprofile", "--norc", "-c",
                                 'export PATH="$FAKE_BIN:$PATH"\n'
                                 'test "$(command -v gh)" = "$FAKE_BIN/gh" || exit 97\n'
                                 'source "$STEP_SCRIPT"'],
                                cwd=self.cwd, env=env, capture_output=True, text=True, encoding="utf-8", timeout=20)
        self.assertEqual(list(self.cwd.iterdir()), [])
        for call in self.calls():
            self.assertEqual(call["cwd_files"], [])
        return result

    def assert_success(self, result: subprocess.CompletedProcess) -> None:
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for call in self.calls():
            self.assertEqual(call["GH_REPO"], REPOSITORY)

    def test_missing_download_makes_no_gh_calls(self) -> None:
        self.assert_success(self.run_step(outcome="failure"))
        self.assertEqual(self.calls(), [])
        self.assertIn("cannot evaluate regressions", self.summary.read_text(encoding="utf-8"))

    def test_missing_alert_makes_no_gh_calls(self) -> None:
        self.assert_success(self.run_step(alert=None))
        self.assertEqual(self.calls(), [])
        self.assertIn("No alert.md", self.summary.read_text(encoding="utf-8"))

    def test_clean_alert_makes_no_gh_calls(self) -> None:
        self.assert_success(self.run_step(alert="### ✅ Performance Improvements\n| parse | -10% |\n"))
        self.assertEqual(self.calls(), [])
        self.assertIn("nothing to file", self.summary.read_text(encoding="utf-8"))

    def test_regression_creates_issue_in_bound_repository(self) -> None:
        self.assert_success(self.run_step())
        self.assertEqual([c["argv"][:2] for c in self.calls()], [["issue", "list"], ["issue", "create"]])
        issue = self.issues()[0]
        self.assertEqual(issue["title"], TITLE)
        self.assertIn("nightly-run 1001 attempt 1", issue["body"])
        self.assertIn("| parse | +25% |", issue["body"])

    def test_existing_issue_receives_comment(self) -> None:
        self.seed_issue()
        self.assert_success(self.run_step(alert=REGRESSION.replace("🔴 Critical", "⚠️ Performance")))
        self.assertEqual([c["argv"][:2] for c in self.calls()], [["issue", "list"], ["issue", "view"], ["issue", "comment"]])
        self.assertIn("nightly-run 1001 attempt 1", self.issues()[0]["comments"][0]["body"])

    def test_same_run_attempt_is_deduplicated_in_body_or_comments(self) -> None:
        for in_comments in (False, True):
            with self.subTest(in_comments=in_comments):
                token = "nightly-run 1001 attempt 1"
                self.seed_issue(body="Earlier run" if in_comments else token,
                                comments=[token] if in_comments else [])
                before = self.issues()
                start = len(self.calls())
                self.assert_success(self.run_step())
                self.assertEqual([c["argv"][:2] for c in self.calls()[start:]], [["issue", "list"], ["issue", "view"]])
                self.assertEqual(self.issues(), before)
                self.assertIn("skipping duplicate", self.summary.read_text(encoding="utf-8"))

    def test_same_sha_different_run_receives_distinct_comment(self) -> None:
        self.assert_success(self.run_step())
        self.assert_success(self.run_step(run_id="1002"))
        issue = self.issues()[0]
        self.assertEqual(len(issue["comments"]), 1)
        self.assertIn("nightly-run 1001 attempt 1", issue["body"])
        self.assertIn("nightly-run 1002 attempt 1", issue["comments"][0]["body"])
        self.assertIn("a" * 40, issue["body"])
        self.assertIn("a" * 40, issue["comments"][0]["body"])

    def test_same_run_new_attempt_receives_distinct_comment(self) -> None:
        self.assert_success(self.run_step())
        self.assert_success(self.run_step(attempt="2"))
        self.assertIn("nightly-run 1001 attempt 2", self.issues()[0]["comments"][0]["body"])

    def test_missing_binding_is_detected_by_fake_gh(self) -> None:
        result = self.run_step(omit_repo=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing or wrong GH_REPO", result.stderr)
        self.assertEqual(len(self.calls()), 1)
        self.assertIsNone(self.calls()[0]["GH_REPO"])
        self.assertEqual(self.issues(), [])

    def test_wrong_per_command_repository_is_rejected(self) -> None:
        # gh's per-command selector overrides GH_REPO. A binding-only fake
        # would incorrectly accept this realistic wrong implementation.
        for selector in ("--repo wrong-owner/wrong-project", "--repo=wrong-owner/wrong-project",
                         "-R wrong-owner/wrong-project", "-Rwrong-owner/wrong-project"):
            with self.subTest(selector=selector):
                mutant = self.step["run"].replace("gh issue create --title", f"gh issue create {selector} --title")
                self.assertNotEqual(mutant, self.step["run"])
                self.script.write_text(mutant, encoding="utf-8", newline="\n")
                result = self.run_step()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("wrong explicit repository", result.stderr)
                self.assertEqual(self.calls()[-1]["argv"][:2], ["issue", "create"])
                self.assertEqual(self.issues(), [])


if __name__ == "__main__":
    unittest.main()
