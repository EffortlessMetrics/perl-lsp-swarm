#!/usr/bin/env python3
"""Byte-level ordering contracts; input arrays and source paths stay fixed.

These are local reporter proofs, not hosted execution or cost measurements.
"""
from __future__ import annotations

import importlib.util
import itertools
import json
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from unittest.mock import patch


def load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


junit = load("ordering_junit", "receipts-to-junit.py")
actuals = load("ordering_actuals", "emit_ci_actuals.py")


def reverse_maps(value):
    """Change object insertion order recursively, preserving semantic array order."""
    if isinstance(value, dict):
        return {key: reverse_maps(item) for key, item in reversed(value.items())}
    if isinstance(value, list):
        return [reverse_maps(item) for item in value]
    return value


def envelope(name: str, status: str, **fields):
    return {"test_results_schema": 1, "test_results": [
        {"suite": "ordering", "name": name, "status": status,
         "duration_ms": 125, **fields}
    ]}


class ReportOrderingTests(unittest.TestCase):
    def junit_variants(self, root, documents):
        paths = [root / name for name in documents]
        for reverse in (False, True):
            for name, value in documents.items():
                (root / name).write_text(json.dumps(reverse_maps(value) if reverse else value))
            for order in itertools.permutations(paths):
                # Patch the filesystem boundary, not the reporter's sorted result.
                # Do not yield inside subTest: a caller assertion closes this
                # generator, and subTest would misclassify GeneratorExit as an error.
                with patch.object(Path, "glob", return_value=iter(order)):
                    tree, *counts = junit.receipts_to_junit(root, "instrument")
                yield ET.tostring(tree, encoding="utf-8"), tuple(counts), tree

    def test_junit_bytes_ignore_enumeration_and_nested_map_order(self):
        documents = {
            "z.json": envelope("skip", "skip", message="not applicable"),
            "a.json": envelope("pass", "pass", parameters={
                "z": [2, 1], "a": {"second": 2, "first": 1}}),
            "m.json": envelope("fail", "fail", message="assertion", file="case.pl", line=4),
        }
        with tempfile.TemporaryDirectory() as tmp:
            baseline = None
            for encoded, counts, tree in self.junit_variants(Path(tmp), documents):
                self.assertEqual((3, 1, 0, 1), counts)
                cases = tree.findall("./testsuite/testcase")
                self.assertEqual(3, len(cases))
                self.assertIn('"a":{"first":1,"second":2},"z":[2,1]', cases[0].get("name"))
                self.assertEqual(["fail", "skip"], [case.get("name") for case in cases[1:]])
                self.assertEqual("assertion\nLocation: case.pl:4", cases[1].findtext("failure"))
                baseline = encoded if baseline is None else baseline
                self.assertEqual(baseline, encoded)

    def test_duplicate_identity_refusal_is_order_independent(self):
        parameters = {"z": 1, "a": {"y": 2, "b": 3}}
        documents = {
            "z.json": envelope("same", "pass", parameters=reverse_maps(parameters)),
            "a.json": envelope("same", "pass", parameters=parameters),
            "m.json": envelope("other", "pass"),
        }
        with tempfile.TemporaryDirectory() as tmp:
            baseline = None
            for encoded, counts, tree in self.junit_variants(Path(tmp), documents):
                self.assertEqual((0, 0, 1, 0), counts)
                self.assertEqual([], tree.findall("./testsuite/testcase"))
                error = tree.findtext("./testsuite/system-err")
                self.assertIn("z.json: ValueError: duplicate atomic identity", error)
                self.assertIn("already emitted by", error)
                self.assertIn("a.json", error)
                baseline = encoded if baseline is None else baseline
                self.assertEqual(baseline, encoded)

    def test_multiple_invalid_inputs_have_stable_diagnostic_order(self):
        documents = {"z.json": {"unknown": True}, "a.json": {"unknown": False},
                     "m.json": envelope("valid", "pass")}
        with tempfile.TemporaryDirectory() as tmp:
            baseline = None
            for encoded, counts, tree in self.junit_variants(Path(tmp), documents):
                self.assertEqual((0, 0, 2, 0), counts)
                self.assertEqual([], tree.findall("./testsuite/testcase"))
                errors = tree.findtext("./testsuite/system-err").splitlines()
                self.assertTrue(errors[0].startswith("a.json: UnrecognizedFormat:"))
                self.assertTrue(errors[1].startswith("z.json: UnrecognizedFormat:"))
                baseline = encoded if baseline is None else baseline
                self.assertEqual(baseline, encoded)

    def test_actuals_bytes_ignore_recursive_enumeration_and_policy_map_order(self):
        documents = {
            "z.json": {"gate_name": "z", "status": "skip", "duration_ms": None},
            "nested/a.json": {"gates": [{"gate_name": "a", "status": "pass", "duration_ms": 60000}]},
            "m.json": {"gate_name": "m", "status": "fail", "duration_ms": 30000},
        }
        lanes = {"z": {"base_lem": 3}, "a": {"base_lem": 1}, "m": {"base_lem": 2}}
        multipliers = {"linux": 1.0, "unused": 2.0}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            paths = [root / name for name in documents]
            baseline = None
            for reverse in (False, True):
                for name, value in documents.items():
                    path = root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(json.dumps(reverse_maps(value) if reverse else value))
                for order in itertools.permutations(paths):
                    with self.subTest(reverse_maps=reverse, enumeration=[p.name for p in order]):
                        with patch.object(Path, "rglob", return_value=iter(order)):
                            receipts = actuals.collect_receipts(root)
                        result = actuals.emit_actuals(
                            receipts=receipts, multipliers=reverse_maps(multipliers) if reverse else multipliers,
                            lanes=reverse_maps(lanes) if reverse else lanes, workflow="fixture", sha="a" * 40,
                            pr=None, runner_default="linux")
                        self.assertEqual(["m", "a", "z"], [job["gate_name"] for job in result["jobs"]])
                        self.assertEqual(["fail", "pass", "skip"], [job["status"] for job in result["jobs"]])
                        self.assertEqual({"actual_lem": 1.5, "estimated_lem": 6.0, "delta_lem": -4.5}, result["totals"])
                        self.assertIsNone(result["jobs"][2]["actual_lem"])
                        encoded = json.dumps(result, indent=2) + "\n"
                        baseline = encoded if baseline is None else baseline
                        self.assertEqual(baseline, encoded)

    def test_parameter_array_order_is_semantic(self):
        def render(values):
            row = envelope("array", "pass", parameters={"values": values})["test_results"][0]
            return ET.tostring(junit.atomic_to_testcase(junit.validate_atomic_entry(row)))
        self.assertNotEqual(render([1, 2]), render([2, 1]))


if __name__ == "__main__":
    unittest.main()
