#!/usr/bin/env python3
"""Focused unit tests for the exact-binary DAP scorecard driver."""

from __future__ import annotations

import importlib.util
import io
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "dap_scorecard_runtime.py"
SPEC = importlib.util.spec_from_file_location("dap_scorecard_runtime", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DapScorecardRuntimeTests(unittest.TestCase):
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

    def test_negative_content_length_fails(self) -> None:
        with self.assertRaises(MODULE.ScorecardError):
            MODULE.read_framed_message(io.BytesIO(b"Content-Length: -1\r\n\r\n"))

    def test_fixture_parser_requires_canonical_name(self) -> None:
        name, path = MODULE._parse_fixture("hello=/tmp/hello.pl")
        self.assertEqual(name, "hello")
        self.assertEqual(path, Path("/tmp/hello.pl"))
        with self.assertRaises(Exception):
            MODULE._parse_fixture("unknown=/tmp/hello.pl")

    def test_scorecard_failures_reject_skip_and_low_rates(self) -> None:
        scorecard = {
            "launch": {"passed": 3, "total": 5, "p50_ms": 1, "p95_ms": 1},
            "attach": {"passed": 5, "total": 5},
            "variables": {"status": "PASS", "detail": "ok"},
            "evaluate": {"status": "PASS", "detail": "ok"},
            "deep_pagination": {"status": "SKIP", "detail": "not exercised"},
            "memory": {"status": "MEASURED", "detail": "ok"},
        }
        failures = MODULE.scorecard_failures(scorecard)
        self.assertTrue(any("launch below threshold" in item for item in failures))
        self.assertTrue(any("deep_pagination" in item for item in failures))


if __name__ == "__main__":
    unittest.main()
