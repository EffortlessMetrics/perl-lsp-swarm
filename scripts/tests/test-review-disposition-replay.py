#!/usr/bin/env python3
"""Exercise the real disposition helper against an offline GitHub mutation spy.

This suite covers replay ordering only. Owning-PR admission, full-history reads,
and reply-identity confirmation remain separate #17495 obligations.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HELPER = Path(__file__).resolve().parents[1] / "reviews" / "disposition"
OLD = "a" * 40
NEW = "b" * 40


def marker(kind: str, evidence: dict, *, version: int = 1) -> str:
    value = {"v": version, "class": kind, "thread_id": "THREAD", "by": "fixture",
             "head": OLD, "evidence": evidence}
    return f"<!-- disposition:v{version} " + json.dumps(value) + " -->"


class DispositionReplayTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        # Missing tools must fail this CI proof, not turn it into a skipped green.
        if not shutil.which("bash") or not shutil.which("jq"):
            raise RuntimeError("bash and jq are required to exercise the real helper")

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.log = self.root / "effects.jsonl"
        self.fixture = self.root / "state.json"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        shim = self.bin / "gh"
        shim.write_text(f"#!{sys.executable}\n" + r'''
import json, os, pathlib, sys
args = sys.argv[1:]
query = next((arg.split("=", 1)[1] for arg in args if arg.startswith("query=")), "")
config = json.loads(pathlib.Path(os.environ["REPLAY_FIXTURE"]).read_text())
if "query($threadId: ID!)" in query:
    if config.get("fail") == "query":
        sys.exit(41)
    print(json.dumps(config["response"]))
    sys.exit(0)
if "addPullRequestReviewThreadReply" in query:
    action = "reply"
    result = {"data": {"addPullRequestReviewThreadReply": {"comment": {"id": "REPLY"}}}}
elif "unresolveReviewThread" in query:
    action = "unresolve"
    result = {"data": {"unresolveReviewThread": {"thread": {"id": "THREAD", "isResolved": False}}}}
elif "resolveReviewThread" in query:
    action = "resolve"
    result = {"data": {"resolveReviewThread": {"thread": {"id": "THREAD", "isResolved": True}}}}
else:
    print("Unexpected gh call: " + repr(args), file=sys.stderr)
    sys.exit(42)
with pathlib.Path(os.environ["REPLAY_LOG"]).open("a") as output:
    output.write(json.dumps(action) + "\n")
if config.get("fail") == action:
    sys.exit(43)
print(json.dumps(result))
''', encoding="utf-8")
        shim.chmod(0o755)

    def invoke(self, bodies: list[str], *, kind: str = "fixed", commit: str = OLD,
               argument: str = "current defect", resolved: bool = False,
               fail: str = "", response: dict | None = None):
        if response is None:
            response = {"data": {"node": {"isResolved": resolved,
                        "comments": {"nodes": [{"body": body} for body in bodies]}}}}
        self.fixture.write_text(json.dumps({"response": response, "fail": fail}), encoding="utf-8")
        self.log.write_text("", encoding="utf-8")
        args = ["bash", str(HELPER), "--pr", "42", "--thread", "THREAD",
                "--class", kind, "--reply", f"Disposition: {kind}\nEvidence: fixture",
                "--repo", "owner/repo", "--head", NEW, "--by", "fixture"]
        if kind == "fixed":
            args += ["--commit", commit]
        else:
            args += ["--argument", argument]
        env = {**os.environ, "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
               "REPLAY_FIXTURE": str(self.fixture), "REPLAY_LOG": str(self.log)}
        proc = subprocess.run(args, env=env, capture_output=True, text=True,
                              encoding="utf-8", timeout=20)
        effects = [json.loads(line) for line in self.log.read_text().splitlines()]
        return proc, effects

    def assert_refused_without_mutation(self, bodies, **kwargs):
        proc, effects = self.invoke(bodies, **kwargs)
        self.assertEqual(2, proc.returncode, proc.stdout + proc.stderr)
        self.assertIn("superseded disposition", proc.stderr)
        self.assertEqual([], effects)

    def test_old_fixed_cannot_resolve_later_blocker(self):
        self.assert_refused_without_mutation([
            marker("fixed", {"commit": OLD}),
            marker("current-blocker", {"argument": "current defect"}),
        ])

    def test_old_fixed_cannot_resolve_later_not_proven(self):
        self.assert_refused_without_mutation([
            marker("fixed", {"commit": OLD}),
            marker("not-proven", {"argument": "missing proof"}),
        ])

    def test_old_refutation_cannot_override_current_blocker(self):
        # Refutations and fixed outcomes share the same historical replay rule.
        self.assert_refused_without_mutation([
            marker("refuted", {"argument": "current defect"}),
            marker("current-blocker", {"argument": "new evidence"}),
        ], kind="refuted")

    def test_old_blocker_cannot_reopen_later_fixed(self):
        self.assert_refused_without_mutation([
            marker("current-blocker", {"argument": "current defect"}),
            marker("fixed", {"commit": NEW}),
        ], kind="current-blocker", resolved=True)

    def test_old_fixed_does_not_replay_over_newer_fixed_evidence(self):
        self.assert_refused_without_mutation([
            marker("fixed", {"commit": OLD}), marker("fixed", {"commit": NEW}),
        ])

    def test_latest_fixed_replay_is_idempotent_across_head_movement(self):
        proc, effects = self.invoke([marker("fixed", {"commit": OLD})])
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual(["resolve"], effects)

    def test_latest_fixed_replay_after_prior_blocker_remains_valid(self):
        proc, effects = self.invoke([
            marker("current-blocker", {"argument": "old defect"}),
            marker("fixed", {"commit": OLD}),
        ])
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual(["resolve"], effects)

    def test_ordinary_comment_does_not_replace_typed_disposition(self):
        proc, effects = self.invoke([marker("fixed", {"commit": OLD}), "Thanks for the explanation."])
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual(["resolve"], effects)

    def test_duplicate_current_marker_remains_idempotent(self):
        value = marker("fixed", {"commit": OLD})
        proc, effects = self.invoke([value, value])
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual(["resolve"], effects)

    def test_fresh_repair_after_blocker_posts_before_resolving(self):
        proc, effects = self.invoke([
            marker("fixed", {"commit": OLD}),
            marker("current-blocker", {"argument": "current defect"}),
        ], commit=NEW)
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual(["reply", "resolve"], effects)

    def test_new_terminal_disposition_is_still_possible(self):
        proc, effects = self.invoke(["Original finding"])
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual(["reply", "resolve"], effects)

    def test_current_blocker_replay_does_not_duplicate_reply(self):
        proc, effects = self.invoke([marker("current-blocker", {"argument": "current defect"})], kind="current-blocker")
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual(["unresolve"], effects)

    def test_new_blocker_reopens_before_post_and_enforces_open_after(self):
        proc, effects = self.invoke([marker("fixed", {"commit": OLD})], kind="current-blocker", resolved=True)
        self.assertEqual(0, proc.returncode, proc.stderr)
        self.assertEqual(["unresolve", "reply", "unresolve"], effects)

    def test_rejected_reply_never_resolves(self):
        proc, effects = self.invoke([], fail="reply")
        self.assertEqual(2, proc.returncode)
        self.assertEqual(["reply"], effects)

    def test_failed_blocker_reply_still_leaves_reopen_first(self):
        proc, effects = self.invoke([], kind="current-blocker", resolved=True, fail="reply")
        self.assertEqual(2, proc.returncode)
        self.assertEqual(["unresolve", "reply"], effects)

    def test_failed_resolve_is_not_success(self):
        proc, effects = self.invoke([], fail="resolve")
        self.assertEqual(2, proc.returncode)
        self.assertEqual(["reply", "resolve"], effects)

    def test_failed_reopen_does_not_post(self):
        proc, effects = self.invoke([], kind="current-blocker", resolved=True, fail="unresolve")
        self.assertEqual(2, proc.returncode)
        self.assertEqual(["unresolve"], effects)

    def test_failed_query_has_no_effects(self):
        proc, effects = self.invoke([], fail="query")
        self.assertEqual(2, proc.returncode)
        self.assertEqual([], effects)

    def test_malformed_response_has_no_effects(self):
        proc, effects = self.invoke([], response={"data": {"node": None}})
        self.assertEqual(2, proc.returncode)
        self.assertEqual([], effects)

    def test_unknown_marker_version_still_refuses_without_effects(self):
        proc, effects = self.invoke([marker("fixed", {"commit": OLD}, version=2)])
        self.assertEqual(2, proc.returncode)
        self.assertEqual([], effects)


if __name__ == "__main__":
    unittest.main()
