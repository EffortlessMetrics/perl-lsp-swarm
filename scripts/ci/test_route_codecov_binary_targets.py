#!/usr/bin/env python3
"""Focused tests for changed-package binary target coverage routing."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import tempfile
import textwrap
import unittest


SCRIPT_PATH = Path(__file__).with_name("route-codecov-packs.py")
SPEC = importlib.util.spec_from_file_location("route_codecov_packs_binary_targets", SCRIPT_PATH)
assert SPEC is not None
router = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(router)


class BinaryTargetRoutingTests(unittest.TestCase):
    def _fallback_pack(self) -> dict[str, object]:
        return {
            "id": router.FALLBACK_PACK_ID,
            "files": ["*.rs"],
            "commands": [],
            "coverage_filters": ["changed-package-targets"],
        }

    def _write_package(
        self,
        root: Path,
        crate_directory: str,
        manifest: str,
        files: list[str],
    ) -> None:
        crate_root = root / "crates" / crate_directory
        crate_root.mkdir(parents=True)
        (crate_root / "Cargo.toml").write_text(
            textwrap.dedent(manifest).lstrip(), encoding="utf-8"
        )
        for relative_path in files:
            path = crate_root / relative_path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("// target fixture\n", encoding="utf-8")

    def _commands_for(
        self,
        root: Path,
        crate_directory: str,
        changed_source: str = "src/changed.rs",
    ) -> list[str]:
        path = f"crates/{crate_directory}/{changed_source}"
        selected = router.selected_packs([self._fallback_pack()], [path])
        self.assertEqual([router.FALLBACK_PACK_ID], [pack["id"] for pack in selected])
        normalized = router.normalize_pack(selected[0], [path], repo_root=root)
        return normalized["commands"]

    def test_perl_ci_hygiene_change_registers_binary_unit_target(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        commands = self._commands_for(repo_root, "perl-ci-hygiene", "src/cli.rs")

        self.assertIn(
            "cargo llvm-cov test --no-report -p perl-ci-hygiene --lib --profile agent --locked",
            commands,
        )
        self.assertIn(
            "cargo llvm-cov test --no-report -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked",
            commands,
        )
        self.assertIn(
            "cargo llvm-cov test --no-report -p perl-ci-hygiene --tests --profile agent --locked -- --test-threads=1",
            commands,
        )

    def test_binary_only_package_uses_manifest_name_without_invalid_lib_target(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "tool-directory",
                """
                [package]
                name = "renamed-tool"
                version = "0.0.0"
                edition = "2024"
                """,
                ["src/main.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "tool-directory")

        self.assertIn(
            "cargo llvm-cov test --no-report -p renamed-tool --bin renamed-tool --profile agent --locked",
            commands,
        )
        self.assertFalse(any(" --lib " in command for command in commands))
        self.assertTrue(all("-p tool-directory" not in command for command in commands))

    def test_library_only_package_does_not_invent_binary_target(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "library-directory",
                """
                [package]
                name = "library-package"
                version = "0.0.0"
                edition = "2024"
                """,
                ["src/lib.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "library-directory")

        self.assertIn(
            "cargo llvm-cov test --no-report -p library-package --lib --profile agent --locked",
            commands,
        )
        self.assertFalse(any(" --bin " in command for command in commands))

    def test_explicit_lib_path_is_registered_without_src_lib(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "custom-lib-directory",
                """
                [package]
                name = "custom-lib"
                version = "0.0.0"
                edition = "2024"

                [lib]
                path = "source/library.rs"
                """,
                ["source/library.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "custom-lib-directory")

        self.assertIn(
            "cargo llvm-cov test --no-report -p custom-lib --lib --profile agent --locked",
            commands,
        )

    def test_implicit_and_explicit_binary_targets_are_complete_and_sorted(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "multi-directory",
                """
                [package]
                name = "multi-tool"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "custom"
                path = "tools/custom.rs"
                required-features = ["json", "cli"]

                [[bin]]
                name = "disabled"
                path = "tools/disabled.rs"
                test = false
                """,
                [
                    "src/lib.rs",
                    "src/main.rs",
                    "src/changed.rs",
                    "src/bin/report.rs",
                    "src/bin/admin/main.rs",
                    "tools/custom.rs",
                    "tools/disabled.rs",
                ],
            )

            commands = self._commands_for(root, "multi-directory")

        binary_commands = [command for command in commands if " --bin " in command]
        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p multi-tool --bin admin --profile agent --locked",
                "cargo llvm-cov test --no-report -p multi-tool --features cli,json --bin custom --profile agent --locked",
                "cargo llvm-cov test --no-report -p multi-tool --bin multi-tool --profile agent --locked",
                "cargo llvm-cov test --no-report -p multi-tool --bin report --profile agent --locked",
            ],
            binary_commands,
        )
        self.assertTrue(all("disabled" not in command for command in commands))

    def test_autobins_false_keeps_explicit_target_only(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "explicit-directory",
                """
                [package]
                name = "explicit-tool"
                version = "0.0.0"
                edition = "2024"
                autobins = false

                [[bin]]
                name = "selected"
                path = "tools/selected.rs"
                """,
                [
                    "src/main.rs",
                    "src/changed.rs",
                    "src/bin/ignored.rs",
                    "tools/selected.rs",
                ],
            )

            commands = self._commands_for(root, "explicit-directory")

        binary_commands = [command for command in commands if " --bin " in command]
        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p explicit-tool --bin selected --profile agent --locked"
            ],
            binary_commands,
        )

    def test_explicit_target_wins_over_duplicate_implicit_name(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "duplicate-directory",
                """
                [package]
                name = "duplicate-tool"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "duplicate-tool"
                path = "src/main.rs"
                required-features = ["cli"]
                """,
                ["src/main.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "duplicate-directory")

        binary_commands = [command for command in commands if " --bin " in command]
        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p duplicate-tool --features cli --bin duplicate-tool --profile agent --locked"
            ],
            binary_commands,
        )

    def test_disabled_lib_target_is_not_registered(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "disabled-lib-directory",
                """
                [package]
                name = "disabled-lib"
                version = "0.0.0"
                edition = "2024"

                [lib]
                test = false
                """,
                ["src/lib.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "disabled-lib-directory")

        self.assertFalse(any(" --lib " in command for command in commands))

    def test_explicit_bin_claiming_main_suppresses_autodiscovered_duplicate(self) -> None:
        """An explicit `[[bin]]` owning `src/main.rs` renames it; Cargo has no
        package-named target left to run."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "renamed-main-directory",
                """
                [package]
                name = "renamed-main"
                edition = "2021"

                [[bin]]
                name = "renamed"
                path = "src/main.rs"
                """,
                ["src/main.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "renamed-main-directory")

        self.assertTrue(any("--bin renamed " in command for command in commands))
        self.assertFalse(any("--bin renamed-main " in command for command in commands))

    def test_explicit_bin_claiming_src_bin_file_suppresses_inferred_name(self) -> None:
        """A claimed `src/bin/*.rs` must not also register under its stem."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "claimed-bin-directory",
                """
                [package]
                name = "claimed-bin"
                edition = "2021"

                [[bin]]
                name = "renamed"
                path = "src/bin/helper.rs"
                """,
                ["src/lib.rs", "src/bin/helper.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "claimed-bin-directory")

        self.assertTrue(any("--bin renamed " in command for command in commands))
        self.assertFalse(any("--bin helper " in command for command in commands))

    def test_pathless_explicit_bin_claims_its_inferred_source(self) -> None:
        """A pathless `[[bin]]` named after the package still claims `src/main.rs`."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "pathless-directory",
                """
                [package]
                name = "pathless"
                edition = "2021"

                [[bin]]
                name = "pathless"
                required-features = ["cli"]
                """,
                ["src/main.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "pathless-directory")

        binary_commands = [command for command in commands if "--bin pathless " in command]
        self.assertEqual(1, len(binary_commands))
        self.assertIn("--features cli", binary_commands[0])

    def test_edition_2015_manual_target_disables_bin_autodiscovery(self) -> None:
        """Cargo 2015 stops autodiscovering `src/bin` once a target is manual."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "legacy-directory",
                """
                [package]
                name = "legacy"
                edition = "2015"

                [[bin]]
                name = "manual"
                path = "src/manual.rs"
                """,
                ["src/manual.rs", "src/bin/auto.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "legacy-directory")

        self.assertTrue(any("--bin manual " in command for command in commands))
        self.assertFalse(any("--bin auto " in command for command in commands))

    def test_edition_2015_without_manual_targets_still_autodiscovers(self) -> None:
        """Negative control: the 2015 rule keys on manual targets, not the edition."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "legacy-auto-directory",
                """
                [package]
                name = "legacy-auto"
                edition = "2015"
                """,
                ["src/bin/auto.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "legacy-auto-directory")

        self.assertTrue(any("--bin auto " in command for command in commands))

    def test_explicit_autobins_overrides_the_edition_2015_default(self) -> None:
        """A declared `autobins` wins over the edition-derived default."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "legacy-forced-directory",
                """
                [package]
                name = "legacy-forced"
                edition = "2015"
                autobins = true

                [[bin]]
                name = "manual"
                path = "src/manual.rs"
                """,
                ["src/manual.rs", "src/bin/auto.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "legacy-forced-directory")

        self.assertTrue(any("--bin auto " in command for command in commands))
        self.assertTrue(any("--bin manual " in command for command in commands))

    def test_untested_explicit_bin_still_claims_its_source(self) -> None:
        """`test = false` removes the target but must not release its source file."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "untested-directory",
                """
                [package]
                name = "untested"
                edition = "2021"

                [[bin]]
                name = "renamed"
                path = "src/main.rs"
                test = false
                """,
                ["src/main.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "untested-directory")

        self.assertFalse(any("--bin " in command for command in commands))

    def test_targets_resolve_independently_of_the_process_cwd(self) -> None:
        """The router is invoked from workflow steps with an unrelated cwd."""
        repo_root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as directory:
            previous = Path.cwd()
            try:
                os.chdir(directory)
                targets = router.package_test_targets("perl-ci-hygiene")
            finally:
                os.chdir(previous)

        self.assertEqual("perl-ci-hygiene", targets.package_name)
        self.assertIn(
            "perl-ci-hygiene", [target.name for target in targets.binaries]
        )
        self.assertTrue((repo_root / "crates" / "perl-ci-hygiene").is_dir())


if __name__ == "__main__":
    unittest.main()
