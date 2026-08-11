#!/usr/bin/env python3
"""Focused unit tests for the exact-binary DAP scorecard driver."""

from __future__ import annotations

import importlib.util
import io
import stat
import tempfile
import textwrap
import time
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "dap_scorecard_runtime.py"
SPEC = importlib.util.spec_from_file_location("dap_scorecard_runtime", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

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
            "deep_pagination": {"status": "PASS", "detail": "ok"},
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
        scorecard["deep_pagination"] = {"status": "SKIP", "detail": "not exercised"}
        scorecard["timing"]["duration_ms"] = MODULE.MAX_SCORECARD_DURATION_MS + 1
        scorecard["subject"]["process_invocations"] = 1
        failures = MODULE.scorecard_failures(scorecard)
        self.assertTrue(any("launch below threshold" in item for item in failures))
        self.assertTrue(any("deep_pagination" in item for item in failures))
        self.assertTrue(any("duration exceeded" in item for item in failures))
        self.assertTrue(any("invocation count" in item for item in failures))

    def test_valid_timing_and_invocation_receipt_has_no_policy_failures(self) -> None:
        self.assertEqual(MODULE.scorecard_failures(self._valid_scorecard()), [])


if __name__ == "__main__":
    unittest.main()
