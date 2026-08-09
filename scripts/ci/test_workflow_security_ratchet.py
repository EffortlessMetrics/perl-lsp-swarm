#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import os
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


if __name__ == "__main__":
    unittest.main()
