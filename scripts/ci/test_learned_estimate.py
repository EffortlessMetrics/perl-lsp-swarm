#!/usr/bin/env python3
"""Focused tests for scripts/ci/learned_estimate.py."""

from __future__ import annotations

import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path


_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

from learned_estimate import estimate_for, main  # noqa: E402


class LearnedEstimateTests(unittest.TestCase):
    def test_estimate_for_reports_missing_lane_history(self) -> None:
        estimate = estimate_for("rust_small", {"lanes": {}})

        self.assertFalse(estimate["learned"])
        self.assertEqual("rust_small", estimate["lane"])
        self.assertIsNone(estimate["estimate"])
        self.assertEqual(0, estimate["samples"])
        self.assertEqual("no history entry for this lane", estimate["reason"])

    def test_estimate_for_uses_static_floor_until_lane_has_enough_samples(self) -> None:
        estimate = estimate_for(
            "rust_small",
            {
                "min_samples_for_learned": 7,
                "lanes": {
                    "rust_small": {
                        "learned": False,
                        "static_floor": 12.0,
                        "samples": 3,
                    }
                },
            },
        )

        self.assertFalse(estimate["learned"])
        self.assertEqual(12.0, estimate["estimate"])
        self.assertEqual(12.0, estimate["static_floor"])
        self.assertEqual(3, estimate["samples"])
        self.assertEqual("only 3 samples; need 7 to learn", estimate["reason"])

    def test_estimate_for_uses_higher_static_floor_over_learned_p50(self) -> None:
        estimate = estimate_for(
            "rust_small",
            {
                "lanes": {
                    "rust_small": {
                        "learned": True,
                        "static_floor": 20.0,
                        "p50": 10.0,
                        "p90": 18.0,
                        "p95": 24.0,
                        "samples": 9,
                    }
                }
            },
        )

        self.assertTrue(estimate["learned"])
        self.assertEqual(20.0, estimate["estimate"])
        self.assertEqual("static_floor (higher than learned)", estimate["estimate_source"])
        self.assertEqual(10.0, estimate["p50"])
        self.assertEqual(18.0, estimate["p90_warning"])
        self.assertEqual(24.0, estimate["p95_hard_planning"])

    def test_estimate_for_uses_learned_p50_when_it_exceeds_static_floor(self) -> None:
        estimate = estimate_for(
            "rust_small",
            {
                "lanes": {
                    "rust_small": {
                        "learned": True,
                        "static_floor": 10.0,
                        "p50": 20.0,
                        "p90": 30.0,
                        "p95": 35.0,
                        "samples": 12,
                    }
                }
            },
        )

        self.assertTrue(estimate["learned"])
        self.assertEqual(23.0, estimate["estimate"])
        self.assertEqual("p50 * 1.15", estimate["estimate_source"])
        self.assertEqual(12, estimate["samples"])

    def test_main_prints_advisory_json_for_missing_and_invalid_history(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            missing_history = root / "missing.json"
            invalid_history = root / "invalid.json"
            invalid_history.write_text("{", encoding="utf-8")

            old_argv = sys.argv
            try:
                sys.argv = [
                    "learned_estimate.py",
                    "--history",
                    str(missing_history),
                    "--lane",
                    "rust_small",
                ]
                missing_stdout = io.StringIO()
                with redirect_stdout(missing_stdout):
                    self.assertEqual(0, main())

                sys.argv = [
                    "learned_estimate.py",
                    "--history",
                    str(invalid_history),
                    "--lane",
                    "rust_small",
                ]
                invalid_stdout = io.StringIO()
                with redirect_stdout(invalid_stdout):
                    self.assertEqual(0, main())
            finally:
                sys.argv = old_argv

        missing = json.loads(missing_stdout.getvalue())
        invalid = json.loads(invalid_stdout.getvalue())
        self.assertFalse(missing["learned"])
        self.assertIn("not present", missing["reason"])
        self.assertFalse(invalid["learned"])
        self.assertIn("error", invalid)


class LaneHistoryVersionTests(unittest.TestCase):
    """A lane-history payload is a versioned wire contract, not free JSON.

    `aggregate_lane_history.py` stamps `schema_version` and the repository
    gates the checked-in payload on it, but this reader loaded the file and
    reached straight for `lanes` / `p50` / `static_floor` by name. A future
    version that renames any of those fields is therefore not refused -- it is
    read with the v1 spellings, silently (#15286).
    """

    LANE = "rust_small"

    def _run_main(self, history: object) -> dict:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "ci-lane-history.json"
            path.write_text(json.dumps(history), encoding="utf-8")
            old_argv = sys.argv
            try:
                sys.argv = [
                    "learned_estimate.py",
                    "--history",
                    str(path),
                    "--lane",
                    self.LANE,
                ]
                out = io.StringIO()
                with redirect_stdout(out):
                    self.assertEqual(0, main())
            finally:
                sys.argv = old_argv
        return json.loads(out.getvalue())

    def test_a_version_bumped_history_is_refused_rather_than_read_by_v1_field_names(self) -> None:
        # v2 renames `static_floor` to `floor`. Read by v1 names, the floor
        # lookup yields None, the `floor is not None` guard never fires, and
        # the estimator reports a bare `p50 * 1.15` -- dropping a 45.0-minute
        # floor to 4.6 while still claiming `learned: true`. Refusing is the
        # only honest answer: this reader does not know the v2 shape.
        emitted = self._run_main(
            {
                "schema_version": 2,
                "min_samples_for_learned": 5,
                "lane_count": 1,
                "lanes": {
                    self.LANE: {
                        "samples": 40,
                        "floor": 45.0,
                        "learned": True,
                        "p50": 4.0,
                        "p90": 6.0,
                        "p95": 7.0,
                    }
                },
            }
        )

        self.assertFalse(emitted["learned"])
        self.assertEqual(self.LANE, emitted["lane"])
        self.assertIsNone(emitted["estimate"])
        self.assertIn("schema_version", emitted["reason"])
        self.assertIn("2", emitted["reason"])

    def test_a_matching_version_history_is_still_read_and_learned(self) -> None:
        # The control: the refusal must key on the version, not on the
        # presence of percentiles or on a fast lane. A genuine v1 payload
        # whose learned estimate sits below its static floor is still learned,
        # and still reported at the floor.
        emitted = self._run_main(
            {
                "schema_version": 1,
                "min_samples_for_learned": 5,
                "lane_count": 1,
                "lanes": {
                    self.LANE: {
                        "samples": 40,
                        "static_floor": 45.0,
                        "learned": True,
                        "p50": 4.0,
                        "p90": 6.0,
                        "p95": 7.0,
                    }
                },
            }
        )

        self.assertTrue(emitted["learned"])
        self.assertEqual(45.0, emitted["estimate"])
        self.assertEqual("static_floor (higher than learned)", emitted["estimate_source"])


if __name__ == "__main__":
    unittest.main()
