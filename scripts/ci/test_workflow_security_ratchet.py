#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import textwrap
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("workflow_security_ratchet.py")
SPEC = importlib.util.spec_from_file_location("workflow_security_ratchet", MODULE_PATH)
assert SPEC and SPEC.loader
ratchet = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ratchet
SPEC.loader.exec_module(ratchet)


class Args:
    max_files = 512
    max_file_bytes = 2 * 1024 * 1024
    max_total_bytes = 16 * 1024 * 1024
    baseline_root: Path | None = None


class WorkflowSecurityRatchetTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / ".github/workflows").mkdir(parents=True)
        (self.root / ".github/actions/example").mkdir(parents=True)
        self.write(
            ".github/workflows/workflow-security-ratchet.yml",
            "name: ratchet\non: [workflow_dispatch]\npermissions:\n  contents: read\njobs: {}\n",
        )
        self.write("scripts/ci/workflow_security_ratchet.py", "# scanner\n")
        self.write("scripts/ci/test_workflow_security_ratchet.py", "# tests\n")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def write(self, relative: str, content: str | bytes) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            path.write_bytes(content)
        else:
            path.write_text(content, encoding="utf-8")
        return path

    def rules(self) -> list[str]:
        return [finding.rule for finding in ratchet.scan(self.root)]

    def baseline_path(self) -> Path:
        return self.root / ".ci/workflow-security-baseline.json"

    def write_current_baseline(self) -> Path:
        path = self.baseline_path()
        self.assertEqual(ratchet.write_baseline(self.root, path, Args()), 0)
        return path

    def exact_args(self) -> Args:
        args = Args()
        args.baseline_root = self.root
        return args

    def test_detects_mutable_external_action_in_composite_action(self) -> None:
        self.write(
            ".github/actions/example/action.yml",
            "name: example\nruns:\n  using: composite\n  steps:\n    - uses: actions/cache@v4\n",
        )
        self.assertIn("mutable_action_ref", self.rules())

    def test_full_sha_action_is_clean(self) -> None:
        self.write(
            ".github/actions/example/action.yml",
            "name: example\nruns:\n  using: composite\n  steps:\n    - uses: actions/cache@" + "a" * 40 + "\n",
        )
        self.assertNotIn("mutable_action_ref", self.rules())

    def test_detects_expression_in_list_form_run_source(self) -> None:
        self.write(
            ".github/workflows/injection.yml",
            "name: injection\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo '${{ github.event.inputs.value }}'\n",
        )
        self.assertIn("expression_in_run_source", self.rules())

    def test_detects_expression_in_quoted_run_key(self) -> None:
        self.write(
            ".github/workflows/injection.yml",
            "name: injection\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - 'run': echo '${{ github.event.inputs.value }}'\n",
        )
        self.assertIn("expression_in_run_source", self.rules())

    def test_detects_expression_in_block_run_source(self) -> None:
        self.write(
            ".github/workflows/injection.yml",
            "name: injection\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          echo '${{ github.event.inputs.value }}'\n",
        )
        self.assertIn("expression_in_run_source", self.rules())

    def test_detects_pr_write_permissions_and_secret_reference(self) -> None:
        self.write(
            ".github/workflows/pr.yml",
            "name: pr\non:\n  pull_request:\npermissions:\n  contents: write\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - env:\n          TOKEN: ${{ secrets.SPECIAL_TOKEN }}\n        run: echo safe\n",
        )
        rules = self.rules()
        self.assertIn("pr_write_permission", rules)
        self.assertIn("pr_secret_reference", rules)

    def test_detects_pr_write_all(self) -> None:
        self.write(
            ".github/workflows/pr.yml",
            "name: pr\non: [pull_request]\npermissions: write-all\njobs: {}\n",
        )
        self.assertIn("pr_write_permission", self.rules())

    def test_pr_trigger_survives_block_sequence_before_pull_request(self) -> None:
        # `branches:` as a block sequence puts a non-key line inside `on:`
        # before `pull_request:`. Treating that as the end of the mapping
        # silently disabled both PR-only rules for the whole file.
        self.write(
            ".github/workflows/pr.yml",
            "name: pr\non:\n  push:\n    branches:\n      - main\n  pull_request:\n    branches:\n      - main\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - env:\n          TOKEN: ${{ secrets.SPECIAL_TOKEN }}\n        run: echo safe\n",
        )
        self.assertIn("pr_secret_reference", self.rules())

    def test_pr_trigger_survives_comment_inside_on_block(self) -> None:
        self.write(
            ".github/workflows/pr.yml",
            "name: pr\non:\n  # only on pull requests\n  pull_request:\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - env:\n          TOKEN: ${{ secrets.SPECIAL_TOKEN }}\n        run: echo safe\n",
        )
        self.assertIn("pr_secret_reference", self.rules())

    def test_non_pr_workflow_is_still_not_treated_as_pr_triggered(self) -> None:
        self.write(
            ".github/workflows/push.yml",
            "name: push\non:\n  push:\n    branches:\n      - main\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - env:\n          TOKEN: ${{ secrets.SPECIAL_TOKEN }}\n        run: echo safe\n",
        )
        self.assertNotIn("pr_secret_reference", self.rules())

    def test_checkout_finding_is_not_cleared_by_a_later_job(self) -> None:
        # `job_a` never sets persist-credentials: false. `job_b` does, and is
        # indented more deeply, so its step list items never hit the
        # same-or-shallower list-item boundary. The scan for `job_a` must not
        # run on into `job_b` and clear `job_a`'s finding.
        self.write(
            ".github/workflows/write.yml",
            "name: write\non: [workflow_dispatch]\npermissions:\n  contents: write\njobs:\n  job_a:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@"
            + "a" * 40
            + "\n  job_b:\n    runs-on: ubuntu-latest\n    steps:\n          - uses: actions/checkout@"
            + "a" * 40
            + "\n            with:\n              persist-credentials: false\n",
        )
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_detects_persisted_checkout_credentials_on_write_surface(self) -> None:
        self.write(
            ".github/workflows/write.yml",
            "name: write\non: [workflow_dispatch]\npermissions:\n  contents: write\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@" + "a" * 40 + "\n",
        )
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_explicitly_disabled_checkout_credentials_are_clean(self) -> None:
        self.write(
            ".github/workflows/write.yml",
            "name: write\non: [workflow_dispatch]\npermissions:\n  contents: write\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@" + "a" * 40 + "\n        with:\n          persist-credentials: false\n",
        )
        self.assertNotIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_gh_aw_generated_lock_safe_outputs_checkout_is_accepted(self) -> None:
        # gh-aw emits persist-credentials: true on exactly one checkout, the
        # safe_outputs create_pull_request step, always shaped with the
        # GH_AW_GITHUB_TOKEN token expression; the lock file is regenerated
        # and diffed by the compile-check workflow, so the ratchet cannot
        # require a different shape.
        self.write(
            ".github/workflows/minimax-coding-agent.lock.yml",
            '# gh-aw-metadata: {"schema_version":"v4","compiler_version":"v0.88.7"}\n'
            "# This file was automatically generated by gh-aw (v0.88.7). DO NOT EDIT.\n"
            "name: minimax-coding-agent\non: [workflow_dispatch]\npermissions: {}\n"
            "jobs:\n  create_pull_request:\n    permissions:\n      contents: write\n"
            "    runs-on: ubuntu-latest\n    steps:\n"
            "      - name: Checkout repository\n"
            "        uses: actions/checkout@" + "a" * 40 + "\n        with:\n"
            "          persist-credentials: true\n          fetch-depth: 0\n"
            "          token: ${{ secrets.GH_AW_GITHUB_TOKEN || secrets.GITHUB_TOKEN }}\n",
        )
        self.assertNotIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_gh_aw_lock_other_checkout_persist_is_still_flagged(self) -> None:
        # Inside a genuine lock file, only the safe_outputs push checkout is
        # exempt; an unrelated checkout with persisted credentials fails.
        self.write(
            ".github/workflows/minimax-coding-agent.lock.yml",
            '# gh-aw-metadata: {"schema_version":"v4","compiler_version":"v0.88.7"}\n'
            "name: minimax-coding-agent\non: [workflow_dispatch]\npermissions: {}\n"
            "jobs:\n  agent:\n    permissions:\n      contents: write\n"
            "    runs-on: ubuntu-latest\n    steps:\n"
            "      - name: Checkout repository\n"
            "        uses: actions/checkout@" + "a" * 40 + "\n        with:\n"
            "          persist-credentials: true\n          fetch-depth: 0\n",
        )
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_spoofed_gh_aw_marker_workflow_is_still_flagged(self) -> None:
        # A candidate-controlled gh-aw marker comment in an ordinary workflow
        # (no .lock.yml identity, no metadata header) must not open the hole.
        self.write(
            ".github/workflows/agent.yml",
            "# This file was automatically generated by gh-aw (v0.88.7). DO NOT EDIT.\n"
            "name: agent\non: [workflow_dispatch]\npermissions: {}\njobs:\n  push:\n"
            "    permissions:\n      contents: write\n    runs-on: ubuntu-latest\n"
            "    steps:\n      - uses: actions/checkout@" + "a" * 40
            + "\n        with:\n          persist-credentials: true\n"
            "          token: ${{ secrets.GH_AW_GITHUB_TOKEN || secrets.GITHUB_TOKEN }}\n",
        )
        self.assertIn("checkout_persists_credentials_on_write_surface", self.rules())

    def test_empty_permissions_map_is_deny_all_not_indirection(self) -> None:
        self.write(
            ".github/workflows/deny.yml",
            "name: deny\non: [workflow_dispatch]\npermissions: {}\njobs: {}\n",
        )
        self.assertNotIn("unsupported_security_yaml_indirection", self.rules())

    def test_detects_floating_cargo_install_in_list_form_run(self) -> None:
        self.write(
            ".github/workflows/install.yml",
            "name: install\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo install cargo-example --locked\n",
        )
        self.assertIn("floating_cargo_install", self.rules())

    def test_accepts_immutable_cargo_install_forms(self) -> None:
        self.write(
            ".github/workflows/install.yml",
            "name: install\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cargo install cargo-a --version 1.2.3 --locked\n          cargo install cargo-b --git https://example.invalid/repo --rev deadbeef --locked\n          cargo install --path tools/local --locked\n",
        )
        self.assertNotIn("floating_cargo_install", self.rules())

    def test_accepts_immutable_cargo_install_with_shell_continuations(self) -> None:
        self.write(
            ".github/workflows/install.yml",
            "name: install\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cargo install cargo-example \\\n            --locked \\\n            --git https://example.invalid/repo \\\n            --rev deadbeef\n",
        )
        self.assertNotIn("floating_cargo_install", self.rules())

    def test_rejects_unpinned_cargo_install_with_shell_continuations(self) -> None:
        self.write(
            ".github/workflows/install.yml",
            "name: install\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cargo install cargo-example \\\n            --locked \\\n            --git https://example.invalid/repo\n",
        )
        self.assertIn("floating_cargo_install", self.rules())

    def test_rejects_unpinned_cargo_install_with_pin_like_comment(self) -> None:
        self.write(
            ".github/workflows/install.yml",
            "name: install\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cargo install cargo-example \\\n            --locked # --version 1.2.3\n",
        )
        self.assertIn("floating_cargo_install", self.rules())

    def test_rejects_unpinned_cargo_install_with_pin_like_follow_on_shell(self) -> None:
        self.write(
            ".github/workflows/install.yml",
            "name: install\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          cargo install cargo-example \\\n            --locked && echo --version 1.2.3\n",
        )
        self.assertIn("floating_cargo_install", self.rules())

    def test_security_sensitive_alias_is_not_silently_accepted(self) -> None:
        self.write(
            ".github/workflows/alias.yml",
            "name: alias\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - *shared_step\n",
        )
        self.assertIn("unsupported_security_yaml_indirection", self.rules())

    def test_security_sensitive_flow_map_is_not_silently_accepted(self) -> None:
        self.write(
            ".github/workflows/flow.yml",
            "name: flow\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - { run: cargo install cargo-example }\n",
        )
        self.assertIn("unsupported_security_yaml_indirection", self.rules())

    def test_baseline_ratchets_new_existing_and_resolved_findings(self) -> None:
        workflow = self.write(
            ".github/workflows/example.yml",
            "name: example\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v7\n",
        )
        baseline_path = self.write_current_baseline()
        args = Args()
        args.baseline_root = self.root
        self.assertEqual(
            ratchet.check(self.root, baseline_path, None, args, exact=False), 0
        )

        workflow.write_text(
            workflow.read_text(encoding="utf-8")
            + "      - run: cargo install cargo-new --locked\n",
            encoding="utf-8",
        )
        report = self.root / "report.json"
        self.assertEqual(
            ratchet.check(self.root, baseline_path, report, args, exact=False), 1
        )
        payload = json.loads(report.read_text(encoding="utf-8"))
        self.assertEqual(payload["counts"]["new"], 1)
        self.assertGreaterEqual(payload["counts"]["existing"], 1)

        workflow.write_text(
            "name: example\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@" + "a" * 40 + "\n",
            encoding="utf-8",
        )
        self.assertEqual(
            ratchet.check(self.root, baseline_path, report, args, exact=False), 0
        )
        payload = json.loads(report.read_text(encoding="utf-8"))
        self.assertGreaterEqual(payload["counts"]["resolved"], 1)

    def test_exact_baseline_rejects_preseeded_future_finding(self) -> None:
        baseline = self.write_current_baseline()
        payload = json.loads(baseline.read_text(encoding="utf-8"))
        raw = ratchet.RawFinding(
            "floating_cargo_install",
            ".github/workflows/future.yml",
            10,
            "cargo install future-tool",
            "cargo install must use --version, --path, or --git with --rev",
        )
        payload["findings"].extend(
            finding.as_dict() for finding in ratchet._fingerprint_findings([raw])
        )
        payload["findings"].sort(
            key=lambda item: (
                item["path"],
                item["rule"],
                item["line"],
                item["evidence"],
            )
        )
        baseline.write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        self.assertEqual(
            ratchet.check(self.root, baseline, None, self.exact_args(), exact=True),
            1,
        )

    def test_exact_baseline_rejects_resolved_debt_until_regenerated(self) -> None:
        workflow = self.write(
            ".github/workflows/debt.yml",
            "name: debt\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/cache@v4\n",
        )
        baseline = self.write_current_baseline()
        workflow.write_text(
            "name: debt\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/cache@" + "a" * 40 + "\n",
            encoding="utf-8",
        )
        self.assertEqual(
            ratchet.check(self.root, baseline, None, self.exact_args(), exact=True),
            1,
        )

    def test_baseline_fingerprint_tampering_is_rejected(self) -> None:
        self.write(
            ".github/workflows/debt.yml",
            "name: debt\non: [workflow_dispatch]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/cache@v4\n",
        )
        baseline = self.write_current_baseline()
        payload = json.loads(baseline.read_text(encoding="utf-8"))
        payload["findings"][0]["fingerprint"] = "0" * 64
        baseline.write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        with self.assertRaisesRegex(ValueError, "invalid fingerprints"):
            ratchet._load_baseline(baseline, root=self.root)

    def test_control_digest_tampering_is_rejected(self) -> None:
        baseline = self.write_current_baseline()
        self.write("scripts/ci/workflow_security_ratchet.py", "# weakened\n")
        with self.assertRaisesRegex(ValueError, "control digests"):
            ratchet._load_baseline(baseline, root=self.root)

    def test_deleted_control_file_is_rejected(self) -> None:
        baseline = self.write_current_baseline()
        (self.root / "scripts/ci/test_workflow_security_ratchet.py").unlink()
        with self.assertRaisesRegex(ValueError, "control is unavailable"):
            ratchet._load_baseline(baseline, root=self.root)

    def test_baseline_generation_is_byte_deterministic(self) -> None:
        first = self.write_current_baseline().read_bytes()
        second_path = self.root / ".ci/second.json"
        self.assertEqual(ratchet.write_baseline(self.root, second_path, Args()), 0)
        self.assertEqual(first, second_path.read_bytes())

    def test_invalid_utf8_is_an_actionable_finding(self) -> None:
        self.write(".github/workflows/invalid.yml", b"name: invalid\n\xff")
        self.assertIn("malformed_utf8", self.rules())

    def test_oversized_candidate_is_an_actionable_finding(self) -> None:
        self.write(".github/workflows/large.yml", "x" * 256)
        findings = ratchet.scan(self.root, max_file_bytes=64)
        self.assertIn(
            "candidate_file_oversized", [finding.rule for finding in findings]
        )

    @unittest.skipIf(os.name == "nt", "symlink semantics differ on Windows")
    def test_symlink_candidate_is_rejected_without_following_it(self) -> None:
        target = self.write("outside.yml", "name: outside\n")
        link = self.root / ".github/workflows/link.yml"
        link.symlink_to(target)
        self.assertIn("unsafe_candidate_file", self.rules())

    def test_candidate_commands_are_never_executed(self) -> None:
        sentinel = self.root / "must-not-exist"
        self.write(
            ".github/workflows/fork-candidate.yml",
            f"name: candidate\non:\n  pull_request:\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: touch {sentinel}\n",
        )
        ratchet.scan(self.root)
        self.assertFalse(sentinel.exists())

    def test_output_is_deterministic(self) -> None:
        self.write(
            ".github/workflows/z.yml",
            "name: z\non: [workflow_dispatch]\njobs:\n  z:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v7\n",
        )
        self.write(
            ".github/actions/example/action.yml",
            "name: a\nruns:\n  using: composite\n  steps:\n    - uses: actions/cache@v4\n",
        )
        first = [finding.as_dict() for finding in ratchet.scan(self.root)]
        second = [finding.as_dict() for finding in ratchet.scan(self.root)]
        self.assertEqual(first, second)


@unittest.skipUnless(shutil.which("git") and shutil.which("bash"), "requires Git and Bash")
class WorkflowSecurityApplicabilityTests(unittest.TestCase):
    """Execute the trusted workflow import step against real divergent Git trees."""

    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.env = dict(os.environ, GIT_AUTHOR_NAME="Test", GIT_AUTHOR_EMAIL="test@example.com",
                        GIT_COMMITTER_NAME="Test", GIT_COMMITTER_EMAIL="test@example.com",
                        GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                        GITHUB_ENV=str(self.root / "env"),
                        GITHUB_STEP_SUMMARY=str(self.root / "summary"), PR_NUMBER="7")
        self.git("init", "-q")
        self.git("remote", "add", "origin", str(self.repo))
        for path in ratchet.CONTROL_SOURCE_PATHS:
            self.write(path, "# inert fixture control\n")
        self.workflow = ".github/workflows/release.yml"
        self.bad = "name: release\non: [workflow_dispatch]\njobs:\n  release:\n    steps:\n      - run: echo '${{ github.event.inputs.version }}'\n"
        self.good = self.bad.replace("${{ github.event.inputs.version }}", "$INPUT_VERSION")
        self.write(self.workflow, self.bad)
        self.baseline()
        self.ancestor = self.commit("old debt")
        source = MODULE_PATH.parents[2] / ".github/workflows/workflow-security-ratchet.yml"
        step = source.read_text().split("      - name: Import exact PR head as inert Git data\n", 1)[1]
        step = step.split("      - name: ", 1)[0]
        self.script = textwrap.dedent(step.split("        run: |\n", 1)[1])
        self.assertIn('git fetch', self.script)
        self.assertIn('git archive', self.script)

    def git(self, *args: str) -> str:
        return subprocess.check_output(["git", "-c", "core.hooksPath=/dev/null", *args],
                                       cwd=self.repo, env=self.env, text=True,
                                       stderr=subprocess.PIPE).strip()

    def write(self, path: str, data: str) -> None:
        dest = self.repo / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(data)

    def baseline(self) -> None:
        ratchet.write_baseline(self.repo, self.repo / ".ci/workflow-security-baseline.json", Args())

    def commit(self, message: str) -> str:
        self.git("add", ".")
        self.git("commit", "-qm", message)
        return self.git("rev-parse", "HEAD")

    def repaired_base(self) -> str:
        self.write(self.workflow, self.good)
        self.baseline()
        return self.commit("repair debt")

    def run_import(self, base: str, head: str, *, git_error: str = "") -> subprocess.CompletedProcess:
        self.git("update-ref", "refs/pull/7/head", head)
        self.git("checkout", "--detach", "-q", base)
        env = dict(self.env, BASE_SHA=base, HEAD_SHA=head)
        if git_error:
            # A failed diff must not be confused with its ordinary 'changed' exit 1.
            bindir = self.root / "bin"
            bindir.mkdir()
            wrapper = bindir / "git"
            wrapper.write_text('#!/bin/bash\nif [ "$1" = "' + git_error +
                               '" ]; then exit 128; fi\nexec "' + shutil.which("git") + '" "$@"\n')
            wrapper.chmod(0o755)
            env["PATH"] = str(bindir) + os.pathsep + env["PATH"]
        return subprocess.run(["bash", "-c", self.script], cwd=self.repo, env=env,
                              text=True, capture_output=True, timeout=20)

    def assert_scanned_and_rejected(self, base: str, head: str) -> None:
        result = self.run_import(base, head)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.root / "env").exists())
        candidate = self.repo / "candidate"
        self.assertTrue(candidate.is_dir())
        args = Args()
        args.baseline_root = self.repo
        self.assertEqual(ratchet.check(candidate, self.repo / ".ci/workflow-security-baseline.json",
                                       None, args, exact=False), 1)
        self.assertIn("expression_in_run_source", [f.rule for f in ratchet.scan(candidate)])

    def test_stale_docs_head_skips_debt_retired_on_base(self) -> None:
        base = self.repaired_base()
        self.git("checkout", "--detach", "-q", self.ancestor)
        self.write("README.md", "docs only\n")
        head = self.commit("docs")
        result = self.run_import(base, head)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / "env").exists(), result.stdout + result.stderr)
        self.assertEqual((self.root / "env").read_text(), "SCOPED_NOOP=true\n")
        self.assertFalse((self.repo / "candidate").exists())

    def test_explicit_reintroduction_from_repaired_ancestor_is_rejected(self) -> None:
        base = self.repaired_base()
        self.write(self.workflow, self.bad)
        self.baseline()  # A matching candidate baseline cannot authorize the reintroduction.
        head = self.commit("restore retired debt")
        self.assert_scanned_and_rejected(base, head)

    def test_new_expression_with_matching_candidate_baseline_is_rejected(self) -> None:
        base = self.repaired_base()
        self.write(".github/workflows/new.yml", self.bad)
        self.baseline()
        head = self.commit("new debt")
        self.assert_scanned_and_rejected(base, head)

    def test_stale_control_change_remains_applicable(self) -> None:
        base = self.repaired_base()
        self.git("checkout", "--detach", "-q", self.ancestor)
        self.write(self.workflow, self.bad + "# changed control\n")
        head = self.commit("stale control change")
        self.assert_scanned_and_rejected(base, head)

    def assert_stale_control_only_is_applicable(self, path: str) -> None:
        base = self.repaired_base()
        self.git("checkout", "--detach", "-q", self.ancestor)
        target = self.repo / path
        self.write(path, target.read_text() + "\n")
        head = self.commit("control-only change")
        self.assert_scanned_and_rejected(base, head)

    def test_baseline_only_change_remains_applicable(self) -> None:
        self.assert_stale_control_only_is_applicable(".ci/workflow-security-baseline.json")

    def test_checker_only_change_remains_applicable(self) -> None:
        self.assert_stale_control_only_is_applicable("scripts/ci/workflow_security_ratchet.py")

    def test_test_only_change_remains_applicable(self) -> None:
        self.assert_stale_control_only_is_applicable("scripts/ci/test_workflow_security_ratchet.py")

    def assert_closed(self, base: str, head: str, *, git_error: str = "") -> None:
        result = self.run_import(base, head, git_error=git_error)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((self.root / "env").exists())
        self.assertFalse((self.repo / "candidate").exists())

    def test_missing_ancestry_fails_closed(self) -> None:
        tree = self.git("rev-parse", "HEAD^{tree}")
        unrelated = self.git("commit-tree", tree, "-m", "unrelated root")
        self.assert_closed(self.ancestor, unrelated)

    def test_ambiguous_ancestry_fails_closed(self) -> None:
        tree = self.git("rev-parse", "HEAD^{tree}")
        left = self.git("commit-tree", tree, "-p", self.ancestor, "-m", "left")
        right = self.git("commit-tree", tree, "-p", self.ancestor, "-m", "right")
        base = self.git("commit-tree", tree, "-p", left, "-p", right, "-m", "merge one")
        head = self.git("commit-tree", tree, "-p", right, "-p", left, "-m", "merge two")
        self.assertEqual(len(self.git("merge-base", "--all", base, head).splitlines()), 2)
        self.assert_closed(base, head)

    def test_diff_failure_fails_closed(self) -> None:
        self.assert_closed(self.ancestor, self.ancestor, git_error="diff")

    def test_merge_base_failure_fails_closed(self) -> None:
        self.assert_closed(self.ancestor, self.ancestor, git_error="merge-base")


if __name__ == "__main__":
    unittest.main()
