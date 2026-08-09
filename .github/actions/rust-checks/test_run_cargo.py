#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("run_cargo.py")
SPEC = importlib.util.spec_from_file_location("rust_checks_run_cargo", MODULE_PATH)
assert SPEC and SPEC.loader
run_cargo = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = run_cargo
SPEC.loader.exec_module(run_cargo)


class RunCargoTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.argv_path = self.root / "argv.json"
        self.cargo = self.root / "cargo"
        self.cargo.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, sys\n"
            "with open(os.environ['FAKE_ARGV_PATH'], 'w', encoding='utf-8') as out:\n"
            "    json.dump(sys.argv[1:], out)\n"
            "raise SystemExit(int(os.environ.get('FAKE_CARGO_EXIT', '0')))\n",
            encoding="utf-8",
        )
        self.cargo.chmod(self.cargo.stat().st_mode | stat.S_IXUSR)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def fake_environment(self, **values: str) -> dict[str, str]:
        return {
            "FAKE_ARGV_PATH": str(self.argv_path),
            "FAKE_CARGO_EXIT": "0",
            **values,
        }

    def read_argv(self) -> list[str]:
        return json.loads(self.argv_path.read_text(encoding="utf-8"))

    def test_fmt_uses_the_fixed_repository_command(self) -> None:
        status = run_cargo.run_command(
            "fmt", environment=self.fake_environment(), executable=str(self.cargo)
        )
        self.assertEqual(status, 0)
        self.assertEqual(self.read_argv(), ["xtask", "fmt", "--check"])

    def test_shell_like_quoting_preserves_one_argument(self) -> None:
        environment = self.fake_environment(
            CLIPPY_ARGS='--features "feature one" --message-format "json rendered"'
        )
        status = run_cargo.run_command(
            "clippy", environment=environment, executable=str(self.cargo)
        )
        self.assertEqual(status, 0)
        self.assertEqual(
            self.read_argv(),
            ["clippy", "--features", "feature one", "--message-format", "json rendered"],
        )

    def test_escaped_space_and_empty_argument_are_preserved(self) -> None:
        environment = self.fake_environment(TEST_ARGS=r'--config path\ with\ spaces ""')
        status = run_cargo.run_command(
            "test", environment=environment, executable=str(self.cargo)
        )
        self.assertEqual(status, 0)
        self.assertEqual(self.read_argv(), ["test", "--config", "path with spaces", ""])

    def test_shell_metacharacters_remain_inert_arguments(self) -> None:
        sentinel = self.root / "must-not-exist"
        environment = self.fake_environment(
            TEST_ARGS=f'--filter "; touch {sentinel}; #"'
        )
        status = run_cargo.run_command(
            "test", environment=environment, executable=str(self.cargo)
        )
        self.assertEqual(status, 0)
        self.assertFalse(sentinel.exists())
        self.assertEqual(
            self.read_argv(), ["test", "--filter", f"; touch {sentinel}; #"]
        )

    def test_production_clippy_uses_its_own_argument_channel(self) -> None:
        environment = self.fake_environment(CLIPPY_PROD_ARGS="--workspace --bins")
        status = run_cargo.run_command(
            "clippy-prod", environment=environment, executable=str(self.cargo)
        )
        self.assertEqual(status, 0)
        self.assertEqual(self.read_argv(), ["clippy", "--workspace", "--bins"])

    def test_main_always_publishes_a_failing_exit_status(self) -> None:
        output = self.root / "github-output"
        environment = {
            "PATH": f"{self.root}{os.pathsep}{os.environ.get('PATH', '')}",
            "FAKE_ARGV_PATH": str(self.argv_path),
            "FAKE_CARGO_EXIT": "23",
            "CLIPPY_ARGS": "--workspace",
            "GITHUB_OUTPUT": str(output),
        }
        with mock.patch.dict(os.environ, environment, clear=True):
            status = run_cargo.main(["--kind", "clippy"])
        self.assertEqual(status, 23)
        self.assertEqual(output.read_text(encoding="utf-8"), "status=23\n")

    def test_main_publishes_status_two_for_malformed_arguments(self) -> None:
        output = self.root / "github-output"
        environment = {
            "CLIPPY_ARGS": "'unterminated",
            "GITHUB_OUTPUT": str(output),
        }
        with mock.patch.dict(os.environ, environment, clear=True):
            status = run_cargo.main(["--kind", "clippy"])
        self.assertEqual(status, 2)
        self.assertEqual(output.read_text(encoding="utf-8"), "status=2\n")

    def test_missing_cargo_is_status_127(self) -> None:
        status = run_cargo.run_command(
            "test",
            environment=self.fake_environment(TEST_ARGS=""),
            executable=str(self.root / "missing"),
        )
        self.assertEqual(status, 127)

    def test_nul_is_rejected_before_execution(self) -> None:
        with self.assertRaisesRegex(ValueError, "NUL"):
            run_cargo.build_command("test", {"TEST_ARGS": "--filter bad\x00value"})


if __name__ == "__main__":
    unittest.main()
