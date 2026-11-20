#!/usr/bin/env python3
"""Falsifiers for retiring duplicate meta-shard fmt after protected rustfmt parity (#9959)."""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import tomllib
import unittest
from copy import deepcopy
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = Path(__file__).with_name("hosted_formatter_producers.py")
SPEC = importlib.util.spec_from_file_location("hosted_formatter_producers", MODULE_PATH)
assert SPEC and SPEC.loader
producers = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = producers
SPEC.loader.exec_module(producers)

EXECUTION_POLICY = ROOT / ".ci" / "gate-shard-execution.json"
GATE_POLICY = ROOT / ".ci" / "gate-policy.yaml"
REQUIRED_CHECKS = ROOT / ".ci" / "policies" / "required-checks.toml"
CI_WORKFLOW_KEY = ".github/workflows/ci.yml"
RUST_SMALL_KEY = ".github/workflows/em-ci-routed-rust.yml"


DECLARED_REASON = (
    "Advisory receipt-producing dedicated formatter context. "
    "Live merge blocking is owned by the required Perl LSP Rust Small Result "
    "path (#9127/#12320). Duplicate meta-shard fmt execution is retired by "
    "#9959. Settings-app promotion of this context remains #7087 and is not "
    "claimed here."
)


def load_workflows() -> dict[str, str]:
    texts: dict[str, str] = {}
    for path in sorted((ROOT / ".github" / "workflows").glob("*.yml")):
        texts[path.relative_to(ROOT).as_posix()] = path.read_text(encoding="utf-8")
    return texts


def load_tree() -> tuple[str, dict[object, object], str, dict[object, object], dict[str, str]]:
    workflows = load_workflows()
    return (
        workflows[CI_WORKFLOW_KEY],
        json.loads(EXECUTION_POLICY.read_text(encoding="utf-8")),
        GATE_POLICY.read_text(encoding="utf-8"),
        tomllib.loads(REQUIRED_CHECKS.read_text(encoding="utf-8")),
        workflows,
    )


def retire_meta_fmt(
    ci: str,
    execution: dict[object, object],
    gate_policy: str,
    required: dict[object, object],
) -> tuple[str, dict[object, object], str, dict[object, object]]:
    """Build the post-#9959 inventory so each mutation test isolates one defect."""
    retired_ci = ci.replace(
        "gates: fmt ignored_tests_check_refs",
        "gates: ignored_tests_check_refs",
        1,
    )
    retired_ci = retired_ci.replace(
        "  # Keep the existing advisory `fmt` meta-shard during the parity window tracked\n"
        "  # by #7087; this independently visible context is the ruleset promotion target.\n",
        "",
        1,
    )
    if "Standalone, candidate-bound formatter result." not in retired_ci:
        retired_ci = retired_ci.replace(
            "  # Standalone, candidate-bound formatter result for protected integration.\n",
            "  # Standalone, candidate-bound formatter result.\n",
            1,
        )
    retired_execution = json.loads(json.dumps(execution))
    gates = retired_execution.get("gates")
    if isinstance(gates, dict):
        gates.pop("fmt", None)
    retired_mapping = gate_policy.replace(
        "        - fmt\n        - ignored_tests_check_refs\n",
        "        - ignored_tests_check_refs\n",
        1,
    )
    retired_required = json.loads(json.dumps(required))
    entry = next(
        item
        for item in retired_required["checks"]
        if isinstance(item, dict) and item.get("name") == "Rust formatting"
    )
    entry["reason"] = DECLARED_REASON
    return retired_ci, retired_execution, retired_mapping, retired_required


class HostedFormatterProducerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        # These tests mutate in-memory fixtures, never the source files. Read the
        # tree once per suite (not in a process-global cache), then isolate every
        # case's nested policy and workflow mutations from that snapshot.
        cls._tree = load_tree()

    def setUp(self) -> None:
        self.ci, self.execution, self.gate_policy, self.required, self.workflows = deepcopy(self._tree)
        (
            self.retired_ci,
            self.retired_execution,
            self.retired_gate_policy,
            self.retired_required,
        ) = retire_meta_fmt(self.ci, self.execution, self.gate_policy, self.required)
        self.retired_workflows = dict(self.workflows)
        self.retired_workflows[CI_WORKFLOW_KEY] = self.retired_ci

    def validate(
        self,
        *,
        ci: str | None = None,
        execution: dict[object, object] | None = None,
        gate_policy: str | None = None,
        required: dict[object, object] | None = None,
        workflows: dict[str, str] | None = None,
    ) -> None:
        ci_text = self.retired_ci if ci is None else ci
        texts = dict(self.retired_workflows if workflows is None else workflows)
        texts[CI_WORKFLOW_KEY] = ci_text
        producers.validate_hosted_formatter_inventory(
            ci_workflow=ci_text,
            execution_policy=self.retired_execution if execution is None else execution,
            gate_policy=self.retired_gate_policy if gate_policy is None else gate_policy,
            required_checks=self.retired_required if required is None else required,
            workflows=texts,
        )

    def test_current_tree_has_one_declared_hosted_formatter_inventory(self) -> None:
        producers.validate_hosted_formatter_inventory(
            ci_workflow=self.ci,
            execution_policy=self.execution,
            gate_policy=self.gate_policy,
            required_checks=self.required,
            workflows=self.workflows,
        )

    def test_reintroducing_meta_fmt_fails_closed(self) -> None:
        broken = self.retired_ci.replace(
            "gates: ignored_tests_check_refs",
            "gates: fmt ignored_tests_check_refs",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "must not re-execute fmt"):
            self.validate(ci=broken)

    def test_fmt_in_a_non_meta_shard_fails_closed(self) -> None:
        broken = self.retired_ci.replace(
            "gates: unit_foundation_full",
            "gates: fmt unit_foundation_full",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "must not host a duplicate fmt producer"):
            self.validate(ci=broken)

    def test_stale_execution_policy_fmt_row_fails_closed(self) -> None:
        broken = json.loads(json.dumps(self.retired_execution))
        gates = broken["gates"]
        assert isinstance(gates, dict)
        gates["fmt"] = {"requires": [], "on_dependency_failure": "blocked_not_proven"}
        with self.assertRaisesRegex(AssertionError, "must not retain a retired meta fmt row"):
            self.validate(execution=broken)

    def test_stale_ci_gate_job_mapping_fails_closed(self) -> None:
        broken = self.retired_gate_policy.replace(
            "      gates:\n        - ignored_tests_check_refs\n",
            "      gates:\n        - fmt\n        - ignored_tests_check_refs\n",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "must not claim matrix-executed fmt"):
            self.validate(gate_policy=broken)

    def test_removing_local_pr_fast_fmt_fails_closed(self) -> None:
        broken = self.retired_gate_policy.replace(
            "  - name: fmt\n    tier: pr_fast\n",
            "  - name: fmt\n    tier: merge_gate\n",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "local pr-fast fmt gate must remain"):
            self.validate(gate_policy=broken)

    def test_removing_staged_rustfmt_fails_closed(self) -> None:
        broken = self.retired_gate_policy.replace(
            "  - name: rustfmt_staged\n    tier: commit\n",
            "  - name: rustfmt_staged\n    tier: nightly\n",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "rustfmt_staged must remain a commit-tier"):
            self.validate(gate_policy=broken)

    def test_removing_dedicated_receipt_producer_fails_closed(self) -> None:
        broken = self.retired_ci.replace(
            "scripts/ci/rustfmt_check.py", "scripts/ci/missing_formatter_producer.py"
        )
        with self.assertRaisesRegex(AssertionError, "must keep rustfmt_check.py"):
            self.validate(ci=broken)

    def test_undeclared_cargo_fmt_in_meta_shard_job_fails_closed(self) -> None:
        broken = self.retired_ci.replace(
            'python3 -m unittest "$self_test"',
            'cargo fmt --all -- --check\n            python3 -m unittest "$self_test"',
            1,
        )
        with self.assertRaisesRegex(AssertionError, "undeclared hosted formatter producer"):
            self.validate(ci=broken)

    def test_undeclared_cargo_fmt_in_another_workflow_fails_closed(self) -> None:
        broken_workflows = dict(self.retired_workflows)
        extra = (
            "name: Extra\n"
            "on: [pull_request]\n"
            "jobs:\n"
            "  extra-fmt:\n"
            "    runs-on: ubuntu-latest\n"
            "    steps:\n"
            "      - run: cargo fmt --all -- --check\n"
        )
        broken_workflows[".github/workflows/extra-fmt.yml"] = extra
        with self.assertRaisesRegex(
            AssertionError,
            r"undeclared hosted formatter producer: .github/workflows/extra-fmt.yml:extra-fmt",
        ):
            self.validate(workflows=broken_workflows)

    def test_undeclared_cargo_fmt_in_rust_small_result_fails_closed(self) -> None:
        rust_small = self.retired_workflows[RUST_SMALL_KEY]
        broken_workflows = dict(self.retired_workflows)
        broken_workflows[RUST_SMALL_KEY] = rust_small.replace(
            "python3 -m unittest \\",
            "cargo fmt --all -- --check\n          python3 -m unittest \\",
            1,
        )
        with self.assertRaisesRegex(
            AssertionError,
            r"undeclared hosted formatter producer: .github/workflows/em-ci-routed-rust.yml:rust-small-result",
        ):
            self.validate(workflows=broken_workflows)

    def _extra_workflow(self, steps: str) -> dict[str, str]:
        texts = dict(self.retired_workflows)
        texts[".github/workflows/extra-fmt.yml"] = (
            "name: Extra\n"
            "on: [pull_request]\n"
            "jobs:\n"
            "  extra-fmt:\n"
            "    runs-on: ubuntu-latest\n"
            "    steps:\n"
            f"{steps}"
        )
        return texts

    def test_undeclared_rust_checks_default_fmt_fails_closed(self) -> None:
        broken = self._extra_workflow(
            "      - uses: ./.github/actions/rust-checks\n"
        )
        with self.assertRaisesRegex(
            AssertionError,
            r"undeclared hosted formatter producer: .github/workflows/extra-fmt.yml:extra-fmt",
        ):
            self.validate(workflows=broken)

    def test_undeclared_rust_checks_in_ci_yml_fails_closed(self) -> None:
        broken_ci = self.retired_ci + (
            "\n  extra-rust-checks:\n"
            "    runs-on: ubuntu-latest\n"
            "    steps:\n"
            "      - uses: ./.github/actions/rust-checks\n"
        )
        with self.assertRaisesRegex(
            AssertionError,
            r"undeclared hosted formatter producer: .github/workflows/ci.yml:extra-rust-checks",
        ):
            self.validate(ci=broken_ci)

    def test_undeclared_rust_checks_explicit_fmt_fails_closed(self) -> None:
        broken = self._extra_workflow(
            "      - uses: ./.github/actions/rust-checks\n"
            "        with:\n"
            "          check-fmt: true\n"
            "          test: false\n"
        )
        with self.assertRaisesRegex(
            AssertionError,
            r"undeclared hosted formatter producer: .github/workflows/extra-fmt.yml:extra-fmt",
        ):
            self.validate(workflows=broken)

    def test_rust_checks_with_check_fmt_false_is_not_a_producer(self) -> None:
        allowed = self._extra_workflow(
            "      - uses: ./.github/actions/rust-checks\n"
            "        with:\n"
            "          check-fmt: false\n"
            "          test: false\n"
        )
        self.validate(workflows=allowed)

    def test_undeclared_direct_rustfmt_check_fails_closed(self) -> None:
        broken = self._extra_workflow(
            "      - run: rustfmt --check\n"
        )
        with self.assertRaisesRegex(
            AssertionError,
            r"undeclared hosted formatter producer: .github/workflows/extra-fmt.yml:extra-fmt",
        ):
            self.validate(workflows=broken)

    def test_undeclared_continued_rustfmt_check_fails_closed(self) -> None:
        broken = self._extra_workflow(
            "      - run: |\n"
            "          rustfmt \\\n"
            "            --edition 2024 \\\n"
            "            --check\n"
        )
        with self.assertRaisesRegex(
            AssertionError,
            r"undeclared hosted formatter producer: .github/workflows/extra-fmt.yml:extra-fmt",
        ):
            self.validate(workflows=broken)

    def test_file_scoped_rustfmt_ratchet_is_not_workspace_producer(self) -> None:
        allowed = self._extra_workflow(
            "      - run: rustfmt --edition 2024 --check xtask/tests/zed_host_receipt.rs\n"
        )
        self.validate(workflows=allowed)

    def test_parity_window_comment_cannot_keep_retired_meta_fmt(self) -> None:
        broken = self.retired_ci.replace(
            "Standalone, candidate-bound formatter result.",
            "Keep the existing advisory `fmt` meta-shard during the parity window tracked by #7087. Standalone, candidate-bound formatter result.",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "parity-window comment"):
            self.validate(ci=broken)

    def test_premature_dedicated_required_policy_fails_closed(self) -> None:
        broken = json.loads(json.dumps(self.retired_required))
        entry = next(
            item
            for item in broken["checks"]
            if isinstance(item, dict) and item.get("name") == "Rust formatting"
        )
        entry["required"] = True
        entry["policy_role"] = "required"
        entry["enforcement"] = "github-ruleset"
        with self.assertRaisesRegex(AssertionError, "must remain advisory"):
            self.validate(required=broken)

    def test_policy_reason_without_declared_parity_fails_closed(self) -> None:
        broken = json.loads(json.dumps(self.retired_required))
        entry = next(
            item
            for item in broken["checks"]
            if isinstance(item, dict) and item.get("name") == "Rust formatting"
        )
        entry["reason"] = "Post-merge promotion target tracked by #6202 and #7087."
        with self.assertRaisesRegex(AssertionError, "declare the current parity relationship"):
            self.validate(required=broken)


class HostedFormatterFixtureTests(unittest.TestCase):
    @staticmethod
    def tree():
        return (
            "ci source",
            {"gates": {"unit": {"requires": []}}},
            "gate policy",
            {"checks": [{"name": "Rust formatting", "reason": "original"}]},
            {CI_WORKFLOW_KEY: "ci source", "other.yml": "other source"},
        )

    def test_one_load_per_suite_preserves_nested_mutation_isolation(self):
        cases = []

        class Probe(HostedFormatterProducerTests):
            def runTest(self):
                cases.append(self)
                if len(cases) == 1:
                    self.execution["gates"]["unit"]["requires"].append("changed")
                    self.required["checks"][0]["reason"] = "changed"
                    self.workflows["other.yml"] = "changed"
                    self.retired_execution["gates"]["unit"]["requires"].append("retired")
                    self.retired_required["checks"][0]["reason"] = "retired"
                    self.retired_workflows["other.yml"] = "retired"

        with patch(__name__ + ".load_tree", side_effect=self.tree) as load:
            result = unittest.TestResult()
            unittest.TestSuite([Probe(), Probe()]).run(result)
        self.assertEqual(result.testsRun, 2)
        self.assertTrue(result.wasSuccessful(), (result.errors, result.failures))
        self.assertEqual(load.call_count, 1)
        second = cases[1]
        self.assertEqual(second.execution, self.tree()[1])
        self.assertEqual(second.required, self.tree()[3])
        self.assertEqual(second.workflows, self.tree()[4])
        self.assertEqual(second.retired_execution, self.tree()[1])
        self.assertEqual(second.retired_required["checks"][0]["reason"], DECLARED_REASON)
        self.assertEqual(second.retired_workflows, self.tree()[4])

    def test_new_suite_reloads_inputs_and_does_not_hide_load_failure(self):
        observed = []

        class Probe(HostedFormatterProducerTests):
            def runTest(self):
                observed.append(self.ci)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workflow = root / CI_WORKFLOW_KEY
            workflow.parent.mkdir(parents=True)
            execution = root / "execution.json"
            execution.write_text(json.dumps(self.tree()[1]), encoding="utf-8")
            gates = root / "gates.yaml"
            gates.write_text("gate policy", encoding="utf-8")
            required = root / "required.toml"
            required.write_text('[[checks]]\nname = "Rust formatting"\nreason = "original"\n', encoding="utf-8")
            # Exercise the real loader so a process-global cache cannot hide
            # edits or a missing input on a subsequent unittest invocation.
            with patch.multiple(
                sys.modules[__name__], ROOT=root, EXECUTION_POLICY=execution,
                GATE_POLICY=gates, REQUIRED_CHECKS=required,
            ):
                for text in ("ci source", "new ci source"):
                    workflow.write_text(text, encoding="utf-8")
                    result = unittest.TestResult()
                    unittest.TestSuite([Probe()]).run(result)
                    self.assertEqual(result.testsRun, 1)
                    self.assertTrue(result.wasSuccessful(), (result.errors, result.failures))
                execution.unlink()
                failed = unittest.TestResult()
                unittest.TestSuite([Probe()]).run(failed)
        self.assertEqual(observed, ["ci source", "new ci source"])
        self.assertFalse(failed.wasSuccessful())
        self.assertEqual(len(failed.errors), 1)
        self.assertIn("FileNotFoundError", failed.errors[0][1])


if __name__ == "__main__":
    unittest.main()
