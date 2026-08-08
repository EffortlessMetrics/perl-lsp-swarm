#!/usr/bin/env python3
"""Focused tests for scripts/ci/aggregate_lane_history.py."""

from __future__ import annotations

import importlib.util
import io
import json
import os
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stdout
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("aggregate_lane_history.py")
SPEC = importlib.util.spec_from_file_location("aggregate_lane_history", SCRIPT_PATH)
assert SPEC is not None
aggregate_lane_history = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(aggregate_lane_history)


class AggregateLaneHistoryTests(unittest.TestCase):
    def test_percentile_uses_linear_interpolation(self) -> None:
        self.assertEqual(0.0, aggregate_lane_history.percentile([], 95))
        self.assertEqual(42.0, aggregate_lane_history.percentile([42.0], 50))
        self.assertEqual(25.0, aggregate_lane_history.percentile([10.0, 20.0, 30.0, 40.0], 50))
        self.assertEqual(37.0, aggregate_lane_history.percentile([10.0, 20.0, 30.0, 40.0], 90))

    def test_collect_actuals_filters_old_invalid_and_incomplete_receipts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            actuals = Path(tmp)
            fresh = actuals / "fresh" / "ci-actuals.json"
            fresh.parent.mkdir()
            fresh.write_text(
                json.dumps(
                    {
                        "jobs": [
                            {"gate_name": "rust-small", "actual_lem": 120},
                            {"lane_id": "ripr", "actual_lem": 42.5},
                            {"lane_id": "missing-actual"},
                            {"lane_id": "bad-actual", "actual_lem": "slow"},
                        ]
                    }
                ),
                encoding="utf-8",
            )
            old = actuals / "old.json"
            old.write_text(
                json.dumps({"jobs": [{"lane_id": "old", "actual_lem": 1}]}),
                encoding="utf-8",
            )
            old_time = time.time() - 3 * 86400
            os.utime(old, (old_time, old_time))
            (actuals / "invalid.json").write_text("{", encoding="utf-8")
            (actuals / "array.json").write_text("[]", encoding="utf-8")

            samples, _stats = aggregate_lane_history.collect_actuals(
                actuals_dir=actuals,
                window_days=1,
                known_lanes={"ripr", "rust-small"},
            )

        self.assertEqual({"ripr": [42.5], "rust-small": [120.0]}, samples)

    # ------------------------------------------------------------------
    # #6217: gate names must not become lanes, and a run that maps nothing
    # must be loud. The pre-existing tests above pass with the mapping fully
    # broken because they only ever use invented lane keys ("rust-small",
    # "ripr") and never assert that a sample reaches a *policy* lane id.
    # ------------------------------------------------------------------

    def test_gate_names_are_not_minted_into_lanes(self) -> None:
        """A gate name that is not a lane id is dropped, not turned into a lane.

        This is the production defect: `fmt`, `clippy_full`, and friends were
        accumulating dozens of samples each in a parallel keyspace no planner
        reads, while every real lane stayed at zero.
        """
        with tempfile.TemporaryDirectory() as tmp:
            actuals = Path(tmp)
            (actuals / "ci-actuals.json").write_text(
                json.dumps(
                    {
                        "jobs": [
                            {"gate_name": "fmt", "actual_lem": 1.0},
                            {"gate_name": "clippy_full", "actual_lem": 2.0},
                            {"gate_name": "unit_foundation_full", "actual_lem": 3.0},
                        ]
                    }
                ),
                encoding="utf-8",
            )

            samples, stats = aggregate_lane_history.collect_actuals(
                actuals_dir=actuals,
                window_days=14,
                known_lanes={"merge_gate_shards", "pr_smoke"},
            )

        self.assertEqual({}, samples, "gate names must not create lanes")
        self.assertEqual(0, stats["accepted_samples"])
        self.assertEqual(3, stats["unmapped_samples"])
        self.assertEqual(
            {"fmt": 1, "clippy_full": 1, "unit_foundation_full": 1},
            stats["unmapped_keys"],
        )

    def test_explicit_lane_id_attributes_samples_to_a_policy_lane(self) -> None:
        """A receipt stamped with --lane-id lands on that real lane.

        The positive half of the pair above: several gates inside one shard
        lane all attribute to that lane, which is what makes a learned
        estimate possible at all.
        """
        with tempfile.TemporaryDirectory() as tmp:
            actuals = Path(tmp)
            (actuals / "ci-actuals.json").write_text(
                json.dumps(
                    {
                        "jobs": [
                            {
                                "lane_id": "merge_gate_shards",
                                "gate_name": "fmt",
                                "actual_lem": 1.0,
                            },
                            {
                                "lane_id": "merge_gate_shards",
                                "gate_name": "clippy_full",
                                "actual_lem": 2.0,
                            },
                        ]
                    }
                ),
                encoding="utf-8",
            )

            samples, stats = aggregate_lane_history.collect_actuals(
                actuals_dir=actuals,
                window_days=14,
                known_lanes={"merge_gate_shards", "pr_smoke"},
            )

        self.assertEqual({"merge_gate_shards": [1.0, 2.0]}, samples)
        self.assertEqual(2, stats["accepted_samples"])
        self.assertEqual(0, stats["unmapped_samples"])

    def test_near_miss_gate_name_does_not_bind_to_a_similar_lane(self) -> None:
        """`compile_all_targets` must not be matched to lane `check_all_targets`.

        The two namespaces contain several near-miss pairs
        (`compile_all_targets`/`check_all_targets`,
        `docs_build`/`docs_gate`). Any fuzzy or prefix match would bind a
        sample to the wrong lane, which is worse than dropping it: it would
        silently corrupt that lane's percentiles.
        """
        with tempfile.TemporaryDirectory() as tmp:
            actuals = Path(tmp)
            (actuals / "ci-actuals.json").write_text(
                json.dumps(
                    {
                        "jobs": [
                            {"gate_name": "compile_all_targets", "actual_lem": 9.0},
                            {"gate_name": "docs_build", "actual_lem": 4.0},
                        ]
                    }
                ),
                encoding="utf-8",
            )

            samples, stats = aggregate_lane_history.collect_actuals(
                actuals_dir=actuals,
                window_days=14,
                known_lanes={"check_all_targets", "docs_gate"},
            )

        self.assertEqual({}, samples)
        self.assertEqual(2, stats["unmapped_samples"])

    def test_gate_name_matching_a_lane_id_exactly_is_still_accepted(self) -> None:
        """Exact equality is not a heuristic, so a 1:1 gate keeps working.

        Guards the rollout: an artifact emitted before --lane-id existed is
        still attributed when its gate name literally is a lane id.
        """
        with tempfile.TemporaryDirectory() as tmp:
            actuals = Path(tmp)
            (actuals / "ci-actuals.json").write_text(
                json.dumps({"jobs": [{"gate_name": "coverage", "actual_lem": 7.0}]}),
                encoding="utf-8",
            )

            samples, stats = aggregate_lane_history.collect_actuals(
                actuals_dir=actuals,
                window_days=14,
                known_lanes={"coverage"},
            )

        self.assertEqual({"coverage": [7.0]}, samples)
        self.assertEqual(1, stats["accepted_samples"])

    def test_main_fails_loudly_when_no_sample_maps_to_a_lane(self) -> None:
        """Samples arrived, none attributed: exit non-zero instead of exit 0.

        Before #6217 this wrote a valid-looking all-zero history and returned
        success, which is indistinguishable from "no data" to every consumer.
        """
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            actuals = root / "actuals"
            actuals.mkdir()
            (actuals / "ci-actuals.json").write_text(
                json.dumps({"jobs": [{"gate_name": "fmt", "actual_lem": 5.0}]}),
                encoding="utf-8",
            )
            lanes = root / "ci-lanes.toml"
            lanes.write_text("[lane.merge_gate_shards]\nbase_lem = 24\n", encoding="utf-8")
            output = root / "history.json"

            old_argv = sys.argv
            try:
                sys.argv = [
                    "aggregate_lane_history.py",
                    "--actuals-dir", str(actuals),
                    "--output", str(output),
                    "--static-lanes", str(lanes),
                ]
                buf = io.StringIO()
                with redirect_stdout(buf):
                    rc = aggregate_lane_history.main()
            finally:
                sys.argv = old_argv

            history = json.loads(output.read_text(encoding="utf-8"))

        self.assertEqual(1, rc, "an all-unmapped run must not report success")
        self.assertEqual(0, history["validation"]["accepted_samples"])
        self.assertEqual(1, history["validation"]["unmapped_samples"])
        # The written history must not have grown a `fmt` lane.
        self.assertEqual(["merge_gate_shards"], sorted(history["lanes"]))

    def test_main_succeeds_when_samples_attribute_to_a_lane(self) -> None:
        """Opposite direction: the loudness must not fire on a healthy run."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            actuals = root / "actuals"
            actuals.mkdir()
            (actuals / "ci-actuals.json").write_text(
                json.dumps(
                    {
                        "jobs": [
                            {
                                "lane_id": "merge_gate_shards",
                                "gate_name": "fmt",
                                "actual_lem": 5.0,
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )
            lanes = root / "ci-lanes.toml"
            lanes.write_text("[lane.merge_gate_shards]\nbase_lem = 24\n", encoding="utf-8")
            output = root / "history.json"

            old_argv = sys.argv
            try:
                sys.argv = [
                    "aggregate_lane_history.py",
                    "--actuals-dir", str(actuals),
                    "--output", str(output),
                    "--static-lanes", str(lanes),
                ]
                buf = io.StringIO()
                with redirect_stdout(buf):
                    rc = aggregate_lane_history.main()
            finally:
                sys.argv = old_argv

            history = json.loads(output.read_text(encoding="utf-8"))

        self.assertEqual(0, rc)
        self.assertEqual(1, history["validation"]["accepted_samples"])
        self.assertEqual(1, history["lanes"]["merge_gate_shards"]["samples"])

    def test_empty_input_stays_quiet(self) -> None:
        """No samples at all is not an error: nothing was claimed and nothing lost.

        Distinguishes the two states the old output conflated. Only
        "data arrived and none of it mapped" is loud.
        """
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            actuals = root / "actuals"
            actuals.mkdir()
            lanes = root / "ci-lanes.toml"
            lanes.write_text("[lane.merge_gate_shards]\nbase_lem = 24\n", encoding="utf-8")
            output = root / "history.json"

            old_argv = sys.argv
            try:
                sys.argv = [
                    "aggregate_lane_history.py",
                    "--actuals-dir", str(actuals),
                    "--output", str(output),
                    "--static-lanes", str(lanes),
                ]
                buf = io.StringIO()
                with redirect_stdout(buf):
                    rc = aggregate_lane_history.main()
            finally:
                sys.argv = old_argv

        self.assertEqual(0, rc)

    def test_static_floors_reads_lane_base_lem_values(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            lanes = Path(tmp) / "ci-lanes.toml"
            lanes.write_text(
                """
[lane.rust-small]
base_lem = 20

[lane.docs]
base_lem = 2.5

[lane.no-floor]
label = "No floor"
""",
                encoding="utf-8",
            )

            floors = aggregate_lane_history.static_floors(lanes)

        self.assertEqual({"docs": 2.5, "rust-small": 20.0}, floors)

    def test_build_history_includes_policy_lanes_without_samples(self) -> None:
        history = aggregate_lane_history.build_history(
            samples={"rust-small": [10, 20, 30, 40, 50]},
            floors={"docs": 3, "rust-small": 15},
            window_days=14,
        )

        self.assertEqual(1, history["schema_version"])
        self.assertEqual(2, history["lane_count"])
        self.assertFalse(history["lanes"]["docs"]["learned"])
        self.assertEqual(0, history["lanes"]["docs"]["samples"])
        self.assertTrue(history["lanes"]["rust-small"]["learned"])
        self.assertEqual(30, history["lanes"]["rust-small"]["p50"])
        self.assertEqual(46, history["lanes"]["rust-small"]["p90"])
        self.assertEqual(48, history["lanes"]["rust-small"]["p95"])
        self.assertEqual(30, history["lanes"]["rust-small"]["mean"])

    def test_main_writes_history_and_summary_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            actuals = root / "actuals"
            actuals.mkdir()
            (actuals / "ci-actuals.json").write_text(
                json.dumps({"jobs": [{"lane_id": "rust-small", "actual_lem": 12}]}),
                encoding="utf-8",
            )
            lanes = root / "ci-lanes.toml"
            lanes.write_text(
                """
[lane.rust-small]
base_lem = 10

[lane.docs]
base_lem = 2
""",
                encoding="utf-8",
            )
            output = root / "history.json"

            old_argv = sys.argv
            try:
                sys.argv = [
                    "aggregate_lane_history.py",
                    "--actuals-dir",
                    str(actuals),
                    "--window-days",
                    "14",
                    "--output",
                    str(output),
                    "--static-lanes",
                    str(lanes),
                ]
                stdout = io.StringIO()
                with redirect_stdout(stdout):
                    status = aggregate_lane_history.main()
            finally:
                sys.argv = old_argv

            history = json.loads(output.read_text(encoding="utf-8"))
            printed = json.loads(stdout.getvalue())

        self.assertEqual(0, status)
        self.assertEqual(2, history["lane_count"])
        self.assertEqual(1, history["lanes"]["rust-small"]["samples"])
        # The summary now also reports attribution, so an operator reading the
        # step log can tell a healthy run from one that mapped nothing (#6217).
        self.assertEqual(
            {
                "lanes": 2,
                "learned": 0,
                "window_days": 14,
                "accepted_samples": 1,
                "unmapped_samples": 0,
            },
            printed,
        )


if __name__ == "__main__":
    unittest.main()
