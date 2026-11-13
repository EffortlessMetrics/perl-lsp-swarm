"""Conflict-check checkout identity; local Git proof, not hosted qualification."""

import ast
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci.yml"


def selected_ref(expression, event, head, ref_name, pr_head="", group_head=""):
    """Evaluate only the string/boolean subset used by this checkout expression."""
    source = expression.removeprefix("${{").removesuffix("}}").strip()
    tree = ast.parse(source.replace("&&", " and ").replace("||", " or "), mode="eval")
    allowed = (ast.Expression, ast.BoolOp, ast.And, ast.Or, ast.Compare, ast.Eq,
               ast.Attribute, ast.Name, ast.Load, ast.Constant)
    if any(not isinstance(node, allowed) for node in ast.walk(tree)):
        raise ValueError("unsupported checkout expression")
    github = SimpleNamespace(
        event_name=event, sha=head, ref_name=ref_name,
        event=SimpleNamespace(
            pull_request=SimpleNamespace(head=SimpleNamespace(sha=pr_head)),
            merge_group=SimpleNamespace(head_sha=group_head),
        ),
    )
    return eval(compile(tree, "<checkout-ref>", "eval"), {"__builtins__": {}}, {"github": github})


class ConflictCheckoutSubject(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        job = yaml.safe_load(WORKFLOW.read_text())["jobs"]["conflict-markers"]
        checkouts = [step for step in job["steps"]
                     if step.get("uses", "").startswith("actions/checkout@")]
        if len(checkouts) != 1:
            raise AssertionError("expected one conflict-check checkout")
        cls.expression = checkouts[0]["with"]["ref"]
        cls.scanner = next(step["run"] for step in job["steps"]
                           if step.get("name") == "Check for conflict markers")

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="conflict-subject-")
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name)
        self.environment = {key: value for key, value in os.environ.items()
                            if not key.startswith("GIT_")}
        self.environment.update(
            GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
            GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
            GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@example.invalid",
        )
        self.git("init", "--quiet")
        self.group = self.commit("<<<<<<< current\nleft\n=======\nright\n>>>>>>> incoming\n")
        self.clean = self.commit("clean content\n")
        self.queue_ref = "gh-readonly-queue/main/pr-1-fixture"
        self.git("update-ref", "refs/heads/" + self.queue_ref, self.group)

    def git(self, *args, input=None, check=True):
        return subprocess.run(["git", *args], cwd=self.repo, env=self.environment,
                              input=input, text=True, capture_output=True,
                              timeout=10, check=check)

    def commit(self, content):
        blob = self.git("hash-object", "-w", "--stdin", input=content).stdout.strip()
        tree = self.git("mktree", input=f"100644 blob {blob}\tfixture.txt\n").stdout.strip()
        return self.git("commit-tree", tree, input="fixture\n").stdout.strip()

    def scan_selected(self, event, head, ref_name, **kwargs):
        selected = selected_ref(self.expression, event, head, ref_name, **kwargs)
        checkout = self.git("checkout", "--quiet", "--detach", selected, check=False)
        self.assertEqual(checkout.returncode, 0, checkout.stderr)
        self.assertEqual(self.git("rev-parse", "HEAD").stdout.strip(), head)
        return subprocess.run(["bash", "-c", self.scanner], cwd=self.repo,
                              env=self.environment, text=True, capture_output=True,
                              timeout=10)

    def test_event_subjects_are_immutable(self):
        pr_head, group_head, event_sha = "1" * 40, "2" * 40, "3" * 40
        for event, expected in (("pull_request", pr_head), ("merge_group", group_head),
                                ("push", event_sha), ("workflow_dispatch", event_sha)):
            with self.subTest(event=event):
                self.assertEqual(selected_ref(self.expression, event, event_sha, "moving-ref",
                                              pr_head, group_head), expected)

    def test_moved_queue_ref_still_scans_original_group(self):
        self.git("update-ref", "refs/heads/" + self.queue_ref, self.clean)
        result = self.scan_selected("merge_group", self.group, self.queue_ref, group_head=self.group)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("fixture.txt", result.stdout)

    def test_deleted_queue_ref_preserves_conflict_and_clean_results(self):
        self.git("update-ref", "-d", "refs/heads/" + self.queue_ref)
        for head, expected in ((self.group, 1), (self.clean, 0)):
            with self.subTest(head=head):
                result = self.scan_selected("merge_group", head, self.queue_ref, group_head=head)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)

    def test_moved_push_branch_still_scans_event_commit(self):
        self.git("update-ref", "refs/heads/main", self.clean)
        result = self.scan_selected("push", self.group, "main")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
