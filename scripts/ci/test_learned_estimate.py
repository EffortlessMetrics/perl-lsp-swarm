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

from learned_estimate import SCHEMA_VERSION, emit_stdout, estimate_for, main  # noqa: E402


def _run_main(history_path: Path, lane: str = "rust_small") -> dict:
    old_argv = sys.argv
    try:
        sys.argv = [
            "learned_estimate.py",
            "--history",
            str(history_path),
            "--lane",
            lane,
        ]
        out = io.StringIO()
        with redirect_stdout(out):
            rc = main()
    finally:
        sys.argv = old_argv
    payload = json.loads(out.getvalue())
    if rc != 0:
        raise AssertionError(f"main() exited {rc} with {payload!r}")
    return payload


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

            missing = _run_main(missing_history)
            invalid = _run_main(invalid_history)

        self.assertFalse(missing["learned"])
        self.assertIn("not present", missing["reason"])
        self.assertFalse(invalid["learned"])
        self.assertIn("error", invalid)


class StdoutSchemaVersionTests(unittest.TestCase):
    """`learned_estimate.py` stdout is a versioned wire object (#15286).

    `pr_plan.py` does not read this stdout; it re-implements the estimate
    model against the history file. The producer field still has to exist so a
    later consumer can refuse an unfamiliar shape instead of scraping keys.
    """

    EXPECTED_SCHEMA = SCHEMA_VERSION
    LANE = "rust_small"

    def _write_history(self, tmp: str, payload: object) -> Path:
        path = Path(tmp) / "ci-lane-history.json"
        path.write_text(json.dumps(payload), encoding="utf-8")
        return path

    def _assert_schema(self, emitted: dict) -> None:
        self.assertIsInstance(emitted, dict)
        self.assertEqual(self.EXPECTED_SCHEMA, emitted.get("schema_version"))
        self.assertEqual(self.LANE, emitted["lane"])

    def test_missing_history_stdout_carries_schema_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            emitted = _run_main(Path(tmp) / "absent.json")
        self._assert_schema(emitted)
        self.assertFalse(emitted["learned"])
        self.assertIn("not present", emitted["reason"])

    def test_decode_error_stdout_carries_schema_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "broken.json"
            path.write_text("{", encoding="utf-8")
            emitted = _run_main(path)
        self._assert_schema(emitted)
        self.assertFalse(emitted["learned"])
        self.assertIn("error", emitted)

    def test_learned_success_stdout_carries_schema_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_history(
                tmp,
                {
                    "schema_version": 1,
                    "min_samples_for_learned": 5,
                    "lanes": {
                        self.LANE: {
                            "learned": True,
                            "static_floor": 10.0,
                            "p50": 20.0,
                            "p90": 30.0,
                            "p95": 35.0,
                            "samples": 12,
                        }
                    },
                },
            )
            emitted = _run_main(path)
        self._assert_schema(emitted)
        self.assertTrue(emitted["learned"])
        self.assertEqual(23.0, emitted["estimate"])
        self.assertEqual("p50 * 1.15", emitted["estimate_source"])

    def test_too_few_samples_stdout_still_carries_schema_version(self) -> None:
        """Opposite control: a non-learned success path is still versioned.

        If only the happy-path dump site grew the field, missing-history and
        decode-error tests could pass while this advisory object stayed bare.
        """
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_history(
                tmp,
                {
                    "schema_version": 1,
                    "min_samples_for_learned": 7,
                    "lanes": {
                        self.LANE: {
                            "learned": False,
                            "static_floor": 12.0,
                            "samples": 3,
                        }
                    },
                },
            )
            emitted = _run_main(path)
        self._assert_schema(emitted)
        self.assertFalse(emitted["learned"])
        self.assertEqual(12.0, emitted["estimate"])

    def test_non_object_history_emits_versioned_json_instead_of_traceback(self) -> None:
        # json.loads succeeds for a list. estimate_for then calls .get and
        # would traceback, so stdout would not be JSON at all. The producer
        # contract is "stdout is versioned JSON", including this shape.
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_history(tmp, [self.LANE])
            emitted = _run_main(path)
        self._assert_schema(emitted)
        self.assertFalse(emitted["learned"])
        self.assertIn("JSON object", emitted["reason"])
        self.assertIn("list", emitted["reason"])

    def test_stdout_schema_token_is_not_the_history_file_integer(self) -> None:
        """A consumer that checks `schema_version == 1` must not match us.

        The history file uses integer 1. This producer's token is a named
        string so the two contracts cannot be confused.
        """
        self.assertEqual("learned_estimate.v1", SCHEMA_VERSION)
        self.assertIsInstance(SCHEMA_VERSION, str)
        self.assertNotEqual(1, SCHEMA_VERSION)

    def test_emit_stdout_stamps_producer_schema_and_ignores_payload_override(
        self,
    ) -> None:
        """The emit helper is the single authority for the producer field.

        A payload that already carries the history-file integer, or a stale
        string, must not leak onto stdout in place of SCHEMA_VERSION.
        """
        out = io.StringIO()
        with redirect_stdout(out):
            emit_stdout(
                {
                    "schema_version": 1,
                    "lane": self.LANE,
                    "learned": False,
                    "reason": "override-check",
                }
            )
        emitted = json.loads(out.getvalue())
        self.assertEqual(SCHEMA_VERSION, emitted["schema_version"])
        self.assertEqual(self.LANE, emitted["lane"])
        self.assertEqual("override-check", emitted["reason"])
        self.assertEqual(
            ["schema_version", "lane", "learned", "reason"],
            list(emitted.keys()),
        )

    def test_estimate_for_does_not_stamp_the_wire_schema(self) -> None:
        """The in-process model is not the stdout envelope.

        Stamping `schema_version` inside estimate_for would leave the
        missing-file and decode-error dump sites unversioned.
        """
        estimate = estimate_for("rust_small", {"lanes": {}})
        self.assertNotIn("schema_version", estimate)
        self.assertFalse(estimate["learned"])

    def test_json_dumps_of_stdout_live_in_one_emitter(self) -> None:
        """A fourth print(json.dumps(...)) cannot silently omit the field."""
        import ast

        source = Path(__file__).with_name("learned_estimate.py").read_text(
            encoding="utf-8"
        )
        tree = ast.parse(source)
        dump_funcs: list[str] = []

        class Visitor(ast.NodeVisitor):
            def __init__(self) -> None:
                self.stack: list[str] = []

            def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
                self.stack.append(node.name)
                self.generic_visit(node)
                self.stack.pop()

            visit_AsyncFunctionDef = visit_FunctionDef

            def visit_Call(self, node: ast.Call) -> None:
                func = node.func
                if (
                    isinstance(func, ast.Attribute)
                    and func.attr == "dumps"
                    and isinstance(func.value, ast.Name)
                    and func.value.id == "json"
                ):
                    dump_funcs.append(self.stack[-1] if self.stack else "<module>")
                self.generic_visit(node)

        Visitor().visit(tree)
        self.assertEqual(["emit_stdout"], dump_funcs)


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
