#!/usr/bin/env python3
"""Real Git controls for RIPR's workflow operand adapter (#15294).

Recording cargo exercises the shipped CLI wiring, not RIPR's domain analysis.
The fixture identities and patch digest are literal independent Git oracles.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ripr.yml"
GUARD = ROOT / "scripts/ci/require-ripr-subject.sh"
LANES = ("ripr-selfhosted", "ripr-github", "ripr-fallback")
A = "022a38b47cf27baf086611017cc522b89ec1df2f"
H = "7db766bf1941a7faa960074e63ed5b204d7fe04b"
E = "b193a7840cf3e9df7c9883dff94d5bdc90cffbf3"
B = "8bd75ed2e244b3f29239593d35bbb2de6dfd8f14"
C = "c824af93c0e3f4f3e0666acc45370a7868457a11"
DIGEST = "066459f45988da46a0ef0757de85dba5cbdc3061ddf25fb09c5d56befe47040c"
GIT_ENV_KEYS = subprocess.check_output(
    ["git", "rev-parse", "--local-env-vars"], text=True).split()
ENV = {k: v for k, v in os.environ.items() if k not in GIT_ENV_KEYS}
ENV.update(
    GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
    GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
    GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@example.invalid",
    GIT_AUTHOR_DATE="2020-01-01T00:00:00+0000",
    GIT_COMMITTER_DATE="2020-01-01T00:00:00+0000",
)
RECORDER = '''#!/usr/bin/env python3
import hashlib, json, subprocess, sys
args = sys.argv[1:]
def arg(name): return args[args.index(name) + 1]
def sha(ref):
    return subprocess.check_output(['git', 'rev-parse', ref + '^{commit}'], text=True).strip()
base, head = arg('--base'), arg('--head')
patch = subprocess.check_output(['git', 'diff', '--binary', '--full-index', base + '...' + head])
print(json.dumps({'base_sha': sha(base), 'head_sha': sha(head),
                  'pr_head_sha': arg('--pr-head'),
                  'digest': hashlib.sha256(patch).hexdigest()}))
'''


def lane_text(lane):
    text = WORKFLOW.read_text()
    return re.search(r"^  " + re.escape(lane) + r":\n(.*?)(?=^  [\w-]+:|\Z)",
                     text, re.M | re.S).group(1)


class RiprEventOperands(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name) / "repo"
        self.repo.mkdir()
        self.env = dict(ENV)
        self.git("init", "--initial-branch=main")
        self.assertEqual(self.commit("src/lib.rs", "pub fn value() -> u8 { 1 }\n", "event base"), A)
        self.git("checkout", "-b", "feature")
        self.assertEqual(self.commit("src/lib.rs", "pub fn value() -> u8 { 2 }\n", "candidate"), H)
        self.git("checkout", "main")
        self.git("merge", "--no-ff", "feature", "-m", "event integration")
        self.assertEqual(self.git("rev-parse", "HEAD"), E)
        self.git("checkout", "-b", "moving-main", A)
        self.assertEqual(self.commit("docs/one.md", "unrelated one\n", "base advance B"), B)
        self.assertEqual(self.commit("docs/two.md", "unrelated two\n", "base advance C"), C)
        self.git("checkout", "--detach", E)
        self.git("update-ref", "refs/remotes/origin/main", A)
        self.git("remote", "add", "origin", str(Path(self.temp.name) / "unavailable.git"))
        self.bin = Path(self.temp.name) / "bin"
        self.bin.mkdir()
        cargo = self.bin / "cargo"
        cargo.write_text(RECORDER)
        cargo.chmod(0o755)
        self.env.update(
            PATH=str(self.bin) + os.pathsep + ENV["PATH"],
            GITHUB_EVENT_NAME="pull_request", RIPR_BASE_SHA=A,
            RIPR_EVENT_HEAD_SHA=H, RIPR_EVALUATED_SHA=E, PR_HEAD_SHA=H,
            BASE_REF="main",  # Retained solely to expose the old implementation.
        )

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo,
                                       env=self.env, stderr=subprocess.DEVNULL,
                                       text=True).strip()

    def commit(self, path, content, message):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
        self.git("add", path)
        self.git("commit", "-m", message)
        return self.git("rev-parse", "HEAD")

    def guard(self, **changes):
        return subprocess.run(["bash", str(GUARD)], cwd=self.repo,
                              env=self.env | changes, text=True, capture_output=True)

    def stage(self, lane, check):
        text = lane_text(lane)
        commands = re.findall(r"cargo xtask ripr-pr --base [^\n]+", text)
        command = next(c for c in commands if ("--check" in c) == check)
        assignment = re.search(r'^\s*base_arg=.*$', text, re.M)
        script = (assignment.group(0) + "\n" if assignment else "") + command
        output = subprocess.check_output(["bash", "-euo", "pipefail", "-c", script],
                                         cwd=self.repo, env=self.env, text=True)
        return json.loads(output)

    def test_workflow_stages_keep_the_event_tuple_when_main_moves(self):
        expected = {"base_sha": A, "head_sha": E, "pr_head_sha": H, "digest": DIGEST}
        for lane in LANES:
            self.git("update-ref", "refs/remotes/origin/main", A)
            with self.subTest(lane=lane, stage="healthy"):
                self.assertEqual(self.stage(lane, False), expected)
            self.git("update-ref", "refs/remotes/origin/main", B)
            # The effective patch and merge-base do not change. Exact A still matters.
            self.assertEqual(self.git("merge-base", B, E), A)
            patch = subprocess.check_output(
                ["git", "diff", "--binary", "--full-index", B + "..." + E],
                cwd=self.repo, env=self.env)
            self.assertEqual(hashlib.sha256(patch).hexdigest(), DIGEST)
            with self.subTest(lane=lane, stage="production-after-B"):
                self.assertEqual(self.stage(lane, False), expected)
            self.git("update-ref", "refs/remotes/origin/main", C)
            with self.subTest(lane=lane, stage="validation-after-C"):
                self.assertEqual(self.stage(lane, True), expected)

    def test_guard_accepts_frozen_tuple_and_unchanged_non_pr_paths(self):
        for moving in (A, B, C):
            self.git("update-ref", "refs/remotes/origin/main", moving)
            result = self.guard()
            self.assertEqual(result.returncode, 0, result.stderr)
        for event in ("push", "merge_group", "workflow_dispatch"):
            result = self.guard(GITHUB_EVENT_NAME=event, RIPR_EVENT_HEAD_SHA=E, PR_HEAD_SHA="")
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_guard_refuses_missing_malformed_mismatched_or_unavailable_inputs(self):
        controls = [
            {"RIPR_BASE_SHA": ""}, {"RIPR_BASE_SHA": "origin/main"},
            {"RIPR_BASE_SHA": "0" * 40}, {"RIPR_BASE_SHA": "1" * 40},
            {"RIPR_BASE_SHA": B}, {"RIPR_EVENT_HEAD_SHA": A},
            {"RIPR_EVENT_HEAD_SHA": ""}, {"RIPR_EVALUATED_SHA": H},
            {"PR_HEAD_SHA": ""}, {"PR_HEAD_SHA": A},
            {"GITHUB_EVENT_NAME": "push", "RIPR_EVENT_HEAD_SHA": H, "PR_HEAD_SHA": ""},
            {"GITHUB_EVENT_NAME": "unknown"},
        ]
        for changes in controls:
            with self.subTest(changes=changes):
                result = self.guard(**changes)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("NOT_PROVEN", result.stderr)
        self.git("checkout", "--detach", H)
        result = self.guard()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("NOT_PROVEN", result.stderr)

    def test_missing_raw_head_is_fetched_by_sha_without_moving_the_tuple(self):
        remote = Path(self.temp.name) / "exact.git"
        self.git("clone", "--bare", str(self.repo), str(remote))
        shallow = Path(self.temp.name) / "shallow"
        self.git("clone", "--depth=1", remote.as_uri(), str(shallow))
        self.repo = shallow
        missing = subprocess.run(["git", "cat-file", "-e", H + "^{commit}"],
                                 cwd=self.repo, env=self.env, capture_output=True)
        self.assertNotEqual(missing.returncode, 0)
        self.git("update-ref", "refs/remotes/origin/main", E)
        result = self.guard()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.git("rev-parse", "HEAD"), E)
        self.assertEqual(self.git("rev-parse", "origin/main"), E)
        self.assertEqual(self.git("rev-parse", "FETCH_HEAD"), H)

    def test_every_consumer_and_container_uses_exact_operands(self):
        for lane in LANES:
            text = lane_text(lane)
            with self.subTest(lane=lane):
                self.assertIn("RIPR_BASE_SHA: ${{ github.event.pull_request.base.sha || github.event.merge_group.base_sha || github.event.before || inputs.base_sha }}", text)
                self.assertIn("RIPR_EVENT_HEAD_SHA: ${{ github.event.pull_request.head.sha || github.event.merge_group.head_sha || (github.event_name == 'push' && github.sha) || inputs.head_sha }}", text)
                self.assertIn("RIPR_EVALUATED_SHA: ${{ github.sha }}", text)
                self.assertNotIn("origin/$BASE_REF", text)
                self.assertNotIn("--head HEAD", text)
                self.assertNotIn("--ripr-head HEAD", text)
                self.assertIn("bash scripts/ci/require-ripr-subject.sh", text)
                for command in re.findall(r"cargo xtask ripr-(?:pr|review-comments) --base [^\n]+", text):
                    self.assertRegex(command, r'--base "\$(?:RIPR_BASE_SHA|base_arg)"')
                    self.assertIn('--head "$RIPR_EVALUATED_SHA"', command)
                    self.assertIn('--pr-head "$PR_HEAD_SHA"', command)
                self.assertIn('--ripr-base "$RIPR_BASE_SHA"', text)
                self.assertIn('--ripr-head "$RIPR_EVALUATED_SHA"', text)
                if lane != "ripr-fallback":
                    for name in ("RIPR_BASE_SHA", "RIPR_EVALUATED_SHA", "RIPR_EVENT_HEAD_SHA", "GITHUB_EVENT_NAME"):
                        self.assertRegex(text, r"-e " + name + r"(?:=|\s)")


if __name__ == "__main__":
    unittest.main()
