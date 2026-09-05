#!/usr/bin/env python3
"""Focused tests for changed-package binary target coverage routing."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
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


    def test_explicit_bin_path_claims_its_source_file(self) -> None:
        """An explicit `[[bin]]` into `src/bin/` must not also autobin its file.

        `crates/perl-corpus` is the live witness: it declares
        `[[bin]] name = "perl-corpus" path = "src/bin/main.rs"`, and name-only
        de-duplication invented a second `--bin main` that Cargo does not have.
        """
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "claimed-source-directory",
                """
                [package]
                name = "claimed-source"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "claimed-source"
                path = "src/bin/main.rs"
                """,
                ["src/lib.rs", "src/bin/main.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("claimed-source-directory", root)
            commands = self._commands_for(root, "claimed-source-directory")

        self.assertEqual(["claimed-source"], [target.name for target in targets.binaries])
        self.assertFalse(any(" --bin main " in command for command in commands))

    def test_pathless_explicit_bin_resolves_its_inferred_source(self) -> None:
        """A pathless `[[bin]]` claims the `src/bin/<name>.rs` Cargo infers."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "pathless-bin-directory",
                """
                [package]
                name = "pathless-bin"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "helper"
                required-features = ["cli"]
                """,
                ["src/lib.rs", "src/bin/helper.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("pathless-bin-directory", root)

        self.assertEqual(["helper"], [target.name for target in targets.binaries])
        self.assertEqual(("cli",), targets.binaries[0].required_features)

    def test_pathless_explicit_bin_resolves_nested_main(self) -> None:
        """Cargo also infers `src/bin/<name>/main.rs` for a pathless binary."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "nested-pathless-directory",
                """
                [package]
                name = "nested-pathless"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "helper"
                """,
                ["src/lib.rs", "src/bin/helper/main.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("nested-pathless-directory", root)

        self.assertEqual(["helper"], [target.name for target in targets.binaries])

    def test_test_disabled_explicit_bin_still_claims_its_source(self) -> None:
        """`test = false` removes the command but must not re-open autobin."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "untested-bin-directory",
                """
                [package]
                name = "untested-bin"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "untested-bin"
                path = "src/bin/main.rs"
                test = false
                """,
                ["src/lib.rs", "src/bin/main.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("untested-bin-directory", root)

        self.assertEqual([], [target.name for target in targets.binaries])

    def test_edition_2015_manual_target_disables_autobins(self) -> None:
        """A 2015 package declaring any target turns Cargo auto-discovery off."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "legacy-2015-directory",
                """
                [package]
                name = "legacy-2015"
                version = "0.0.0"
                edition = "2015"

                [[bin]]
                name = "declared"
                path = "src/declared.rs"
                """,
                ["src/lib.rs", "src/declared.rs", "src/bin/undiscovered.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("legacy-2015-directory", root)

        self.assertEqual(["declared"], [target.name for target in targets.binaries])

    def test_edition_2015_without_manual_targets_keeps_autobins(self) -> None:
        """Auto-discovery stays on for a 2015 package with no manual target."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "legacy-2015-auto-directory",
                """
                [package]
                name = "legacy-2015-auto"
                version = "0.0.0"
                edition = "2015"
                """,
                ["src/lib.rs", "src/bin/discovered.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("legacy-2015-auto-directory", root)

        self.assertEqual(["discovered"], [target.name for target in targets.binaries])

    def test_edition_2018_manual_target_keeps_autobins(self) -> None:
        """The 2015 rule must not leak into later editions."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "modern-manual-directory",
                """
                [package]
                name = "modern-manual"
                version = "0.0.0"
                edition = "2018"

                [[bin]]
                name = "declared"
                path = "src/declared.rs"
                """,
                ["src/lib.rs", "src/declared.rs", "src/bin/discovered.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("modern-manual-directory", root)

        self.assertEqual(
            ["declared", "discovered"], [target.name for target in targets.binaries]
        )

    def test_edition_2015_lib_does_not_disable_autobins(self) -> None:
        """Only a manual `[[bin]]` disables binary discovery in edition 2015.

        `cargo metadata` on a 2015 crate with `[lib]` plus `src/bin/foo.rs`
        reports `bin foo`: `[lib]` governs the library target only.
        """
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "legacy-lib-only-directory",
                """
                [package]
                name = "legacy-lib-only"
                version = "0.0.0"
                edition = "2015"

                [lib]
                path = "src/lib.rs"
                """,
                ["src/lib.rs", "src/bin/foo.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("legacy-lib-only-directory", root)

        self.assertEqual(["foo"], [target.name for target in targets.binaries])

    def test_omitted_edition_is_treated_as_2015(self) -> None:
        """An absent `edition` key is 2015, so a manual bin stops discovery.

        `cargo metadata` on an edition-less crate with an explicit `[[bin]]` and
        a second `src/bin/other.rs` reports only the declared binary.
        """
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "omitted-edition-directory",
                """
                [package]
                name = "omitted-edition"
                version = "0.0.0"

                [[bin]]
                name = "declared"
                path = "src/declared.rs"
                """,
                ["src/lib.rs", "src/declared.rs", "src/bin/other.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("omitted-edition-directory", root)

        self.assertEqual(["declared"], [target.name for target in targets.binaries])

    def test_omitted_edition_without_manual_bin_keeps_autobins(self) -> None:
        """The 2015 default only bites when binaries are declared manually."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "omitted-edition-auto-directory",
                """
                [package]
                name = "omitted-edition-auto"
                version = "0.0.0"
                """,
                ["src/lib.rs", "src/bin/foo.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("omitted-edition-auto-directory", root)

        self.assertEqual(["foo"], [target.name for target in targets.binaries])

    def test_workspace_inherited_edition_is_resolved(self) -> None:
        """`edition.workspace = true` must resolve, not read as a table.

        43 of this workspace's 46 crates inherit their edition.  Reading the key
        directly yields a dict, which silently takes the modern path regardless
        of what the workspace actually declares.
        """
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            (root / "Cargo.toml").write_text(
                textwrap.dedent(
                    """
                    [workspace]
                    members = ["crates/*"]

                    [workspace.package]
                    edition = "2015"
                    """
                ).lstrip(),
                encoding="utf-8",
            )
            self._write_package(
                root,
                "inherited-edition-directory",
                """
                [package]
                name = "inherited-edition"
                version = "0.0.0"
                edition.workspace = true

                [[bin]]
                name = "declared"
                path = "src/declared.rs"
                """,
                ["src/lib.rs", "src/declared.rs", "src/bin/other.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("inherited-edition-directory", root)

        self.assertEqual(["declared"], [target.name for target in targets.binaries])


    def test_explicit_lib_path_is_required_to_exist(self) -> None:
        """An explicit `[lib]` whose source is absent must not emit `--lib`."""
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "absent-lib-directory",
                """
                [package]
                name = "absent-lib"
                version = "0.0.0"
                edition = "2024"

                [lib]
                path = "src/library.rs"
                """,
                ["src/main.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("absent-lib-directory", root)

        self.assertFalse(targets.has_lib)
        self.assertEqual(["absent-lib"], [target.name for target in targets.binaries])

    def test_library_sharing_a_main_source_still_registers_the_binary(self) -> None:
        """Cargo emits a lib *and* a bin from one shared source file.

        Verified with `cargo metadata` on a scratch crate whose only manifest
        target is `[lib] path = "src/main.rs"`: Cargo reports both `lib c` and
        `bin c`.  A library therefore must not occupy a binary source.
        """
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "lib-shares-main-directory",
                """
                [package]
                name = "lib-shares-main"
                version = "0.0.0"
                edition = "2024"

                [lib]
                path = "src/main.rs"
                """,
                ["src/main.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("lib-shares-main-directory", root)

        self.assertTrue(targets.has_lib)
        self.assertEqual(["lib-shares-main"], [target.name for target in targets.binaries])

    def test_routing_is_stable_from_an_unrelated_working_directory(self) -> None:
        """Manifest and required-feature lookups must not depend on the cwd.

        The workflow does not guarantee the router runs from the checkout root,
        and a cwd-relative lookup fails silently: the feature flag disappears
        and the instrumented command stops matching the real target.
        """
        with tempfile.TemporaryDirectory() as temp_dir, tempfile.TemporaryDirectory() as elsewhere:
            root = Path(temp_dir)
            self._write_package(
                root,
                "cwd-stable-directory",
                """
                [package]
                name = "cwd-stable"
                version = "0.0.0"
                edition = "2024"

                [[bin]]
                name = "cwd-stable"
                path = "src/main.rs"
                """,
                ["src/lib.rs", "src/main.rs", "src/changed.rs"],
            )
            gated_test = root / "crates/cwd-stable-directory/tests/gated.rs"
            gated_test.parent.mkdir(parents=True, exist_ok=True)
            gated_test.write_text('#![cfg(feature = "integration")]\n', encoding="utf-8")
            changed = [
                "crates/cwd-stable-directory/src/changed.rs",
                "crates/cwd-stable-directory/tests/gated.rs",
            ]

            original_cwd = Path.cwd()
            try:
                os.chdir(elsewhere)
                selected = router.selected_packs([self._fallback_pack()], changed)
                commands = router.normalize_pack(selected[0], changed, repo_root=root)["commands"]
            finally:
                os.chdir(original_cwd)

        self.assertTrue(any(" --bin cwd-stable " in command for command in commands))
        self.assertTrue(
            any("--features integration" in command for command in commands),
            f"required features were dropped off-root: {commands}",
        )


    def test_bare_main_path_bin_falls_back_to_package_name(self) -> None:
        """A nameless `[[bin]]` at a bare `main.rs` must not derive an empty name.

        `PurePosixPath("main.rs").parent.name` is `""`, which produced a
        `--bin ` command with no target at all.
        """
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self._write_package(
                root,
                "bare-main-directory",
                """
                [package]
                name = "bare-main"
                version = "0.0.0"
                edition = "2024"
                autobins = false

                [[bin]]
                path = "main.rs"
                """,
                ["src/lib.rs", "main.rs", "src/changed.rs"],
            )

            targets = router.package_test_targets("bare-main-directory", root)

        self.assertEqual(["bare-main"], [target.name for target in targets.binaries])


class WorkspaceTargetOracleTests(unittest.TestCase):
    """Differential proof against Cargo's own view of the real workspace.

    Fixtures encode what we believe Cargo does; `cargo metadata` reports what it
    actually does.  Comparing every local crate against that oracle is what
    caught the `perl-corpus` and `tree-sitter-perl-c` phantom targets, and it
    keeps catching manifest forms no fixture anticipated.
    """

    def test_derived_targets_match_cargo_metadata(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        try:
            completed = subprocess.run(
                ["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"],
                cwd=repo_root,
                capture_output=True,
                text=True,
                timeout=300,
                check=False,
            )
        except (OSError, subprocess.SubprocessError) as error:
            self.skipTest(f"cargo metadata is unavailable: {error}")
        # Skip only when cargo is genuinely absent (handled above).  A cargo that
        # runs and fails is a real workspace or manifest error, and this oracle is
        # the control that should surface it rather than quietly pass.
        self.assertEqual(
            0,
            completed.returncode,
            f"cargo metadata failed: {completed.stderr.strip()[-400:]}",
        )

        packages = json.loads(completed.stdout)["packages"]
        mismatches: list[str] = []
        compared = 0
        for package in packages:
            manifest_path = Path(package["manifest_path"])
            if manifest_path.parent.parent.name != "crates":
                continue
            compared += 1
            expected_bins = sorted(
                target["name"]
                for target in package["targets"]
                if "bin" in target["kind"] and target.get("test", True)
            )
            expected_lib = any(
                kind in ("lib", "rlib", "proc-macro", "cdylib")
                for target in package["targets"]
                if target.get("test", True)
                for kind in target["kind"]
            )
            derived = router.package_test_targets(manifest_path.parent.name, repo_root)
            actual_bins = sorted(target.name for target in derived.binaries)
            if (
                actual_bins != expected_bins
                or derived.has_lib != expected_lib
                or derived.package_name != package["name"]
            ):
                mismatches.append(
                    f"{manifest_path.parent.name}: bins {actual_bins} != {expected_bins}, "
                    f"lib {derived.has_lib} != {expected_lib}"
                )

        self.assertGreater(compared, 0, "no local crates were compared against cargo metadata")
        self.assertEqual([], mismatches)



if __name__ == "__main__":
    unittest.main()
