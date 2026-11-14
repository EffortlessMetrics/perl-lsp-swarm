#!/usr/bin/env python3
"""Tests for scripts/ci/publish_crates_json.py and publish_crates_topo envelopes."""

from __future__ import annotations

import importlib.util
import io
import json
import os
import subprocess
import sys
import unittest
from contextlib import redirect_stderr
from pathlib import Path

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPOSITORY_ROOT / "scripts"
CLI_PATH = REPOSITORY_ROOT / "scripts" / "ci" / "publish_crates_json.py"


def _load_publish_topo():
    spec = importlib.util.spec_from_file_location(
        "publish_topo",
        SCRIPTS_DIR / "publish-topo.py",
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


publish_topo = _load_publish_topo()


class TestPublishCratesTopoEnvelope(unittest.TestCase):
    def test_encode_includes_schema_version(self) -> None:
        payload = publish_topo.encode_publish_crates_payload(
            [{"name": "demo", "version": "1.2.3"}]
        )
        self.assertEqual(
            payload["schema_version"],
            publish_topo.PUBLISH_CRATES_TOPO_SCHEMA_VERSION,
        )
        self.assertEqual(payload["crates"][0]["name"], "demo")

    def test_bare_array_rejected_with_clear_message(self) -> None:
        with self.assertRaises(ValueError) as ctx:
            publish_topo.load_publish_crates_payload([{"name": "a", "version": "0.1.0"}])
        message = str(ctx.exception)
        self.assertIn("schema_version", message)
        self.assertIn("bare JSON array", message)

    def test_unsupported_schema_version_rejected(self) -> None:
        payload = {
            "schema_version": "publish_crates_topo.v2",
            "crates": [{"name": "a", "version": "0.1.0"}],
        }
        with self.assertRaises(ValueError) as ctx:
            publish_topo.load_publish_crates_payload(payload)
        self.assertIn("publish_crates_topo.v2", str(ctx.exception))

    def test_cli_count_requires_envelope(self) -> None:
        result = subprocess.run(
            [sys.executable, str(CLI_PATH), "--count"],
            input=json.dumps([{"name": "a", "version": "0.1.0"}]),
            text=True,
            capture_output=True,
            check=False,
            cwd=REPOSITORY_ROOT,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("schema_version", result.stderr)


class TestPublishCratesJsonCli(unittest.TestCase):
    def _run_cli(self, *args: str, payload: object, env: dict | None = None) -> subprocess.CompletedProcess[str]:
        environment = os.environ.copy()
        if env:
            environment.update(env)
        return subprocess.run(
            [sys.executable, str(CLI_PATH), *args],
            input=json.dumps(payload),
            text=True,
            capture_output=True,
            check=False,
            cwd=REPOSITORY_ROOT,
            env=environment,
        )

    def test_count_and_list(self) -> None:
        payload = publish_topo.encode_publish_crates_payload(
            [
                {"name": "alpha", "version": "1.0.0"},
                {"name": "beta", "version": "1.0.0"},
            ]
        )
        count = self._run_cli("--count", payload=payload)
        self.assertEqual(count.returncode, 0, count.stderr)
        self.assertEqual(count.stdout.strip(), "2")

        listed = self._run_cli("--list", payload=payload)
        self.assertEqual(listed.returncode, 0, listed.stderr)
        self.assertEqual(
            listed.stdout.splitlines(),
            ["alpha 1.0.0", "beta 1.0.0"],
        )

    def test_validate_target_version(self) -> None:
        payload = publish_topo.encode_publish_crates_payload(
            [{"name": "alpha", "version": "1.0.0"}]
        )
        ok = self._run_cli(
            "--validate-target-version",
            payload=payload,
            env={"TARGET_VERSION": "1.0.0"},
        )
        self.assertEqual(ok.returncode, 0, ok.stderr)

        bad = self._run_cli(
            "--validate-target-version",
            payload=payload,
            env={"TARGET_VERSION": "9.9.9"},
        )
        self.assertNotEqual(bad.returncode, 0)
        self.assertIn("do not match release target", bad.stderr)


if __name__ == "__main__":
    unittest.main()
