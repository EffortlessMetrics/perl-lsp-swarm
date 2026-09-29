#!/usr/bin/env python3
"""Focused unit tests for the exact-binary DAP scorecard driver."""

from __future__ import annotations

import importlib.util
import io
import stat
import tempfile
import textwrap
import threading
import time
import unittest
from pathlib import Path
from types import SimpleNamespace

SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "dap_scorecard_runtime.py"
SPEC = importlib.util.spec_from_file_location("dap_scorecard_runtime", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

import dap_scorecard_probes as PROBES  # noqa: E402
import dap_scorecard_transport as TRANSPORT  # noqa: E402


class _FakeProcess:
    def __init__(self) -> None:
        self.stdin = io.BytesIO()
        self.returncode: int | None = None
        self.wait_timeouts: list[float] = []

    def wait(self, timeout: float) -> int:
        self.wait_timeouts.append(timeout)
        self.returncode = 0
        return 0

    def poll(self) -> int | None:
        return self.returncode


class DapScorecardRuntimeTests(unittest.TestCase):
    def _executable(self, root: Path, name: str, source: str) -> Path:
        path = root / name
        path.write_text("#!/usr/bin/env python3\n" + textwrap.dedent(source), encoding="utf-8")
        path.chmod(path.stat().st_mode | stat.S_IXUSR)
        return path

    def _valid_scorecard(self) -> dict:
        ended = time.time_ns() // 1_000_000
        started = ended - 100
        return {
            "created_unix_seconds": ended // 1000,
            "timing": {
                "started_unix_ms": started,
                "ended_unix_ms": ended,
                "duration_ms": 100,
                "max_duration_ms": MODULE.MAX_SCORECARD_DURATION_MS,
            },
            "subject": {
                "process_invocations": MODULE.REQUIRED_PROCESS_INVOCATIONS,
            },
            "launch": {"passed": 5, "total": 5, "p50_ms": 1, "p95_ms": 1},
            "attach": {"passed": 5, "total": 5},
            "variables": {"status": "PASS", "detail": "ok"},
            "evaluate": {"status": "PASS", "detail": "ok"},
            "deep_pagination": {"status": "NOT_PROVEN", "detail": "honest marker"},
            "memory": {"status": "MEASURED", "detail": "ok"},
        }

    def test_percentile_uses_harness_nearest_rank(self) -> None:
        values = [50, 10, 40, 20, 30]
        self.assertEqual(MODULE.percentile(values, 50), 30)
        self.assertEqual(MODULE.percentile(values, 95), 50)
        self.assertIsNone(MODULE.percentile([], 50))

    def test_frame_round_trip(self) -> None:
        message = {"type": "event", "seq": 1, "event": "stopped", "body": {"threadId": 7}}
        framed = MODULE.frame_message(message)
        self.assertEqual(MODULE.read_framed_message(io.BytesIO(framed)), message)

    def test_missing_content_length_fails(self) -> None:
        with self.assertRaises(MODULE.ScorecardError):
            MODULE.read_framed_message(io.BytesIO(b"X-Test: 1\r\n\r\n{}"))

    def test_duplicate_content_length_fails(self) -> None:
        with self.assertRaises(MODULE.ScorecardError):
            MODULE.read_framed_message(
                io.BytesIO(b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}")
            )

    def test_negative_content_length_fails(self) -> None:
        with self.assertRaises(MODULE.ScorecardError):
            MODULE.read_framed_message(io.BytesIO(b"Content-Length: -1\r\n\r\n"))

    def test_oversized_body_is_rejected_before_allocation(self) -> None:
        oversized = TRANSPORT.MAX_FRAME_BODY_BYTES + 1
        with self.assertRaisesRegex(MODULE.ScorecardError, "body exceeds"):
            MODULE.read_framed_message(
                io.BytesIO(f"Content-Length: {oversized}\r\n\r\n".encode("ascii"))
            )

    def test_fixture_parser_requires_canonical_name(self) -> None:
        name, path = MODULE._parse_fixture("hello=/tmp/hello.pl")
        self.assertEqual(name, "hello")
        self.assertEqual(path, Path("/tmp/hello.pl"))
        with self.assertRaises(Exception):
            MODULE._parse_fixture("unknown=/tmp/hello.pl")

    def test_noisy_server_hits_bounded_retention_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            binary = self._executable(
                Path(temp_dir),
                "noisy-adapter",
                f"""
                import json
                import sys
                import time

                for index in range({TRANSPORT.MAX_RETAINED_MESSAGES + 32}):
                    body = json.dumps(
                        {{
                            "type": "event",
                            "seq": index + 1,
                            "event": "noise",
                            "body": {{"output": str(index)}},
                        }},
                        separators=(",", ":"),
                    ).encode("utf-8")
                    sys.stdout.buffer.write(
                        f"Content-Length: {{len(body)}}\\r\\n\\r\\n".encode("ascii") + body
                    )
                    sys.stdout.buffer.flush()
                    time.sleep(0.001)
                time.sleep(30)
                """,
            )
            with TRANSPORT.DapProcess(binary, 3.0) as dap:
                with self.assertRaises(MODULE.ScorecardError) as raised:
                    dap.wait_event("never-arrives")
            self.assertTrue(
                "envelope" in str(raised.exception) or "bounded capacity" in str(raised.exception),
                str(raised.exception),
            )

    def test_stderr_tail_is_hard_byte_bounded(self) -> None:
        suffix = b"TAIL"
        dap = object.__new__(TRANSPORT.DapProcess)
        dap.process = SimpleNamespace(
            stderr=io.BytesIO(
                b"x" * (TRANSPORT.MAX_STDERR_TAIL_BYTES + TRANSPORT.STDERR_READ_CHUNK_BYTES)
                + suffix
            )
        )
        dap._stderr = bytearray()
        dap._stderr_lock = threading.Lock()

        dap._stderr_loop()

        self.assertLessEqual(len(dap._stderr), TRANSPORT.MAX_STDERR_TAIL_BYTES)
        self.assertTrue(dap.stderr_tail().endswith(suffix.decode("ascii")))

    def test_invocation_counter_records_real_process_spawn(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            binary = self._executable(
                Path(temp_dir),
                "sleeping-adapter",
                """
                import time
                time.sleep(30)
                """,
            )
            counter = TRANSPORT.InvocationCounter()
            dap = TRANSPORT.DapProcess(binary, 1.0, counter)
            try:
                self.assertEqual(counter.count, 1)
            finally:
                dap.close()

    def test_disconnect_requires_response_terminated_and_clean_exit(self) -> None:
        dap = object.__new__(TRANSPORT.DapProcess)
        dap.timeout_seconds = 2.0
        dap.process = _FakeProcess()
        requests: list[tuple[str, dict]] = []
        events: list[str] = []
        dap.request = (
            lambda command, arguments=None: requests.append((command, arguments or {})) or {}
        )
        dap.wait_event = lambda event: events.append(event) or {}

        dap.disconnect()

        self.assertEqual(requests, [("disconnect", {})])
        self.assertEqual(events, ["terminated"])
        self.assertTrue(dap.process.stdin.closed)
        self.assertEqual(dap.process.returncode, 0)
        self.assertEqual(dap.process.wait_timeouts, [2.0])

    def test_disconnect_failure_is_not_swallowed(self) -> None:
        dap = object.__new__(TRANSPORT.DapProcess)
        dap.timeout_seconds = 2.0
        dap.process = _FakeProcess()

        def fail_request(command: str, arguments: dict | None = None) -> dict:
            raise MODULE.ScorecardError(f"{command} failed")

        dap.request = fail_request
        dap.wait_event = lambda event: {}
        with self.assertRaisesRegex(MODULE.ScorecardError, "disconnect failed"):
            dap.disconnect()

    def test_scorecard_failures_reject_skip_low_rates_and_bad_timing(self) -> None:
        scorecard = self._valid_scorecard()
        scorecard["launch"]["passed"] = 3
        scorecard["deep_pagination"] = {"status": "PASS", "detail": "claimed a page"}
        scorecard["timing"]["duration_ms"] = MODULE.MAX_SCORECARD_DURATION_MS + 1
        scorecard["subject"]["process_invocations"] = 1
        failures = MODULE.scorecard_failures(scorecard)
        self.assertTrue(any("launch below threshold" in item for item in failures))
        self.assertTrue(any("deep_pagination" in item for item in failures))
        self.assertTrue(any("duration exceeded" in item for item in failures))
        self.assertTrue(any("invocation count" in item for item in failures))

    def test_valid_timing_and_invocation_receipt_has_no_policy_failures(self) -> None:
        self.assertEqual(MODULE.scorecard_failures(self._valid_scorecard()), [])

    def test_deep_pagination_selects_unique_locals_big(self) -> None:
        expected = {"name": "@big", "value": "ARRAY(0x0)"}
        scopes = {
            "Package": [
                {
                    "name": "@unrelated",
                    "variablesReference": 12_001,
                    "indexedVariables": 200,
                }
            ],
            "Locals": [expected],
        }
        self.assertEqual(PROBES._require_lexical_big(scopes), expected)

    def test_deep_pagination_rejects_duplicate_big(self) -> None:
        row = {"name": "@big", "value": "ARRAY(0x0)"}
        with self.assertRaisesRegex(MODULE.ScorecardError, "exactly one"):
            PROBES._require_lexical_big({"Locals": [row, dict(row)]})

    def test_unexpanded_big_accepts_the_honest_marker(self) -> None:
        # The exact binary renders the marker quoted; both forms are honest.
        row = {"name": "@big", "value": '"ARRAY(0x0)"'}
        self.assertEqual(PROBES._validate_unexpanded_lexical_big(row), '"ARRAY(0x0)"')
        self.assertEqual(
            PROBES._validate_unexpanded_lexical_big(
                {"name": "@big", "value": "ARRAY(0x0)"}
            ),
            "ARRAY(0x0)",
        )
        # A zero reference is the DAP encoding for "not expandable" and is fine.
        self.assertEqual(
            PROBES._validate_unexpanded_lexical_big(dict(row, variablesReference=0)),
            '"ARRAY(0x0)"',
        )

    def test_unexpanded_big_rejects_unproven_expansion_claims(self) -> None:
        row = {"name": "@big", "value": "ARRAY(0x0)"}
        with self.assertRaisesRegex(MODULE.ScorecardError, "expandable variablesReference"):
            PROBES._validate_unexpanded_lexical_big(
                dict(row, variablesReference=2_000_720_896)
            )
        with self.assertRaisesRegex(MODULE.ScorecardError, "fabricated indexedVariables"):
            PROBES._validate_unexpanded_lexical_big(dict(row, indexedVariables=500))
        with self.assertRaisesRegex(MODULE.ScorecardError, "fabricated namedVariables"):
            PROBES._validate_unexpanded_lexical_big(dict(row, namedVariables=1))

    def test_unexpanded_big_rejects_a_substituted_value(self) -> None:
        with self.assertRaisesRegex(MODULE.ScorecardError, "opaque ARRAY marker"):
            PROBES._validate_unexpanded_lexical_big(
                {"name": "@big", "value": "[1,2,3]"}
            )

    def test_launch_probes_keep_the_configured_trusted_root(self) -> None:
        # The caller's trusted root is the boundary under test: deriving it
        # per-probe from each script's own directory made every launch
        # self-validating. Probes are stubbed (no process spawn); what matters
        # is which root each launch probe receives.
        import shutil

        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir) / "root"
            root.mkdir()
            binary = Path(temp_dir) / "perl-dap"
            shutil.copy("/bin/true", binary)
            binary.chmod(0o755)
            fixtures = {}
            for name in ("hello", "loops", "eval", "args", "begin_end"):
                script = root / f"{name}.pl"
                script.write_text("print 1;\n", encoding="utf-8")
                fixtures[name] = script
            seen_roots: list = []
            real_launch = MODULE.probe_launch
            MODULE.probe_launch = (  # type: ignore[method-assign]
                lambda b, s, t, i, r: seen_roots.append(r) or 7
            )
            real_attach = MODULE.probe_attach
            MODULE.probe_attach = lambda *a, **k: 7  # type: ignore[method-assign]
            real_metrics = MODULE.probe_session_metrics
            MODULE.probe_session_metrics = (  # type: ignore[method-assign]
                lambda *a, **k: ({}, {}, {}, {})
            )
            try:
                scorecard = MODULE.build_scorecard(
                    binary, binary, fixtures, 1.0, root
                )
            finally:
                MODULE.probe_launch = real_launch  # type: ignore[method-assign]
                MODULE.probe_attach = real_attach  # type: ignore[method-assign]
                MODULE.probe_session_metrics = real_metrics  # type: ignore[method-assign]
            self.assertEqual(len(seen_roots), 5)
            for received in seen_roots:
                self.assertEqual(received, root)
            for detail in scorecard["launch"]["details"]:
                self.assertIsNone(detail["error"])

    def test_fixture_outside_the_trusted_root_is_a_caller_error(self) -> None:
        import shutil

        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir) / "root"
            root.mkdir()
            elsewhere = Path(temp_dir) / "elsewhere"
            elsewhere.mkdir()
            binary = Path(temp_dir) / "perl-dap"
            shutil.copy("/bin/true", binary)
            binary.chmod(0o755)
            fixtures = {}
            for name in ("hello", "loops", "eval", "args", "begin_end"):
                script = elsewhere / f"{name}.pl"
                script.write_text("print 1;\n", encoding="utf-8")
                fixtures[name] = script
            real_launch = MODULE.probe_launch
            MODULE.probe_launch = lambda *a, **k: self.fail(  # type: ignore[method-assign]
                "out-of-root fixture must never reach the probe"
            )
            real_attach = MODULE.probe_attach
            MODULE.probe_attach = lambda *a, **k: 7  # type: ignore[method-assign]
            real_metrics = MODULE.probe_session_metrics
            MODULE.probe_session_metrics = (  # type: ignore[method-assign]
                lambda *a, **k: ({}, {}, {}, {})
            )
            try:
                scorecard = MODULE.build_scorecard(
                    binary, binary, fixtures, 1.0, root
                )
            finally:
                MODULE.probe_launch = real_launch  # type: ignore[method-assign]
                MODULE.probe_attach = real_attach  # type: ignore[method-assign]
                MODULE.probe_session_metrics = real_metrics  # type: ignore[method-assign]
            for detail in scorecard["launch"]["details"]:
                self.assertIsNotNone(detail["error"])
                self.assertIn("trusted root", detail["error"])

    def test_trusted_root_reaches_the_server_unresolved(self) -> None:
        # The startup contract rejects symlink roots before canonicalizing;
        # resolving here would hide that seam and test a different root than
        # the caller named. Capture the spawned argv without starting perl-dap.
        captured: list = []
        real_popen = TRANSPORT.subprocess.Popen

        class _RecordingPopen:
            def __init__(self, argv, **kwargs):
                captured.append(argv)
                self.stdin = io.BytesIO()
                self.stdout = io.BytesIO()
                self.stderr = io.BytesIO()
                self.returncode = 0

            def poll(self):
                return 0

        TRANSPORT.subprocess.Popen = _RecordingPopen  # type: ignore[method-assign]
        try:
            with tempfile.TemporaryDirectory() as temp_dir:
                real = Path(temp_dir) / "real"
                real.mkdir()
                link = Path(temp_dir) / "link"
                try:
                    link.symlink_to(real, target_is_directory=True)
                except OSError:
                    self.skipTest("symlinks unavailable")
                    return
                TRANSPORT.DapProcess(
                    Path(temp_dir) / "perl-dap", 1.0, None, link
                ).close()
        finally:
            TRANSPORT.subprocess.Popen = real_popen  # type: ignore[method-assign]
        self.assertEqual(len(captured), 1)
        root_arg = captured[0][captured[0].index("--trusted-root") + 1]
        self.assertEqual(root_arg, str(link))
        self.assertNotEqual(root_arg, str(real.resolve()))


if __name__ == "__main__":
    unittest.main()
