#!/usr/bin/env python3
"""Focused admission tests for scripts/release_topology_json.py."""

import importlib.util
import sys
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("release_topology_json.py")
sys.path.insert(0, str(MODULE_PATH.parent))
SPEC = importlib.util.spec_from_file_location("release_topology_json", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReleaseTopologyJsonTests(unittest.TestCase):
    def test_admits_exact_schema_versions(self):
        for version in ("1", "2"):
            decoded = MODULE.load_topology_json('{"schema": %s}' % version)
            self.assertEqual(decoded, {"schema": int(version)})

    def test_rejects_missing_or_duplicate_schema(self):
        for raw in ('{"targets": []}', '{"schema": 1, "schema": 2}', '{"schema": 3}'):
            with self.assertRaises(ValueError, msg=raw):
                MODULE.load_topology_json(raw)

    def test_rejects_non_json_constants(self):
        for token in ("NaN", "Infinity", "-Infinity"):
            raw = '{"schema": 2, "unchecked": %s}' % token
            with self.assertRaises(ValueError, msg=token):
                MODULE.load_topology_json(raw)

    def test_rejects_constant_in_schema_position(self):
        with self.assertRaises(ValueError):
            MODULE.load_topology_json('{"schema": NaN}')


if __name__ == "__main__":
    unittest.main()
