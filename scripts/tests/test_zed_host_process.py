import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from scripts.zed_host import common, finalize, prepare, process


class ZedHostProcessTests(unittest.TestCase):
    def test_preexisting_process_does_not_count_as_observed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run_dir, manifest = self._prepared_run(root)
            preexisting = [
                {
                    "pid": 41,
                    "parent_pid": 40,
                    "executable": "/opt/perllsp",
                    "command": "/opt/perllsp --stdio",
                }
            ]
            fake_zed = self._fake_zed()

            with (
                patch.object(
                    process,
                    "matching_processes",
                    side_effect=[
                        (preexisting, {41: 40}),
                        (preexisting, {41: 40}),
                        (preexisting, {41: 40}),
                    ],
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
            expected_manifest = common.sha256_file(run_dir / "manifest.json")
            self.assertEqual(launch["result"], "fail")
            self.assertFalse(launch["perllsp_observed"])
            self.assertEqual(launch["prepared_manifest_sha256"], expected_manifest)
            self.assertEqual(inventory["prepared_manifest_sha256"], expected_manifest)
            self.assertEqual(inventory["preexisting_perllsp_pids"], [41])
            self.assertEqual(inventory["perllsp_samples"], [])
            self.assertEqual(inventory["zed_pid"], self.ZED_PID)

    def test_new_process_is_observed_and_terminated_process_is_not_a_leak(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run_dir, manifest = self._prepared_run(root)
            observed = [
                {
                    "pid": 42,
                    "parent_pid": 5,
                    "executable": "/opt/perllsp",
                    "command": "/opt/perllsp --stdio",
                }
            ]
            fake_zed = self._fake_zed()
            # 42 descends from the Zed app (5), which descends from the launched
            # CLI (the mocked Popen pid).
            table = {42: 5, 5: self.ZED_PID}

            with (
                patch.object(
                    process,
                    "matching_processes",
                    side_effect=[([], {}), (observed, table), ([], {})],
                ),
                patch.object(process.subprocess, "Popen", return_value=fake_zed),
                patch.object(process.time, "sleep"),
            ):
                result = process.launch(manifest, run_dir, timeout_seconds=10)

            self.assertEqual(result, 0)
            launch = json.loads((run_dir / "launch.json").read_text(encoding="utf-8"))
            inventory = json.loads(
                (run_dir / "artifacts" / "process-inventory.json").read_text(
                    encoding="utf-8"
                )
            )
            self.assertEqual(launch["result"], "pass")
            self.assertTrue(launch["perllsp_observed"])
            self.assertEqual(launch["new_surviving_perllsp_pids"], [])
            self.assertEqual(inventory["perllsp_samples"][0]["rows"][0]["pid"], 42)
            self.assertEqual(
                inventory["perllsp_samples"][0]["rows"][0]["parent_pid"], 5
            )
            common.verify_artifact_reference(
                run_dir / "artifacts" / "process-inventory.json",
                run_dir,
                launch["process_inventory"],
                "process inventory",
            )

    def test_unrelated_new_process_does_not_satisfy_observed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run_dir, manifest = self._prepared_run(root)
            unrelated = [
                {
                    "pid": 42,
                    "parent_pid": 7,
                    "executable": "/opt/perllsp",
                    "command": "/opt/perllsp --stdio",
                }
            ]
            fake_zed = self._fake_zed()
            # 42's ancestor chain never reaches the launched Zed process tree.
            table = {42: 7, 7: 1}

            with (
                patch.object(
                    process,
                    "matching_processes",
                    side_effect=[([], {}), (unrelated, table), ([], {})],
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
            self.assertEqual(inventory["perllsp_samples"], [])

    def test_descends_from_terminates_on_cycles(self) -> None:
        table = {2: 3, 3: 2}
        self.assertTrue(process._descends_from(3, table, 3))
        self.assertFalse(process._descends_from(2, table, 1))
        self.assertFalse(process._descends_from(9, table, 3))

    def test_worktree_route_recheck_binds_launch_environment(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            perllsp = (Path(temporary) / "perllsp").resolve()
            perllsp.write_bytes(b"binary")
            manifest = {
                "perllsp": {
                    "resolution_route": "worktree_path",
                    "command": str(perllsp),
                }
            }
            with patch.object(process.shutil, "which", return_value=str(perllsp)):
                process._require_worktree_resolution(manifest, perllsp)
            with self.assertRaisesRegex(
                process.HostReceiptError, "PATH `perllsp` to be the"
            ):
                with patch.object(
                    process.shutil, "which", return_value=str(Path(temporary))
                ):
                    process._require_worktree_resolution(manifest, perllsp)
            with self.assertRaisesRegex(process.HostReceiptError, "session's PATH"):
                with patch.object(process.shutil, "which", return_value=None):
                    process._require_worktree_resolution(manifest, perllsp)
            process._require_worktree_resolution(
                {"perllsp": {"resolution_route": "binary_override"}}, perllsp
            )

    def test_windows_falls_back_when_pwsh_is_missing(self) -> None:
        target = Path("/opt/perllsp").resolve()
        output = json.dumps(
            {
                "ProcessId": 42,
                "ParentProcessId": 9,
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
            rows, table = process._windows_processes(target)

        self.assertEqual(rows[0]["pid"], 42)
        self.assertEqual(rows[0]["parent_pid"], 9)
        self.assertEqual(table[42], 9)
        self.assertEqual(run.call_args.args[0][0], "/Windows/System32/powershell.exe")

    def test_windows_without_either_shell_fails_closed(self) -> None:
        with patch.object(process.shutil, "which", return_value=None):
            with self.assertRaises(process.HostReceiptError):
                process._windows_processes(Path("/opt/perllsp").resolve())

    def test_macos_requires_exact_canonical_executable_path(self) -> None:
        completed = SimpleNamespace(
            stdout=(
                "  101      1 /opt/bin/perllsp-old --stdio\n"
                "  102      1 /opt/bin/perllsp-wrapper --stdio\n"
                "  103      9 /opt/bin/perllsp --stdio\n"
            )
        )
        with patch.object(process.subprocess, "run", return_value=completed):
            rows, table = process._macos_processes(Path("/opt/bin/perllsp"))

        self.assertEqual([row["pid"] for row in rows], [103])
        self.assertEqual(rows[0]["parent_pid"], 9)
        self.assertEqual(table[101], 1)

    def test_platform_identity_normalizes_darwin_to_schema_value(self) -> None:
        with (
            patch.object(common.platform, "system", return_value="Darwin"),
            patch.object(common.platform, "version", return_value="test"),
            patch.object(common.platform, "machine", return_value="arm64"),
        ):
            identity = common.platform_identity()

        self.assertEqual(identity["os"], "macos")
        self.assertEqual(identity["architecture"], "aarch64")

    def test_perllsp_embedded_revision_must_match_full_subject(self) -> None:
        output = "perllsp 0.18.0\nGit commit: abcdef0"
        self.assertEqual(
            prepare._parse_perllsp_identity(
                output,
                "0.18.0",
                "abcdef0123456789abcdef0123456789abcdef01",
            ),
            "abcdef0",
        )
        with self.assertRaisesRegex(
            common.HostReceiptError,
            "binary revision does not match",
        ):
            prepare._parse_perllsp_identity(
                output,
                "0.18.0",
                "1111111111111111111111111111111111111111",
            )

    def test_configuration_observation_types_are_rejected_before_emission(self) -> None:
        valid = {
            "workspace_configuration_observed": True,
            "precedence_observed": "perl settings won",
            "live_update_observed": None,
        }
        self.assertEqual(
            finalize._configuration_observation(valid),
            valid,
        )
        with self.assertRaisesRegex(
            common.HostReceiptError,
            r"precedence_observed must be a string or null",
        ):
            finalize._configuration_observation(
                {**valid, "precedence_observed": True}
            )
        with self.assertRaisesRegex(
            common.HostReceiptError,
            r"live_update_observed must be a string or null",
        ):
            finalize._configuration_observation(
                {**valid, "live_update_observed": False}
            )
        with self.assertRaisesRegex(
            common.HostReceiptError,
            r"workspace_configuration_observed must be a boolean or null",
        ):
            finalize._configuration_observation(
                {**valid, "workspace_configuration_observed": "yes"}
            )

    def test_rust_validation_mode_follows_recorded_result(self) -> None:
        completed = SimpleNamespace(returncode=0, stdout="", stderr="")
        with (
            patch.object(
                finalize.subprocess, "run", return_value=completed
            ) as run,
            tempfile.TemporaryDirectory() as temporary,
        ):
            receipt = Path(temporary) / "receipt.json"
            receipt.write_text("{}", encoding="utf-8")
            finalize._validate_with_rust(Path("/repo"), receipt, schema_only=True)
            schema_only_args = run.call_args.args[0]
            finalize._validate_with_rust(Path("/repo"), receipt, schema_only=False)
            semantic_args = run.call_args.args[0]

        self.assertIn("--schema-only", schema_only_args)
        self.assertLess(
            schema_only_args.index("--schema-only"),
            schema_only_args.index(str(receipt)),
        )
        self.assertNotIn("--schema-only", semantic_args)
        self.assertEqual(
            semantic_args[-1], str(receipt)
        )

    def test_rust_validation_failure_is_a_receipt_error(self) -> None:
        completed = SimpleNamespace(
            returncode=1, stdout="", stderr="boom"
        )
        with (
            patch.object(
                finalize.subprocess, "run", return_value=completed
            ),
            tempfile.TemporaryDirectory() as temporary,
        ):
            receipt = Path(temporary) / "receipt.json"
            receipt.write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(
                common.HostReceiptError, "validator rejected"
            ):
                finalize._validate_with_rust(
                    Path("/repo"), receipt, schema_only=False
                )

    def test_cross_run_evidence_binding_is_rejected(self) -> None:
        expected = "sha256:" + "a" * 64
        observations = {
            "prepared_manifest_sha256": expected,
            "language_server_log": {
                "prepared_manifest_sha256": expected,
            },
        }
        launch = {"prepared_manifest_sha256": expected}
        inventory = {"prepared_manifest_sha256": "sha256:" + "b" * 64}

        with self.assertRaisesRegex(
            common.HostReceiptError,
            "process inventory is not bound",
        ):
            finalize._require_run_binding(
                expected,
                observations,
                launch,
                inventory,
            )

    def test_workspace_change_is_rejected_before_receipt_creation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            extension = root / "extension"
            workspace = root / "workspace"
            extension.mkdir()
            workspace.mkdir()
            manifest_path = extension / "extension.toml"
            wasm = extension / "extension.wasm"
            zed_cli = root / "zed-cli"
            zed_app = root / "zed-app"
            perllsp = root / "perllsp"
            settings = root / "settings.json"
            fixture = workspace / "main.pl"
            for path, content in [
                (manifest_path, "id = 'perl'\n"),
                (wasm, "wasm"),
                (zed_cli, "zed-cli"),
                (zed_app, "zed-app"),
                (perllsp, "perllsp"),
                (settings, "{}"),
                (fixture, "print 1;\n"),
            ]:
                path.write_text(content, encoding="utf-8")

            manifest = {
                "zed": {
                    "cli": str(zed_cli),
                    "cli_sha256": common.sha256_file(zed_cli),
                    "app": str(zed_app),
                    "app_sha256": common.sha256_file(zed_app),
                },
                "extension": {
                    "manifest": str(manifest_path),
                    "manifest_sha256": common.sha256_file(manifest_path),
                    "wasm": str(wasm),
                    "wasm_sha256": common.sha256_file(wasm),
                    "directory": str(extension),
                    "tree_sha256": common.sha256_tree(extension),
                },
                "perllsp": {
                    "command": str(perllsp),
                    "binary_sha256": common.sha256_file(perllsp),
                },
                "configuration": {
                    "settings": str(settings),
                    "settings_sha256": common.sha256_file(settings),
                },
                "workspace": {
                    "directory": str(workspace),
                    "fixture_sha256": common.sha256_tree(
                        workspace,
                        ignored=(".git",),
                    ),
                },
            }
            finalize._require_unchanged(manifest)
            fixture.write_text("print 2;\n", encoding="utf-8")
            with self.assertRaisesRegex(
                common.HostReceiptError,
                "workspace fixture changed",
            ):
                finalize._require_unchanged(manifest)

    ZED_PID = 99

    @staticmethod
    def _fake_zed() -> Mock:
        fake_zed = Mock()
        fake_zed.pid = ZedHostProcessTests.ZED_PID
        fake_zed.poll.side_effect = [None, 0]
        fake_zed.returncode = 0
        return fake_zed

    @staticmethod
    def _prepared_run(root: Path) -> tuple[Path, dict]:
        run_dir = root / "run"
        run_dir.mkdir()
        manifest = ZedHostProcessTests._manifest(root)
        common.write_json(run_dir / "manifest.json", manifest)
        return run_dir, manifest

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
