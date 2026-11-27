#!/usr/bin/env python3
"""Fixture tests for read-cpan-corpus-sweep-summary.py (#15315).

Run directly:

    python3 .ci/scripts/test_read_cpan_corpus_sweep_summary.py
"""

from __future__ import annotations

import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path

_MODULE_PATH = Path(__file__).parent / "read-cpan-corpus-sweep-summary.py"
_SPEC = importlib.util.spec_from_file_location("read_cpan_corpus_sweep_summary", _MODULE_PATH)
_MODULE = importlib.util.module_from_spec(_SPEC)
assert _SPEC.loader is not None
_SPEC.loader.exec_module(_MODULE)


def valid_report(**overrides):
    payload = {
        "schema_version": 1,
        "total_files": 100,
        "clean_files": 90,
    }
    payload.update(overrides)
    return payload


class ParseSweepReportTest(unittest.TestCase):
    def test_accepts_current_producer_envelope(self):
        total, clean = _MODULE.parse_sweep_report(valid_report())
        self.assertEqual((total, clean), (100, 90))

    def test_rejects_v2_schema_version(self):
        with self.assertRaisesRegex(ValueError, "unsupported SweepReport schema_version"):
            _MODULE.parse_sweep_report(valid_report(schema_version=2))

    def test_rejects_string_schema_version(self):
        with self.assertRaisesRegex(ValueError, "unsupported SweepReport schema_version"):
            _MODULE.parse_sweep_report(valid_report(schema_version="1"))

    def test_rejects_renamed_counters_without_defaults(self):
        payload = valid_report()
        del payload["total_files"]
        del payload["clean_files"]
        payload["total"] = 100
        payload["clean"] = 90
        with self.assertRaisesRegex(ValueError, "missing total_files"):
            _MODULE.parse_sweep_report(payload)

    def test_rejects_boolean_counters(self):
        with self.assertRaisesRegex(ValueError, "total_files must be an integer counter"):
            _MODULE.parse_sweep_report(valid_report(total_files=True))
        with self.assertRaisesRegex(ValueError, "clean_files must be an integer counter"):
            _MODULE.parse_sweep_report(valid_report(clean_files=False))

    def test_rejects_zero_total_silent_percentage_shape(self):
        with self.assertRaisesRegex(ValueError, "total_files must be positive"):
            _MODULE.parse_sweep_report(valid_report(total_files=0, clean_files=0))

    def test_rejects_clean_above_total(self):
        with self.assertRaisesRegex(ValueError, "0 <= clean <= total_files"):
            _MODULE.parse_sweep_report(valid_report(clean_files=101))


class MainIntegrationTest(unittest.TestCase):
    def test_writes_step_summary_for_valid_report(self):
        with tempfile.TemporaryDirectory() as directory:
            report_path = Path(directory) / "bounded-sweep.json"
            report_path.write_text(json.dumps(valid_report()), encoding="utf-8")
            summary_path = Path(directory) / "summary.md"
            previous = os.environ.get("GITHUB_STEP_SUMMARY")
            os.environ["GITHUB_STEP_SUMMARY"] = str(summary_path)
            try:
                exit_code = _MODULE.main(
                    [
                        str(report_path),
                        "--heading",
                        "### CPAN Corpus (Bounded — Top 50)",
                    ]
                )
            finally:
                if previous is None:
                    os.environ.pop("GITHUB_STEP_SUMMARY", None)
                else:
                    os.environ["GITHUB_STEP_SUMMARY"] = previous

            self.assertEqual(exit_code, 0)
            rendered = summary_path.read_text(encoding="utf-8")
            self.assertIn("Parse-clean | 90 (90.0%)", rendered)
            self.assertIn("Parse failures | 10", rendered)


if __name__ == "__main__":
    unittest.main()
