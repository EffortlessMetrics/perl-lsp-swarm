#!/usr/bin/env python3
"""Executable contract for the gate-receipt step-summary renderer.

The regression these tests exist for: the renderer used to look for gate
statuses `passed` / `failed`, which the receipt contract never emits. Against a
real receipt no gate matched either branch, `failed` was always 0, and a
receipt describing five *failed* gates rendered as
``**Status**: All 0/5 gates passed``.

So the load-bearing cases here are the ones that read a receipt in the shape
`.ci/receipt.schema.json` actually defines, and the ones that bind this
renderer's vocabulary back to that schema and to `gates.rs`.
"""

from __future__ import annotations

import importlib.util
import json
import os
import re
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("render_summary.py")
SPEC = importlib.util.spec_from_file_location("upload_receipt_render_summary", MODULE_PATH)
assert SPEC and SPEC.loader
render_summary = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = render_summary
SPEC.loader.exec_module(render_summary)

REPO_ROOT = Path(__file__).resolve().parents[3]
RECEIPT_SCHEMA = REPO_ROOT / ".ci" / "receipt.schema.json"
GATES_SOURCE = REPO_ROOT / "xtask" / "src" / "tasks" / "gates.rs"
ACTION_YML = Path(__file__).with_name("action.yml")


def gate(name: str, status: str, **overrides: object) -> dict:
    """One gate result in the shape `.ci/receipt.schema.json` defines."""
    result = {
        "gate_name": name,
        "tier": "pr_fast",
        "status": status,
        "duration_ms": 1500,
        "command": f"cargo xtask {name}",
        "exit_code": 0 if status == "pass" else 1,
    }
    result.update(overrides)
    return result


def receipt(*gates: dict, **overrides: object) -> dict:
    """A contract-shaped receipt wrapping `gates`."""
    data = {
        "schema_version": "1.0.0",
        "metadata": {
            "timestamp": "2026-01-24T15:30:00Z",
            "git_sha": "13c4d91c1234567890abcdef1234567890abcdef",
            "git_branch": "main",
        },
        "gates": list(gates),
        "summary": {
            "total_gates": len(gates),
            "passed": sum(1 for g in gates if g.get("status") == "pass"),
            "failed": sum(1 for g in gates if g.get("status") == "fail"),
            "skipped": sum(1 for g in gates if g.get("status") == "skip"),
            "total_duration_ms": 12000,
            "overall_status": "pass",
        },
    }
    data.update(overrides)
    return data


class FailingReceiptTests(unittest.TestCase):
    """The reported regression, stated as behaviour."""

    def test_five_failed_gates_never_render_as_passed(self) -> None:
        rendered = render_summary.render(
            receipt(*(gate(f"gate_{i}", "fail") for i in range(5)))
        )
        self.assertNotIn("gates passed", rendered)
        self.assertIn("**Status**: 5/5 gates failing (5 failed)", rendered)

    def test_the_exact_reported_string_is_gone(self) -> None:
        rendered = render_summary.render(
            receipt(*(gate(f"gate_{i}", "fail") for i in range(5)))
        )
        self.assertNotIn("All 0/5 gates passed", rendered)

    def test_gate_names_are_read_from_the_contract_field(self) -> None:
        """`gate_name` is the contract field; the old renderer read `name`."""
        rendered = render_summary.render(receipt(gate("clippy", "fail")))
        self.assertIn("| clippy |", rendered)
        self.assertNotIn("unknown", rendered)

    def test_a_single_failure_among_passes_is_reported(self) -> None:
        rendered = render_summary.render(
            receipt(gate("fmt", "pass"), gate("clippy", "fail"), gate("unit", "pass"))
        )
        self.assertIn("**Status**: 1/3 gates failing (1 failed), 2 passed", rendered)
        self.assertIn("| clippy | **FAIL** | 1 | 1.5s |", rendered)


    def test_a_failing_headline_accounts_for_every_gate(self) -> None:
        """Skips must not vanish from the headline once a failure is present."""
        gates = (
            [gate(f"p{i}", "pass") for i in range(6)]
            + [gate("unit", "fail")]
            + [gate(f"s{i}", "skip") for i in range(4)]
        )
        line = next(
            line
            for line in render_summary.render(receipt(*gates)).splitlines()
            if line.startswith("**Status**")
        )
        self.assertIn("4 skipped", line)
        tallied = [
            int(n)
            for n in re.findall(
                r"(\d+) (?:failed|timed out|errored|skipped|passed)", line
            )
        ]
        self.assertEqual(
            sum(tallied), len(gates), f"headline does not account for every gate: {line}"
        )


class BlockingStatusTests(unittest.TestCase):
    """`timeout` and `error` block just as `fail` does."""

    def test_timeout_blocks(self) -> None:
        rendered = render_summary.render(receipt(gate("unit", "timeout")))
        self.assertIn("**Status**: 1/1 gates failing (1 timed out)", rendered)
        self.assertNotIn("gates passed", rendered)

    def test_error_blocks(self) -> None:
        rendered = render_summary.render(receipt(gate("unit", "error")))
        self.assertIn("**Status**: 1/1 gates failing (1 errored)", rendered)
        self.assertNotIn("gates passed", rendered)

    def test_mixed_blocking_statuses_are_itemised(self) -> None:
        rendered = render_summary.render(
            receipt(
                gate("a", "fail"),
                gate("b", "timeout"),
                gate("c", "error"),
                gate("d", "pass"),
            )
        )
        self.assertIn(
            "**Status**: 3/4 gates failing (1 failed, 1 timed out, 1 errored), 1 passed",
            rendered,
        )


class PassingReceiptTests(unittest.TestCase):
    def test_all_passing_reports_success(self) -> None:
        rendered = render_summary.render(receipt(gate("fmt", "pass"), gate("unit", "pass")))
        self.assertIn("**Status**: All 2/2 gates passed", rendered)

    def test_skips_are_reported_rather_than_counted_as_passes(self) -> None:
        rendered = render_summary.render(
            receipt(gate("fmt", "pass"), gate("nightly", "skip"))
        )
        self.assertIn("**Status**: 1/2 gates passed, 1 skipped", rendered)
        self.assertNotIn("All 1/2", rendered)


class NotProvenTests(unittest.TestCase):
    """An unusable gate set is never success."""

    def test_absent_gates_key(self) -> None:
        data = receipt()
        del data["gates"]
        rendered = render_summary.render(data)
        self.assertIn("NOT_PROVEN", rendered)
        self.assertNotIn("gates passed", rendered)

    def test_empty_gate_list(self) -> None:
        rendered = render_summary.render(receipt())
        self.assertIn("**Status**: NOT_PROVEN — receipt reports no gates", rendered)
        self.assertNotIn("gates passed", rendered)

    def test_non_list_gates(self) -> None:
        rendered = render_summary.render(receipt(**{"gates": {"fmt": "pass"}}))
        self.assertIn("NOT_PROVEN", rendered)
        self.assertIn("must be an array of objects", rendered)

    def test_list_of_non_objects(self) -> None:
        rendered = render_summary.render(receipt(**{"gates": ["fmt", "clippy"]}))
        self.assertIn("NOT_PROVEN", rendered)

    def test_unrecognized_status_is_not_proven(self) -> None:
        """The old vocabulary is now explicitly outside the contract."""
        rendered = render_summary.render(
            receipt(gate("fmt", "passed"), gate("clippy", "failed"))
        )
        self.assertIn("NOT_PROVEN", rendered)
        self.assertIn("2/2 gates carry a status outside the receipt contract", rendered)
        self.assertNotIn("gates passed", rendered)

    def test_one_unrecognized_status_poisons_an_otherwise_green_set(self) -> None:
        rendered = render_summary.render(
            receipt(gate("fmt", "pass"), gate("clippy", "succeeded"))
        )
        self.assertIn("**Status**: NOT_PROVEN — 1/2 gates carry a status", rendered)

    def test_a_gate_status_colliding_with_the_tally_key_is_not_proven(self) -> None:
        """`unrecognized` is a tally key, not a gate status the contract allows."""
        counts = render_summary.count_statuses([gate("fmt", "unrecognized")])
        self.assertEqual(counts["unrecognized"], 1)
        rendered = render_summary.render(receipt(gate("fmt", "unrecognized")))
        self.assertIn("NOT_PROVEN", rendered)

    def test_missing_status_field_is_not_proven(self) -> None:
        broken = gate("fmt", "pass")
        del broken["status"]
        rendered = render_summary.render(receipt(broken))
        self.assertIn("NOT_PROVEN", rendered)

    def test_a_bare_status_word_is_not_a_gate(self) -> None:
        """A gate missing every other required field must not report a pass."""
        rendered = render_summary.render({"gates": [{"status": "pass"}]})
        self.assertIn("NOT_PROVEN", rendered)
        self.assertNotIn("gates passed", rendered)

    def test_each_contract_required_gate_field_is_load_bearing(self) -> None:
        for field in render_summary.GATE_REQUIRED_FIELDS:
            with self.subTest(missing=field):
                broken = gate("fmt", "pass")
                del broken[field]
                rendered = render_summary.render(receipt(broken))
                self.assertIn("NOT_PROVEN", rendered)
                self.assertNotIn("gates passed", rendered)

    def test_incomplete_gates_are_counted_in_the_status_line(self) -> None:
        broken = gate("clippy", "pass")
        del broken["command"]
        rendered = render_summary.render(receipt(gate("fmt", "pass"), broken))
        self.assertIn(
            "**Status**: NOT_PROVEN — 1/2 gates are missing contract-required fields",
            rendered,
        )

    def test_legacy_name_alone_still_satisfies_gate_identity(self) -> None:
        """The `name` fallback the issue asks for is not defeated by this check."""
        legacy = gate("fmt", "pass")
        del legacy["gate_name"]
        legacy["name"] = "fmt"
        rendered = render_summary.render(receipt(legacy))
        self.assertIn("**Status**: All 1/1 gates passed", rendered)

    def test_a_gate_legitimately_named_unknown_is_still_complete(self) -> None:
        """`unknown` matches the contract's gate-name pattern; it is not a sentinel."""
        self.assertTrue(render_summary.gate_is_complete(gate("unknown", "pass")))

    def test_each_contract_required_top_level_field_is_load_bearing(self) -> None:
        for field in render_summary.RECEIPT_REQUIRED_FIELDS:
            with self.subTest(missing=field):
                data = receipt(gate("fmt", "pass"))
                del data[field]
                rendered = render_summary.render(data)
                self.assertIn("NOT_PROVEN", rendered)
                self.assertNotIn("gates passed", rendered)

    def test_non_object_receipt(self) -> None:
        rendered = render_summary.render(["not", "a", "receipt"])
        self.assertIn("NOT_PROVEN — receipt is not a JSON object", rendered)


class CanonicalExampleReceiptTests(unittest.TestCase):
    """Render the receipts the repository actually commits as canonical.

    `.ci/examples/receipt-*.json` are real `gates.rs` output, not fixtures
    written for this test. Each one carries its own `summary`, which makes it
    an oracle independent of this renderer: the counts the renderer derives
    from `gates` must equal the counts the receipt reports for itself.
    """

    def examples(self) -> list[tuple[str, dict]]:
        paths = sorted((REPO_ROOT / ".ci" / "examples").glob("receipt-*.json"))
        self.assertGreaterEqual(len(paths), 3, "canonical example receipts missing")
        return [(p.name, json.loads(p.read_text(encoding="utf-8"))) for p in paths]

    def test_derived_counts_match_each_receipts_own_summary(self) -> None:
        for name, data in self.examples():
            with self.subTest(receipt=name):
                counts = render_summary.count_statuses(data["gates"])
                summary = data["summary"]
                self.assertEqual(counts["unrecognized"], 0)
                self.assertEqual(counts["pass"], summary["passed"])
                self.assertEqual(counts["fail"], summary["failed"])
                self.assertEqual(counts["skip"], summary["skipped"])
                self.assertEqual(counts["timeout"], summary.get("timeout", 0))
                self.assertEqual(counts["error"], summary.get("error", 0))
                self.assertEqual(len(data["gates"]), summary["total_gates"])

    def test_no_example_renders_a_verdict_its_summary_contradicts(self) -> None:
        for name, data in self.examples():
            with self.subTest(receipt=name):
                rendered = render_summary.render(data)
                if data["summary"]["overall_status"] == "pass":
                    self.assertIn("gates passed", rendered)
                else:
                    self.assertNotIn("gates passed", rendered)
                    self.assertIn("gates failing", rendered)

    def test_the_committed_partial_failure_receipt_is_not_reported_as_passed(self) -> None:
        """The headline case, against a receipt the repository already ships."""
        path = REPO_ROOT / ".ci" / "examples" / "receipt-partial-failure.json"
        rendered = render_summary.render_receipt_file(str(path))
        self.assertIn("**Status**: 1/11 gates failing (1 failed), 4 skipped, 6 passed", rendered)
        self.assertNotIn("gates passed", rendered)
        self.assertIn("| test-lib | **FAIL** |", rendered)
        self.assertIn("| policy | skip |", rendered)

    def test_the_committed_success_receipt_still_reports_success(self) -> None:
        path = REPO_ROOT / ".ci" / "examples" / "receipt-full-success.json"
        rendered = render_summary.render_receipt_file(str(path))
        self.assertIn("**Status**: All 11/11 gates passed", rendered)


class ContractBindingTests(unittest.TestCase):
    """Bind the renderer to the authorities it reports, not to a copy of them."""

    def test_status_vocabulary_matches_the_receipt_schema(self) -> None:
        schema = json.loads(RECEIPT_SCHEMA.read_text(encoding="utf-8"))
        enum = schema["$defs"]["gate_result"]["properties"]["status"]["enum"]
        self.assertEqual(
            sorted(render_summary.RECOGNIZED_STATUSES),
            sorted(enum),
            "renderer vocabulary drifted from .ci/receipt.schema.json",
        )

    def test_blocking_statuses_match_gates_rs(self) -> None:
        """Bind to the statuses the function names, not to how it is written.

        This reads Rust from Python, so it must fail only on a real vocabulary
        change. Matching the exact `matches!(status, …)` shape would also fail
        on a parameter rename, a rewrite as a `match` expression, or a plain
        rustfmt re-split — false reds charged to unrelated Rust work. Taking the
        function body and collecting its string literals tolerates all three
        while still catching a changed status set, which is the only thing this
        guard is for.
        """
        source = GATES_SOURCE.read_text(encoding="utf-8")
        match = re.search(
            r"fn is_blocking_gate_status\s*\([^)]*\)\s*->\s*bool\s*\{(.*?)\n\}",
            source,
            re.DOTALL,
        )
        self.assertIsNotNone(
            match, "could not locate is_blocking_gate_status in gates.rs"
        )
        assert match is not None
        body = re.sub(r"/\*.*?\*/", "", match.group(1), flags=re.DOTALL)
        body = re.sub(r"//[^\n]*", "", body)
        # Ask only which *contract* statuses the function names. Collecting
        # every literal instead would make an added log line or diagnostic
        # string read as a status and fail CI without any policy change.
        declared = sorted(
            status
            for status in render_summary.RECOGNIZED_STATUSES
            if f'"{status}"' in body
        )
        self.assertEqual(
            sorted(render_summary.BLOCKING_STATUSES),
            declared,
            "renderer blocking statuses drifted from gates.rs",
        )

    def test_required_field_sets_match_the_receipt_schema(self) -> None:
        schema = json.loads(RECEIPT_SCHEMA.read_text(encoding="utf-8"))
        self.assertEqual(
            sorted(render_summary.RECEIPT_REQUIRED_FIELDS),
            sorted(schema["required"]),
            "renderer top-level required fields drifted from the schema",
        )
        self.assertEqual(
            sorted(render_summary.GATE_REQUIRED_FIELDS),
            sorted(schema["$defs"]["gate_result"]["required"]),
            "renderer gate required fields drifted from the schema",
        )

    def test_the_contract_job_runs_when_either_authority_changes(self) -> None:
        """The drift guard is only real if a change to an authority triggers it.

        `test_status_vocabulary_matches_the_receipt_schema` and
        `test_blocking_statuses_match_gates_rs` are the guard; they are inert
        unless this workflow's path filters select the files they read.
        """
        workflow = (
            REPO_ROOT / ".github" / "workflows" / "composite-action-contracts.yml"
        ).read_text(encoding="utf-8")
        for trigger_block in workflow.split("paths:")[1:3]:
            for authority in (".ci/receipt.schema.json", "xtask/src/tasks/gates.rs"):
                self.assertIn(authority, trigger_block)

    def test_blocking_statuses_are_part_of_the_vocabulary(self) -> None:
        for status in render_summary.BLOCKING_STATUSES:
            self.assertIn(status, render_summary.RECOGNIZED_STATUSES)

    def test_the_old_field_names_are_not_expressible_in_the_contract(self) -> None:
        """Negative control: the previous renderer read keys the schema forbids."""
        schema = json.loads(RECEIPT_SCHEMA.read_text(encoding="utf-8"))
        self.assertFalse(schema.get("additionalProperties", True))
        for forbidden in ("generated_at", "commit", "total_duration_seconds"):
            self.assertNotIn(forbidden, schema["properties"])
        gate_result = schema["$defs"]["gate_result"]
        self.assertFalse(gate_result.get("additionalProperties", True))
        for forbidden in ("name", "duration_seconds"):
            self.assertNotIn(forbidden, gate_result["properties"])

    def test_the_fixtures_are_shaped_like_real_receipts(self) -> None:
        """Anti-vacuity control.

        These tests only prove anything if the receipt they feed the renderer
        is the receipt the contract describes. Both objects declare
        `additionalProperties: false`, so key-set containment is a real check:
        an invented field would fail schema validation in CI, and a missing
        required field would mean the fixture is not a receipt at all.
        """
        schema = json.loads(RECEIPT_SCHEMA.read_text(encoding="utf-8"))
        fixture = receipt(gate("fmt", "pass"), gate("clippy", "fail"))

        self.assertLessEqual(set(fixture), set(schema["properties"]))
        self.assertLessEqual(set(schema["required"]), set(fixture))

        gate_result = schema["$defs"]["gate_result"]
        for entry in fixture["gates"]:
            self.assertLessEqual(set(entry), set(gate_result["properties"]))
            self.assertLessEqual(set(gate_result["required"]), set(entry))

        summary = schema["$defs"]["summary"]
        self.assertLessEqual(set(fixture["summary"]), set(summary["properties"]))
        self.assertLessEqual(set(summary["required"]), set(fixture["summary"]))

        metadata = schema["$defs"]["metadata"]
        self.assertLessEqual(set(fixture["metadata"]), set(metadata["properties"]))

    def test_action_invokes_this_renderer(self) -> None:
        """The module is the action's renderer, not a parallel copy of it."""
        action = ACTION_YML.read_text(encoding="utf-8")
        self.assertIn('python3 "$GITHUB_ACTION_PATH/render_summary.py"', action)


class ProvenanceTests(unittest.TestCase):
    def test_metadata_is_read_from_the_contract_location(self) -> None:
        rendered = render_summary.render(receipt(gate("fmt", "pass")))
        self.assertIn("**Generated**: 2026-01-24T15:30:00Z", rendered)
        self.assertIn("<code>13c4d91c1234</code>", rendered)

    def test_empty_metadata_reports_unknown_provenance(self) -> None:
        """Present but uninformative metadata degrades; it does not block."""
        data = receipt(gate("fmt", "pass"))
        data["metadata"] = {}
        rendered = render_summary.render(data)
        self.assertIn("**Generated**: unknown", rendered)
        self.assertIn("<code>unknown</code>", rendered)

    def test_total_duration_comes_from_summary_total_duration_ms(self) -> None:
        rendered = render_summary.render(receipt(gate("fmt", "pass")))
        self.assertIn("**Total duration**: 12.0s", rendered)

    def test_absent_total_duration_is_omitted_rather_than_guessed(self) -> None:
        data = receipt(gate("fmt", "pass"))
        del data["summary"]["total_duration_ms"]
        self.assertNotIn("Total duration", render_summary.render(data))

    def test_null_exit_code_is_rendered_as_missing(self) -> None:
        rendered = render_summary.render(receipt(gate("unit", "timeout", exit_code=None)))
        self.assertIn(f"| {render_summary.MISSING} |", rendered)
        self.assertNotIn("None", rendered)

    def test_unusable_duration_is_rendered_as_missing(self) -> None:
        """Present but non-numeric: the field is there, the value is not usable."""
        rendered = render_summary.render(receipt(gate("fmt", "pass", duration_ms="soon")))
        self.assertIn(f"| {render_summary.MISSING} |", rendered)
        self.assertNotIn("soon", rendered)


class EscapingTests(unittest.TestCase):
    """Receipt content is data; it must not restructure the summary."""

    def test_pipe_in_gate_name_cannot_add_table_columns(self) -> None:
        rendered = render_summary.render(receipt(gate("a | b | c", "pass")))
        self.assertIn("a &#124; b &#124; c", rendered)

    def test_newline_in_gate_name_cannot_add_table_rows(self) -> None:
        rendered = render_summary.render(receipt(gate("a\n| evil | pass | 0 | 0s |", "pass")))
        row_count = sum(1 for line in rendered.splitlines() if line.startswith("| "))
        self.assertEqual(row_count, 2, "header row plus exactly one gate row")

    def test_markup_in_gate_name_is_escaped(self) -> None:
        rendered = render_summary.render(receipt(gate("<img src=x>", "pass")))
        self.assertNotIn("<img", rendered)
        self.assertIn("&lt;img", rendered)


class FileEntryPointTests(unittest.TestCase):
    def test_missing_receipt_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            rendered = render_summary.render_receipt_file(str(Path(tmp) / "absent.json"))
        self.assertIn("**Status**: NOT_PROVEN — receipt file is missing", rendered)
        self.assertIn("Receipt not found at", rendered)

    def test_malformed_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "receipt.json"
            path.write_text("{not json", encoding="utf-8")
            rendered = render_summary.render_receipt_file(str(path))
        self.assertIn("**Status**: NOT_PROVEN — receipt could not be read", rendered)
        self.assertIn("Failed to read receipt", rendered)

    def test_undecodable_receipt_file(self) -> None:
        """Invalid UTF-8 is a read failure, not a crash."""
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "receipt.json"
            path.write_bytes(b'{"gates": [\xff\xfe]}')
            rendered = render_summary.render_receipt_file(str(path))
        self.assertIn("**Status**: NOT_PROVEN — receipt could not be read", rendered)

    def test_an_unrepresentable_duration_degrades_instead_of_crashing(self) -> None:
        """A schema-valid integer can exceed float range; `/` raises OverflowError.

        `duration_ms` is bounded below but not above, and JSON integers are
        unbounded, so this receipt is contract-valid. A crash here would publish
        no verdict at all — the same failure as an unreadable receipt, reached
        through the duration column.
        """
        huge = 10**400
        rendered = render_summary.render(receipt(gate("fmt", "pass", duration_ms=huge)))
        self.assertIn(f"| {render_summary.MISSING} |", rendered)
        self.assertIn("**Status**: All 1/1 gates passed", rendered)

    def test_an_unrepresentable_total_duration_is_omitted_not_fatal(self) -> None:
        data = receipt(gate("fmt", "pass"))
        data["summary"]["total_duration_ms"] = 10**400
        rendered = render_summary.render(data)
        self.assertNotIn("Total duration", rendered)
        self.assertIn("**Status**: All 1/1 gates passed", rendered)

    def test_non_finite_durations_are_unusable(self) -> None:
        for value in (float("inf"), float("-inf"), float("nan")):
            with self.subTest(value=value):
                self.assertIsNone(render_summary.milliseconds_as_seconds(value))

    def test_every_rendering_carries_exactly_one_status_line(self) -> None:
        """The invariant behind the whole claim: no output is silent about its verdict.

        A rendering with no `**Status**` line leaves a consumer scanning for one
        with nothing to find, which is the failure mode this module exists to
        remove — reached by omission rather than by a false pass.
        """
        renderings = [
            render_summary.render(receipt(gate("fmt", "pass"))),
            render_summary.render(receipt(gate("fmt", "fail"), gate("u", "skip"))),
            render_summary.render(receipt()),
            render_summary.render(receipt(gate("fmt", "passed"))),
            render_summary.render(receipt(**{"gates": {"fmt": "pass"}})),
            render_summary.render(receipt(**{"gates": ["fmt"]})),
            render_summary.render({"gates": [{"status": "pass"}]}),
            render_summary.render(["not", "a", "receipt"]),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            renderings.append(
                render_summary.render_receipt_file(str(Path(tmp) / "absent.json"))
            )
            bad = Path(tmp) / "bad.json"
            bad.write_text("{not json", encoding="utf-8")
            renderings.append(render_summary.render_receipt_file(str(bad)))
            good = Path(tmp) / "good.json"
            good.write_text(json.dumps(receipt(gate("fmt", "pass"))), encoding="utf-8")
            renderings.append(render_summary.render_receipt_file(str(good)))

        for rendered in renderings:
            with self.subTest(first_line=rendered.splitlines()[:1]):
                status_lines = [
                    line
                    for line in rendered.splitlines()
                    if line.startswith("**Status**")
                ]
                self.assertEqual(len(status_lines), 1, rendered)

    def test_main_writes_the_rendered_summary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt_path = Path(tmp) / "receipt.json"
            receipt_path.write_text(
                json.dumps(receipt(gate("clippy", "fail"))), encoding="utf-8"
            )
            summary_path = Path(tmp) / "summary.md"
            env = {
                "RECEIPT_PATH": str(receipt_path),
                "GITHUB_STEP_SUMMARY": str(summary_path),
            }
            with mock.patch.dict(os.environ, env, clear=True):
                self.assertEqual(render_summary.main(), 0)
            written = summary_path.read_text(encoding="utf-8")
        self.assertIn("**Status**: 1/1 gates failing (1 failed)", written)

    def test_main_is_a_no_op_without_a_step_summary(self) -> None:
        with mock.patch.dict(os.environ, {}, clear=True):
            self.assertEqual(render_summary.main(), 0)


if __name__ == "__main__":
    unittest.main()
