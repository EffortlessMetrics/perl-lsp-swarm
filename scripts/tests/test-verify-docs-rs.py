#!/usr/bin/env python3
"""
Cheap guard for scripts/verify-docs-rs.sh (#14987).

Asserts:
1. ``verify-docs-rs.sh --list`` matches publish-topo.derive_publish_list from live metadata.
2. Every listed crate has a directory under crates/.

On main before the fix, the hand-maintained ALL_CRATES literal includes names that
are absent from crates/, so check (2) fails once --list reflects metadata.

Run: python3 scripts/tests/test-verify-docs-rs.py
"""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import unittest

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
SCRIPTS_DIR = os.path.join(REPO_ROOT, "scripts")
VERIFY_SCRIPT = os.path.join(SCRIPTS_DIR, "verify-docs-rs.sh")
CARGO_METADATA_TIMEOUT_SECONDS = 120
VERIFY_SCRIPT_TIMEOUT_SECONDS = 60

_spec = importlib.util.spec_from_file_location(
    "publish_topo", os.path.join(SCRIPTS_DIR, "publish-topo.py")
)
assert _spec is not None and _spec.loader is not None
publish_topo = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(publish_topo)  # type: ignore[union-attr]


def _cargo_metadata() -> dict:
    try:
        completed = subprocess.run(
            [
                "cargo",
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
                os.path.join(REPO_ROOT, "Cargo.toml"),
            ],
            check=True,
            capture_output=True,
            text=True,
            timeout=CARGO_METADATA_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as exc:
        raise AssertionError(
            f"cargo metadata timed out after {CARGO_METADATA_TIMEOUT_SECONDS}s"
        ) from exc
    return json.loads(completed.stdout)


def _list_from_script() -> list[str]:
    try:
        completed = subprocess.run(
            ["bash", VERIFY_SCRIPT, "--list"],
            check=True,
            capture_output=True,
            text=True,
            cwd=REPO_ROOT,
            timeout=VERIFY_SCRIPT_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as exc:
        raise AssertionError(
            f"verify-docs-rs.sh --list timed out after {VERIFY_SCRIPT_TIMEOUT_SECONDS}s"
        ) from exc
    lines = [line.strip() for line in completed.stdout.splitlines() if line.strip()]
    return lines


class VerifyDocsRsTests(unittest.TestCase):
    def test_list_matches_metadata_publishable_set(self) -> None:
        meta = _cargo_metadata()
        expected = publish_topo.derive_publish_list(meta)
        actual = _list_from_script()
        self.assertEqual(actual, expected)

    def test_every_listed_crate_exists_under_crates(self) -> None:
        for name in _list_from_script():
            crate_dir = os.path.join(REPO_ROOT, "crates", name)
            self.assertTrue(
                os.path.isdir(crate_dir),
                f"verify-docs-rs --list names missing crate directory: {name}",
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)
