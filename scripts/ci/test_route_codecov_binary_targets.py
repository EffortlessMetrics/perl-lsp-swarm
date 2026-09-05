#!/usr/bin/env python3
"""Focused tests for changed-package binary target coverage routing."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import tempfile
import tomllib
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
        with_changed_integration_test: bool = True,
    ) -> list[str]:
        """Route one changed source file and return the emitted commands.

        A changed integration test is included by default so the route takes the
        targeted `--test <name>` branch.  In the other branch the route emits a
        blanket `--tests`, which already selects the library and the ordinary
        binaries, so per-target commands are deliberately suppressed there and
        these target assertions would have nothing to observe.  The suppression
        itself is proved by `TestsFallbackSuppressionTests`.
        """
        paths = [f"crates/{crate_directory}/{changed_source}"]
        if with_changed_integration_test:
            paths.append(f"crates/{crate_directory}/tests/routed_smoke.rs")
        selected = router.selected_packs([self._fallback_pack()], paths)
        self.assertEqual([router.FALLBACK_PACK_ID], [pack["id"] for pack in selected])
        normalized = router.normalize_pack(selected[0], paths, repo_root=root)
        return normalized["commands"]

    def test_perl_ci_hygiene_binary_target_is_registered_in_the_targeted_branch(self) -> None:
        """The real witness crate resolves its bin target from its manifest.

        When a changed integration test replaces the blanket `--tests` with
        targeted `--test <name>` commands, the binary unit target has no other
        route and needs its own command.
        """
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
        self.assertFalse(any(" --tests " in command for command in commands))

    def test_perl_ci_hygiene_fallback_branch_defers_to_tests(self) -> None:
        """`--tests` already selects this crate's lib and bin unit targets.

        This is the honest form of the original #13061 witness: the blanket
        `--tests` command the route has always emitted does register
        `unittests src/main.rs`, so adding per-target commands beside it would
        only re-run that work.
        """
        repo_root = Path(__file__).resolve().parents[2]
        commands = self._commands_for(
            repo_root, "perl-ci-hygiene", "src/cli.rs", with_changed_integration_test=False
        )

        self.assertEqual(
            [
                "cargo llvm-cov test --no-report -p perl-ci-hygiene "
                "--tests --profile agent --locked -- --test-threads=1"
            ],
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

    def test_explicit_bin_path_is_normalized_before_dedup(self) -> None:
        """Source occupancy compares normalized paths, so `./src/main.rs` counts."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "dotted-path-directory",
                """
                [package]
                name = "dotted-path"
                edition = "2021"

                [[bin]]
                name = "renamed"
                path = "./src/main.rs"
                """,
                ["src/main.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "dotted-path-directory")

        self.assertTrue(any("--bin renamed " in command for command in commands))
        self.assertFalse(any("--bin dotted-path " in command for command in commands))

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
            "cargo llvm-cov test --no-report -p surviving "
            "--tests --profile agent --locked -- --test-threads=1",
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


    def test_edition_2015_explicit_lib_alone_keeps_bin_autodiscovery(self) -> None:
        """Cargo's 2015 rule is per target kind: only `[[bin]]` disables autobins.

        Verified against `cargo metadata` for an edition-2015 package declaring
        `[lib]` alongside `src/main.rs` -- the implicit bin is still exposed, so
        suppressing it here would drop real coverage.
        """
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "lib-only-2015",
                """
                [package]
                name = "lib-only-2015"
                version = "0.0.0"
                edition = "2015"

                [lib]
                path = "src/lib.rs"
                """,
                ["src/lib.rs", "src/main.rs", "src/changed.rs"],
            )

            commands = self._commands_for(root, "lib-only-2015")

        self.assertIn(
            "cargo llvm-cov test --no-report -p lib-only-2015 --lib --profile agent --locked",
            commands,
        )
        self.assertIn(
            "cargo llvm-cov test --no-report -p lib-only-2015 --bin lib-only-2015 --profile agent --locked",
            commands,
        )


    def test_integration_test_features_resolve_against_the_repository_root(self) -> None:
        """Feature discovery must be anchored the same way target derivation is.

        A cwd-relative read returns no features from any other working
        directory, and the router then emits a `--test` command without the
        `--features` Cargo needs to build that target.
        """
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            tests_root = root / "crates" / "gated-directory" / "tests"
            tests_root.mkdir(parents=True)
            (tests_root / "gated.rs").write_text(
                '#![cfg(feature = "slow")]\n', encoding="utf-8"
            )

            previous = os.getcwd()
            os.chdir(temp_dir)
            try:
                from_cwd = router.changed_integration_test_targets(
                    ["crates/gated-directory/tests/gated.rs"]
                )
            finally:
                os.chdir(previous)

            from_root = router.changed_integration_test_targets(
                ["crates/gated-directory/tests/gated.rs"], root
            )

        self.assertEqual({"gated-directory": [("gated", ("slow",))]}, from_root)
        # The default root is the repository, not the process cwd, so the
        # temporary package is invisible even while it is the cwd.
        self.assertEqual({"gated-directory": [("gated", ())]}, from_cwd)


    def test_manifest_required_features_are_merged_into_changed_test_targets(self) -> None:
        """A test target's features can be declared in the manifest, not the source.

        `perl-parser`'s `incremental_parse_snapshot` requires `incremental` with
        no crate-level cfg at all. Emitting `--test incremental_parse_snapshot`
        without `--features incremental` is a command Cargo rejects, so the
        changed test contributes no coverage.
        """
        repo_root = Path(__file__).resolve().parents[2]

        targets = router.changed_integration_test_targets(
            ["crates/perl-parser/tests/incremental_parse_snapshot.rs"], repo_root
        )

        self.assertEqual(
            {"perl-parser": [("incremental_parse_snapshot", ("incremental",))]}, targets
        )

    def test_manifest_and_source_feature_gates_are_unioned(self) -> None:
        """Neither source cfg nor manifest alone is the whole feature set."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "gated-directory",
                """
                [package]
                name = "gated-package"
                version = "0.0.0"
                edition = "2024"

                [[test]]
                name = "gated"
                required-features = ["from-manifest"]
                """,
                [],
            )
            tests_root = root / "crates" / "gated-directory" / "tests"
            tests_root.mkdir(parents=True)
            (tests_root / "gated.rs").write_text(
                '#![cfg(feature = "from-source")]\n', encoding="utf-8"
            )

            targets = router.changed_integration_test_targets(
                ["crates/gated-directory/tests/gated.rs"], root
            )

        self.assertEqual(
            {"gated-directory": [("gated", ("from-manifest", "from-source"))]}, targets
        )

    def test_manifest_features_tolerate_a_crate_without_a_manifest(self) -> None:
        """A deleted or non-package directory must not abort sibling routing."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self.assertEqual({}, router.manifest_test_targets("absent", root))

    def test_renamed_test_target_keeps_its_declared_name_and_features(self) -> None:
        """Cargo binds a target to its source file, not to the file's stem.

        Selecting `--test <stem>` for a renamed target names a target Cargo does
        not have, and drops its required features with it.
        """
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "renamed-test-directory",
                """
                [package]
                name = "renamed-test-package"
                version = "0.0.0"
                edition = "2024"

                [[test]]
                name = "declared_name"
                path = "tests/source_stem.rs"
                required-features = ["gated"]
                """,
                ["tests/source_stem.rs"],
            )

            targets = router.changed_integration_test_targets(
                ["crates/renamed-test-directory/tests/source_stem.rs"], root
            )

        self.assertEqual(
            {"renamed-test-directory": [("declared_name", ("gated",))]}, targets
        )

    def test_undeclared_test_file_still_falls_back_to_its_stem(self) -> None:
        """Most test targets are auto-discovered and have no `[[test]]` row."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "auto-test-directory",
                """
                [package]
                name = "auto-test-package"
                version = "0.0.0"
                edition = "2024"
                """,
                ["tests/discovered.rs"],
            )

            targets = router.changed_integration_test_targets(
                ["crates/auto-test-directory/tests/discovered.rs"], root
            )

        self.assertEqual({"auto-test-directory": [("discovered", ())]}, targets)

    def test_every_declared_test_target_feature_set_is_recovered(self) -> None:
        """Parity oracle: the router must agree with every manifest in the tree.

        Guards the whole class of defect rather than the three crates that
        happen to exercise it today.
        """
        repo_root = Path(__file__).resolve().parents[2]
        mismatches = []
        for manifest_path in sorted((repo_root / "crates").glob("*/Cargo.toml")):
            crate_directory = manifest_path.parent.name
            manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
            for declared in manifest.get("test", []):
                required = declared.get("required-features") or []
                name = declared.get("name")
                if not required or not name:
                    continue
                source = declared.get("path") or f"tests/{name}.rs"
                if not (manifest_path.parent / source).is_file():
                    continue
                derived = router.changed_integration_test_targets(
                    [f"crates/{crate_directory}/{source}"], repo_root
                )
                emitted = dict(derived.get(crate_directory, []))
                for feature in required:
                    if feature not in emitted.get(name, ()):
                        mismatches.append((crate_directory, name, feature))

        self.assertEqual([], mismatches)


class PackageTestTargetsUnitTests(unittest.TestCase):
    """Direct assertions on the derived data structure.

    The routing tests above assert on emitted command strings, which cannot
    distinguish a dropped `has_lib` from a command-emission change, and never
    observe binary ordering.  The command loop depends on both.
    """

    def _write_package(
        self, root: Path, crate_directory: str, manifest: str, files: list[str]
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

    def test_dual_target_package_shape(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "dual-directory",
                """
                [package]
                name = "dual"
                edition = "2021"
                """,
                ["src/lib.rs", "src/main.rs"],
            )
            targets = router.package_test_targets("dual-directory", root)

        self.assertIsNotNone(targets)
        assert targets is not None
        self.assertEqual("dual", targets.package_name)
        self.assertTrue(targets.has_lib)
        self.assertEqual((router.BinaryTestTarget("dual"),), targets.binaries)

    def test_autolib_false_drops_the_library(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "nolib-directory",
                """
                [package]
                name = "nolib"
                edition = "2021"
                autolib = false
                """,
                ["src/lib.rs", "src/main.rs"],
            )
            targets = router.package_test_targets("nolib-directory", root)

        assert targets is not None
        self.assertFalse(targets.has_lib)

    def test_required_features_are_carried_on_the_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "gated-directory",
                """
                [package]
                name = "gated"
                edition = "2021"

                [[bin]]
                name = "gated"
                required-features = ["beta", "alpha"]
                """,
                ["src/main.rs"],
            )
            targets = router.package_test_targets("gated-directory", root)

        assert targets is not None
        self.assertEqual(
            (router.BinaryTestTarget("gated", ("alpha", "beta")),), targets.binaries
        )

    def test_binaries_are_ordered_by_name(self) -> None:
        """The command loop emits in iteration order; routing must be stable."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "ordered-directory",
                """
                [package]
                name = "ordered"
                edition = "2021"
                """,
                ["src/bin/zulu.rs", "src/bin/alpha.rs", "src/bin/mike.rs"],
            )
            targets = router.package_test_targets("ordered-directory", root)

        assert targets is not None
        names = [target.name for target in targets.binaries]
        self.assertEqual(sorted(names), names)
        self.assertEqual(["alpha", "mike", "zulu"], names)

    def test_non_package_directory_returns_none(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "crates" / "vendored").mkdir(parents=True)
            self.assertIsNone(router.package_test_targets("vendored", root))


class TestsFallbackSuppressionTests(unittest.TestCase):
    """`--tests` selects lib and bin unit targets, so the route must not duplicate them.

    Verified against Cargo directly: `cargo test -p perl-ci-hygiene --tests
    --no-run` builds both `unittests src/lib.rs` and `unittests src/main.rs`.
    """

    def _fallback_pack(self) -> dict[str, object]:
        return {
            "id": router.FALLBACK_PACK_ID,
            "files": ["*.rs"],
            "commands": [],
            "coverage_filters": ["changed-package-targets"],
        }

    def _write_package(
        self, root: Path, crate_directory: str, manifest: str, files: list[str]
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

    def _commands(self, root: Path, paths: list[str]) -> list[str]:
        selected = router.selected_packs([self._fallback_pack()], paths)
        return router.normalize_pack(selected[0], paths, repo_root=root)["commands"]

    def _gated_package(self, root: Path) -> None:
        self._write_package(
            root,
            "gated-directory",
            """
            [package]
            name = "gated"
            edition = "2021"

            [[bin]]
            name = "plain"
            path = "src/bin/plain.rs"

            [[bin]]
            name = "gated"
            required-features = ["cli"]
            """,
            ["src/lib.rs", "src/main.rs", "src/bin/plain.rs", "src/changed.rs"],
        )

    def test_fallback_branch_emits_only_tests_and_feature_gated_targets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._gated_package(root)
            commands = self._commands(root, ["crates/gated-directory/src/changed.rs"])

        # `--tests` covers the library and the plain binary; the required-feature
        # binary is unreachable that way and keeps its own command.
        self.assertFalse(any(" --lib " in command for command in commands))
        self.assertFalse(any("--bin plain " in command for command in commands))
        self.assertTrue(
            any(
                "--features cli --bin gated " in command for command in commands
            ),
            commands,
        )
        self.assertTrue(any(" --tests " in command for command in commands))

    def test_targeted_branch_keeps_every_per_target_command(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._gated_package(root)
            commands = self._commands(
                root,
                [
                    "crates/gated-directory/src/changed.rs",
                    "crates/gated-directory/tests/smoke.rs",
                ],
            )

        self.assertTrue(any(" --lib " in command for command in commands))
        self.assertTrue(any("--bin plain " in command for command in commands))
        self.assertTrue(any("--features cli --bin gated " in command for command in commands))
        self.assertTrue(any("--test smoke " in command for command in commands))
        self.assertFalse(any(" --tests " in command for command in commands))


class IntegrationTargetFeatureTests(unittest.TestCase):
    """Manifest `[[test]]` identity and required features reach the command.

    Cargo rejects `--test <name>` outright when the target is renamed or its
    required features are missing:

        error: target `incremental_parse_snapshot` in package `perl-parser`
        requires the features: `incremental`

    Source-level `#![cfg(feature = ...)]` scanning alone does not see them.
    """

    def _fallback_pack(self) -> dict[str, object]:
        return {
            "id": router.FALLBACK_PACK_ID,
            "files": ["*.rs"],
            "commands": [],
            "coverage_filters": ["changed-package-targets"],
        }

    def _write_package(
        self, root: Path, crate_directory: str, manifest: str, files: list[str]
    ) -> None:
        crate_root = root / "crates" / crate_directory
        crate_root.mkdir(parents=True)
        (crate_root / "Cargo.toml").write_text(
            textwrap.dedent(manifest).lstrip(), encoding="utf-8"
        )
        for relative_path, contents in files:
            path = crate_root / relative_path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents, encoding="utf-8")

    def _targeted_command(self, root: Path, crate_directory: str, test_file: str) -> str:
        paths = [
            f"crates/{crate_directory}/src/changed.rs",
            f"crates/{crate_directory}/tests/{test_file}",
        ]
        selected = router.selected_packs([self._fallback_pack()], paths)
        commands = router.normalize_pack(selected[0], paths, repo_root=root)["commands"]
        targeted = [command for command in commands if " --test " in command]
        self.assertEqual(1, len(targeted), commands)
        return targeted[0]

    def test_manifest_features_reach_a_test_with_no_source_cfg(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "gated-test-directory",
                """
                [package]
                name = "gated-test"
                edition = "2021"

                [[test]]
                name = "snapshot"
                required-features = ["incremental"]
                """,
                [
                    ("src/lib.rs", "// lib\n"),
                    ("src/changed.rs", "// changed\n"),
                    ("tests/snapshot.rs", "// no crate-level cfg gate\n"),
                ],
            )
            command = self._targeted_command(root, "gated-test-directory", "snapshot.rs")

        self.assertIn("--features incremental", command)
        self.assertIn("--test snapshot ", command)

    def test_manifest_and_source_features_are_merged(self) -> None:
        """The two gates can name different features; both are required."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "dual-gate-directory",
                """
                [package]
                name = "dual-gate"
                edition = "2021"

                [[test]]
                name = "capability"
                path = "tests/capability.rs"
                required-features = ["test-helpers"]
                """,
                [
                    ("src/lib.rs", "// lib\n"),
                    ("src/changed.rs", "// changed\n"),
                    ("tests/capability.rs", '#![cfg(feature = "phase2")]\n'),
                ],
            )
            command = self._targeted_command(root, "dual-gate-directory", "capability.rs")

        self.assertIn("--features phase2,test-helpers", command)

    def test_manifest_target_name_overrides_the_file_stem(self) -> None:
        """A renamed `[[test]]` must be selected by its declared name."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "renamed-test-directory",
                """
                [package]
                name = "renamed-test"
                edition = "2021"

                [[test]]
                name = "declared_name"
                path = "tests/file_stem.rs"
                """,
                [
                    ("src/lib.rs", "// lib\n"),
                    ("src/changed.rs", "// changed\n"),
                    ("tests/file_stem.rs", "// renamed target\n"),
                ],
            )
            command = self._targeted_command(root, "renamed-test-directory", "file_stem.rs")

        self.assertIn("--test declared_name ", command)
        self.assertNotIn("--test file_stem ", command)

    def test_undeclared_test_file_still_uses_its_stem(self) -> None:
        """Negative control: autodiscovered tests keep the existing behavior."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_package(
                root,
                "plain-test-directory",
                """
                [package]
                name = "plain-test"
                edition = "2021"
                """,
                [
                    ("src/lib.rs", "// lib\n"),
                    ("src/changed.rs", "// changed\n"),
                    ("tests/smoke.rs", "// plain\n"),
                ],
            )
            command = self._targeted_command(root, "plain-test-directory", "smoke.rs")

        self.assertIn("--test smoke ", command)
        self.assertNotIn("--features", command)

if __name__ == "__main__":
    unittest.main()
