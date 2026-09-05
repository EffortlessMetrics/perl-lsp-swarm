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

    def test_workspace_witnesses_match_cargo_target_identity(self) -> None:
        """Real manifests where an explicit bin renames a `src/bin` source.

        `perl-corpus` declares `perl-corpus` at `src/bin/main.rs` and
        `tree-sitter-perl-c` declares `bench_parser_c` at
        `src/bin/bench_parser.rs`.  Deriving names from the file layout alone
        invents `--bin main` and `--bin bench_parser`, which Cargo rejects.
        """
        repo_root = Path(__file__).resolve().parents[2]

        corpus = router.package_test_targets("perl-corpus", repo_root)
        self.assertEqual(
            ["perl-corpus"], [target.name for target in corpus.binaries]
        )

        tree_sitter = router.package_test_targets("tree-sitter-perl-c", repo_root)
        self.assertEqual(
            ["bench_parser_c", "parse_c"],
            [target.name for target in tree_sitter.binaries],
        )
        self.assertEqual(
            ("test-utils",), tree_sitter.binaries[0].required_features
        )

    def test_explicit_path_suppresses_the_implicit_target_for_the_same_source(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "renamed-directory",
                """
                [package]
                name = "renamed-bin"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "published"
                path = "src/bin/internal.rs"
                """,
                ["src/lib.rs", "src/changed.rs", "src/bin/internal.rs"],
            )

            commands = self._commands_for(root, "renamed-directory")

        binary_commands = [command for command in commands if " --bin " in command]
        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p renamed-bin --bin published --profile agent --locked"
            ],
            binary_commands,
        )

    def test_untestable_explicit_target_still_reserves_its_source_path(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "reserved-directory",
                """
                [package]
                name = "reserved-tool"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "reserved"
                path = "src/bin/helper.rs"
                test = false
                """,
                ["src/lib.rs", "src/changed.rs", "src/bin/helper.rs"],
            )

            commands = self._commands_for(root, "reserved-directory")

        self.assertFalse(any(" --bin " in command for command in commands))

    def test_pathless_explicit_bin_reserves_its_inferred_source(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "pathless-directory",
                """
                [package]
                name = "pathless-tool"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "helper"
                required-features = ["cli"]
                """,
                ["src/lib.rs", "src/changed.rs", "src/bin/helper.rs"],
            )

            commands = self._commands_for(root, "pathless-directory")

        binary_commands = [command for command in commands if " --bin " in command]
        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p pathless-tool --features cli --bin helper --profile agent --locked"
            ],
            binary_commands,
        )

    def test_edition_2015_manual_bin_disables_autobin_discovery(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "legacy-directory",
                """
                [package]
                name = "legacy-tool"
                version = "0.0.0"
                edition = "2015"

                [[bin]]
                name = "declared"
                path = "tools/declared.rs"
                """,
                [
                    "src/lib.rs",
                    "src/changed.rs",
                    "src/bin/undeclared.rs",
                    "tools/declared.rs",
                ],
            )

            commands = self._commands_for(root, "legacy-directory")

        binary_commands = [command for command in commands if " --bin " in command]
        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p legacy-tool --bin declared --profile agent --locked"
            ],
            binary_commands,
        )

    def test_edition_2015_without_manual_bin_keeps_autobin_discovery(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "legacy-auto-directory",
                """
                [package]
                name = "legacy-auto"
                version = "0.0.0"
                edition = "2015"
                """,
                ["src/lib.rs", "src/changed.rs", "src/bin/discovered.rs"],
            )

            commands = self._commands_for(root, "legacy-auto-directory")

        binary_commands = [command for command in commands if " --bin " in command]
        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p legacy-auto --bin discovered --profile agent --locked"
            ],
            binary_commands,
        )

    def test_missing_edition_defaults_to_2015_autobin_behavior(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "no-edition-directory",
                """
                [package]
                name = "no-edition"
                version = "0.0.0"

                [[bin]]
                name = "declared"
                path = "tools/declared.rs"
                """,
                ["src/lib.rs", "src/changed.rs", "src/bin/undeclared.rs", "tools/declared.rs"],
            )

            commands = self._commands_for(root, "no-edition-directory")

        binary_commands = [command for command in commands if " --bin " in command]
        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p no-edition --bin declared --profile agent --locked"
            ],
            binary_commands,
        )

    def test_inherited_workspace_edition_is_resolved(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            (root / "Cargo.toml").write_text(
                textwrap.dedent(
                    """
                    [workspace]
                    members = ["crates/inherited-directory"]

                    [workspace.package]
                    edition = "2024"
                    """
                ).lstrip(),
                encoding="utf-8",
            )
            self._write_package(
                root,
                "inherited-directory",
                """
                [package]
                name = "inherited-tool"
                version = "0.0.0"
                edition.workspace = true

                [[bin]]
                name = "declared"
                path = "tools/declared.rs"
                """,
                [
                    "src/lib.rs",
                    "src/changed.rs",
                    "src/bin/discovered.rs",
                    "tools/declared.rs",
                ],
            )

            commands = self._commands_for(root, "inherited-directory")

        binary_commands = [command for command in commands if " --bin " in command]
        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p inherited-tool --bin declared --profile agent --locked",
                "cargo llvm-cov test --no-report -p inherited-tool --bin discovered --profile agent --locked",
            ],
            binary_commands,
        )

    def test_inherited_edition_without_workspace_declaration_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            (root / "Cargo.toml").write_text(
                "[workspace]\nmembers = []\n", encoding="utf-8"
            )
            self._write_package(
                root,
                "unresolvable-directory",
                """
                [package]
                name = "unresolvable-tool"
                version = "0.0.0"
                edition.workspace = true
                """,
                ["src/lib.rs", "src/changed.rs"],
            )

            with self.assertRaises(ValueError):
                router.package_test_targets("unresolvable-directory", root)

    def test_derivation_is_independent_of_the_process_working_directory(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        expected = router.package_test_targets("perl-ci-hygiene", repo_root)

        original_cwd = Path.cwd()
        with tempfile.TemporaryDirectory() as temp_dir:
            os.chdir(temp_dir)
            try:
                default_root = router.package_test_targets("perl-ci-hygiene")
            finally:
                os.chdir(original_cwd)

        self.assertEqual(expected, default_root)


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

    def test_non_package_crate_directory_is_skipped_not_fatal(self) -> None:
        """`crates/tree-sitter-perl` is a vendored grammar, not a Cargo package."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            grammar_src = root / "crates" / "vendored-grammar" / "src"
            grammar_src.mkdir(parents=True)
            (grammar_src / "scanner.rs").write_text("// grammar\n", encoding="utf-8")

            commands = self._commands_for(root, "vendored-grammar", "src/scanner.rs")

        self.assertEqual([], commands)

    def test_real_vendored_grammar_directory_does_not_abort_the_route(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        if not (repo_root / "crates" / "tree-sitter-perl").is_dir():
            self.skipTest("vendored tree-sitter-perl directory is absent")
        self.assertFalse((repo_root / "crates" / "tree-sitter-perl" / "Cargo.toml").is_file())
        self.assertIsNone(router.package_test_targets("tree-sitter-perl", repo_root))

    def test_deleted_crate_does_not_abort_surviving_packages(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "surviving",
                """
                [package]
                name = "surviving"
                version = "0.0.0"
                edition = "2024"
                """,
                ["src/lib.rs", "src/changed.rs"],
            )
            paths = [
                "crates/removed-crate/src/gone.rs",
                "crates/surviving/src/changed.rs",
            ]
            selected = router.selected_packs([self._fallback_pack()], paths)
            commands = router.normalize_pack(selected[0], paths, repo_root=root)["commands"]

        self.assertIn(
            "cargo llvm-cov test --no-report -p surviving --lib --profile agent --locked",
            commands,
        )
        self.assertTrue(all("removed-crate" not in command for command in commands))

    def test_explicit_bin_without_source_is_not_registered(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "ghost-directory",
                """
                [package]
                name = "ghost-tool"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "ghost"
                path = "tools/ghost.rs"
                """,
                ["src/lib.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "ghost-directory")

        self.assertFalse(any(" --bin " in command for command in commands))
        self.assertIn(
            "cargo llvm-cov test --no-report -p ghost-tool --lib --profile agent --locked",
            commands,
        )

    def test_pathless_explicit_bin_without_source_is_not_registered(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "ghostless-directory",
                """
                [package]
                name = "ghostless-tool"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "absent"
                """,
                ["src/lib.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "ghostless-directory")

        self.assertFalse(any(" --bin " in command for command in commands))


if __name__ == "__main__":
    unittest.main()
