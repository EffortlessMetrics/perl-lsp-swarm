#!/usr/bin/env python3
"""Contract tests for the shared bash resolver (#15889).

The resolver exists because Windows CreateProcess resolves the bare name
``bash`` against System32 before PATH, silently routing bash spawns to WSL
where host paths do not exist (#15868). These tests pin the resolution
contract on both platform shapes without depending on which perls, bashes,
or installations a host happens to carry.
"""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

from bash_binary import bash_binary, bash_path  # noqa: E402  (path set above)


class BashBinaryResolutionTests(unittest.TestCase):
    def test_posix_hosts_keep_the_bare_name(self) -> None:
        with mock.patch.object(sys, "platform", "linux"):
            self.assertEqual(bash_binary(), "bash")
        with mock.patch.object(sys, "platform", "darwin"):
            self.assertEqual(bash_binary(), "bash")

    def test_windows_resolution_skips_system32_and_prefers_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            system32 = root / "system32"
            git_usrs = root / "git-usr" / "bin"
            system32.mkdir()
            git_usrs.mkdir(parents=True)
            (system32 / "bash.exe").write_bytes(b"fake")
            (git_usrs / "bash.exe").write_bytes(b"fake")

            fake_path = os.pathsep.join(
                [
                    f'"{system32}"',  # quoted entries must still resolve
                    str(git_usrs),
                ]
            )
            with mock.patch.object(sys, "platform", "win32"):
                with mock.patch.dict(os.environ, {"PATH": fake_path}):
                    resolved = bash_binary()
            self.assertEqual(
                Path(resolved).parent.resolve(),
                git_usrs.resolve(),
                "System32's WSL bash must lose to the PATH bash; quoted entries "
                f"must resolve. Got: {resolved}",
            )

    def test_windows_resolution_falls_back_to_standard_git_locations(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git_bin = root / "Git" / "bin"
            git_bin.mkdir(parents=True)
            (git_bin / "bash.exe").write_bytes(b"fake")

            with mock.patch.object(sys, "platform", "win32"):
                with mock.patch.dict(
                    os.environ,
                    {"PATH": str(root / "empty"), "ProgramFiles": str(root)},
                ):
                    resolved = bash_binary()
            self.assertEqual(
                Path(resolved).resolve(),
                (git_bin / "bash.exe").resolve(),
                "a Git-for-Windows install outside PATH must be found by the fallback",
            )

    def test_windows_resolution_returns_bare_name_when_nothing_exists(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with mock.patch.object(sys, "platform", "win32"):
                with mock.patch.dict(
                    os.environ,
                    {"PATH": directory, "ProgramFiles": str(Path(directory) / "absent")},
                ):
                    # No bash.exe anywhere in the fake PATH or fallback roots.
                    self.assertEqual(bash_binary(), "bash")


class BashPathRenderingTests(unittest.TestCase):
    def test_windows_native_paths_render_forward_slashed(self) -> None:
        rendered = bash_path(Path(r"F:\code\rust2\repo\scripts\x.sh"))
        self.assertEqual(rendered, "F:/code/rust2/repo/scripts/x.sh")

    def test_posix_paths_render_unchanged(self) -> None:
        self.assertEqual(bash_path(Path("/usr/local/repo/scripts/x.sh")), "/usr/local/repo/scripts/x.sh")


class StandaloneDeploymentTests(unittest.TestCase):
    def test_worktree_manager_loads_alone_and_degrades_to_bare_bash(self) -> None:
        """A standalone copy of worktree-manager.py (the self-test ships it
        alone) must import and resolve bash without the shared module, in a
        fresh interpreter — the deployment shape the allocate self-test uses."""
        import shutil
        import subprocess

        source = Path(__file__).resolve().parent / "worktree-manager.py"
        with tempfile.TemporaryDirectory() as directory:
            standalone = Path(directory) / "scripts" / "worktree-manager.py"
            standalone.parent.mkdir()
            shutil.copy(source, standalone)
            driver = (
                "import importlib.util, pathlib, sys\n"
                f"spec = importlib.util.spec_from_file_location('wm_standalone', r'{standalone}')\n"
                "module = importlib.util.module_from_spec(spec)\n"
                "spec.loader.exec_module(module)\n"
                "print(*module._bash_spawn(pathlib.Path('F:/repo/scripts/cleanup.sh')))\n"
            )
            result = subprocess.run(
                [sys.executable, "-c", driver],
                capture_output=True,
                text=True,
                cwd=directory,
                timeout=60,
            )
            self.assertEqual(
                result.returncode, 0,
                f"standalone worktree-manager failed to load: {result.stderr}",
            )
            self.assertEqual(
                result.stdout.strip(),
                "bash F:/repo/scripts/cleanup.sh",
                "without the shared module the spawn must degrade to the bare "
                "name with the portable path rendering",
            )


if __name__ == "__main__":
    unittest.main()
