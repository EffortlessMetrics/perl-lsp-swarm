#!/usr/bin/env python3
"""Focused tests for scripts/ci/check_perl_lsp_rs_core_package.py."""

from __future__ import annotations

import importlib.util
import io
import json
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


SCRIPT_PATH = Path(__file__).with_name("check_perl_lsp_rs_core_package.py")
SPEC = importlib.util.spec_from_file_location("check_perl_lsp_rs_core_package", SCRIPT_PATH)
assert SPEC is not None
core_package = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(core_package)


def metadata_fixture(root: Path) -> str:
    return json.dumps(
        {
            "workspace_members": [
                "path+file:///repo#perl-lsp-rs-core@0.1.0",
                "path+file:///repo#perl-parser@0.1.0",
                "path+file:///repo#perl-tdd-support@0.1.0",
                "path+file:///repo#external-shadow@0.1.0",
            ],
            "packages": [
                {
                    "id": "path+file:///repo#perl-lsp-rs-core@0.1.0",
                    "name": "perl-lsp-rs-core",
                    "version": "0.1.0",
                    "manifest_path": str(root / "crates" / "perl-lsp-rs-core" / "Cargo.toml"),
                    "dependencies": [
                        {"name": "perl-parser", "kind": None},
                        {"name": "perl-tdd-support", "kind": "dev"},
                        {"name": "serde", "kind": None},
                    ],
                },
                {
                    "id": "path+file:///repo#perl-parser@0.1.0",
                    "name": "perl-parser",
                    "version": "0.1.0",
                    "manifest_path": str(root / "crates" / "perl-parser" / "Cargo.toml"),
                    "dependencies": [],
                },
                {
                    "id": "path+file:///repo#perl-tdd-support@0.1.0",
                    "name": "perl-tdd-support",
                    "version": "0.1.0",
                    "manifest_path": str(root / "crates" / "perl-tdd-support" / "Cargo.toml"),
                    "dependencies": [],
                },
                {
                    "id": "path+file:///repo#external-shadow@0.1.0",
                    "name": "external-shadow",
                    "version": "0.1.0",
                    "manifest_path": str(root / "vendor" / "external-shadow" / "Cargo.toml"),
                    "dependencies": [],
                },
                {
                    "id": "registry+https://example.invalid#serde@1.0.0",
                    "name": "serde",
                    "version": "1.0.0",
                    "manifest_path": str(root / "registry" / "serde" / "Cargo.toml"),
                    "dependencies": [],
                },
            ],
        }
    )


class CorePackageValidatorTests(unittest.TestCase):
    def test_workspace_package_version_finds_workspace_member(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(core_package, "run", return_value=metadata_fixture(Path(tmp))):
                version = core_package.workspace_package_version("perl-lsp-rs-core")

        self.assertEqual("0.1.0", version)

    def test_workspace_package_version_rejects_unknown_member(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(core_package, "run", return_value=metadata_fixture(Path(tmp))):
                with self.assertRaises(SystemExit) as raised:
                    core_package.workspace_package_version("missing-crate")

        self.assertIn("workspace package not found: missing-crate", str(raised.exception))

    def test_workspace_patch_args_include_only_transitive_normal_deps_by_default(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(core_package, "run", return_value=metadata_fixture(Path(tmp))):
                patch_args = core_package.workspace_patch_args(
                    "perl-lsp-rs-core",
                    include_dev_deps=False,
                )

        self.assertEqual(1, len(patch_args))
        self.assertIn("patch.crates-io.perl-parser.path", patch_args[0])
        self.assertNotIn("perl-tdd-support", "\n".join(patch_args))
        self.assertNotIn("serde", "\n".join(patch_args))

    def test_workspace_patch_args_can_include_dev_dependency_workspace_members(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(core_package, "run", return_value=metadata_fixture(Path(tmp))):
                patch_args = core_package.workspace_patch_args(
                    "perl-lsp-rs-core",
                    include_dev_deps=True,
                )

        rendered = "\n".join(patch_args)
        self.assertIn("patch.crates-io.external-shadow.path", rendered)
        self.assertIn("patch.crates-io.perl-parser.path", rendered)
        self.assertIn("patch.crates-io.perl-tdd-support.path", rendered)
        self.assertNotIn("patch.crates-io.perl-lsp-rs-core.path", rendered)
        self.assertNotIn("serde", rendered)

    def test_safe_extract_rejects_archive_path_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            archive = root / "malicious.tar"
            with tarfile.open(archive, "w") as tar:
                payload = b"escape"
                member = tarfile.TarInfo("../escape.txt")
                member.size = len(payload)
                tar.addfile(member, io.BytesIO(payload))

            with self.assertRaises(SystemExit) as raised:
                core_package.safe_extract(archive, root / "out")

        self.assertIn("refusing to extract path outside destination", str(raised.exception))

    def test_strip_dev_dependencies_removes_dev_tables_only(self) -> None:
        manifest_text = """[package]
name = "demo"

[dependencies]
serde = "1"

[dev-dependencies]
tempfile = "3"

[build-dependencies]
cc = "1"
"""
        with tempfile.TemporaryDirectory() as tmp:
            manifest = Path(tmp) / "Cargo.toml"
            manifest.write_text(manifest_text, encoding="utf-8")

            core_package.strip_dev_dependencies(manifest)

            stripped = manifest.read_text(encoding="utf-8")

        self.assertIn("[package]", stripped)
        self.assertIn("[dependencies]", stripped)
        self.assertIn("[build-dependencies]", stripped)
        self.assertNotIn("[dev-dependencies]", stripped)
        self.assertNotIn("tempfile", stripped)

    def test_package_args_preserve_patch_verify_and_dirty_flags(self) -> None:
        args = core_package.package_args(
            "--list",
            allow_dirty=True,
            patch_args=["--config=patch.crates-io.foo.path=\"/tmp/foo\""],
            no_verify=True,
        )

        self.assertEqual(
            [
                "cargo",
                "package",
                "-p",
                "perl-lsp-rs-core",
                "--locked",
                "--config=patch.crates-io.foo.path=\"/tmp/foo\"",
                "--list",
                "--no-verify",
                "--allow-dirty",
            ],
            args,
        )

    def test_main_checks_archive_from_cargos_effective_target_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            repository = root / "repository"
            target = root / "private target"
            archive = target / "package" / "perl-lsp-rs-core-0.1.0.crate"
            archive.parent.mkdir(parents=True)
            archive.write_bytes(b"packaged archive")
            smoke = root / "smoke"
            extracted = smoke / "perl-lsp-rs-core-0.1.0"
            extracted.mkdir(parents=True)
            (extracted / "build_catalog.rs").write_text("// build catalog\n", encoding="utf-8")
            (extracted / "Cargo.toml").write_text('[package]\nname = "demo"\n', encoding="utf-8")

            def run(command: list[str], *, capture: bool = False) -> str:
                if command[1] == "metadata":
                    return json.dumps({"target_directory": str(target)})
                if "--list" in command:
                    return "build_catalog.rs\nCargo.toml\n"
                return ""

            with (
                patch.object(core_package, "ROOT", repository),
                patch.object(core_package, "parse_args", return_value=SimpleNamespace(allow_dirty=False)),
                patch.object(core_package, "workspace_package_version", return_value="0.1.0"),
                patch.object(core_package, "workspace_patch_args", return_value=[]),
                patch.object(core_package, "run", side_effect=run) as run_mock,
                patch.object(core_package, "safe_extract") as extract_mock,
                patch.object(core_package.tempfile, "mkdtemp", return_value=str(smoke)),
            ):
                result = core_package.main()

            self.assertEqual(0, result)
            extract_mock.assert_called_once_with(archive, smoke)
            self.assertTrue(any(call.args[0][1] == "check" for call in run_mock.call_args_list))

    def test_workspace_target_directory_uses_cargo_metadata(self) -> None:
        with patch.object(
            core_package,
            "run",
            return_value=json.dumps({"target_directory": "/custom target"}),
        ) as run_mock:
            target = core_package.workspace_target_directory()

        self.assertEqual(Path("/custom target"), target)
        run_mock.assert_called_once_with(
            ["cargo", "metadata", "--format-version=1", "--no-deps"],
            capture=True,
        )

    def test_main_refuses_missing_custom_archive_despite_stale_default_archive(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stale = root / "target" / "package" / "perl-lsp-rs-core-0.1.0.crate"
            stale.parent.mkdir(parents=True)
            stale.write_bytes(b"stale default archive")
            target = root / "private target"

            def run(command: list[str], *, capture: bool = False) -> str:
                if command[1] == "metadata":
                    return json.dumps({"target_directory": str(target)})
                if "--list" in command:
                    return "build_catalog.rs\n"
                return ""

            with (
                patch.object(core_package, "ROOT", root),
                patch.object(core_package, "parse_args", return_value=SimpleNamespace(allow_dirty=False)),
                patch.object(core_package, "workspace_package_version", return_value="0.1.0"),
                patch.object(core_package, "workspace_patch_args", return_value=[]),
                patch.object(core_package, "run", side_effect=run),
                patch.object(core_package, "safe_extract") as extract_mock,
                patch("sys.stderr", new_callable=io.StringIO) as stderr,
            ):
                result = core_package.main()

            self.assertEqual(1, result)
            self.assertIn(str(target / "package"), stderr.getvalue())
            extract_mock.assert_not_called()


class CorePackageSmokeLifecycleTests(unittest.TestCase):
    def test_owned_smoke_tree_is_removed_on_success_and_failure(self) -> None:
        for outcome in (
            "success",
            "missing-catalog",
            "invalid-archive",
            "traversal",
            "invalid-manifest",
            "generate-lockfile",
            "check",
        ):
            with self.subTest(outcome=outcome):
                self.check_smoke_lifecycle(outcome)

    def test_cleanup_preserves_unowned_symlink_target(self) -> None:
        self.check_smoke_lifecycle("success", link_unowned=True)

    def check_smoke_lifecycle(self, outcome: str, *, link_unowned: bool = False) -> None:
        with tempfile.TemporaryDirectory(prefix="package-smoke-lifecycle-") as tmp:
            root = Path(tmp)
            # Same-prefix sibling: only the directory allocated by main is owned.
            unowned = root / "perl-lsp-rs-core-package-smoke-unowned"
            unowned.mkdir()
            sentinel = unowned / "keep.txt"
            sentinel.write_bytes(b"CALLER-OWNED\x00\n")
            target = root / "private target"
            archive = target / "package" / "perl-lsp-rs-core-0.1.0.crate"
            archive.parent.mkdir(parents=True)
            if outcome == "invalid-archive":
                archive.write_bytes(b"not a tar archive")
            else:
                files = {
                    "perl-lsp-rs-core-0.1.0/Cargo.toml": (
                        b"\xff" if outcome == "invalid-manifest" else
                        b'[package]\nname = "demo"\n[dev-dependencies]\ntempfile = "3"\n'
                    ),
                }
                if outcome != "missing-catalog":
                    files["perl-lsp-rs-core-0.1.0/build_catalog.rs"] = b"// catalog\n"
                if outcome == "traversal":
                    files["../perl-lsp-rs-core-package-smoke-unowned/keep.txt"] = b"OVERWRITTEN"
                with tarfile.open(archive, "w") as tar:
                    for name, payload in files.items():
                        member = tarfile.TarInfo(name)
                        member.size = len(payload)
                        tar.addfile(member, io.BytesIO(payload))
            original_archive = archive.read_bytes()
            metadata = json.loads(metadata_fixture(root))
            metadata["target_directory"] = str(target)
            commands = []
            child_failure = subprocess.CalledProcessError(37, ["cargo", outcome])

            def run(command: list[str], *, capture: bool = False) -> str:
                commands.append(command[1])
                if command[1] == "metadata":
                    return json.dumps(metadata)
                if "--list" in command:
                    return "build_catalog.rs\nCargo.toml\n"
                if command[1] in ("generate-lockfile", "check"):
                    # Prove the real archive was extracted and the real manifest
                    # editor ran before the simulated child result is delivered.
                    manifest = Path(command[command.index("--manifest-path") + 1])
                    self.assertEqual(
                        '[package]\nname = "demo"\n', manifest.read_text(encoding="utf-8")
                    )
                    if command[1] == outcome:
                        raise child_failure
                return ""

            allocated = []
            real_mkdtemp = tempfile.mkdtemp

            def allocate(*args, **kwargs) -> str:
                directory = real_mkdtemp(*args, **kwargs, dir=root)
                allocated.append(Path(directory))
                if link_unowned:
                    try:
                        (Path(directory) / "unowned-link").symlink_to(unowned, target_is_directory=True)
                    except (OSError, NotImplementedError) as error:
                        self.skipTest(f"directory symlink unavailable: {error}")
                return directory

            errors = {
                "invalid-archive": tarfile.ReadError,
                "traversal": SystemExit,
                "invalid-manifest": UnicodeDecodeError,
                "generate-lockfile": subprocess.CalledProcessError,
                "check": subprocess.CalledProcessError,
            }
            with (
                patch.object(core_package, "parse_args", return_value=SimpleNamespace(allow_dirty=False)),
                patch.object(core_package, "run", side_effect=run),
                patch.object(core_package.tempfile, "mkdtemp", side_effect=allocate),
                patch("sys.stdout", new_callable=io.StringIO) as stdout,
                patch("sys.stderr", new_callable=io.StringIO) as stderr,
            ):
                if outcome in errors:
                    with self.assertRaises(errors[outcome]) as raised:
                        core_package.main()
                    if outcome in ("generate-lockfile", "check"):
                        self.assertIs(raised.exception, child_failure)
                    if outcome == "traversal":
                        self.assertIn("refusing to extract path outside destination", str(raised.exception))
                else:
                    result = core_package.main()
                    self.assertEqual(1 if outcome == "missing-catalog" else 0, result)
                if outcome == "missing-catalog":
                    self.assertIn("unpacked perl-lsp-rs-core package is missing build_catalog.rs", stderr.getvalue())
                if outcome == "success":
                    self.assertIn("perl-lsp-rs-core-0.1.0.crate is self-contained", stdout.getvalue())

            expected_children = {
                "success": ["generate-lockfile", "check"],
                "generate-lockfile": ["generate-lockfile"],
                "check": ["generate-lockfile", "check"],
            }
            self.assertEqual(
                expected_children.get(outcome, []),
                [command for command in commands if command in ("generate-lockfile", "check")],
            )
            self.assertEqual(b"CALLER-OWNED\x00\n", sentinel.read_bytes())
            self.assertEqual(original_archive, archive.read_bytes())
            self.assertEqual(1, len(allocated))
            self.assertFalse(allocated[0].exists(), f"owned smoke tree leaked after {outcome}")
            self.assertCountEqual([unowned, target], root.iterdir())


if __name__ == "__main__":
    unittest.main()
