#!/usr/bin/env python3
"""Proof for scripts/ci/docker_publish_metadata.py.

The point of these tests is the *negative* direction. A hardening change that
only demonstrates "the safe version still works" proves nothing about the
boundary it claims to close, so the bulk of what follows asserts that unsafe
input is rejected and that a rejection cannot itself become a runner command.
"""

from __future__ import annotations

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("docker_publish_metadata.py")
SPEC = importlib.util.spec_from_file_location("docker_publish_metadata", MODULE_PATH)
assert SPEC and SPEC.loader
metadata = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = metadata
SPEC.loader.exec_module(metadata)


class DeriveRejectsUnsafeInput(unittest.TestCase):
    """Every value here must be refused before any tag is derived."""

    def assert_rejected(self, version: str) -> None:
        with self.assertRaises(metadata.InvalidVersion):
            metadata.derive(version)

    def test_rejects_workflow_command_injection(self) -> None:
        # The specific escape the maintainer flagged: a newline turns the
        # remainder into a second runner command.
        self.assert_rejected("1.2.3\n::add-mask::hunter2")
        self.assert_rejected("1.2.3\n::error::forged")
        self.assert_rejected("1.2.3\n::set-output name=version::9.9.9")
        self.assert_rejected("1.2.3\n::stop-commands::token")

    def test_rejects_github_output_injection(self) -> None:
        # A newline would otherwise append an attacker-chosen output line.
        self.assert_rejected("1.2.3\nis_stable=true")
        self.assert_rejected("1.2.3\r\nmajor=99")

    def test_rejects_shell_metacharacters(self) -> None:
        for version in (
            "1.2.3; curl evil.example",
            "1.2.3 && whoami",
            "1.2.3 | tee /tmp/x",
            "$(id)",
            "`id`",
            "1.2.3$(id)",
            "1.2.3'; echo pwned; '",
            '1.2.3"; echo pwned; "',
            "${IFS}1.2.3",
        ):
            with self.subTest(version=version):
                self.assert_rejected(version)

    def test_rejects_trailing_newline(self) -> None:
        # Regression: Python's '$' anchor also matches immediately before a
        # trailing newline, so a '$'-anchored grammar accepts "1.2.3\n" — the
        # leading half of a workflow-command injection payload. The grammar
        # must anchor with \Z.
        self.assert_rejected("1.2.3\n")
        self.assert_rejected("1.2.3-rc.1\n")

    def test_rejects_control_characters(self) -> None:
        self.assert_rejected("1.2.3\x00")
        self.assert_rejected("\x001.2.3")
        self.assert_rejected("1.2.3\r")
        self.assert_rejected("1.2.3\t")

    def test_rejects_registry_path_escapes(self) -> None:
        # A tag that escapes its repository would retag an unrelated image.
        self.assert_rejected("1.2.3/../../latest")
        self.assert_rejected("../1.2.3")
        self.assert_rejected("1.2.3:latest")
        self.assert_rejected("latest")

    def test_rejects_malformed_versions(self) -> None:
        for version in ("", "1", "1.2", "v1.2.3", "1.2.3.4", "01.2.3", "1.2.3+build", " 1.2.3", "1.2.3 "):
            with self.subTest(version=version):
                self.assert_rejected(version)


class PrereleaseDoesNotClaimStableChannels(unittest.TestCase):
    """A prerelease must not move latest / <major> / <major>.<minor>."""

    def test_prerelease_is_not_stable(self) -> None:
        for version in ("1.2.3-rc.1", "1.2.3-alpha", "0.12.0-beta.2", "2.0.0-rc-1"):
            with self.subTest(version=version):
                self.assertEqual(metadata.derive(version)["is_stable"], "false")

    def test_stable_is_stable(self) -> None:
        for version in ("1.2.3", "0.0.0", "10.20.30"):
            with self.subTest(version=version):
                self.assertEqual(metadata.derive(version)["is_stable"], "true")

    def test_prerelease_keeps_its_exact_tag(self) -> None:
        # The full tag is still published; only the aliases are withheld.
        self.assertEqual(metadata.derive("1.2.3-rc.1")["version"], "1.2.3-rc.1")

    def test_alias_values_ignore_the_prerelease_suffix(self) -> None:
        # When a stable release does claim aliases they must come from the
        # numeric core, not from the raw string.
        derived = metadata.derive("1.2.3")
        self.assertEqual(derived["major"], "1")
        self.assertEqual(derived["major_minor"], "1.2")


class DiagnosticsAreNotRunnerCommands(unittest.TestCase):
    """A rejected value must not reach workflow-command data unescaped."""

    def run_main(self, version: str) -> tuple[int, str]:
        from contextlib import redirect_stderr, redirect_stdout
        from io import StringIO

        out, err = StringIO(), StringIO()
        previous = os.environ.get("VERSION_INPUT")
        os.environ["VERSION_INPUT"] = version
        os.environ.pop("GITHUB_OUTPUT", None)
        try:
            with redirect_stdout(out), redirect_stderr(err):
                status = metadata.main([])
        finally:
            if previous is None:
                os.environ.pop("VERSION_INPUT", None)
            else:
                os.environ["VERSION_INPUT"] = previous
        return status, out.getvalue()

    def test_rejection_exits_nonzero(self) -> None:
        status, _ = self.run_main("1.2.3\n::add-mask::hunter2")
        self.assertEqual(status, 1)

    def test_error_annotation_carries_no_untrusted_data(self) -> None:
        status, stdout = self.run_main("1.2.3\n::add-mask::hunter2")
        self.assertEqual(status, 1)
        command_lines = [line for line in stdout.splitlines() if line.startswith("::")]
        # Exactly one runner command, and it is ours.
        self.assertEqual(len(command_lines), 1)
        self.assertNotIn("add-mask", stdout)
        self.assertNotIn("hunter2", stdout)

    def test_escape_command_data_encodes_percent_first(self) -> None:
        # Encoding CR/LF before '%' would double-encode and corrupt the value.
        self.assertEqual(metadata.escape_command_data("100%\n"), "100%25%0A")
        self.assertEqual(metadata.escape_command_data("a\r\nb"), "a%0D%0Ab")


class OutputWriting(unittest.TestCase):
    def test_writes_exactly_the_expected_output_lines(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "github_output"
            metadata.write_outputs(metadata.derive("1.2.3-rc.1"), str(path))
            self.assertEqual(
                path.read_text(encoding="utf-8"),
                "version=1.2.3-rc.1\nmajor=1\nmajor_minor=1.2\nis_stable=false\n",
            )

    def test_write_outputs_refuses_multiline_values(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "github_output"
            with self.assertRaises(metadata.InvalidVersion):
                metadata.write_outputs({"version": "1.2.3\nmajor=9"}, str(path))

    def test_no_output_file_is_not_an_error(self) -> None:
        metadata.write_outputs(metadata.derive("1.2.3"), None)


if __name__ == "__main__":
    unittest.main()
