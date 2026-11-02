import importlib.util
import contextlib
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "measure_cargo_storage.py"
spec = importlib.util.spec_from_file_location("measurement", SCRIPT)
subject = importlib.util.module_from_spec(spec)
spec.loader.exec_module(subject)


class StorageMeasurement(unittest.TestCase):
    def test_directory_churn_does_not_stop_later_periodic_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            observed = root / "target"
            observed.mkdir()
            (observed / "payload").write_bytes(b"x" * 4096)
            evidence = root / "receipt"
            program = "import time; time.sleep(.12)"
            periodic_calls = {"count": 0}
            real_observe = subject.observe

            def observe(roots, stop=None):
                row = real_observe(roots, stop)
                if stop is not None:
                    periodic_calls["count"] += 1
                    if periodic_calls["count"] == 1:
                        row["vanished_entries"] = 1
                        row["coverage_complete"] = False
                return row

            with patch.object(subject, "observe", side_effect=observe):
                status = subject.main(
                    [
                        "--root",
                        str(observed),
                        "--evidence",
                        str(evidence),
                        "--interval",
                        ".02",
                        "--",
                        sys.executable,
                        "-c",
                        program,
                    ]
                )
            rows = [json.loads(line) for line in evidence.read_text().splitlines()]
            periodic = [row for row in rows if row.get("kind") == "periodic"]
            summary = rows[-1]
            self.assertGreaterEqual(len(periodic), 2, rows)
            self.assertFalse(summary["measurement_complete"])
            self.assertTrue(summary["partial_coverage"])
            self.assertGreater(summary["sampled_peak_allocated_bytes"], 0)
            self.assertEqual(status, 75)

    def test_inaccessible_entry_is_diagnostic_not_zero_allocation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            blocked = root / "blocked"
            blocked.mkdir()
            allowed = root / "allowed"
            allowed.write_bytes(b"x" * 8192)
            diagnostics = {}
            real_lstat = Path.lstat

            def lstat(path):
                if Path(path) == blocked:
                    raise PermissionError("controlled permission denial")
                return real_lstat(path)

            with patch.object(Path, "lstat", lstat):
                total = subject.allocation([blocked, allowed], diagnostics=diagnostics)
            self.assertEqual(total, allowed.stat().st_blocks * 512)
            self.assertEqual(diagnostics["inaccessible_entry_count"], 1)
            self.assertFalse(diagnostics["coverage_complete"])

    def test_partial_coverage_withholds_complete_growth_claim_after_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            observed = root / "target"
            observed.mkdir()
            evidence = root / "receipt"
            calls = {"n": 0}
            real_observe = subject.observe

            def observe(roots, stop=None):
                row = real_observe(roots, stop)
                calls["n"] += 1
                if calls["n"] == 2:
                    row["coverage_complete"] = False
                    row["vanished_entries"] = 1
                return row

            with patch.object(subject, "observe", side_effect=observe):
                status = subject.main(
                    [
                        "--root",
                        str(observed),
                        "--evidence",
                        str(evidence),
                        "--interval",
                        ".02",
                        "--",
                        sys.executable,
                        "-c",
                        "import time; time.sleep(.08)",
                    ]
                )
            summary = json.loads(evidence.read_text().splitlines()[-1])
            self.assertFalse(summary["measurement_complete"])
            self.assertFalse(summary["growth_claim_complete"])
            self.assertTrue(summary["partial_coverage"])
            self.assertIsNotNone(summary["sampled_peak_allocated_bytes"])
            self.assertEqual(status, 75)

    def test_original_fatal_re_raise_stops_sampling(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            observed = root / "target"
            observed.mkdir()

            def fatal_allocation(*args, **kwargs):
                raise FileNotFoundError("original instrument re-raised churn")

            with patch.object(subject, "allocation", side_effect=fatal_allocation):
                with self.assertRaises(FileNotFoundError):
                    subject.observe([observed])

    def test_unexpected_periodic_failures_signal_real_child_even_if_it_exits_zero(self):
        for error_type in (RuntimeError, MemoryError):
            with self.subTest(error=error_type.__name__), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                ready = root / "ready"
                signalled = root / "signalled"
                evidence = root / "receipt"
                program = (
                    "from pathlib import Path\nimport signal,time\n"
                    "def stop(*_):\n Path(" + repr(str(signalled)) + ").touch()\n raise SystemExit(0)\n"
                    "signal.signal(signal.SIGINT,stop)\nPath(" + repr(str(ready)) + ").touch()\n"
                    "time.sleep(.4)\n"
                )
                real_observe = subject.observe

                def observation(roots, event=None):
                    if event is not None:
                        deadline = time.monotonic() + 2
                        while not ready.exists() and time.monotonic() < deadline:
                            time.sleep(0.005)
                        if not ready.exists():
                            raise AssertionError("real child did not become ready")
                        raise error_type("injected periodic observer failure")
                    return real_observe(roots, event)

                diagnostics = io.StringIO()
                with patch.object(subject, "observe", side_effect=observation), contextlib.redirect_stderr(
                    diagnostics
                ):
                    status = subject.main(
                        [
                            "--root",
                            str(root / "target"),
                            "--evidence",
                            str(evidence),
                            "--interval",
                            ".01",
                            "--",
                            sys.executable,
                            "-c",
                            program,
                        ]
                    )
                summary = json.loads(evidence.read_text().splitlines()[-1])
                self.assertEqual(status, 75)
                self.assertFalse(summary["measurement_complete"])
                self.assertEqual(summary["command_status"], 0)

    def test_vanishing_entries_are_reported_sampling_limit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            diagnostics = {}
            with patch.object(subject.os, "scandir", side_effect=FileNotFoundError("concurrent removal")):
                count = subject.allocation([root], diagnostics=diagnostics)
            self.assertEqual(count, root.stat().st_blocks * 512)
            self.assertEqual(diagnostics["vanished_entries"], 1)
            self.assertFalse(diagnostics["coverage_complete"])

    def test_windows_job_api_fixture_is_importable_on_windows(self):
        if os.name != "nt":
            self.skipTest("Windows Job Object proof is host-specific")
        script = Path(__file__).resolve().parents[1] / "generate-badges.py"
        badge_spec = importlib.util.spec_from_file_location("badges", script)
        module = importlib.util.module_from_spec(badge_spec)
        badge_spec.loader.exec_module(module)
        job = module.WindowsJob()
        self.addCleanup(job.close)
        self.assertIsNotNone(job._handle)


if __name__ == "__main__":
    unittest.main()
