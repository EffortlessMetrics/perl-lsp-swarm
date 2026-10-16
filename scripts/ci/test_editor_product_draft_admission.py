#!/usr/bin/env python3
"""Cheap contracts for draft exclusion before editor product job allocation.

This is an interim exclusion boundary, not reviewed premerge admission: ready PRs
and existing manual/scheduled routes retain their current behavior.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = {
    "vscode-managed-binary-smoke.yml": ("managed-binary-smoke", "perl5-alias-smoke"),
    "vscode-current-source-linux-smoke.yml": ("current-source-linux-smoke",),
    "vscode-installed-first-hour-windows.yml": ("installed-first-hour-windows",),
    "vim-hermetic-host.yml": ("hermetic-host-run",),
    "sublime-real-host.yml": ("host-journey",),
    "sublime-dap-real-host.yml": ("exact-source-runtime-and-host",),
}
GUARD = "github.event_name != 'pull_request' || github.event.pull_request.draft == false"


def job_bodies(source):
    jobs = source.split("\njobs:\n", 1)[1]
    return dict(re.findall(r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)", jobs, re.M | re.S))


def admission(body):
    guards = re.findall(r"^    if: (.+)$", body, re.M)
    if guards != [GUARD]:
        raise AssertionError("product job needs the exact job-level draft guard")
    return guards[0]


def admitted(expression, event, draft):
    # Evaluate the actual pinned expression, not an independently coded policy.
    if expression != GUARD:
        raise AssertionError("unexpected admission expression")
    return eval(expression.replace("github.event_name", repr(event))
                .replace("github.event.pull_request.draft", repr(draft))
                .replace("false", "False").replace("||", "or"),
                {"__builtins__": {}}, {})


class EditorProductDraftAdmissionTests(unittest.TestCase):
    def sources(self):
        for name, expected in WORKFLOWS.items():
            source = (ROOT / ".github/workflows" / name).read_text()
            jobs = job_bodies(source)
            self.assertEqual(set(jobs), set(expected), name)
            yield name, source, jobs

    def test_every_product_job_excludes_drafts_before_allocation(self):
        for name, _, jobs in self.sources():
            for job, body in jobs.items():
                with self.subTest(workflow=name, job=job):
                    expression = admission(body)
                    self.assertLess(body.index("    if:"), body.index("    runs-on:"))
                    self.assertFalse(admitted(expression, "pull_request", True))
                    self.assertTrue(admitted(expression, "pull_request", False))
                    for event in ("workflow_dispatch", "schedule", "push", "merge_group"):
                        self.assertTrue(admitted(expression, event, None))

    def test_no_new_ready_or_label_trigger_is_invented(self):
        for name, source, _ in self.sources():
            triggers = source.split("\non:\n", 1)[1].split("\nconcurrency:", 1)[0]
            self.assertNotRegex(triggers, r"\b(?:ready_for_review|converted_to_draft|labeled)\b", name)
            self.assertNotIn("types:", triggers, name)

    def test_absent_step_only_or_permissive_guards_are_rejected(self):
        for name, _, jobs in self.sources():
            for job, body in jobs.items():
                line = f"    if: {GUARD}\n"
                mutations = (
                    body.replace(line, ""),
                    body.replace(line, f"        if: {GUARD}\n"),
                    body.replace(GUARD, GUARD + " || true"),
                    body.replace("draft == false", "draft == true"),
                )
                for mutation in mutations:
                    with self.subTest(workflow=name, job=job, mutation=mutation[:80]):
                        with self.assertRaises(AssertionError):
                            admission(mutation)

    def test_contract_is_wired_into_cheap_self_tests(self):
        source = (ROOT / ".github/workflows/ci-gate-self-tests.yml").read_text()
        self.assertIn("python3 -m unittest scripts.ci.test_editor_product_draft_admission", source)
        self.assertIn("'scripts/ci/test_editor_product_draft_admission.py'", source)
        for name in WORKFLOWS:
            self.assertIn(f"'.github/workflows/{name}'", source)


if __name__ == "__main__":
    unittest.main()
