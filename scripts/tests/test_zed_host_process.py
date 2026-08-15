import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from scripts.zed_host import process


class ZedHostProcessTests(unittest.TestCase):
    def test_preexisting_process_does_not_count_as_observed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run_dir = root / "run"
            manifest = self._manifest(root)
            preexisting = [
                {
                    "pid": 41,
                    "executable": "/opt/perllsp",
                    "command": "/opt/perllsp --stdio",
                }
            ]
            fake_zed = Mock()
            fake_zed.poll.side_effect = [None, 0]
            fake_zed.returncode = 0

            with (
                patch.object(
                    process,
                    "matching_processes",
                    side_effect=[preexisting, preexisting, preexisting],
                ),
                patch.object(process.subprocess, "Popen", return_value=fake_zed),
                patch.object(process.time, "sleep"),
            ):
                result = process.launch(manifest, run_dir, timeout_seconds=10)

            self.assertEqual(result, 1)
            launch = json.loads((run_dir / "launch.json").read_text(encoding="utf-8"))
            inventory = json.loads(
                (run_dir / "artifacts" / "process-inventory.json").read_text(
                    encoding="utf-8"
                )
            )
            self.assertEqual(launch["result"], "fail")
            self.assertFalse(launch["perllsp_observed"])
            self.assertEqual(inventory["preexisting_perllsp_pids"], [41])
            self.assertEqual(inventory["perllsp_samples"], [])

    def test_new_process_is_observed_and_terminated_process_is_not_a_leak(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run_dir = root / "run"
            manifest = self._manifest(root)
            observed = [
                {
                    "pid": 42,
                    "executable": "/opt/perllsp",
                    "command": "/opt/perllsp --stdio",
                }
            ]
            fake_zed = Mock()
            fake_zed.poll.side_effect = [None, 0]
            fake_zed.returncode = 0

            with (
                patch.object(
                    process, "matching_processes", side_effect=[[], observed, []]
                ),
                patch.object(process.subprocess, "Popen", return_value=fake_zed),
                patch.object(process.time, "sleep"),
            ):
                result = process.launch(manifest, run_dir, timeout_seconds=10)

            self.assertEqual(result, 0)
            launch = json.loads((run_dir / "launch.json").read_text(encoding="utf-8"))
            self.assertEqual(launch["result"], "pass")
            self.assertTrue(launch["perllsp_observed"])
            self.assertEqual(launch["new_surviving_perllsp_pids"], [])

    def test_windows_falls_back_when_pwsh_is_missing(self) -> None:
        target = Path("/opt/perllsp").resolve()
        output = json.dumps(
            {
                "ProcessId": 42,
                "ExecutablePath": str(target),
                "CommandLine": f"{target} --stdio",
            }
        )
        completed = SimpleNamespace(stdout=output)

        def which(name: str) -> str | None:
            return None if name == "pwsh" else "/Windows/System32/powershell.exe"

        with (
            patch.object(process.shutil, "which", side_effect=which),
            patch.object(process.subprocess, "run", return_value=completed) as run,
        ):
            rows = process._windows_processes(target)

        self.assertEqual(rows[0]["pid"], 42)
        self.assertEqual(run.call_args.args[0][0], "/Windows/System32/powershell.exe")

    def test_windows_without_either_shell_fails_closed(self) -> None:
        with patch.object(process.shutil, "which", return_value=None):
            with self.assertRaises(process.HostReceiptError):
                process._windows_processes(Path("/opt/perllsp").resolve())

    def test_macos_requires_exact_canonical_executable_path(self) -> None:
        completed = SimpleNamespace(
            stdout=(
                "101 /opt/bin/perllsp-old --stdio\n"
                "102 /opt/bin/perllsp-wrapper --stdio\n"
                "103 /opt/bin/perllsp --stdio\n"
            )
        )
        with patch.object(process.subprocess, "run", return_value=completed):
            rows = process._macos_processes(Path("/opt/bin/perllsp"))

        self.assertEqual([row["pid"] for row in rows], [103])

    @staticmethod
    def _manifest(root: Path) -> dict[str, dict[str, str]]:
        return {
            "zed": {"cli": str(root / "zed-cli"), "app": str(root / "zed-app")},
            "profile": {"directory": str(root / "profile")},
            "workspace": {"directory": str(root / "workspace")},
            "extension": {"directory": str(root / "extension")},
            "perllsp": {"command": str(root / "perllsp")},
        }


if __name__ == "__main__":
    unittest.main()
