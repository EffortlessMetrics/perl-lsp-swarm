#!/usr/bin/env python3
"""Contracts for paused automatic Droid lanes and explicit PR-mention execution."""

from __future__ import annotations

import hashlib
import io
import json
import os
import re
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCAN_WORKFLOW = ROOT / ".github/workflows/droid-security-scan.yml"
CONTRACT_WORKFLOW = ROOT / ".github/workflows/droid-security-boundary.yml"
FULL_SHA_ACTION = re.compile(
    r"(?m)^\s*(?:-\s*)?uses:\s+"
    r"[0-9A-Za-z_.-]+/[0-9A-Za-z_.-]+@[0-9a-f]{40}(?:\s+#.*)?$"
)


def _code_lines(text: str) -> list[tuple[int, str]]:
    """Return non-comment YAML lines with indentation and comment text removed."""
    lines: list[tuple[int, str]] = []
    for raw in text.splitlines():
        stripped = raw.lstrip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(raw) - len(stripped)
        lines.append((indent, stripped.split(" #", 1)[0].rstrip()))
    return lines


def _top_level_mapping(text: str, parent: str) -> list[str]:
    """Return direct child keys of a top-level YAML mapping."""
    lines = _code_lines(text)
    parent_index = next(
        (index for index, (indent, line) in enumerate(lines) if indent == 0 and line == f"{parent}:"),
        None,
    )
    if parent_index is None:
        return []
    keys: list[str] = []
    for indent, line in lines[parent_index + 1 :]:
        if indent == 0:
            break
        if indent == 2:
            match = re.match(r"([A-Za-z0-9_-]+):", line)
            if match:
                keys.append(match.group(1))
    return keys


def _permission_blocks(text: str) -> list[tuple[int, str]]:
    """Return every workflow/job permissions mapping, including inline mappings."""
    blocks: list[tuple[int, str]] = []
    lines = _code_lines(text)
    for index, (indent, line) in enumerate(lines):
        if line == "permissions:" or line.startswith("permissions: "):
            values = line.partition(":")[2].strip()
            if values:
                blocks.append((indent, values))
                continue
            children: list[str] = []
            for child_indent, child in lines[index + 1 :]:
                if child_indent <= indent:
                    break
                if child_indent == indent + 2:
                    children.append(child)
            blocks.append((indent, "\n".join(children)))
    return blocks


def _permission_values(text: str) -> list[str]:
    values: list[str] = []
    for _, block in _permission_blocks(text):
        values.extend(re.findall(r"\b(?:contents|issues|pull-requests|checks|statuses|actions|id-token|attestations)\s*:\s*([A-Za-z-]+)", block))
        values.extend(re.findall(r"\b(?:read-all|write-all)\b", block))
    return values


class DroidSecurityBoundaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.scan = SCAN_WORKFLOW.read_text(encoding="utf-8")
        cls.contract = CONTRACT_WORKFLOW.read_text(encoding="utf-8")

    def test_execution_workflow_is_manual_only(self) -> None:
        self.assertEqual(
            _top_level_mapping(self.scan, "on"),
            ["workflow_dispatch"],
        )

    def test_execution_workflow_has_read_only_authority(self) -> None:
        self.assertTrue(_permission_blocks(self.scan))
        self.assertNotIn("write", _permission_values(self.scan))
        self.assertNotIn("write-all", _permission_values(self.scan))
        self.assertNotIn("id-token", self.scan)

    def test_no_secret_or_mutable_tooling_is_reachable(self) -> None:
        executable = "\n".join(
            line
            for _, line in _code_lines(self.scan)
            if not line.startswith(("#", "##"))
        )
        self.assertNotIn("secrets.", executable)
        self.assertNotIn("MINIMAX_API_KEY", executable)
        self.assertNotIn("FACTORY_API_KEY", executable)
        self.assertNotIn("droid-action", executable)
        self.assertNotIn("factory.ai", executable)
        self.assertNotIn("factory-plugins", executable)
        self.assertNotIn("bun install", executable)
        self.assertNotIn("curl ", executable)
        self.assertNotRegex(executable, r"(?m)^\s*uses:")
        self.assertNotRegex(executable, r"(?m)^\s*runs-on:.*self-hosted")

    def test_pause_is_explicit_truthful_and_routed(self) -> None:
        required = (
            "Droid security scan is paused",
            "issues/6098",
            "no checkout",
            "provider-secret access",
            "OIDC exchange",
            "no review",
            "merge authority",
        )
        lowered = self.scan.lower()
        for token in required:
            with self.subTest(token=token):
                self.assertIn(token.lower(), lowered)

    def test_contract_workflow_is_path_scoped_and_read_only(self) -> None:
        for path in (
            ".github/workflows/droid-security-scan.yml",
            ".github/workflows/droid-security-boundary.yml",
            "scripts/ci/test_droid_security_boundary.py",
        ):
            self.assertIn(path, self.contract)
        self.assertTrue(_permission_blocks(self.contract))
        self.assertNotIn("write", _permission_values(self.contract))
        self.assertNotIn("id-token", self.contract)

    def test_contract_checkout_is_immutable_and_does_not_persist_credentials(self) -> None:
        action_lines = [
            line
            for line in self.contract.splitlines()
            if line.lstrip().startswith(("uses:", "- uses:"))
        ]
        self.assertEqual(len(action_lines), 1)
        self.assertRegex(action_lines[0], FULL_SHA_ACTION)
        self.assertIn("actions/checkout@", action_lines[0])
        self.assertRegex(
            self.contract,
            r"(?m)^\s+persist-credentials:\s+false\s*$",
        )

    def test_contract_executes_only_the_focused_static_test(self) -> None:
        run_lines = [
            line.strip()
            for line in self.contract.splitlines()
            if line.strip().startswith("run:")
        ]
        self.assertEqual(
            run_lines,
            ["run: python3 -m unittest scripts/ci/test_droid_security_boundary.py"],
        )
        forbidden = ("secrets.", "id-token:", "self-hosted", "droid-action")
        for token in forbidden:
            with self.subTest(token=token):
                self.assertNotIn(token, self.contract)


def _mention_step(text: str, name: str) -> str:
    """Extract one named step from this workflow's fixed six-space step layout."""
    steps = re.findall(r"(?ms)^      - name: ([^\n]+)\n(.*?)(?=^      - name:|\Z)", text)
    return next(body for title, body in steps if title == name)


def _mention_script(text: str, name: str) -> str:
    """Extract the actual inline program, not a test-side reimplementation."""
    body = _mention_step(text, name)
    script = body.split("        run: |\n", 1)[1]
    lines = []
    for line in script.splitlines():
        if line and not line.startswith("          "):
            break
        lines.append(line[10:] if line else "")
    return "\n".join(lines) + "\n"


class DroidMentionBoundaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.mention = (ROOT / ".github/workflows/droid.yml").read_text(encoding="utf-8")

    def run_script(self, name: str, root: Path, extra: dict[str, str] | None = None, script: str | None = None) -> subprocess.CompletedProcess[str]:
        env = {key: value for key, value in os.environ.items() if not key.endswith("TOKEN") and not key.endswith("API_KEY")}
        env.update({"HOME": str(root), "RUNNER_TEMP": str(root), "GITHUB_REPOSITORY": "EffortlessMetrics/perl-lsp-swarm"})
        for key in ("GITHUB_OUTPUT", "GITHUB_ENV", "GITHUB_PATH"):
            path = root / key
            path.touch()
            env[key] = str(path)
        env.update(extra or {})
        return subprocess.run(
            ["/bin/bash", "--noprofile", "--norc", "-e", "-o", "pipefail"],
            input=script or _mention_script(self.mention, name),
            env=env, text=True, capture_output=True, timeout=15,
        )

    def test_mentions_remain_explicit_human_requests_without_cancellation(self) -> None:
        self.assertEqual(_top_level_mapping(self.mention, "on"), ["issue_comment", "pull_request_review_comment", "pull_request_review"])
        self.assertEqual(self.mention.count(".user.type == 'User'"), 3)
        self.assertEqual(self.mention.count('fromJSON(\'["OWNER","MEMBER","COLLABORATOR"]\')'), 3)
        self.assertIn("github.repository == 'EffortlessMetrics/perl-lsp-swarm'", self.mention)
        self.assertIn("cancel-in-progress: false", self.mention)
        self.assertIn("github.event.issue.number || github.event.pull_request.number", self.mention)

    def test_model_credentials_and_action_pin_are_consistent(self) -> None:
        action = _mention_step(self.mention, "Run Droid with MiniMax M3 BYOK")
        self.assertIn("droid-action-safe@22d9c91d516983f637dfee20cb8035a8ba229d35", action)
        for key in ("review_model", "security_model", "fill_model"):
            self.assertIn(f'{key}: "custom:MiniMax-M3-0"', action)
        for key in ("automatic_review", "automatic_security_review", "upload_debug_artifacts", "show_full_output"):
            self.assertIn(f"{key}: false", action)
        for key in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"):
            self.assertIn(f'{key}: ""', action)
        self.assertNotIn("MiniMax-M2.7", self.mention)
        self.assertTrue(all(indent >= 10 for indent, line in _code_lines(self.mention) if "secrets." in line))

    def test_safe_action_pin_has_reviewed_provenance(self) -> None:
        import tomllib

        pins = tomllib.loads((ROOT / ".ci/policies/action-pin-provenance.toml").read_text())["pin"]
        action = _mention_step(self.mention, "Run Droid with MiniMax M3 BYOK")
        match = re.search(r"uses: ([^@ ]+)@([0-9a-f]{40}) # ([^\n]+)", action)
        self.assertIsNotNone(match)
        expected = {"action": match[1], "sha": match[2], "kind": "branch_commit", "value": match[3]}
        self.assertIn(expected, pins)

    def test_exact_subject_and_prerequisites_precede_provider_credentials(self) -> None:
        names = re.findall(r"(?m)^      - name: (.+)$", self.mention)
        ordered = ["Create private Droid runtime", "Install verified GitHub CLI before Droid preparation", "Resolve authorized PR head", "Checkout authorized PR head", "Configure MiniMax M3 BYOK", "Run Droid with MiniMax M3 BYOK", "Remove private Droid runtime"]
        self.assertEqual(names, ordered)
        self.assertIn("ref: ${{ steps.subject.outputs.head }}", self.mention)
        self.assertIn("expected_head_sha: ${{ steps.subject.outputs.head }}", self.mention)
        self.assertIn("persist-credentials: false", self.mention)
        self.assertIn("if: always() && steps.runtime.outputs.home != ''", self.mention)
        self.assertNotIn("contents: write", self.mention)

    def test_automatic_review_stays_paused_and_contract_covers_mentions(self) -> None:
        review = (ROOT / ".github/workflows/droid-review.yml").read_text(encoding="utf-8")
        self.assertIn("if: ${{ false }}", review)
        self.assertNotIn("uses:", review)
        contract = CONTRACT_WORKFLOW.read_text(encoding="utf-8")
        for path in (".github/workflows/droid.yml", ".github/workflows/droid-review.yml", ".ci/policies/action-pin-provenance.toml"):
            self.assertEqual(contract.count(f"- '{path}'"), 2)

    def test_settings_serialize_escaped_key_privately_without_logging_it(self) -> None:

        key = 'fixture-"quoted"\\key\nnot-a-real-secret'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".factory").mkdir()
            result = self.run_script("Configure MiniMax M3 BYOK", root, {"MINIMAX_API_KEY": key, "FACTORY_API_KEY": "fixture-factory"})
            self.assertEqual(result.returncode, 0, result.stderr)
            path = root / ".factory/settings.local.json"
            model = json.loads(path.read_text())["customModels"][0]
            self.assertEqual(model["apiKey"], key)
            self.assertEqual(model["model"], "MiniMax-M3")
            self.assertEqual(model["provider"], "anthropic")
            self.assertEqual(model["baseUrl"], "https://api.minimax.io/anthropic")
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertNotIn(key, result.stdout + result.stderr)
            self.assertNotIn("fixture-factory", result.stdout + result.stderr)

    def test_missing_keys_fail_before_creating_settings(self) -> None:

        for missing in ("MINIMAX_API_KEY", "FACTORY_API_KEY"):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / ".factory").mkdir()
                env = {"MINIMAX_API_KEY": "fixture", "FACTORY_API_KEY": "fixture", missing: " "}
                result = self.run_script("Configure MiniMax M3 BYOK", root, env)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(missing, result.stderr)
                self.assertFalse((root / ".factory/settings.local.json").exists())

    def test_settings_do_not_overwrite_existing_file_or_follow_symlink(self) -> None:

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".factory").mkdir()
            sentinel = root / "sentinel"
            sentinel.write_text("preserve")
            (root / ".factory/settings.local.json").symlink_to(sentinel)
            result = self.run_script("Configure MiniMax M3 BYOK", root, {"MINIMAX_API_KEY": "fixture", "FACTORY_API_KEY": "fixture"})
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(sentinel.read_text(), "preserve")

    def test_subject_resolution_accepts_all_three_pr_event_shapes(self) -> None:

        repo = "EffortlessMetrics/perl-lsp-swarm"
        head = "a" * 40
        for event in ({"issue": {"number": 42, "pull_request": {"url": "fixture"}}}, {"pull_request": {"number": 42}, "comment": {}}, {"pull_request": {"number": 42}, "review": {}}):
            with self.subTest(event=event), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "event.json").write_text(json.dumps(event))
                (root / "pr.json").write_text(json.dumps({"state": "open", "head": {"sha": head, "repo": {"full_name": repo}}}))
                gh = root / "gh"
                gh.write_text('#!/bin/sh\n[ "$*" = "api repos/EffortlessMetrics/perl-lsp-swarm/pulls/42" ] || exit 2\ncat "$HOME/pr.json"\n')
                gh.chmod(0o755)
                result = self.run_script("Resolve authorized PR head", root, {"GITHUB_EVENT_PATH": str(root / "event.json"), "PATH": f"{root}:{os.environ['PATH']}"})
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((root / "GITHUB_OUTPUT").read_text(), f"head={head}\n")

    def test_subject_resolution_rejects_foreign_closed_missing_and_malformed_subjects(self) -> None:

        repo = "EffortlessMetrics/perl-lsp-swarm"
        cases = [
            ({"issue": {"number": 42}}, "open", repo, "a" * 40),
            ({"pull_request": {"number": True}}, "open", repo, "a" * 40),
            ({"pull_request": {"number": "42; echo bad"}}, "open", repo, "a" * 40),
            ({"pull_request": {"number": 42}}, "open", "other/fork", "a" * 40),
            ({"pull_request": {"number": 42}}, "closed", repo, "a" * 40),
            ({"pull_request": {"number": 42}}, "open", None, "a" * 40),
            ({"pull_request": {"number": 42}}, "open", repo, "a" * 40 + "\nbad=value"),
        ]
        for event, state, owner, head in cases:
            with self.subTest(event=event, state=state, owner=owner, head=head), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "event.json").write_text(json.dumps(event))
                (root / "pr.json").write_text(json.dumps({"state": state, "head": {"sha": head, "repo": {"full_name": owner} if owner else None}}))
                gh = root / "gh"
                gh.write_text('#!/bin/sh\ncat "$HOME/pr.json"\n')
                gh.chmod(0o755)
                result = self.run_script("Resolve authorized PR head", root, {"GITHUB_EVENT_PATH": str(root / "event.json"), "PATH": f"{root}:{os.environ['PATH']}"})
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual((root / "GITHUB_OUTPUT").read_text(), "")

    def test_gh_install_verifies_archive_before_extraction_or_execution(self) -> None:

        for valid in (True, False):
            with self.subTest(valid=valid), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / ".local/bin").mkdir(parents=True)
                archive = root / "fixture.tar.gz"
                with tarfile.open(archive, "w:gz") as bundle:
                    program = b'#!/bin/sh\ntouch "$HOME/gh-executed"\necho "gh version fixture"\n'
                    info = tarfile.TarInfo("gh_2.101.0_linux_amd64/bin/gh")
                    info.size, info.mode = len(program), 0o755
                    bundle.addfile(info, io.BytesIO(program))
                curl = root / "curl"
                curl.write_text('#!/bin/sh\nwhile [ "$#" -gt 0 ]; do\n  if [ "$1" = "--output" ]; then cp "$FAKE_ARCHIVE" "$2"; exit; fi\n  shift\ndone\nexit 2\n')
                curl.chmod(0o755)
                script = _mention_script(self.mention, "Install verified GitHub CLI before Droid preparation")
                official = "9bca2d1c16825f109907a23307628a2f0698fbf99662b73a5cf0b020293072b8"
                self.assertIn(official, script)
                digest = hashlib.sha256(archive.read_bytes()).hexdigest() if valid else "0" * 64
                # Substitute only the approved digest; exercise the shipped install program.
                script = script.replace(official, digest)
                result = self.run_script("Install verified GitHub CLI before Droid preparation", root, {"PATH": f"{root}:{os.environ['PATH']}", "FAKE_ARCHIVE": str(archive)}, script)
                self.assertEqual(result.returncode == 0, valid, result.stderr)
                self.assertEqual((root / ".local/bin/gh").exists(), valid)
                self.assertEqual((root / "gh-executed").exists(), valid)

    def test_private_runtime_and_cleanup_do_not_touch_runner_home(self) -> None:

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sentinel = root / "keep"
            sentinel.write_text("runner state")
            result = self.run_script("Create private Droid runtime", root)
            self.assertEqual(result.returncode, 0, result.stderr)
            home = Path((root / "GITHUB_OUTPUT").read_text().strip().removeprefix("home="))
            self.assertNotEqual(home, root)
            self.assertEqual(home.stat().st_mode & 0o777, 0o700)
            (home / ".factory/settings.local.json").write_text("fixture")
            prompts = root / "droid-prompts"
            prompts.mkdir()
            (prompts / "pr.diff").write_text("fixture")
            result = self.run_script("Remove private Droid runtime", root, {"DROID_HOME": str(home)})
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(home.exists())
            self.assertFalse(prompts.exists())
            self.assertEqual(sentinel.read_text(), "runner state")
            result = self.run_script("Remove private Droid runtime", root, {"DROID_HOME": str(root)})
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(sentinel.read_text(), "runner state")


if __name__ == "__main__":
    unittest.main()
