#!/usr/bin/env python3
"""Literal-byte, failure and controlled-mutant tests for #17231."""
from __future__ import annotations

import importlib.util
import io
import json
import os
import stat
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "pr_smoke_artifact_inventory", Path(__file__).with_name("pr_smoke_artifact_inventory.py"))
inventory = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(inventory)


class InventoryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.parent = Path(self.tmp.name)
        self.root = self.parent / "target"
        self.root.mkdir()
        self.exe = self.root / "test-bin"
        self.alias = self.root / "test-bin-alias"
        self.dep = self.root / "lib.rlib"
        self.other = self.root / "sparse-unknown"
        for path in (self.exe, self.dep, self.other):
            path.write_bytes(b"x")
        os.link(self.exe, self.alias)
        self.identity = {
            "source_sha": "a" * 40, "tree_sha": "b" * 40, "git_dirty": False,
            "run_id": "37702955025", "run_attempt": 1,
            "cache": {"class": "cold", "resolved_key": "fixture-cache"},
            "profile": {"test_debug": "line-tables-only"},
            "toolchain": "rustc fixture", "target_triple": "fixture-linux",
            "command": ["cargo", "test", "--locked", "--tests", "-p", "fixture"],
        }
        self.identity_file = self.parent / "identity.json"
        self.identity_file.write_text(json.dumps(self.identity))
        self.capture = self.parent / "cargo.log"
        self.write_capture()

    def unit(self, path, test):
        return {
            "reason": "compiler-artifact", "package_id": "fixture#1.0",
            "target": {"kind": ["test"] if test else ["lib"], "name": path.name},
            "features": ["default", "wire"], "profile": {"test": test, "debuginfo": "line-tables-only"},
            "filenames": [str(path)], "executable": str(path) if test else None, "fresh": False,
        }

    def write_capture(self, *, identity=None, terminal=True, extra=None):
        rows = [
            {"reason": "pr-smoke-artifact-binding", "identity": self.identity if identity is None else identity},
            self.unit(self.exe, True), self.unit(self.dep, False),
        ]
        if extra:
            rows.append(extra)
        if terminal:
            rows.append({"reason": "build-finished", "success": True})
        self.capture.write_text("\n".join(json.dumps(row) for row in rows) + "\n")

    def fake_stat(self, path, *args, **kwargs):
        absolute = os.path.abspath(path)
        if absolute in (str(self.exe), str(self.alias), str(self.dep), str(self.other)):
            key, blocks, length = {
                str(self.exe): (101, 8, 6000), str(self.alias): (101, 8, 6000),
                str(self.dep): (102, 16, 10000), str(self.other): (103, 8, 5000000),
            }[absolute]
            return SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_dev=9, st_ino=key,
                                   st_blocks=blocks, st_size=length, st_mtime_ns=1)
        return self.real_stat(path, *args, **kwargs)

    def measured(self, **kwargs):
        self.real_stat = os.lstat
        with mock.patch.object(inventory, "_entry_info", side_effect=lambda fd, name, path: self.fake_stat(path)):
            return inventory.collect(self.root, self.identity, artifacts=self.capture, **kwargs)

    def assert_literal_partition(self):
        result = self.measured()
        self.assertEqual(result["regular_paths"], 4)
        self.assertEqual(result["unique_regular_inodes"], 3)
        self.assertEqual(result["complete_inode_allocated_bytes"], 16384)
        self.assertEqual(result["observed_known_logical_bytes"], 5016000)
        self.assertEqual(result["categories"]["test_executable"]["known_inode_allocated_bytes"], 4096)
        self.assertEqual(result["categories"]["dependency_output"]["known_inode_allocated_bytes"], 8192)
        self.assertEqual(result["categories"]["other"]["known_inode_allocated_bytes"], 4096)
        self.assertTrue(result["artifact_attribution_complete"])
        return result

    def test_literal_partition_deduplicates_hardlink_and_counts_sparse_allocation(self):
        self.assert_literal_partition()

    def test_controlled_red_logical_size_is_not_allocated_bytes(self):
        with mock.patch.object(inventory, "allocated_bytes", side_effect=lambda info: info.st_size):
            with self.assertRaises(AssertionError):
                self.assert_literal_partition()
        print("CONTROLLED_RED: logical-size-as-allocation mutation rejected")

    def test_controlled_red_per_path_counting_is_not_inode_accounting(self):
        counter = iter(range(100))
        with mock.patch.object(inventory, "inode_key", side_effect=lambda info: (info.st_dev, next(counter))):
            with self.assertRaises(AssertionError):
                self.assert_literal_partition()
        print("CONTROLLED_RED: per-path inode mutation rejected")


    def test_literal_partition_is_independent_of_directory_order(self):
        original_scandir = os.scandir
        class AliasFirstScan:
            def __init__(self, fd):
                with original_scandir(fd) as entries:
                    self.entries = iter(sorted(entries, key=lambda entry: (entry.name != "test-bin-alias", entry.name)))
            def __next__(self):
                return next(self.entries)
            def close(self):
                pass
        with mock.patch.object(inventory.os, "scandir", side_effect=AliasFirstScan):
            self.assert_literal_partition()

    def test_declared_artifact_removed_before_walk_is_incomplete_attribution(self):
        self.exe.unlink()
        result = self.measured()
        self.assertFalse(result["artifact_attribution_complete"])
        self.assertIn("artifact_path_unobserved", result["uncertainty_counts"])
        self.assertEqual(result["observed_known_inode_allocated_bytes"], 16384)
        self.assertEqual(result["categories"]["dependency_output"]["known_inode_allocated_bytes"], 8192)

    def test_declared_artifact_behind_child_symlink_is_incomplete_attribution(self):
        linked = self.root / "linked-dir"
        os.symlink(self.root, linked, target_is_directory=True)
        self.write_capture(extra=self.unit(linked / "test-bin", True))
        result = self.measured()
        self.assertFalse(result["artifact_attribution_complete"])
        self.assertIn("artifact_path_unobserved", result["uncertainty_counts"])
        self.assertEqual(result["symlinks_skipped"], 1)
        self.assertEqual(result["observed_known_inode_allocated_bytes"], 16384)
        self.assertEqual(result["categories"]["test_executable"]["known_inode_allocated_bytes"], 4096)
        self.assertEqual(result["categories"]["dependency_output"]["known_inode_allocated_bytes"], 8192)

    def test_actual_host_hardlinks_have_one_inode_contribution(self):
        result = inventory.collect(self.root, self.identity, artifacts=self.capture)
        self.assertEqual(result["unique_regular_inodes"], 3)
        info = os.stat(self.exe)
        if hasattr(info, "st_blocks"):
            expected = os.stat(self.exe).st_blocks * 512 + os.stat(self.dep).st_blocks * 512 + os.stat(self.other).st_blocks * 512
            self.assertEqual(result["complete_inode_allocated_bytes"], expected)
        else:
            self.assertIsNone(result["complete_inode_allocated_bytes"])

    def test_symlinked_directory_and_file_are_not_followed(self):
        outside = self.parent / "outside"
        outside.mkdir()
        (outside / "large").write_bytes(b"outside")
        os.symlink(outside, self.root / "linked-dir", target_is_directory=True)
        os.symlink(self.exe, self.root / "linked-file")
        result = self.measured()
        self.assertEqual(result["symlinks_skipped"], 2)
        self.assertEqual(result["regular_paths"], 4)
        self.assertEqual(result["complete_inode_allocated_bytes"], 16384)

    def test_symlinked_target_is_refused(self):
        link = self.parent / "target-link"
        os.symlink(self.root, link, target_is_directory=True)
        result = inventory.collect(link, self.identity)
        self.assertFalse(result["scan_complete"])
        self.assertIsNone(result["complete_inode_allocated_bytes"])


    def test_directory_swapped_to_outside_symlink_is_not_traversed(self):
        victim = self.root / "victim"
        victim.mkdir()
        outside = self.parent / "outside"
        outside.mkdir()
        (outside / "must-not-count").write_bytes(b"outside")
        original_open = os.open
        def swapped(path, flags, *args, **kwargs):
            if path == "victim" and "dir_fd" in kwargs:
                victim.rmdir()
                os.symlink(outside, victim, target_is_directory=True)
            return original_open(path, flags, *args, **kwargs)
        with mock.patch.object(inventory.os, "open", side_effect=swapped):
            result = self.measured()
        self.assertEqual(result["regular_paths"], 4)
        self.assertEqual(result["observed_known_inode_allocated_bytes"], 16384)
        self.assertIsNone(result["complete_inode_allocated_bytes"])
        self.assertTrue(any(k.endswith("_entry") for k in result["uncertainty_counts"]))

    def test_directory_replaced_between_stat_and_open_is_uncertain(self):
        victim = self.root / "victim"
        victim.mkdir()
        original_open = os.open
        def replaced(path, flags, *args, **kwargs):
            if path == "victim" and "dir_fd" in kwargs:
                victim.rename(self.parent / "original-victim")
                victim.mkdir()
                (victim / "new-output").write_bytes(b"new")
            return original_open(path, flags, *args, **kwargs)
        with mock.patch.object(inventory.os, "open", side_effect=replaced):
            result = self.measured()
        self.assertEqual(result["regular_paths"], 4)
        self.assertIsNone(result["complete_inode_allocated_bytes"])
        self.assertIn("directory_changed", result["uncertainty_counts"])

    def test_unsupported_secure_traversal_is_unknown(self):
        with mock.patch.object(inventory, "SECURE_TRAVERSAL_SUPPORTED", False):
            result = self.measured()
        self.assertFalse(result["scan_complete"])
        self.assertIsNone(result["complete_inode_allocated_bytes"])
        self.assertIn("OSError_scan", result["uncertainty_counts"])

    def test_nonregular_fifo_is_not_opened(self):
        fifo = self.root / "fifo"
        fifo.write_bytes(b"placeholder")
        self.real_stat = os.lstat
        def probe(path, *args, **kwargs):
            if os.path.abspath(path) == str(fifo):
                return SimpleNamespace(st_mode=stat.S_IFIFO, st_dev=9, st_ino=104)
            return self.fake_stat(path, *args, **kwargs)
        with mock.patch.object(inventory, "_entry_info", side_effect=lambda fd, name, path: probe(path)):
            result = inventory.collect(self.root, self.identity, artifacts=self.capture)
        self.assertEqual(result["nonregular_skipped"], 1)
        self.assertEqual(result["complete_inode_allocated_bytes"], 16384)

    def test_vanished_and_unreadable_entries_are_unknown_not_zero(self):
        for error in (FileNotFoundError("vanished"), PermissionError("unreadable"),
                      NotADirectoryError("raced ancestor"), InterruptedError("interrupted")):
            with self.subTest(error=type(error).__name__):
                self.real_stat = os.lstat
                def probe(path, *args, **kwargs):
                    if os.path.abspath(path) == str(self.dep):
                        raise error
                    return self.fake_stat(path, *args, **kwargs)
                with mock.patch.object(inventory, "_entry_info", side_effect=lambda fd, name, path: probe(path)):
                    result = inventory.collect(self.root, self.identity, artifacts=self.capture)
                self.assertIsNone(result["complete_inode_allocated_bytes"])
                self.assertEqual(result["observed_known_inode_allocated_bytes"], 8192)
                self.assertIn(type(error).__name__ + "_entry", result["uncertainty_counts"])
                self.assertFalse(result["artifact_attribution_complete"])

    def test_unsupported_allocated_bytes_do_not_fall_back_to_logical_size(self):
        self.real_stat = os.lstat
        def probe(path, *args, **kwargs):
            value = self.fake_stat(path, *args, **kwargs)
            if os.path.abspath(path) == str(self.dep):
                del value.st_blocks
            return value
        with mock.patch.object(inventory, "_entry_info", side_effect=lambda fd, name, path: probe(path)):
            result = inventory.collect(self.root, self.identity, artifacts=self.capture)
        self.assertIsNone(result["complete_inode_allocated_bytes"])
        self.assertEqual(result["observed_known_inode_allocated_bytes"], 8192)
        self.assertIn("allocated_bytes_unsupported", result["uncertainty_counts"])

    def test_changed_hardlink_observation_is_uncertain(self):
        self.real_stat = os.lstat
        def probe(path, *args, **kwargs):
            value = self.fake_stat(path, *args, **kwargs)
            if os.path.abspath(path) == str(self.alias):
                value.st_size += 1
            return value
        with mock.patch.object(inventory, "_entry_info", side_effect=lambda fd, name, path: probe(path)):
            result = inventory.collect(self.root, self.identity, artifacts=self.capture)
        self.assertIsNone(result["complete_inode_allocated_bytes"])
        self.assertEqual(result["observed_known_inode_allocated_bytes"], 12288)

    def test_missing_target_is_unknown_not_an_empty_success(self):
        result = inventory.collect(self.parent / "gone", self.identity)
        self.assertFalse(result["scan_complete"])
        self.assertIsNone(result["complete_inode_allocated_bytes"])
        self.assertEqual(result["status"], "partial")

    def test_stale_capture_binding_does_not_attribute_outputs(self):
        wrong = {**self.identity, "source_sha": "c" * 40}
        self.write_capture(identity=wrong)
        result = self.measured()
        self.assertFalse(result["artifact_attribution_complete"])
        self.assertEqual(result["identity"], self.identity)
        self.assertEqual(result["categories"]["test_executable"]["inodes"], 0)
        self.assertEqual(result["categories"]["other"]["known_inode_allocated_bytes"], 16384)


    def test_late_matching_binding_cannot_qualify_prior_cargo_records(self):
        rows = self.capture.read_text().splitlines()
        self.capture.write_text("\n".join(rows[1:] + rows[:1]) + "\n")
        result = self.measured()
        self.assertFalse(result["artifact_attribution_complete"])
        self.assertIn("artifact_binding_order", result["uncertainty_counts"])
        self.assertEqual(result["categories"]["other"]["known_inode_allocated_bytes"], 16384)

    def test_incomplete_identity_is_retained_and_cannot_qualify_attribution(self):
        for identity in ({}, {**self.identity, "cache": {"class": "unknown", "resolved_key": None}}):
            with self.subTest(identity=identity):
                self.write_capture(identity=identity)
                result = inventory.collect(self.root, identity, artifacts=self.capture)
                self.assertEqual(result["identity"], identity)
                self.assertFalse(result["artifact_attribution_complete"])
                self.assertIn("identity_incomplete", result["uncertainty_counts"])


    def test_parent_or_dot_components_in_artifact_filenames_cannot_fake_observation(self):
        for declared in (str(self.root / "missing" / ".." / "test-bin"), str(self.exe) + "/."):
            with self.subTest(declared=declared):
                self.assertFalse(os.path.exists(declared))
                extra = self.unit(self.exe, True)
                extra["filenames"] = [declared]
                self.write_capture(extra=extra)
                result = self.measured()
                self.assertFalse(result["artifact_attribution_complete"])
                self.assertIn("artifact_path_noncanonical", result["uncertainty_counts"])
                self.assertEqual(result["observed_known_inode_allocated_bytes"], 16384)

    def test_parent_or_dot_components_in_executable_cannot_fake_observation(self):
        for declared in (str(self.root / "missing" / ".." / "test-bin"), str(self.exe) + "/."):
            with self.subTest(declared=declared):
                self.assertFalse(os.path.exists(declared))
                extra = self.unit(self.exe, True)
                extra["executable"] = declared
                self.write_capture(extra=extra)
                result = self.measured()
                self.assertFalse(result["artifact_attribution_complete"])
                self.assertIn("artifact_path_noncanonical", result["uncertainty_counts"])

    def test_parent_components_in_target_declaration_cannot_fake_a_present_target(self):
        declared = str(self.root / "missing" / "..")
        self.assertFalse(os.path.exists(declared))
        result = inventory.collect(declared, self.identity, artifacts=self.capture)
        self.assertFalse(result["scan_complete"])
        self.assertIsNone(result["complete_inode_allocated_bytes"])
        self.assertEqual(result["declared_target_dir"], declared)

    def test_dot_characters_in_filename_are_allowed_without_parent_components(self):
        renamed = self.root / "test..bin"
        self.exe.rename(renamed)
        self.exe = renamed
        self.write_capture()
        self.assert_literal_partition()

    def test_relative_artifact_path_cannot_be_resolved_using_collector_cwd(self):
        extra = self.unit(Path("relative-bin"), True)
        self.write_capture(extra=extra)
        result = self.measured()
        self.assertFalse(result["artifact_attribution_complete"])
        self.assertIn("artifact_path_outside_target", result["uncertainty_counts"])

    def test_missing_or_truncated_capture_is_not_qualified(self):
        self.write_capture(terminal=False)
        self.capture.write_text(self.capture.read_text() + '{"reason":')
        result = self.measured()
        self.assertFalse(result["artifact_attribution_complete"])
        self.assertFalse(result["artifact_digest_complete"])
        self.assertIn("artifact_json_invalid", result["uncertainty_counts"])
        self.assertIn("artifact_terminal_absent", result["uncertainty_counts"])

    def test_outside_target_artifact_path_is_uncertain(self):
        self.write_capture(extra=self.unit(self.parent / "outside-bin", True))
        result = self.measured()
        self.assertFalse(result["artifact_attribution_complete"])
        self.assertIn("artifact_path_outside_target", result["uncertainty_counts"])

    def test_exact_identity_and_mixed_profiles_are_retained_without_compatibility_claim(self):
        extra = self.unit(self.exe, False)
        extra["profile"] = {"test": False, "debuginfo": 2}
        extra["features"] = ["all-features"]
        self.write_capture(extra=extra)
        result = self.measured()
        self.assertEqual(result["identity"], self.identity)
        self.assertEqual(len(result["compiler_artifacts"]), 3)
        self.assertEqual(result["compiler_artifacts"][2]["features"], ["all-features"])
        self.assertEqual(result["compiler_artifacts"][2]["profile"]["debuginfo"], 2)
        self.assertIn("no independent", result["identity_authority"])
        self.assertIn("peak and savings NOT_PROVEN", result["observation"])

    def test_compilation_success_does_not_qualify_test_execution(self):
        result = self.measured()
        self.assertTrue(result["cargo_build_success"])
        self.assertNotIn("tests_passed", result)
        self.assertNotIn("runtime_qualified", result)

    def test_entry_limit_reports_partial_accounting(self):
        result = self.measured(max_entries=1)
        self.assertFalse(result["scan_complete"])
        self.assertIsNone(result["complete_inode_allocated_bytes"])
        self.assertIn("entry_limit", result["uncertainty_counts"])


    def test_capture_input_line_and_retained_identity_bounds_are_uncertain(self):
        for name in ("MAX_INPUT_BYTES", "MAX_LINE_BYTES", "MAX_UNIT_BYTES"):
            with self.subTest(bound=name), mock.patch.object(inventory, name, 1):
                result = self.measured()
                self.assertFalse(result["artifact_attribution_complete"])
                self.assertEqual(result["categories"]["other"]["known_inode_allocated_bytes"], 16384)
                self.assertTrue(any("limit" in k for k in result["uncertainty_counts"]))

    def test_depth_limit_closes_walk_and_reports_unknown_total(self):
        deepest = self.root
        for _ in range(129):
            deepest /= "d"
            deepest.mkdir()
        (deepest / "unvisited").write_bytes(b"must-not-count")
        result = self.measured()
        self.assertIsNone(result["complete_inode_allocated_bytes"])
        self.assertIn("depth_limit", result["uncertainty_counts"])
        self.assertEqual(result["regular_paths"], 4)

    def test_shared_budget_never_regrants_thirty_seconds(self):
        now = [0.0]
        clock = lambda: now[0]
        first = inventory.Budget(30, clock=clock)
        now[0] = 8.0
        charged = first.charged()
        self.assertEqual(charged, 9.0)
        second = inventory.Budget(30, spent=charged, clock=clock)
        self.assertEqual(second.remaining(), 21.0)
        now[0] = 29.0
        with self.assertRaises(inventory.BudgetExpired):
            second.check()
        result = self.measured(budget=second)
        self.assertFalse(result["scan_complete"])
        self.assertIsNone(result["complete_inode_allocated_bytes"])

    def test_invalid_or_nonfinite_budget_is_refused(self):
        for seconds, spent in ((31, 0), (float("nan"), 0), (30, 31), (30, -1)):
            with self.subTest(seconds=seconds, spent=spent):
                with self.assertRaises(ValueError):
                    inventory.Budget(seconds, spent)

    def test_output_bound_is_valid_uncertain_json(self):
        data = inventory.encode_report({"identity": "x" * 10000, "build_exit_code": 101}, limit=128)
        self.assertLessEqual(len(data), 128)
        result = json.loads(data)
        self.assertEqual(result["status"], "partial")
        self.assertEqual(result["build_exit_code"], 101)
        self.assertIsNone(result["complete_inode_allocated_bytes"])

    def cli_args(self, code=101):
        return ["--target-dir", str(self.root), "--identity-json", str(self.identity_file),
                "--artifact-jsonl", str(self.capture), "--output", str(self.parent / "report.log"),
                "--build-exit-code", str(code)]

    def test_cli_preserves_original_build_failure_with_successful_inventory(self):
        self.assertEqual(inventory.main(self.cli_args()), 101)
        report = json.loads((self.parent / "report.log").read_text())
        self.assertEqual(report["build_exit_code"], 101)
        self.assertTrue(report["scan_complete"])


    def test_cli_writes_partial_report_using_reserved_remainder(self):
        now = [0.0]
        original_budget = inventory.Budget
        original_collect = inventory.collect
        def bounded(seconds, spent):
            return original_budget(seconds, spent, clock=lambda: now[0])
        def at_reserve(*args, **kwargs):
            now[0] = 29.25
            return original_collect(*args, **kwargs)
        with mock.patch.object(inventory, "Budget", side_effect=bounded), \
                mock.patch.object(inventory, "collect", side_effect=at_reserve):
            self.assertEqual(inventory.main(self.cli_args()), 101)
        report = json.loads((self.parent / "report.log").read_text())
        self.assertEqual(report["status"], "partial")
        self.assertIsNone(report["complete_inode_allocated_bytes"])
        self.assertIn("BudgetExpired_scan", report["uncertainty_counts"])
        self.assertEqual(report["charged_collection_seconds"], 30.0)

    def test_output_reserve_cannot_disable_an_expired_hard_timer(self):
        with mock.patch.object(inventory.signal, "getitimer", return_value=(0.0, 0.0)):
            with self.assertRaises(inventory.BudgetExpired):
                with inventory._deadline_write(inventory.Budget()):
                    self.fail("expired timer was disabled")

    def test_cli_missing_identity_preserves_original_build_failure(self):
        self.identity_file.unlink()
        with redirect_stderr(io.StringIO()) as output:
            self.assertEqual(inventory.main(self.cli_args()), 101)
        self.assertIn("NOT_PROVEN", output.getvalue())


    def test_cli_charge_includes_delay_before_arming_output_reserve(self):
        now = [0.0]
        original_budget = inventory.Budget
        original_collect = inventory.collect
        original_write = inventory._deadline_write
        def bounded(seconds, spent):
            return original_budget(seconds, spent, clock=lambda: now[0])
        def at_eight_seconds(*args, **kwargs):
            now[0] = 8.0
            return original_collect(*args, **kwargs)
        def delayed_write(budget):
            now[0] += 0.75
            return original_write(budget)
        with mock.patch.object(inventory, "Budget", side_effect=bounded), \
                mock.patch.object(inventory, "collect", side_effect=at_eight_seconds), \
                mock.patch.object(inventory, "_deadline_write", side_effect=delayed_write):
            self.assertEqual(inventory.main(self.cli_args()), 101)
        report = json.loads((self.parent / "report.log").read_text())
        self.assertEqual(report["charged_collection_seconds"], 9.75)
        self.assertIn("caller must supervise", report["charge_scope"])

    def test_cli_failed_error_stream_preserves_original_build_failure(self):
        self.identity_file.unlink()
        for error in (BrokenPipeError("closed pipe"), OSError("disk full"),
                      UnicodeError("unsupported encoding"), ValueError("closed stream")):
            with self.subTest(error=type(error).__name__):
                failed = SimpleNamespace(write=mock.Mock(side_effect=error), flush=lambda: None)
                with redirect_stderr(failed):
                    self.assertEqual(inventory.main(self.cli_args()), 101)

    def test_cli_zero_build_status_survives_failed_telemetry(self):
        self.identity_file.unlink()
        with redirect_stderr(io.StringIO()) as output:
            self.assertEqual(inventory.main(self.cli_args(code=0)), 0)
        self.assertIn("NOT_PROVEN", output.getvalue())
        self.assertFalse((self.parent / "report.log").exists())

    def test_cli_enospc_output_preserves_original_build_failure(self):
        original_open = os.open
        def failed(path, *args, **kwargs):
            if str(path) == str(self.parent / "report.log"):
                raise OSError(28, "No space left on device")
            return original_open(path, *args, **kwargs)
        with mock.patch.object(inventory.os, "open", side_effect=failed), redirect_stderr(io.StringIO()) as output:
            self.assertEqual(inventory.main(self.cli_args()), 101)
        self.assertIn("No space left on device", output.getvalue())

    def test_cli_existing_output_is_not_overwritten(self):
        report = self.parent / "report.log"
        report.write_text("previous evidence")
        with redirect_stderr(io.StringIO()):
            self.assertEqual(inventory.main(self.cli_args()), 101)
        self.assertEqual(report.read_text(), "previous evidence")

    def test_cli_unsupported_hard_deadline_preserves_build_result(self):
        with mock.patch.object(inventory.signal, "setitimer", side_effect=OSError("unsupported")), redirect_stderr(io.StringIO()):
            self.assertEqual(inventory.main(self.cli_args()), 101)

    def test_cli_exhausted_shared_budget_preserves_build_result(self):
        with redirect_stderr(io.StringIO()) as output:
            self.assertEqual(inventory.main(self.cli_args() + ["--spent-seconds", "30"]), 101)
        self.assertIn("shared collection budget exhausted", output.getvalue())


if __name__ == "__main__":
    unittest.main(verbosity=2)
