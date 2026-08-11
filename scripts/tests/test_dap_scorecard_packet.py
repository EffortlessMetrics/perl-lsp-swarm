#!/usr/bin/env python3
"""Negative controls for scripts/ci/dap_scorecard_packet.py."""

from __future__ import annotations

import importlib.util
import json
import stat
import tempfile
import time
import unittest
from argparse import Namespace
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "dap_scorecard_packet.py"
SPEC = importlib.util.spec_from_file_location("dap_scorecard_packet", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DapScorecardPacketTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "target/receipts/dap-scorecard").mkdir(parents=True)
        (self.root / "docs/project/status").mkdir(parents=True)
        fixture_root = self.root / "crates/perl-dap/tests/fixtures"
        fixture_root.mkdir(parents=True)
        for fixture in MODULE.REQUIRED_FIXTURES:
            path = self.root / fixture
            path.write_text(f"fixture {path.name}\n", encoding="utf-8")

        self.binary = self.root / "target/debug/perl-dap"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_text("#!/bin/sh\necho 'perl-dap 0.17.0'\n", encoding="utf-8")
        self.binary.chmod(self.binary.stat().st_mode | stat.S_IXUSR)

        self.perl = self.root / "fake-perl"
        self.perl.write_text("#!/bin/sh\necho -n 'v5.40.0'\n", encoding="utf-8")
        self.perl.chmod(self.perl.stat().st_mode | stat.S_IXUSR)

        self.raw = self.root / "target/dap_scorecard_receipt.json"
        self.status = self.root / "docs/project/status/dap.md"
        self.packet = self.root / "target/receipts/dap-scorecard/packet.json"
        self._write_scorecard()
        self._write_status()

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _scorecard(self) -> dict:
        return {
            "perl_available": True,
            "launch": {
                "passed": 5,
                "total": 5,
                "threshold_pct": 80,
                "p50_ms": 1,
                "p95_ms": 2,
                "details": [
                    {"name": name, "elapsed_ms": 1, "error": None}
                    for name in MODULE.REQUIRED_LAUNCH_FIXTURE_NAMES
                ],
            },
            "attach": {
                "passed": 5,
                "total": 5,
                "threshold_pct": 80,
                "p50_ms": None,
                "p95_ms": None,
                "details": [
                    {"name": "tcp_loopback", "elapsed_ms": None, "error": None}
                    for _ in range(5)
                ],
            },
            "variables": {"status": "PASS", "detail": "variables proven"},
            "evaluate": {"status": "PASS", "detail": "evaluate proven"},
            "deep_pagination": {"status": "PASS", "detail": "pagination proven"},
            "memory": {"status": "MEASURED", "detail": "memory measured"},
        }

    def _write_scorecard(self, value: dict | None = None) -> None:
        self.raw.write_text(json.dumps(value or self._scorecard()), encoding="utf-8")

    def _write_status(self, verdict: str = "PASS") -> None:
        self.status.write_text(
            "# DAP\n"
            "<!-- BEGIN: DAP_LAUNCH_SCORECARD -->\n"
            f"| Metric | Value | Target | Status |\n|---|---|---|---|\n| Launch | 5/5 | 80% | {verdict} |\n"
            "<!-- END: DAP_LAUNCH_SCORECARD -->\n"
            "<!-- BEGIN: DAP_SESSION_SCORECARD -->\n"
            f"| Metric | Value | Target | Status |\n|---|---|---|---|\n| Session | proven | proven | {verdict} |\n"
            "<!-- END: DAP_SESSION_SCORECARD -->\n",
            encoding="utf-8",
        )

    def _build_args(self) -> Namespace:
        return Namespace(
            repository_root=str(self.root),
            repository_sha="a" * 40,
            repository_dirty=False,
            run_id="123",
            run_attempt="2",
            binary=str(self.binary.relative_to(self.root)),
            perl=str(self.perl),
            raw_receipt=str(self.raw.relative_to(self.root)),
            status=str(self.status.relative_to(self.root)),
            fixture=list(MODULE.REQUIRED_FIXTURES),
            output=str(self.packet),
        )

    def _build(self) -> dict:
        packet = MODULE.build_packet(self._build_args())
        self.packet.write_text(json.dumps(packet), encoding="utf-8")
        return packet

    def _validate_args(self, **overrides: object) -> Namespace:
        values = {
            "repository_root": str(self.root),
            "packet": str(self.packet),
            "expected_repository_sha": "a" * 40,
            "expected_binary_sha256": MODULE._sha256(self.binary),
            "expected_run_id": "123",
            "expected_run_attempt": "2",
            "max_age_seconds": 7200,
        }
        values.update(overrides)
        return Namespace(**values)

    def assertPacketError(self, callback) -> None:  # noqa: N802 - unittest convention helper
        with self.assertRaises(MODULE.PacketError):
            callback()

    def test_happy_path_builds_and_validates(self) -> None:
        self._build()
        MODULE.validate_packet(self._validate_args())

    def test_missing_receipt_fails(self) -> None:
        self.raw.unlink()
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_malformed_receipt_fails(self) -> None:
        self.raw.write_text("{", encoding="utf-8")
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_cross_sha_packet_fails(self) -> None:
        self._build()
        self.assertPacketError(
            lambda: MODULE.validate_packet(
                self._validate_args(expected_repository_sha="b" * 40)
            )
        )

    def test_cross_binary_packet_fails(self) -> None:
        self._build()
        self.assertPacketError(
            lambda: MODULE.validate_packet(
                self._validate_args(expected_binary_sha256="0" * 64)
            )
        )

    def test_stale_packet_fails(self) -> None:
        packet = self._build()
        packet["created_unix_seconds"] = int(time.time()) - 10_000
        self.packet.write_text(json.dumps(packet), encoding="utf-8")
        self.assertPacketError(
            lambda: MODULE.validate_packet(self._validate_args(max_age_seconds=60))
        )

    def test_missing_required_row_fails(self) -> None:
        scorecard = self._scorecard()
        scorecard.pop("evaluate")
        self._write_scorecard(scorecard)
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_duplicate_fixture_identity_fails(self) -> None:
        packet = self._build()
        packet["fixtures"].append(dict(packet["fixtures"][0]))
        self.packet.write_text(json.dumps(packet), encoding="utf-8")
        self.assertPacketError(lambda: MODULE.validate_packet(self._validate_args()))

    def test_duplicate_launch_row_fails(self) -> None:
        scorecard = self._scorecard()
        scorecard["launch"]["details"][4]["name"] = "hello"
        self._write_scorecard(scorecard)
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_contradictory_rate_verdict_fails(self) -> None:
        scorecard = self._scorecard()
        scorecard["attach"]["details"][0]["error"] = "connection failed"
        self._write_scorecard(scorecard)
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_skip_cannot_be_represented_as_pass(self) -> None:
        scorecard = self._scorecard()
        scorecard["deep_pagination"] = {"status": "SKIP", "detail": "not measured"}
        self._write_scorecard(scorecard)
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_perl_instrument_failure_cannot_be_green(self) -> None:
        scorecard = self._scorecard()
        scorecard["perl_available"] = False
        self._write_scorecard(scorecard)
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_embedded_scorecard_cannot_diverge_from_raw_receipt(self) -> None:
        packet = self._build()
        packet["scorecard"]["variables"]["detail"] = "forged green detail"
        self.packet.write_text(json.dumps(packet), encoding="utf-8")
        self.assertPacketError(lambda: MODULE.validate_packet(self._validate_args()))

    def test_binary_version_output_cannot_be_forged(self) -> None:
        packet = self._build()
        packet["binary"]["version_output"] = "perl-dap 999.0.0"
        self.packet.write_text(json.dumps(packet), encoding="utf-8")
        self.assertPacketError(lambda: MODULE.validate_packet(self._validate_args()))

    def test_status_mutation_after_packet_fails(self) -> None:
        self._build()
        self.status.write_text(
            self.status.read_text(encoding="utf-8") + "mutated\n", encoding="utf-8"
        )
        self.assertPacketError(lambda: MODULE.validate_packet(self._validate_args()))

    def test_generated_status_skip_fails(self) -> None:
        self._write_status("SKIP")
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_dirty_candidate_fails(self) -> None:
        args = self._build_args()
        args.repository_dirty = True
        self.assertPacketError(lambda: MODULE.build_packet(args))


if __name__ == "__main__":
    unittest.main()
