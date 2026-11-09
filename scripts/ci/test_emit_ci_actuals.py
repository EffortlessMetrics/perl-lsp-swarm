#!/usr/bin/env python3
"""Compiler-free regression tests for CI actuals observation admission."""

from __future__ import annotations

import copy
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from contextlib import contextmanager, redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

import aggregate_lane_history as history  # noqa: E402
import emit_ci_actuals as emitter  # noqa: E402


SHA = "a" * 40
LANE = "fixture_lane"
RUNNER = "ubuntu_24_04"


def gate(name="fixture_gate", duration=60000, status="pass"):
    return {
        "gate_name": name, "tier": "merge_gate", "status": status,
        "duration_ms": duration, "command": "fixture only; never executed",
    }


def envelope(rows=None):
    rows = [gate()] if rows is None else rows
    return {
        "schema_version": "gates.v1",
        "metadata": {
            "timestamp": "2026-01-01T00:00:00Z",
            "git_sha": SHA, "git_sha_short": SHA[:7], "git_branch": "main",
            "git_dirty": False, "toolchain": {"rustc_version": "1.95.0"},
            "platform": {"os": "linux", "arch": "x86_64"},
            "environment": {"type": "ci", "ci_provider": "github-actions",
                            "ci_run_id": "123", "ci_run_attempt": 1},
        },
        "gates": rows,
        "summary": {
            "total_gates": len(rows), "passed": len(rows),
            "failed": 0, "skipped": 0,
            # The producer measures wall time separately; it is not a gate sum.
            "total_duration_ms": 7, "overall_status": "pass",
        },
    }


class EmitCiActualsTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="ci-actuals-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.receipts = self.root / "receipts"
        self.receipts.mkdir()
        self.output = self.root / "history/run-123/artifacts/fixture/ci-actuals.json"
        self.budget = self.root / "budget.toml"
        self.lanes = self.root / "lanes.toml"
        self.protected = {}
        self.write(self.root / "neighbor.txt", "neighbor must survive\n")
        self.write_policy()
        self.write_receipt(envelope())

    def write(self, path, content):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        self.protected[path] = path.read_bytes()

    def write_policy(self, multiplier="1.0", floor="5.0"):
        self.write(self.budget, f"[runner_multipliers]\n{RUNNER} = {multiplier}\n")
        self.write(self.lanes, f"[lane.{LANE}]\nbase_lem = {floor}\n")

    def write_receipt(self, document, name="canonical.json"):
        # Some refusal fixtures deliberately contain NaN/Infinity.
        self.write(self.receipts / name, json.dumps(document) + "\n")

    def seed_stale(self):
        self.output.parent.mkdir(parents=True, exist_ok=True)
        self.output.write_text('{"stale": true}\n', encoding="utf-8")

    def argv(self, extra=()):
        return [
            str(_HERE / "emit_ci_actuals.py"),
            "--receipts-dir", str(self.receipts), "--budget", str(self.budget),
            "--lanes", str(self.lanes), "--lane-id", LANE,
            "--json-out", str(self.output), "--sha", SHA,
            "--workflow", "CI fixture", "--pr", "42",
            *extra,
        ]

    def assert_inputs_preserved(self):
        for path, data in self.protected.items():
            self.assertEqual(data, path.read_bytes(), str(path))

    def cli(self, extra=()):
        result = subprocess.run(
            [sys.executable, "-B", *self.argv(extra)], cwd=self.root,
            capture_output=True, text=True, timeout=5,
        )
        self.assert_inputs_preserved()
        return result

    def call_main(self):
        with patch.object(sys, "argv", self.argv()):
            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                result = emitter.main()
        self.assert_inputs_preserved()
        return result

    def success(self, extra=()):
        result = self.cli(extra)
        self.assertEqual(0, result.returncode, result.stderr)
        text = self.output.read_text(encoding="utf-8")
        self.assertNotIn("NaN", text)
        self.assertNotIn("Infinity", text)
        return json.loads(text)

    def refusal(self, extra=()):
        self.seed_stale()
        result = self.cli(extra)
        self.assertNotEqual(0, result.returncode)
        self.assertTrue(result.stderr.strip())
        self.assertEqual("", result.stdout)
        self.assertFalse(self.output.exists())

    def emit(self, rows, **overrides):
        arguments = {
            "receipts": rows, "multipliers": {RUNNER: 1.0},
            "lanes": {LANE: {"base_lem": 5.0}},
            "workflow": "CI fixture", "sha": SHA, "pr": 42,
            "runner_default": RUNNER, "lane_id": LANE,
        }
        arguments.update(overrides)
        return emitter.emit_actuals(**arguments)

    def test_complete_cli_preserves_schema_and_arithmetic(self):
        self.write_policy(multiplier="3.0", floor="7.0")
        document = self.success()
        self.assertEqual(1, document["schema_version"])
        self.assertEqual(SHA, document["sha"])
        self.assertEqual(42, document["pr"])
        self.assertEqual("CI fixture", document["workflow"])
        self.assertEqual(
            {"actual_lem": 3.0, "estimated_lem": 7.0, "delta_lem": -4.0},
            document["totals"],
        )
        self.assertEqual(1.0, document["jobs"][0]["actual_minutes"])

    def test_measured_zero_including_skip_is_numeric(self):
        self.write_receipt(envelope([gate(duration=0, status="skip")]))
        document = self.success()
        self.assertEqual(0.0, document["jobs"][0]["actual_lem"])
        self.assertEqual(0.0, document["jobs"][0]["actual_minutes"])
        self.assertEqual("skip", document["jobs"][0]["status"])
        self.assertEqual(0.0, self.emit([gate(duration=0)])["totals"]["actual_lem"])

    def test_shard_summary_is_not_counted_again(self):
        self.write_receipt({
            "schema_version": "ci_gate_shard.v1", "gates": [gate()],
        }, "summary.json")
        document = self.success()
        self.assertEqual(1, len(document["jobs"]))
        self.assertEqual(1.0, document["totals"]["actual_lem"])

    def test_lane_floor_is_counted_once_and_gate_estimates_stay_unknown(self):
        document = self.emit([gate(), gate("second", 120000)])
        self.assertEqual(
            {"actual_lem": 3.0, "estimated_lem": 5.0, "delta_lem": -2.0},
            document["totals"],
        )
        self.assertEqual([None, None], [r["estimated_lem"] for r in document["jobs"]])

    def test_exact_lane_fallback_without_fuzzy_matching(self):
        document = self.emit(
            [gate("pr_smoke", 120000), gate("compile_all_targets")],
            lane_id=None,
            lanes={"pr_smoke": {"base_lem": 5}, "check_all_targets": {"base_lem": 8}},
            multipliers={RUNNER: 2},
        )
        self.assertEqual(["pr_smoke", None], [r["lane_id"] for r in document["jobs"]])
        self.assertEqual([5.0, None], [r["estimated_lem"] for r in document["jobs"]])
        self.assertEqual(5.0, document["totals"]["estimated_lem"])
        self.assertEqual(6.0, document["totals"]["actual_lem"])

    def test_zero_floor_and_multiplier_are_valid(self):
        self.write_policy(multiplier="0.0", floor="0.0")
        self.assertEqual(
            {"actual_lem": 0.0, "estimated_lem": 0.0, "delta_lem": 0.0},
            self.success()["totals"],
        )

    def test_gate_source_path_cannot_spoof_collected_provenance(self):
        row = gate()
        row["_source_path"] = "invented.json"
        self.write_receipt(envelope([row]))
        document = self.success()
        self.assertEqual(str(self.receipts / "canonical.json"),
                         document["jobs"][0]["source_path"])

    def test_invalid_envelopes_refuse_and_remove_stale_output(self):
        valid = envelope()
        missing_version = copy.deepcopy(valid)
        del missing_version["schema_version"]
        renamed = copy.deepcopy(valid)
        renamed["schema_version"] = "gates.v999"
        renamed["renamed_gates"] = renamed.pop("gates")
        cases = [
            [], gate(), missing_version, renamed,
            {**valid, "schema_version": "gates.v999"},
            {**valid, "schema_version": 1},
            {**valid, "metadata": None},
            {k: v for k, v in valid.items() if k != "metadata"},
            {k: v for k, v in valid.items() if k != "summary"},
            {**valid, "summary": []},
            {**valid, "gates": []},
            {**valid, "gates": "not an array"},
            {**valid, "gates": ["not a gate"]},
            {**valid, "summary": {**valid["summary"], "total_gates": 2}},
            {**valid, "summary": {**valid["summary"], "total_gates": True}},
            {"schema_version": "ci_gate_shard.v1", "gates": [gate()]},
        ]
        for document in cases:
            with self.subTest(document=document):
                self.write_receipt(document)
                self.refusal()

    def test_malformed_file_cannot_disappear_beside_valid_evidence(self):
        for content in ("{", "\udcff"):
            with self.subTest(content=repr(content)):
                # Invalid UTF-8 is written as bytes to exercise the reader.
                broken = self.receipts / "broken.json"
                broken.write_bytes(content.encode("utf-8", errors="surrogateescape"))
                self.protected[broken] = broken.read_bytes()
                self.refusal()

    def test_missing_and_empty_receipt_directories_refuse(self):
        path = self.receipts / "canonical.json"
        path.unlink()
        del self.protected[path]
        self.refusal()
        self.receipts.rmdir()
        self.refusal()

    def test_missing_and_malformed_gate_fields_refuse(self):
        for field in ("gate_name", "tier", "status"):
            for value in (None, "", " ", 42):
                with self.subTest(field=field, value=value):
                    row = gate()
                    if value is None:
                        del row[field]
                    else:
                        row[field] = value
                    self.write_receipt(envelope([row]))
                    self.refusal()

    def test_invalid_and_missing_duration_refuse(self):
        for value in (None, True, -1, 1.5, 60000.0, 2**64, "60000",
                      float("nan"), float("inf")):
            with self.subTest(duration=value):
                self.write_receipt(envelope([gate(duration=value)]))
                self.refusal()
        missing = gate("unmeasured")
        del missing["duration_ms"]
        for rows in ([missing], [gate(), missing]):
            with self.subTest(rows=rows):
                self.write_receipt(envelope(rows))
                self.refusal()
                with self.assertRaises(ValueError):
                    self.emit(rows)

    def test_invalid_cost_inputs_refuse(self):
        for field in ("multiplier", "floor"):
            for value in ("true", "-1.0", "inf", "nan", '"1.0"'):
                with self.subTest(field=field, value=value):
                    self.write_policy(**{field: value})
                    self.refusal()

    def test_finite_inputs_cannot_overflow_derived_costs(self):
        self.write_policy(multiplier="1e308")
        self.write_receipt(envelope([gate(duration=2**64 - 1)]))
        self.refusal()
        with self.assertRaises(ValueError):
            self.emit([gate(), gate("second")], multipliers={RUNNER: 1e308})
        with self.assertRaises(ValueError):
            self.emit(
                [gate("first"), gate("second")], lane_id=None,
                lanes={"first": {"base_lem": 1e308}, "second": {"base_lem": 1e308}},
            )

    def test_unknown_lane_and_policy_parse_failures_invalidate_output(self):
        self.refusal(("--lane-id", "unknown_lane"))
        self.write(self.budget, "[runner_multipliers\n")
        self.refusal()

    def test_missing_policy_invalidates_output(self):
        self.budget.unlink()
        del self.protected[self.budget]
        self.refusal()

    def test_argument_failures_invalidate_selected_output(self):
        for arguments in (("--pr", "invalid"), ("--unknown-argument",),
                          ("--workflow",), ("--budget",), ("--lanes",),
                          ("--receipts-dir",)):
            with self.subTest(arguments=arguments):
                self.refusal(arguments)

    def test_help_is_read_only(self):
        self.seed_stale()
        result = self.cli(("--help",))
        self.assertEqual(0, result.returncode)
        self.assertEqual('{"stale": true}\n', self.output.read_text())

    def test_input_output_overlap_refuses_without_deleting_inputs(self):
        for path in (self.budget, self.lanes, self.receipts / "canonical.json"):
            with self.subTest(path=path):
                result = self.cli(("--json-out", str(path)))
                self.assertNotEqual(0, result.returncode)

    def test_parent_symlink_cannot_hide_receipt_overlap(self):
        alias = self.root / "alias"
        alias.symlink_to(self.receipts, target_is_directory=True)
        result = self.cli(("--json-out", str(alias / "canonical.json")))
        self.assertNotEqual(0, result.returncode)

    def test_output_cannot_delete_a_policy_input_symlink(self):
        target = self.root / "budget-target.toml"
        self.write(target, self.budget.read_text())
        self.budget.unlink()
        self.budget.symlink_to(target)
        result = self.cli(("--json-out", str(self.budget)))
        self.assertNotEqual(0, result.returncode)
        self.assertTrue(self.budget.is_symlink())

    def test_output_cannot_delete_a_receipt_symlink_referent(self):
        self.write(self.output, json.dumps(envelope()))
        link = self.receipts / "linked.json"
        link.symlink_to(self.output)
        self.protected[link] = link.read_bytes()
        result = self.cli()
        self.assertNotEqual(0, result.returncode)
        self.assertTrue(link.is_symlink())

    def test_unreadable_subtree_cannot_publish_partial_observations(self):
        hidden = self.receipts / "unreadable"
        hidden.mkdir()
        self.write(hidden / "another.json", json.dumps(envelope([gate("second")])))
        self.seed_stale()
        real_scandir = os.scandir

        def scandir(path):
            if Path(path) == hidden:
                raise PermissionError("fixture traversal failure")
            return real_scandir(path)

        with patch.object(emitter.os, "scandir", side_effect=scandir):
            self.assertEqual(1, self.call_main())
        self.assertFalse(self.output.exists())

    def test_receipt_directory_symlink_is_explicitly_unsupported(self):
        elsewhere = self.root / "elsewhere"
        elsewhere.mkdir()
        self.write(elsewhere / "another.json", json.dumps(envelope([gate("second")])))
        (self.receipts / "linked-directory").symlink_to(elsewhere, target_is_directory=True)
        self.refusal()

    def test_directory_symlink_refusal_preserves_its_input_referents(self):
        elsewhere = self.root / "elsewhere"
        source = elsewhere / "another.json"
        self.write(source, json.dumps(envelope()))
        (self.receipts / "linked-directory").symlink_to(elsewhere, target_is_directory=True)
        result = self.cli(("--json-out", str(source)))
        self.assertNotEqual(0, result.returncode)

    @unittest.skipUnless(hasattr(os, "mkfifo"), "requires POSIX FIFO fixture")
    def test_non_regular_receipt_refuses_without_opening_it(self):
        fifo = self.receipts / "blocked.json"
        os.mkfifo(fifo)
        self.refusal()
        self.assertTrue(fifo.exists())

    def test_output_links_do_not_modify_their_neighbors(self):
        self.output.parent.mkdir(parents=True, exist_ok=True)
        self.output.symlink_to(self.root / "neighbor.txt")
        self.success()
        self.assertFalse(self.output.is_symlink())
        self.output.unlink()
        os.link(self.budget, self.output)
        self.success()
        self.assertNotEqual(self.output.stat().st_ino, self.budget.stat().st_ino)

    def test_atomic_publication_exposes_only_complete_json(self):
        self.seed_stale()
        real_replace = os.replace
        observed = []

        def replace(source, destination):
            source = Path(source)
            self.assertEqual(self.output.parent, source.parent)
            self.assertFalse(self.output.exists())
            payload = json.loads(source.read_text())
            self.assertEqual(1.0, payload["totals"]["actual_lem"])
            observed.append(source)
            real_replace(source, destination)

        with patch.object(emitter.os, "replace", side_effect=replace):
            self.assertEqual(0, self.call_main())
        self.assertEqual(1, len(observed))
        self.assertFalse(observed[0].exists())
        self.assertTrue(self.output.is_file())

    def test_atomic_replace_and_flush_failures_leave_no_report_or_owned_temp(self):
        for operation in ("replace", "fsync"):
            with self.subTest(operation=operation):
                self.seed_stale()
                with patch.object(emitter.os, operation, side_effect=OSError("fixture failure")):
                    self.assertEqual(1, self.call_main())
                self.assertFalse(self.output.exists())
                self.assertEqual([], list(self.output.parent.glob(".*.tmp")))

    def test_partial_temp_write_failure_preserves_neighbors(self):
        self.seed_stale()
        retained = self.output.parent / ".unrelated.tmp"
        self.write(retained, "unrelated temporary file\n")
        real_factory = tempfile.NamedTemporaryFile

        @contextmanager
        def failing_file(**kwargs):
            with real_factory(**kwargs) as stream:
                real_write = stream.write

                def write_part_then_fail(text):
                    real_write(text[:8])
                    raise OSError("fixture write failure")

                with patch.object(stream, "write", side_effect=write_part_then_fail):
                    yield stream

        with patch.object(emitter.tempfile, "NamedTemporaryFile", side_effect=failing_file):
            self.assertEqual(1, self.call_main())
        self.assertFalse(self.output.exists())
        self.assertEqual([retained], list(self.output.parent.glob(".*.tmp")))

    def test_strict_serialization_failure_cannot_leave_stale_report(self):
        self.seed_stale()
        real_dumps = json.dumps

        def fail_actuals(value, **kwargs):
            if isinstance(value, dict) and value.get("schema_version") == 1:
                raise ValueError("fixture serialization failure")
            return real_dumps(value, **kwargs)

        with patch.object(emitter.json, "dumps", side_effect=fail_actuals):
            self.assertEqual(1, self.call_main())
        self.assertFalse(self.output.exists())
        self.assertEqual([], list(self.output.parent.glob(".*.tmp")))

    def test_publication_sink_rejects_nonfinite_json_even_without_input_validation(self):
        for value in (float("nan"), float("inf"), float("-inf")):
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    emitter.publish_actuals(self.output, {"actual_lem": value})
                self.assertFalse(self.output.exists())

    def history_samples(self):
        self.write(self.root / "history/run-123/trusted-run.json", json.dumps({
            "schema_version": "trusted_run_marker.v1", "run_id": 123,
            "repository": "example/fixture", "event": "merge_group",
            "head_branch": "gh-readonly-queue/fixture", "head_sha": SHA,
            "conclusion": "success", "created_at": "2026-01-01T00:00:00Z",
        }))
        with patch.object(history.time, "time", return_value=1767225600.0):
            samples, _ = history.collect_actuals(
                actuals_dir=self.root / "history", window_days=14,
                allowed_lanes={LANE}, require_trusted_markers=True,
                repository="example/fixture", default_branch="main",
            )
        return samples

    def test_real_history_accepts_complete_lane_but_not_partial_timing(self):
        self.success(("--pr", "0"))
        self.assertEqual({LANE: [1.0]}, self.history_samples())
        missing = gate("missing")
        del missing["duration_ms"]
        self.write_receipt(envelope([gate(), missing]))
        self.refusal(("--pr", "0"))
        self.assertEqual({}, self.history_samples())


if __name__ == "__main__":
    unittest.main()
