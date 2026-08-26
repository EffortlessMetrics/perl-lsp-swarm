#!/usr/bin/env python3
"""Falsifiers for retiring duplicate meta-shard fmt after protected rustfmt parity (#9959)."""

from __future__ import annotations

import importlib.util
import json
import sys
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = Path(__file__).with_name("hosted_formatter_producers.py")
SPEC = importlib.util.spec_from_file_location("hosted_formatter_producers", MODULE_PATH)
assert SPEC and SPEC.loader
producers = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = producers
SPEC.loader.exec_module(producers)

CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
EXECUTION_POLICY = ROOT / ".ci" / "gate-shard-execution.json"
GATE_POLICY = ROOT / ".ci" / "gate-policy.yaml"
REQUIRED_CHECKS = ROOT / ".ci" / "policies" / "required-checks.toml"


DECLARED_REASON = (
    "Advisory receipt-producing dedicated formatter context. "
    "Live merge blocking is owned by the required Perl LSP Rust Small Result "
    "path (#9127/#12320). Duplicate meta-shard fmt execution is retired by "
    "#9959. Settings-app promotion of this context remains #7087 and is not "
    "claimed here."
)


def load_tree() -> tuple[str, dict[object, object], str, dict[object, object]]:
    return (
        CI_WORKFLOW.read_text(encoding="utf-8"),
        json.loads(EXECUTION_POLICY.read_text(encoding="utf-8")),
        GATE_POLICY.read_text(encoding="utf-8"),
        tomllib.loads(REQUIRED_CHECKS.read_text(encoding="utf-8")),
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
    def setUp(self) -> None:
        self.ci, self.execution, self.gate_policy, self.required = load_tree()
        (
            self.retired_ci,
            self.retired_execution,
            self.retired_gate_policy,
            self.retired_required,
        ) = retire_meta_fmt(self.ci, self.execution, self.gate_policy, self.required)

    def validate(
        self,
        *,
        ci: str | None = None,
        execution: dict[object, object] | None = None,
        gate_policy: str | None = None,
        required: dict[object, object] | None = None,
    ) -> None:
        producers.validate_hosted_formatter_inventory(
            ci_workflow=self.retired_ci if ci is None else ci,
            execution_policy=self.retired_execution if execution is None else execution,
            gate_policy=self.retired_gate_policy if gate_policy is None else gate_policy,
            required_checks=self.retired_required if required is None else required,
        )

    def test_current_tree_has_one_declared_hosted_formatter_inventory(self) -> None:
        producers.validate_hosted_formatter_inventory(
            ci_workflow=self.ci,
            execution_policy=self.execution,
            gate_policy=self.gate_policy,
            required_checks=self.required,
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
            "python3 -m unittest scripts/ci/test_run_gate_shard.py",
            "cargo fmt --all -- --check\n          python3 -m unittest scripts/ci/test_run_gate_shard.py",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "undeclared hosted formatter producer"):
            self.validate(ci=broken)

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


if __name__ == "__main__":
    unittest.main()
