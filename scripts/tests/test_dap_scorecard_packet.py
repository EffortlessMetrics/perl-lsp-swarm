#!/usr/bin/env python3
"""Negative controls for scripts/ci/dap_scorecard_packet.py."""

from __future__ import annotations

import importlib.util
import json
import shutil
import stat
import subprocess
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
        for relative in (*MODULE.REQUIRED_FIXTURES, *MODULE.REQUIRED_SOURCE_SUBJECTS):
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            if relative == "scripts/ci/dap_scorecard_packet.py":
                shutil.copyfile(SCRIPT, path)
            else:
                path.write_text(f"tracked {relative}\n", encoding="utf-8")
        self.binary = self._executable("target/debug/perl-dap", "echo 'perl-dap 0.17.0'")
        self.perl = self._executable("target/fake-perl", "echo -n 'v5.40.0'")
        self.raw = self.root / "target/dap_scorecard_receipt.json"
        self.status = self.root / MODULE.GENERATED_STATUS_PATH
        self.packet = self.root / "target/receipts/dap-scorecard/packet.json"
        self._write_scorecard()
        self._write_status()
        (self.root / ".gitignore").write_text("/target/\n", encoding="utf-8")
        self._git("init", "-b", "main")
        self._git("config", "user.name", "EffortlessSteven")
        self._git("config", "user.email", "git@effortlesssteven.com")
        self._git("add", ".")
        self._git("commit", "-m", "fixture")
        self.repository_sha = self._git("rev-parse", "HEAD")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _executable(self, relative: str, command: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"#!/bin/sh\n{command}\n", encoding="utf-8")
        path.chmod(path.stat().st_mode | stat.S_IXUSR)
        return path

    def _git(self, *args: str) -> str:
        result = subprocess.run(
            ["git", *args],
            cwd=self.root,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        return result.stdout.strip()

    def _scorecard(self) -> dict:
        def rate(names: tuple[str, ...], latencies: list[int]) -> dict:
            return {
                "passed": len(names),
                "total": len(names),
                "threshold_pct": 80,
                "p50_ms": sorted(latencies)[(len(latencies) + 1) // 2 - 1],
                "p95_ms": max(latencies),
                "details": [
                    {"name": name, "elapsed_ms": elapsed, "error": None}
                    for name, elapsed in zip(names, latencies)
                ],
            }

        ended_unix_ms = time.time_ns() // 1_000_000
        started_unix_ms = ended_unix_ms - 100
        return {
            "schema_version": MODULE.RUNTIME_SCHEMA_VERSION,
            "created_unix_seconds": ended_unix_ms // 1000,
            "timing": {
                "started_unix_ms": started_unix_ms,
                "ended_unix_ms": ended_unix_ms,
                "duration_ms": 100,
                "max_duration_ms": 180_000,
            },
            "subject": {
                "transport": "stdio",
                "binary_path": str(self.binary.resolve()),
                "binary_sha256": MODULE._sha256(self.binary),
                "version_output": "perl-dap 0.17.0",
                "process_invocations": MODULE.REQUIRED_PROCESS_INVOCATIONS,
            },
            "perl_available": True,
            "perl": {"path": str(self.perl.resolve()), "version": "v5.40.0"},
            "launch": rate(MODULE.REQUIRED_LAUNCH_FIXTURE_NAMES, [1, 2, 3, 4, 5]),
            "attach": rate(MODULE.REQUIRED_ATTACH_NAMES, [6, 7, 8, 9, 10]),
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
            f"| Metric | Value | Target | Status |\n"
            f"|---|---|---|---|\n"
            f"| Launch | 5/5 | 80% | {verdict} |\n"
            "<!-- END: DAP_LAUNCH_SCORECARD -->\n"
            "<!-- BEGIN: DAP_SESSION_SCORECARD -->\n"
            f"| Metric | Value | Target | Status |\n"
            f"|---|---|---|---|\n"
            f"| Session | proven | proven | {verdict} |\n"
            "<!-- END: DAP_SESSION_SCORECARD -->\n",
            encoding="utf-8",
        )

    def _build_args(self) -> Namespace:
        return Namespace(
            repository_root=str(self.root),
            repository_sha=self.repository_sha,
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
            "expected_repository_sha": self.repository_sha,
            "expected_binary_sha256": MODULE._sha256(self.binary),
            "expected_run_id": "123",
            "expected_run_attempt": "2",
            "max_age_seconds": 7200,
        }
        values.update(overrides)
        return Namespace(**values)

    def assertPacketError(self, callback) -> None:  # noqa: N802
        with self.assertRaises(MODULE.PacketError):
            callback()

    def test_happy_path_builds_and_validates(self) -> None:
        packet = self._build()
        self.assertEqual(packet["binary"]["transport"], "stdio")
        MODULE.validate_packet(self._validate_args())

    def test_rate_policy_is_fixed_and_recomputed(self) -> None:
        mutations = (
            lambda scorecard: scorecard["launch"].__setitem__("threshold_pct", 0),
            lambda scorecard: scorecard["launch"].__setitem__("p95_ms", 0),
            lambda scorecard: scorecard["launch"]["details"][0].__setitem__(
                "elapsed_ms", -1
            ),
            lambda scorecard: scorecard["attach"].update(
                {
                    "passed": 1,
                    "total": 1,
                    "p50_ms": 6,
                    "p95_ms": 6,
                    "details": scorecard["attach"]["details"][:1],
                }
            ),
        )
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                scorecard = self._scorecard()
                mutate(scorecard)
                self._write_scorecard(scorecard)
                self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_timing_and_invocation_policy_is_fixed(self) -> None:
        mutations = (
            lambda scorecard: scorecard["timing"].__setitem__("max_duration_ms", 0),
            lambda scorecard: scorecard["timing"].__setitem__("duration_ms", 180_001),
            lambda scorecard: scorecard["timing"].__setitem__(
                "ended_unix_ms", scorecard["timing"]["started_unix_ms"] - 1
            ),
            lambda scorecard: scorecard["timing"].__setitem__("duration_ms", 20_000),
            lambda scorecard: scorecard["subject"].__setitem__("process_invocations", 10),
        )
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                scorecard = self._scorecard()
                mutate(scorecard)
                self._write_scorecard(scorecard)
                self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_subject_must_be_the_exact_stdio_binary(self) -> None:
        for field, value in (("transport", "in-process"), ("binary_sha256", "0" * 64)):
            with self.subTest(field=field):
                scorecard = self._scorecard()
                scorecard["subject"][field] = value
                self._write_scorecard(scorecard)
                self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_candidate_git_objects_cannot_be_mutated(self) -> None:
        for relative in (MODULE.REQUIRED_FIXTURES[0], MODULE.REQUIRED_SOURCE_SUBJECTS[0]):
            with self.subTest(relative=relative):
                path = self.root / relative
                original = path.read_text(encoding="utf-8")
                path.write_text("mutated\n", encoding="utf-8")
                self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))
                path.write_text(original, encoding="utf-8")

    def test_only_generated_status_may_differ_after_run(self) -> None:
        self.status.write_text(self.status.read_text(encoding="utf-8") + "generated\n")
        packet = self._build()
        self.assertEqual(
            packet["repository"]["status_porcelain"],
            [f" M {MODULE.GENERATED_STATUS_PATH}"],
        )
        MODULE.validate_packet(self._validate_args())

    def test_unrelated_tracked_diff_fails(self) -> None:
        path = self.root / MODULE.REQUIRED_SOURCE_SUBJECTS[-1]
        path.write_text("changed generator\n", encoding="utf-8")
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_packet_identity_and_freshness_are_revalidated(self) -> None:
        packet = self._build()
        cases = (
            {"expected_repository_sha": "b" * 40},
            {"expected_binary_sha256": "0" * 64},
        )
        for overrides in cases:
            with self.subTest(overrides=overrides):
                self.assertPacketError(
                    lambda: MODULE.validate_packet(self._validate_args(**overrides))
                )
        packet["created_unix_seconds"] = int(time.time()) - 10_000
        self.packet.write_text(json.dumps(packet), encoding="utf-8")
        self.assertPacketError(
            lambda: MODULE.validate_packet(self._validate_args(max_age_seconds=60))
        )

    def test_packet_cannot_forge_embedded_or_duplicate_subjects(self) -> None:
        packet = self._build()
        packet["scorecard"]["variables"]["detail"] = "forged"
        self.packet.write_text(json.dumps(packet), encoding="utf-8")
        self.assertPacketError(lambda: MODULE.validate_packet(self._validate_args()))
        packet = self._build()
        packet["fixtures"].append(dict(packet["fixtures"][0]))
        self.packet.write_text(json.dumps(packet), encoding="utf-8")
        self.assertPacketError(lambda: MODULE.validate_packet(self._validate_args()))

    def test_required_statuses_and_generated_status_fail_closed(self) -> None:
        scorecard = self._scorecard()
        scorecard["deep_pagination"] = {"status": "SKIP", "detail": "not measured"}
        self._write_scorecard(scorecard)
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))
        self._write_scorecard()
        self._write_status("SKIP")
        self.assertPacketError(lambda: MODULE.build_packet(self._build_args()))

    def test_post_packet_status_mutation_and_dirty_flag_fail(self) -> None:
        self._build()
        self.status.write_text(self.status.read_text(encoding="utf-8") + "mutated\n")
        self.assertPacketError(lambda: MODULE.validate_packet(self._validate_args()))
        self._write_status()
        args = self._build_args()
        args.repository_dirty = True
        self.assertPacketError(lambda: MODULE.build_packet(args))


if __name__ == "__main__":
    unittest.main()
