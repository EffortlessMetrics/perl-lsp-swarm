#!/usr/bin/env python3
"""Focused falsifiers for source-specific GitHub enforcement bindings."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import subprocess
import tempfile
import sys
import unittest
from pathlib import Path
from typing import Any
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("validate_gate_enforcement_contract.py")
SPEC = importlib.util.spec_from_file_location("gate_enforcement_contract_bindings", SCRIPT)
assert SPEC and SPEC.loader
contract = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = contract
SPEC.loader.exec_module(contract)


def required_entry(enforcement: str, **overrides: Any) -> dict[str, Any]:
    entry: dict[str, Any] = {
        "name": "Bound Check",
        "producer": "external",
        "workflow": "github-app",
        "required": True,
        "policy_role": "required",
        "applicability": "always-or-scoped-noop",
        "enforcement": enforcement,
    }
    entry.update(overrides)
    return entry


def finding_codes(entry: dict[str, Any]) -> set[str]:
    findings, _ = contract.validate_context(entry, {}, {})
    return {finding.code for finding in findings}


def canonical_digest(entry: dict[str, Any]) -> str:
    payload = json.dumps(
        contract._canonical_context(entry),
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


class AppBindingContractTests(unittest.TestCase):
    def test_valid_source_specific_bindings_are_accepted(self) -> None:
        cases = (
            required_entry(
                "github-branch-protection",
                classic_app_id=15368,
            ),
            required_entry(
                "github-ruleset",
                ruleset_integration_id=15368,
            ),
            required_entry(
                "github-branch-protection+ruleset",
                classic_app_id=15368,
                ruleset_integration_id=15368,
            ),
        )
        for entry in cases:
            with self.subTest(entry=entry):
                self.assertEqual(set(), finding_codes(entry))

    def test_canonical_subject_retains_explicit_bindings(self) -> None:
        canonical = contract._canonical_context(
            required_entry(
                "github-branch-protection+ruleset",
                classic_app_id=15368,
                ruleset_integration_id=4242,
            )
        )
        self.assertEqual(15368, canonical["classic_app_id"])
        self.assertEqual(4242, canonical["ruleset_integration_id"])

    def test_absent_bindings_are_not_synthesized_from_producer(self) -> None:
        entry = required_entry("github-ruleset", producer="repository-job")
        canonical = contract._canonical_context(entry)
        self.assertNotIn("classic_app_id", canonical)
        self.assertNotIn("ruleset_integration_id", canonical)

    def test_invalid_binding_values_fail_closed(self) -> None:
        for field in ("classic_app_id", "ruleset_integration_id"):
            enforcement = (
                "github-branch-protection"
                if field == "classic_app_id"
                else "github-ruleset"
            )
            for value in (0, -1, "15368", 15368.0, True, None):
                with self.subTest(field=field, value=value):
                    entry = required_entry(enforcement, **{field: value})
                    self.assertIn(f"invalid_{field}", finding_codes(entry))

    def test_unknown_binding_aliases_fail_closed(self) -> None:
        for field in ("app_id", "classic_appid", "ruleset_app_id"):
            with self.subTest(field=field):
                entry = required_entry(
                    "github-branch-protection",
                    **{field: 15368},
                )
                self.assertIn("unknown_context_field", finding_codes(entry))

    def test_classic_binding_requires_classic_enforcement(self) -> None:
        entry = required_entry("github-ruleset", classic_app_id=15368)
        self.assertIn(
            "classic_app_id_source_mismatch",
            finding_codes(entry),
        )

    def test_ruleset_binding_requires_ruleset_enforcement(self) -> None:
        entry = required_entry(
            "github-branch-protection",
            ruleset_integration_id=15368,
        )
        self.assertIn(
            "ruleset_integration_id_source_mismatch",
            finding_codes(entry),
        )

    def test_binding_change_changes_static_subject_identity(self) -> None:
        first = required_entry(
            "github-branch-protection",
            classic_app_id=15368,
        )
        second = required_entry(
            "github-branch-protection",
            classic_app_id=4242,
        )
        self.assertNotEqual(canonical_digest(first), canonical_digest(second))


class GitSubjectIsolationTests(unittest.TestCase):
    """Real repositories and the actual CLI; no mocked Git verdicts."""

    WORKFLOW = ".github/workflows/ci.yml"
    POLICY = ".ci/policies/required-checks.toml"
    WORKFLOW_TEXT = (
        "name: Fixture\non:\n  workflow_dispatch: {}\njobs:\n"
        "  check:\n    name: Bound Check\n    runs-on: ubuntu-latest\n    steps: []\n"
    )
    POLICY_TEXT = (
        'version = 2\nsource = "github-enforcement-union"\n[[checks]]\n'
        'name = "Bound Check"\nproducer = "repository-job"\n'
        'workflow = ".github/workflows/ci.yml"\njob = "check"\n'
        'workflow_result = "propagate"\nevents = ["workflow_dispatch"]\n'
        'required = true\npolicy_role = "required"\n'
        'applicability = "always-or-scoped-noop"\nenforcement = "github-ruleset"\n'
        'ruleset_integration_id = 15368\n'
    )

    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="gate-subject-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.selected = self.root / "selected"
        self.foreign = self.root / "foreign"
        # Fixture setup deliberately does not use the production isolation helper.
        self.clean_env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        for repo in (self.selected, self.foreign):
            repo.mkdir()
            self.git(repo, "init", "-q", "--template=")
            for name, content in ((self.WORKFLOW, self.WORKFLOW_TEXT),
                                  (self.POLICY, self.POLICY_TEXT),
                                  ("marker", repo.name)):
                path = repo / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8")
            self.git(repo, "add", ".")
            self.git(repo, "commit", "-qm", repo.name)
        self.head = self.git(self.selected, "rev-parse", "HEAD")
        self.assertNotEqual(self.head, self.git(self.foreign, "rev-parse", "HEAD"))
        self.foreign_env = {
            "GIT_DIR": str(self.foreign / ".git"),
            "GIT_COMMON_DIR": str(self.foreign / ".git"),
            "GIT_WORK_TREE": str(self.foreign),
            "GIT_INDEX_FILE": str(self.foreign / ".git/index"),
            "GIT_OBJECT_DIRECTORY": str(self.foreign / ".git/objects"),
        }

    def git(self, root: Path, *args: str) -> str:
        return subprocess.check_output(
            ["git", "-C", str(root), "-c", "core.hooksPath=", "-c", "commit.gpgsign=false",
             "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", *args],
            env=self.clean_env, text=True, stderr=subprocess.PIPE,
        ).strip()

    def cli(self, *, env=None, root=None, selected_source=False):
        receipt = self.root / "receipt.json"
        args = [sys.executable, "-B", str(SCRIPT), "--root", str(root or self.selected),
                "--json", "--receipt", str(receipt)]
        if selected_source:
            args.extend(("--qualification-source", self.WORKFLOW))
        result = subprocess.run(args, cwd=self.root, env=dict(self.clean_env, **(env or {})),
                                text=True, capture_output=True, timeout=20, check=False)
        self.assertEqual(result.stderr, "", result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload, json.loads(receipt.read_text(encoding="utf-8")))
        return result.returncode, payload

    def test_foreign_repository_cannot_change_clean_cli_receipt(self) -> None:
        for selected_source in (False, True):
            with self.subTest(selected_source=selected_source):
                code, baseline = self.cli(selected_source=selected_source)
                self.assertEqual(code, 0)
                self.assertEqual(baseline["subjects"]["repository_sha"], self.head)
                contaminated_code, contaminated = self.cli(
                    env=self.foreign_env, selected_source=selected_source)
                self.assertEqual(contaminated_code, 0)
                self.assertEqual(contaminated, baseline)
                self.assertEqual(contaminated["live_enforcement_status"], "NOT_PROVEN")

    def test_clean_foreign_repository_cannot_hide_selected_dirty_source(self) -> None:
        path = self.selected / self.WORKFLOW
        path.write_text(self.WORKFLOW_TEXT + "# uncommitted\n", encoding="utf-8")
        code, receipt = self.cli(env=self.foreign_env)
        self.assertEqual(code, 1)
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertIsNone(receipt["subjects"])
        self.assertIn("tracked modifications", receipt["findings"][0]["message"])

    def test_foreign_index_cannot_admit_untracked_selected_workflow(self) -> None:
        name = ".github/workflows/extra.yml"
        for repo in (self.selected, self.foreign):
            (repo / name).write_text("name: Extra\non: workflow_dispatch\njobs: {}\n", encoding="utf-8")
        self.git(self.foreign, "add", name)
        self.git(self.foreign, "commit", "-qm", "track only foreign workflow")
        code, receipt = self.cli(env=self.foreign_env)
        self.assertEqual(code, 1)
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertIn("not tracked by git", receipt["findings"][0]["message"])
        # Also isolate the index-only seam: identity must not depend on a foreign index.
        with patch.dict(os.environ, dict(self.clean_env,
                                        GIT_INDEX_FILE=str(self.foreign / ".git/index")), clear=True):
            with self.assertRaisesRegex(ValueError, "not tracked by git"):
                contract._safe_repo_path(self.selected, name, subject="workflow")

    def test_runtime_git_config_cannot_change_selected_worktree(self) -> None:
        baseline = self.cli()
        runtime_config = {
            "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "core.worktree",
            "GIT_CONFIG_VALUE_0": str(self.foreign),
        }
        self.assertEqual(self.cli(env=runtime_config), baseline)

    def test_linked_worktree_keeps_its_selected_identity(self) -> None:
        linked = self.root / "linked"
        self.git(self.selected, "worktree", "add", "--detach", str(linked), "HEAD")
        self.assertTrue((linked / ".git").is_file())
        baseline = self.cli(root=linked)
        self.assertEqual(baseline[0], 0)
        self.assertEqual(self.cli(root=linked, env=self.foreign_env), baseline)

    def test_in_process_validation_preserves_parent_environment_and_foreign_repo(self) -> None:
        foreign_before = self.git(self.foreign, "status", "--porcelain")
        with patch.dict(os.environ, dict(self.clean_env, **self.foreign_env,
                                        GATE_ISOLATION_SENTINEL="preserve"), clear=True):
            before = dict(os.environ)
            result = contract.validate(self.selected, self.selected / self.POLICY)
            self.assertEqual(dict(os.environ), before)
            self.assertEqual(result["subjects"]["repository_sha"], self.head)
        self.assertEqual(self.git(self.foreign, "status", "--porcelain"), foreign_before)

    def test_invalid_selected_root_does_not_fall_back_to_foreign_repository(self) -> None:
        empty = self.root / "not-a-repository"
        empty.mkdir()
        code, receipt = self.cli(root=empty, env=self.foreign_env)
        self.assertEqual(code, 1)
        self.assertEqual(receipt["status"], "NOT_PROVEN")
        self.assertIsNone(receipt["subjects"])
        self.assertIn("rev-parse", receipt["findings"][0]["message"])


if __name__ == "__main__":
    unittest.main()
