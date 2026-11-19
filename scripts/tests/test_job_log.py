#!/usr/bin/env python3
"""Falsifiers for scripts/ci/job_log.py (#19559).

The property under test is the one that made a failing nightly lane read as
"no failure found": a truncated read of a job log can omit the failing test,
so a reader that reports "the pattern is absent" must also report how much of
the log it actually read.

These tests build a job-log payload in the shape GitHub returns (a zip archive
of ordinal-named log files, or plain text for a single file) and a
deliberately-truncated prefix of it, then assert that the reader over the
former finds a marker the latter cannot.
"""

from __future__ import annotations

import importlib.util
import io
import json
import sys
import tempfile
import unittest
import zipfile
from contextlib import redirect_stdout, redirect_stderr
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

SPEC = importlib.util.spec_from_file_location("job_log", _HERE.parent / "ci" / "job_log.py")
assert SPEC is not None and SPEC.loader is not None
job_log = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = job_log
SPEC.loader.exec_module(job_log)

# The marker stands in for the `test result: FAILED` line that the truncated
# read lost in the job measured in #19559.
FAILING_MARKER = "test result: FAILED. 1 passed; 1 failed"

BODY_LINES = 400
MARKER_LINE = BODY_LINES  # the failing line is the last one written


def full_log_lines() -> list[str]:
    lines = ["2026-11-19T03:16:2%d.000Z step output line %d" % (i % 10, i) for i in range(BODY_LINES)]
    lines.append(FAILING_MARKER)
    return lines


def zip_payload(lines: list[str]) -> bytes:
    """A job-log archive in the shape GitHub returns for a multi-file job."""
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        archive.writestr("0_job.log", "\n".join(lines[: len(lines) // 2]) + "\n")
        archive.writestr("1_step.log", "\n".join(lines[len(lines) // 2 :]) + "\n")
    return buffer.getvalue()


def write_fixture(directory: Path, name: str, payload: bytes) -> Path:
    target = directory / name
    target.write_bytes(payload)
    return target


class ReadPathTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        self.lines = full_log_lines()

    def test_zip_payload_is_read_in_full_and_finds_the_failing_marker(self) -> None:
        path = write_fixture(self.dir, "job.zip", zip_payload(self.lines))
        text = job_log.read_log_file(str(path))
        hits = job_log.grep_lines(text, r"test result: FAILED")
        self.assertEqual(len(hits), 1, "the failing marker must be visible in the full read")
        self.assertEqual(hits[0][0], MARKER_LINE + 1)

    def test_truncated_payload_omits_the_failing_marker(self) -> None:
        """The defect from #19559, reproduced: the cut hides the failing line."""
        truncated = "\n".join(self.lines[: BODY_LINES // 2]) + "\n"
        path = write_fixture(self.dir, "truncated.log", truncated.encode("utf-8"))
        text = job_log.read_log_file(str(path))
        self.assertEqual(job_log.grep_lines(text, r"test result: FAILED"), [])
        self.assertLess(len(text.splitlines()), len(self.lines))

    def test_receipt_reports_the_lines_read_so_an_absent_pattern_is_distinguishable(self) -> None:
        path = write_fixture(self.dir, "job.zip", zip_payload(self.lines))
        text = job_log.read_log_file(str(path))
        receipt = job_log.build_receipt("fixture", text, len(job_log.grep_lines(text, r"FAILED")))
        self.assertEqual(receipt["lines_read"], len(self.lines))
        self.assertEqual(receipt["pattern_matches"], 1)

        truncated_text = "\n".join(self.lines[: BODY_LINES // 2]) + "\n"
        truncated_receipt = job_log.build_receipt(
            "fixture", truncated_text, len(job_log.grep_lines(truncated_text, r"FAILED"))
        )
        self.assertEqual(truncated_receipt["pattern_matches"], 0)
        self.assertLess(truncated_receipt["lines_read"], receipt["lines_read"])

    def test_plain_text_payload_is_decoded(self) -> None:
        path = write_fixture(self.dir, "job.txt", ("\n".join(self.lines) + "\n").encode("utf-8"))
        text = job_log.read_log_file(str(path))
        self.assertEqual(len(text.splitlines()), len(self.lines))
        self.assertEqual(job_log.grep_lines(text, r"test result: FAILED")[0][0], MARKER_LINE + 1)

    def test_archive_members_are_concatenated_in_ordinal_order(self) -> None:
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr("1_second.log", "second\n")
            archive.writestr("0_first.log", "first\n")
        path = write_fixture(self.dir, "ordered.zip", buffer.getvalue())
        text = job_log.read_log_file(str(path))
        self.assertEqual(text.splitlines(), ["first", "second"])


class ExitCodeTest(unittest.TestCase):
    """A pattern that is absent must be distinguishable from a read that failed."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def _run(self, path: Path, pattern: str) -> tuple[int, dict]:
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = job_log.main(["--from-file", str(path), "--grep", pattern, "--json"])
        payload = out.getvalue()
        return code, (json.loads(payload) if payload.strip() else {})

    def test_absent_pattern_exits_one_and_still_reports_what_was_read(self) -> None:
        path = write_fixture(self.dir, "clean.log", b"nothing to see here\n")
        code, receipt = self._run(path, r"test result: FAILED")
        self.assertEqual(code, 1)
        self.assertEqual(receipt["pattern_matches"], 0)
        self.assertEqual(receipt["lines_read"], 1)

    def test_present_pattern_exits_zero(self) -> None:
        path = write_fixture(self.dir, "dirty.log", ("boom\n" + FAILING_MARKER + "\n").encode("utf-8"))
        code, receipt = self._run(path, r"test result: FAILED")
        self.assertEqual(code, 0)
        self.assertEqual(receipt["pattern_matches"], 1)
        self.assertEqual(receipt["matches"][0]["line"], 2)

    def test_missing_file_exits_two(self) -> None:
        code, _ = self._run(self.dir / "absent.log", r"FAILED")
        self.assertEqual(code, 2)


class StepConclusionTest(unittest.TestCase):
    """The step conclusions are the surface that names the failing step."""

    def test_parse_job_steps_keeps_only_the_conclusion_fields(self) -> None:
        payload = {
            "steps": [
                {"number": 1, "name": "Set up job", "status": "completed", "conclusion": "success"},
                {"number": 2, "name": "Evaluate routed result", "status": "completed", "conclusion": "failure"},
            ]
        }
        steps = job_log.parse_job_steps(payload)
        self.assertEqual(len(steps), 2)
        self.assertEqual(steps[1]["name"], "Evaluate routed result")
        self.assertEqual(steps[1]["conclusion"], "failure")

    def test_parse_job_steps_tolerates_a_payload_without_steps(self) -> None:
        self.assertEqual(job_log.parse_job_steps({}), [])


if __name__ == "__main__":
    unittest.main()
