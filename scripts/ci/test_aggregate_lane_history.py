#!/usr/bin/env python3
"""Focused tests for scripts/ci/aggregate_lane_history.py."""

from __future__ import annotations

from datetime import datetime, timezone
import importlib.util
import io
import json
import os
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stdout
from datetime import date
from pathlib import Path

SCRIPT_PATH = Path(__file__).with_name("aggregate_lane_history.py")
SPEC = importlib.util.spec_from_file_location("aggregate_lane_history", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load {SCRIPT_PATH}")
aggregate_lane_history = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = aggregate_lane_history
SPEC.loader.exec_module(aggregate_lane_history)

REPOSITORY = "EffortlessMetrics/perl-lsp-swarm"
DEFAULT_BRANCH = "main"
HEAD_SHA = "a" * 40


def current_timestamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def write_run(
    root: Path,
    *,
    run_id: int = 123,
    event: str = "push",
    branch: str = DEFAULT_BRANCH,
    repository: str = REPOSITORY,
    conclusion: str = "success",
    created_at: str | None = None,
    marker_run_id: int | None = None,
    marker_sha: str = HEAD_SHA,
    receipt_sha: str = HEAD_SHA,
    receipt_repo: str = "perl-lsp",
    receipt_pr: int | None = 0,
    receipt_workflow: str = "CI",
    receipt_schema: int = 1,
    jobs: list[object] | None = None,
    write_marker: bool = True,
    nested_marker: dict[str, object] | None = None,
) -> Path:
    run_dir = root / f"run-{run_id}"
    artifact_dir = run_dir / "artifacts" / "ci-actuals-meta"
    artifact_dir.mkdir(parents=True)
    if write_marker:
        (run_dir / aggregate_lane_history.TRUSTED_MARKER).write_text(
            json.dumps(
                {
                    "run_id": marker_run_id if marker_run_id is not None else run_id,
                    "repository": repository,
                    "event": event,
                    "head_branch": branch,
                    "head_sha": marker_sha,
                    "conclusion": conclusion,
                    "created_at": created_at or current_timestamp(),
                }
            )
            + "\n",
            encoding="utf-8",
        )
    if nested_marker is not None:
        (artifact_dir / aggregate_lane_history.TRUSTED_MARKER).write_text(
            json.dumps(nested_marker) + "\n", encoding="utf-8"
        )
    receipt = artifact_dir / "ci-actuals-meta.json"
    receipt.write_text(
        json.dumps(
            {
                "schema_version": receipt_schema,
                "repo": receipt_repo,
                "sha": receipt_sha,
                "pr": receipt_pr,
                "workflow": receipt_workflow,
                "jobs": jobs or [],
            }
        )
        + "\n",
        encoding="utf-8",
    )
    return receipt


def collect(root: Path) -> tuple[dict[str, list[float]], dict[str, object]]:
    samples, raw_stats = aggregate_lane_history.collect_actuals(
        actuals_dir=root,
        window_days=14,
        allowed_lanes={"meta"},
        require_trusted_markers=True,
        repository=REPOSITORY,
        default_branch=DEFAULT_BRANCH,
    )
    return samples, aggregate_lane_history.serializable_stats(raw_stats)


class AggregateLaneHistoryTests(unittest.TestCase):
    def test_percentile_uses_linear_interpolation(self) -> None:
        self.assertEqual(0.0, aggregate_lane_history.percentile([], 95))
        self.assertEqual(42.0, aggregate_lane_history.percentile([42.0], 50))
        self.assertEqual(
            25.0,
            aggregate_lane_history.percentile([10.0, 20.0, 30.0, 40.0], 50),
        )
        self.assertEqual(
            37.0,
            aggregate_lane_history.percentile([10.0, 20.0, 30.0, 40.0], 90),
        )

    def test_static_floors_fails_closed_on_missing_or_invalid_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            valid = root / "valid.toml"
            valid.write_text(
                "[lane.meta]\nbase_lem = 2.5\n[lane.docs]\nbase_lem = 3\n",
                encoding="utf-8",
            )
            self.assertEqual(
                {"docs": 3.0, "meta": 2.5},
                aggregate_lane_history.static_floors(valid),
            )

            empty = root / "empty.toml"
            empty.write_text("", encoding="utf-8")
            invalid = root / "invalid.toml"
            invalid.write_text('[lane.meta]\nbase_lem = "large"\n', encoding="utf-8")
            non_finite = root / "non-finite.toml"
            non_finite.write_text("[lane.meta]\nbase_lem = nan\n", encoding="utf-8")

            for path in (root / "missing.toml", empty, invalid, non_finite):
                with self.subTest(path=path.name):
                    with self.assertRaises(ValueError):
                        aggregate_lane_history.static_floors(path)

    def test_trusted_default_branch_push_and_merge_group_are_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_run(
                root,
                run_id=1,
                marker_sha="a" * 40,
                receipt_sha="a" * 40,
                jobs=[{"gate_name": "meta", "actual_lem": 4.5}],
            )
            write_run(
                root,
                run_id=2,
                marker_sha="b" * 40,
                receipt_sha="b" * 40,
                event="merge_group",
                branch="gh-readonly-queue/main/pr-1-deadbeef",
                jobs=[{"lane_id": "meta", "actual_lem": 7}],
            )
            samples, stats = collect(root)

        self.assertEqual({"meta": [4.5, 7.0]}, samples)
        self.assertEqual([1, 2], stats["source_run_ids"])
        self.assertEqual(2, stats["accepted_samples"])

    def test_shared_receipt_sha_keeps_distinct_trusted_runs_separate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shared_sha = "c" * 40
            write_run(
                root,
                run_id=1,
                marker_sha=shared_sha,
                receipt_sha=shared_sha,
                jobs=[{"lane_id": "meta", "actual_lem": 4.0}],
            )
            write_run(
                root,
                run_id=2,
                marker_sha=shared_sha,
                receipt_sha=shared_sha,
                jobs=[{"lane_id": "meta", "actual_lem": 6.0}],
            )
            samples, stats = collect(root)

        self.assertEqual({"meta": [4.0, 6.0]}, samples)
        self.assertEqual([1, 2], stats["source_run_ids"])
        self.assertEqual(2, stats["lane_executions"])
        self.assertEqual(2, stats["accepted_samples"])

    def test_exact_run_marker_cannot_be_forged_inside_downloaded_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            forged = {
                "run_id": 3,
                "repository": REPOSITORY,
                "event": "push",
                "head_branch": DEFAULT_BRANCH,
                "head_sha": HEAD_SHA,
                "conclusion": "success",
                "created_at": current_timestamp(),
            }
            write_run(
                root,
                run_id=3,
                repository="attacker/fork",
                nested_marker=forged,
                jobs=[{"lane_id": "meta", "actual_lem": 3}],
            )
            samples, stats = collect(root)

        self.assertEqual({}, samples)
        self.assertEqual(1, stats["rejected"].get("foreign_repository"))

    def test_untrusted_provenance_classes_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_run(
                root,
                run_id=3,
                repository="attacker/fork",
                jobs=[{"lane_id": "meta", "actual_lem": 3}],
            )
            write_run(
                root,
                run_id=4,
                conclusion="failure",
                jobs=[{"lane_id": "meta", "actual_lem": 4}],
            )
            write_run(
                root,
                run_id=5,
                branch="feature",
                jobs=[{"lane_id": "meta", "actual_lem": 5}],
            )
            write_run(
                root,
                run_id=6,
                event="pull_request",
                branch="feature",
                jobs=[{"lane_id": "meta", "actual_lem": 6}],
            )
            write_run(
                root,
                run_id=7,
                marker_run_id=999,
                jobs=[{"lane_id": "meta", "actual_lem": 7}],
            )
            write_run(
                root,
                run_id=8,
                jobs=[{"lane_id": "meta", "actual_lem": 8}],
                write_marker=False,
            )
            samples, stats = collect(root)

        self.assertEqual({}, samples)
        rejected = stats["rejected"]
        self.assertEqual(1, rejected.get("foreign_repository"))
        self.assertEqual(1, rejected.get("unsuccessful_run"))
        self.assertEqual(1, rejected.get("untrusted_branch"))
        self.assertEqual(1, rejected.get("untrusted_event"))
        self.assertEqual(1, rejected.get("run_id_mismatch"))
        self.assertEqual(1, rejected.get("missing_marker"))

    def test_marker_timestamp_not_extraction_mtime_controls_window(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipt = write_run(
                root,
                created_at="2000-01-01T00:00:00Z",
                jobs=[{"lane_id": "meta", "actual_lem": 6}],
            )
            now = time.time()
            os.utime(receipt, (now, now))
            samples, stats = collect(root)

        self.assertEqual({}, samples)
        self.assertEqual(1, stats["rejected"].get("outside_window"))

    def test_receipt_identity_is_bound_to_trusted_run(self) -> None:
        cases = [
            ({"receipt_schema": 2}, "unsupported_receipt_schema"),
            ({"receipt_repo": "attacker"}, "receipt_repo_mismatch"),
            ({"receipt_pr": 12}, "pull_request_receipt"),
            ({"receipt_workflow": ""}, "missing_workflow_identity"),
            ({"receipt_sha": "not-a-sha"}, "invalid_receipt_sha"),
            ({"receipt_sha": "b" * 40}, "receipt_sha_mismatch"),
        ]
        for kwargs, expected_reason in cases:
            with self.subTest(reason=expected_reason), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                write_run(
                    root,
                    jobs=[{"lane_id": "meta", "actual_lem": 1}],
                    **kwargs,
                )
                samples, stats = collect(root)
                self.assertEqual({}, samples)
                self.assertEqual(1, stats["rejected"].get(expected_reason))

    def test_unknown_and_invalid_samples_cannot_enter_history(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_run(
                root,
                jobs=[
                    {"lane_id": "unknown", "actual_lem": 1},
                    {"lane_id": "meta", "actual_lem": True},
                    {"lane_id": "meta", "actual_lem": "1"},
                    {"lane_id": "meta", "actual_lem": float("nan")},
                    {"lane_id": "meta", "actual_lem": -1},
                    {
                        "lane_id": "meta",
                        "actual_lem": aggregate_lane_history.MAX_ACTUAL_LEM + 1,
                    },
                    {"lane_id": "meta", "actual_lem": 3},
                ],
            )
            samples, stats = collect(root)

        self.assertEqual({"meta": [3.0]}, samples)
        rejected = stats["rejected"]
        self.assertEqual(1, stats["unmapped_samples"])
        self.assertEqual(2, rejected.get("invalid_actual"))
        self.assertEqual(1, rejected.get("non_finite_actual"))
        self.assertEqual(2, rejected.get("out_of_range_actual"))

    def test_receipt_byte_and_job_limits_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            oversized = write_run(root, run_id=1, jobs=[])
            oversized.write_bytes(b" " * (aggregate_lane_history.MAX_RECEIPT_BYTES + 1))
            write_run(
                root,
                run_id=2,
                jobs=[
                    {"lane_id": "meta", "actual_lem": index}
                    for index in range(aggregate_lane_history.MAX_JOBS_PER_RECEIPT + 1)
                ],
            )
            samples, stats = collect(root)

        self.assertEqual({}, samples)
        self.assertEqual(2, stats["rejected"].get("oversized_receipt"))

    def test_gate_names_are_not_minted_into_lanes(self) -> None:
        """A gate name that is not a lane id is dropped, not turned into a lane."""
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
                allowed_lanes={"merge_gate_shards", "pr_smoke"},
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
        estimate possible at all. They also *sum* into one sample for the
        lane execution rather than landing as one sample each — see
        `test_one_sample_per_lane_execution_not_per_gate` for why.
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
                allowed_lanes={"merge_gate_shards", "pr_smoke"},
            )

        self.assertEqual({"merge_gate_shards": [3.0]}, samples)
        self.assertEqual(2, stats["accepted_samples"], "two gates accepted")
        self.assertEqual(1, stats["lane_executions"], "summed into one lane sample")
        self.assertEqual(0, stats["unmapped_samples"])

    def test_one_sample_per_lane_execution_not_per_gate(self) -> None:
        """Gates in one lane run sum into a single sample, across shard artifacts.

        Production shape: one `merge_gate_shards` execution spans eight matrix
        jobs and dozens of gates, all stamped with the same lane. One sample
        per gate would let a single run clear the five-sample learned
        threshold on its own, and would make the percentiles describe a gate
        rather than the lane.
        """
        with tempfile.TemporaryDirectory() as tmp:
            actuals = Path(tmp)
            for shard, gates in (("meta", (5.0, 6.0)), ("lsp", (7.0,))):
                d = actuals / f"ci-actuals-{shard}"
                d.mkdir()
                (d / "ci-actuals.json").write_text(
                    json.dumps(
                        {
                            "sha": "abc123",
                            "workflow": "CI",
                            "jobs": [
                                {
                                    "lane_id": "merge_gate_shards",
                                    "gate_name": f"{shard}_gate_{i}",
                                    "actual_lem": lem,
                                }
                                for i, lem in enumerate(gates)
                            ],
                        }
                    ),
                    encoding="utf-8",
                )

            samples, stats = aggregate_lane_history.collect_actuals(
                actuals_dir=actuals,
                window_days=14,
                allowed_lanes={"merge_gate_shards"},
            )

        # 5 + 6 + 7 across two shard artifacts of the same run = one 18.0 sample.
        self.assertEqual({"merge_gate_shards": [18.0]}, samples)
        self.assertEqual(3, stats["accepted_samples"], "three gates were accepted")
        self.assertEqual(1, stats["lane_executions"], "but they are one lane execution")

    def test_separate_runs_stay_separate_samples(self) -> None:
        """Grouping must not merge distinct runs into one sample.

        The opposite-direction control for the grouping: summing is keyed on
        run identity, so two runs of the same lane produce two samples rather
        than one inflated one.
        """
        with tempfile.TemporaryDirectory() as tmp:
            actuals = Path(tmp)
            for sha, lem in (("sha_one", 10.0), ("sha_two", 20.0)):
                d = actuals / sha
                d.mkdir()
                (d / "ci-actuals.json").write_text(
                    json.dumps(
                        {
                            "sha": sha,
                            "workflow": "CI",
                            "jobs": [
                                {
                                    "lane_id": "merge_gate_shards",
                                    "gate_name": "fmt",
                                    "actual_lem": lem,
                                }
                            ],
                        }
                    ),
                    encoding="utf-8",
                )

            samples, stats = aggregate_lane_history.collect_actuals(
                actuals_dir=actuals,
                window_days=14,
                allowed_lanes={"merge_gate_shards"},
            )

        self.assertEqual([10.0, 20.0], sorted(samples["merge_gate_shards"]))
        self.assertEqual(2, stats["lane_executions"])

    def test_lane_can_calibrate_above_its_static_floor(self) -> None:
        """The harm the grouping prevents: a lane that can never learn upward.

        Five runs of a lane whose gates each cost less than the 24-LEM floor
        but which together cost 30. Per-gate sampling would put p50 at 10, lose
        to the floor in `max(static_floor, p50 * 1.15)`, and report 24 forever
        even though the lane really costs 30.
        """
        with tempfile.TemporaryDirectory() as tmp:
            actuals = Path(tmp)
            for run in range(5):
                d = actuals / f"run-{run}"
                d.mkdir()
                (d / "ci-actuals.json").write_text(
                    json.dumps(
                        {
                            "sha": f"sha{run}",
                            "workflow": "CI",
                            "jobs": [
                                {
                                    "lane_id": "merge_gate_shards",
                                    "gate_name": g,
                                    "actual_lem": 10.0,
                                }
                                for g in ("a", "b", "c")
                            ],
                        }
                    ),
                    encoding="utf-8",
                )

            samples, _stats = aggregate_lane_history.collect_actuals(
                actuals_dir=actuals,
                window_days=14,
                allowed_lanes={"merge_gate_shards"},
            )
            history = aggregate_lane_history.build_history(
                samples=samples,
                floors={"merge_gate_shards": 24.0},
                window_days=14,
            )

        lane = history["lanes"]["merge_gate_shards"]
        self.assertEqual(5, lane["samples"], "five runs, not fifteen gates")
        self.assertTrue(lane["learned"])
        self.assertEqual(30.0, lane["p50"], "the lane's real cost, not one gate's")
        # p50 * 1.15 must now beat the floor, so the lane can calibrate upward.
        self.assertGreater(lane["p50"] * 1.15, lane["static_floor"])

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
                allowed_lanes={"check_all_targets", "docs_gate"},
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
                allowed_lanes={"coverage"},
            )

        self.assertEqual({"coverage": [7.0]}, samples)
        self.assertEqual(1, stats["accepted_samples"])

    def test_main_fails_loudly_when_no_sample_maps_to_a_lane(self) -> None:
        """Samples arrived carrying lane_ids, none attributed: exit non-zero.

        Before #6217 this wrote a valid-looking all-zero history and returned
        success, which is indistinguishable from "no data" to every consumer.

        The artifact carries a `lane_id`, so this is the live mapping failure
        rather than the rollout window, and it fails regardless of date. The
        rollout counterpart is
        `test_main_warns_but_succeeds_for_pre_wiring_artifacts`.
        """
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            actuals = root / "actuals"
            actuals.mkdir()
            (actuals / "ci-actuals.json").write_text(
                json.dumps(
                    {
                        "jobs": [
                            {
                                "lane_id": "not_a_real_lane",
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

        self.assertEqual(1, rc, "an all-unmapped run must not report success")
        self.assertEqual(0, history["validation"]["accepted_samples"])
        self.assertEqual(1, history["validation"]["unmapped_samples"])
        # The written history must not have grown a `fmt` lane.
        self.assertEqual(["merge_gate_shards"], sorted(history["lanes"]))

    def test_main_warns_but_succeeds_for_pre_wiring_artifacts(self) -> None:
        """End-to-end rollout case: gate-name-only artifacts warn, exit 0.

        Every artifact in the window predates `--lane-id`. That is mechanical
        and self-resolving, so it must not red a scheduled workflow for two
        weeks — a chronic red is an ignored red.
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

        # Guarded so this does not silently flip to a failure assertion the
        # day the deadline passes and start testing a different thing.
        if date.today() < aggregate_lane_history.LANE_ID_ROLLOUT_DEADLINE:
            self.assertEqual(0, rc, "the rollout window must not fail the workflow")
        else:
            self.assertEqual(1, rc, "past the deadline this must have become an error")
        self.assertEqual(0, history["validation"]["jobs_with_lane_id"])
        self.assertEqual(0, history["validation"]["accepted_samples"])

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

    # ------------------------------------------------------------------
    # #6217 rollout discrimination. "Nothing attributed" has two causes with
    # opposite correct responses, and collapsing them either hides the real
    # defect or ships a chronic red that trains everyone to ignore it.
    # ------------------------------------------------------------------

    @staticmethod
    def _stats(**over: object) -> dict:
        base = {
            "source_files": 1,
            "jobs_seen": 3,
            "jobs_with_sample": 3,
            "jobs_with_lane_id": 0,
            "accepted_samples": 0,
            "lane_executions": 0,
            "unmapped_samples": 3,
            "unmapped_keys": {"fmt": 2, "clippy_full": 1},
        }
        base.update(over)
        return base

    def test_verdict_is_quiet_when_nothing_arrived(self) -> None:
        code, msg = aggregate_lane_history.attribution_verdict(
            self._stats(jobs_with_sample=0, unmapped_samples=0, unmapped_keys={}),
            today=date(2026, 8, 10),
        )
        self.assertEqual(0, code)
        self.assertIsNone(msg)

    def test_verdict_is_quiet_when_samples_attributed(self) -> None:
        code, msg = aggregate_lane_history.attribution_verdict(
            self._stats(
                accepted_samples=3,
                lane_executions=1,
                unmapped_samples=0,
                unmapped_keys={},
            ),
            today=date(2026, 8, 10),
        )
        self.assertEqual(0, code)
        self.assertIsNone(msg)

    def test_verdict_warns_during_rollout_when_no_artifact_has_lane_id(self) -> None:
        """Pre-wiring artifacts are mechanical and self-resolving: warn, do not fail."""
        code, msg = aggregate_lane_history.attribution_verdict(
            self._stats(jobs_with_lane_id=0),
            today=date(2026, 8, 10),
            deadline=date(2026, 9, 1),
        )
        self.assertEqual(0, code, "the rollout window must not fail the workflow")
        self.assertIsNotNone(msg)
        self.assertIn("::warning::", msg)
        # The expiry must be stated in the text, so a reader of the warning
        # knows it is time-boxed rather than permanent.
        self.assertIn("2026-09-01", msg)

    def test_verdict_fails_when_lane_ids_are_present_but_unmapped(self) -> None:
        """The real defect: wiring exists and still produces nothing usable.

        Fails from day one, inside the rollout window, because this is not
        the rollout condition.
        """
        code, msg = aggregate_lane_history.attribution_verdict(
            self._stats(jobs_with_lane_id=3),
            today=date(2026, 8, 10),
            deadline=date(2026, 9, 1),
        )
        self.assertEqual(1, code)
        self.assertIn("::error::", msg)
        self.assertIn("not the rollout window", msg)

    def test_verdict_fails_after_the_rollout_deadline(self) -> None:
        """The warn expires, so a never-wired workflow cannot warn forever."""
        code, msg = aggregate_lane_history.attribution_verdict(
            self._stats(jobs_with_lane_id=0),
            today=date(2026, 9, 1),
            deadline=date(2026, 9, 1),
        )
        self.assertEqual(1, code, "the rollout grace must expire on the deadline")
        self.assertIn("::error::", msg)
        self.assertIn("rollout window closed", msg)

    def test_rollout_deadline_is_in_the_future_relative_to_the_change(self) -> None:
        """Guards against shipping a grace period that is already expired."""
        self.assertGreater(
            aggregate_lane_history.LANE_ID_ROLLOUT_DEADLINE,
            date(2026, 8, 8),
            "the rollout deadline must postdate the change that introduces it",
        )

    def test_static_floors_reads_lane_base_lem_values(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            lanes = Path(tmp) / "ci-lanes.toml"
            lanes.write_text(
                """
[lane.rust-small]
base_lem = 20

[lane.docs]
base_lem = 2.5
""",
                encoding="utf-8",
            )

            floors = aggregate_lane_history.static_floors(lanes)

        self.assertEqual({"docs": 2.5, "rust-small": 20.0}, floors)

    def test_lane_sample_cap_discards_only_excess_samples(self) -> None:
        original_cap = aggregate_lane_history.MAX_SAMPLES_PER_LANE
        aggregate_lane_history.MAX_SAMPLES_PER_LANE = 2
        try:
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                for index, lem in enumerate((1, 2, 3), start=1):
                    receipt_sha = f"{index:040x}"
                    write_run(
                        root,
                        run_id=index,
                        marker_sha=receipt_sha,
                        receipt_sha=receipt_sha,
                        jobs=[{"lane_id": "meta", "actual_lem": lem}],
                    )
                samples, stats = collect(root)
        finally:
            aggregate_lane_history.MAX_SAMPLES_PER_LANE = original_cap

        self.assertEqual({"meta": [1.0, 2.0]}, samples)
        self.assertEqual(1, stats["rejected"].get("lane_sample_cap"))

    def test_build_history_includes_policy_lanes_without_samples(self) -> None:
        history = aggregate_lane_history.build_history(
            samples={"rust-small": [10, 20, 30, 40, 50]},
            floors={"docs": 3, "rust-small": 15},
            window_days=14,
            validation={"accepted_samples": 5},
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
        self.assertEqual({"accepted_samples": 5}, history["validation"])

    def test_main_writes_validation_summary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            actuals = root / "actuals"
            actuals.mkdir()
            write_run(
                actuals,
                jobs=[{"lane_id": "rust-small", "actual_lem": 12}],
            )
            lanes = root / "ci-lanes.toml"
            lanes.write_text(
                "[lane.rust-small]\nbase_lem = 10\n[lane.docs]\nbase_lem = 2\n",
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
                    "--require-trusted-markers",
                    "--repository",
                    REPOSITORY,
                    "--default-branch",
                    DEFAULT_BRANCH,
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
        self.assertEqual(1, history["validation"]["accepted_samples"])
        self.assertEqual(
            {
                "lanes": 2,
                "learned": 0,
                "window_days": 14,
                "accepted_samples": 1,
                "rejected_samples": 0,
                "source_runs": 1,
                "unmapped_samples": 0,
            },
            printed,
        )


class PayloadOracleTests(unittest.TestCase):
    """Negative controls for the checked-in payload oracle (#11731).

    The validator must go red on each defect class the issue names; a gate
    that cannot fail is not an oracle.
    """

    @staticmethod
    def clean_payload() -> dict[str, object]:
        return {
            "schema_version": 1,
            "generated_at": "2026-08-20T05:23:25Z",
            "window_days": 14,
            "min_samples_for_learned": 5,
            "lane_count": 2,
            "lanes": {
                "merge_gate_shards": {
                    "samples": 6,
                    "static_floor": 24.0,
                    "learned": True,
                    "p50": 24.6,
                    "p90": 26.0,
                    "p95": 26.5,
                    "min": 21.7,
                    "max": 28.4,
                    "mean": 24.7,
                },
                "conflict_markers": {"samples": 0, "static_floor": 1.0, "learned": False},
            },
            "validation": {
                "files_seen": 10,
                "files_accepted": 10,
                "jobs_seen": 40,
                "accepted_samples": 6,
                "jobs_with_sample": 6,
                "jobs_with_lane_id": 6,
                "lane_executions": 6,
                "unmapped_samples": 0,
                "unmapped_keys": {},
                "rejected": {},
                "source_run_count": 2,
                "source_run_ids": [111, 222],
            },
        }

    def test_clean_payload_validates(self) -> None:
        self.assertEqual([], aggregate_lane_history.validate_history_payload(self.clean_payload()))

    def test_builder_output_validates(self) -> None:
        history = aggregate_lane_history.build_history(
            samples={"merge_gate_shards": [24.0, 25.0, 26.0, 27.0, 28.0, 24.5]},
            floors={"merge_gate_shards": 24.0, "conflict_markers": 1.0},
            window_days=14,
            validation={
                "files_seen": 1,
                "files_accepted": 1,
                "jobs_seen": 6,
                "accepted_samples": 6,
                "jobs_with_sample": 6,
                "jobs_with_lane_id": 6,
                "lane_executions": 6,
                "unmapped_samples": 0,
                "unmapped_keys": {},
                "rejected": {},
                "source_run_count": 1,
                "source_run_ids": [111],
            },
        )
        self.assertEqual([], aggregate_lane_history.validate_history_payload(history))

    def test_corrupt_percentile_ordering_fails(self) -> None:
        payload = self.clean_payload()
        payload["lanes"]["merge_gate_shards"]["p95"] = 24.0  # below p50
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("p90 > p95" in v for v in violations), violations)

    def test_learned_disagrees_with_samples_fails(self) -> None:
        payload = self.clean_payload()
        payload["min_samples_for_learned"] = 5
        payload["lanes"]["merge_gate_shards"]["samples"] = 2
        payload["lanes"]["merge_gate_shards"]["learned"] = True
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("learned=True disagrees" in v for v in violations), violations)

    def test_counter_identity_mismatch_fails(self) -> None:
        payload = self.clean_payload()
        payload["validation"]["source_run_count"] = 3
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("source_run_count" in v for v in violations), violations)

    def test_bool_source_run_id_fails(self) -> None:
        payload = self.clean_payload()
        payload["validation"]["source_run_ids"] = [True, 222]
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("source_run_ids must be a list of ints" in v for v in violations), violations)

    def test_bool_source_run_count_fails(self) -> None:
        payload = self.clean_payload()
        payload["validation"]["source_run_count"] = True
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("source_run_count must be a non-negative int" in v for v in violations), violations)

    def test_lane_count_identity_mismatch_fails(self) -> None:
        payload = self.clean_payload()
        payload["lane_count"] = 3
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("lane_count" in v for v in violations), violations)

    def test_bool_lane_count_fails(self) -> None:
        payload = self.clean_payload()
        payload["lane_count"] = True
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("lane_count must be a non-negative int" in v for v in violations), violations)

    def test_unknown_lane_identity_fails_against_explicit_set(self) -> None:
        payload = self.clean_payload()
        payload["lanes"]["spoofed_lane"] = payload["lanes"].pop("merge_gate_shards")
        violations = aggregate_lane_history.validate_history_payload(
            payload,
            expected_lane_ids={"merge_gate_shards", "conflict_markers"},
        )
        self.assertTrue(any("unknown lane ids" in v for v in violations), violations)
        self.assertTrue(any("missing expected lane ids" in v for v in violations), violations)

    def test_percentile_fields_without_samples_fails(self) -> None:
        payload = self.clean_payload()
        payload["lanes"]["conflict_markers"]["p50"] = 1.0
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("percentile fields but samples == 0" in v for v in violations), violations)

    def test_non_finite_statistic_fails(self) -> None:
        payload = self.clean_payload()
        payload["lanes"]["merge_gate_shards"]["mean"] = float("inf")
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("mean must be finite" in v for v in violations), violations)

    def test_lane_executions_disagree_with_sample_sum_fails(self) -> None:
        payload = self.clean_payload()
        payload["validation"]["lane_executions"] = 7
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("lane_executions" in v and "sum of lane samples" in v for v in violations), violations)

    def test_capped_executions_validate_and_mismatch_fails(self) -> None:
        # Executions dropped by the per-lane sample cap still count in
        # lane_executions; the identity must include the capped count
        # (#11817 review).
        payload = self.clean_payload()
        payload["validation"]["rejected"] = {"lane_sample_cap": 2}
        payload["validation"]["lane_executions"] = 8
        self.assertEqual([], aggregate_lane_history.validate_history_payload(payload))

        payload["validation"]["lane_executions"] = 6  # forgot the capped 2
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("+ capped" in v for v in violations), violations)

    def test_negative_capped_count_fails(self) -> None:
        payload = self.clean_payload()
        payload["validation"]["rejected"] = {"lane_sample_cap": -1}
        violations = aggregate_lane_history.validate_history_payload(payload)
        self.assertTrue(any("lane_sample_cap" in v for v in violations), violations)

    def test_checked_in_history_validates(self) -> None:
        """The current committed payload must pass its own oracle."""
        repo_root = SCRIPT_PATH.parent.parent.parent
        checked_in = repo_root / ".ci" / "metrics" / "ci-lane-history.json"
        if not checked_in.exists():
            self.skipTest(f"{checked_in} not present")
        data = json.loads(checked_in.read_text(encoding="utf-8"))
        expected_lane_ids = set(
            aggregate_lane_history.static_floors(repo_root / "policy" / "ci-lanes.toml")
        )
        self.assertEqual(
            [],
            aggregate_lane_history.validate_history_payload(
                data, expected_lane_ids=expected_lane_ids
            ),
        )


if __name__ == "__main__":
    unittest.main()
