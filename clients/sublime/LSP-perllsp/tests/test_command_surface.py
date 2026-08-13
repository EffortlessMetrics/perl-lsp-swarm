from __future__ import annotations

import importlib.util
import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
REPO = Path(__file__).resolve().parents[4]
SPEC = importlib.util.spec_from_file_location(
    "perllsp_command_surface",
    PACKAGE / "command_surface.py",
)
assert SPEC and SPEC.loader
surface = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = surface
SPEC.loader.exec_module(surface)


class CommandSurfaceContractTests(unittest.TestCase):
    def test_palette_is_curated_and_registry_owned(self) -> None:
        palette = json.loads((PACKAGE / "Default.sublime-commands").read_text(encoding="utf-8"))
        actions = {entry["args"]["action"] for entry in palette}
        self.assertEqual(actions, set(surface.COMMAND_SPECS))
        self.assertEqual({entry["command"] for entry in palette}, {"perllsp_execute"})
        exposed = {surface.COMMAND_SPECS[action].command_id for action in actions}
        self.assertTrue(exposed.isdisjoint(surface.DESTRUCTIVE_COMMAND_IDS))
        critic = next(entry for entry in palette if entry["args"]["action"] == "run_critic_compatibility")
        self.assertIn("Compatibility Surface", critic["caption"])

    def test_registry_is_checked_against_server_command_authority(self) -> None:
        source = (
            REPO / "crates" / "perl-lsp-rs-core" / "src" / "protocol" / "capabilities.rs"
        ).read_text(encoding="utf-8")
        match = re.search(
            r"SUPPORTED_COMMANDS[^=]*=\s*&\[(.*?)\];",
            source,
            re.DOTALL,
        )
        self.assertIsNotNone(match, "SUPPORTED_COMMANDS declaration was not found")
        block = match.group(1)
        for command_id in sorted(surface.command_ids()):
            self.assertIn(f'"{command_id}"', block)

    def test_current_file_commands_use_the_active_saved_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            file_path = Path(directory) / "lib" / "Example.pm"
            for action in ("run_current_file", "run_current_test", "run_critic_compatibility"):
                invocation = surface.prepare_invocation(
                    action,
                    surface.command_ids(),
                    file_path=str(file_path),
                    workspace_folders=[directory],
                    line=3,
                    character=8,
                )
                self.assertEqual(invocation.arguments, [str(file_path.resolve())])

    def test_workspace_command_selects_the_deepest_owning_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            nested = root / "service"
            file_path = nested / "t" / "feature.t"
            invocation = surface.prepare_invocation(
                "run_workspace_tests",
                surface.command_ids(),
                file_path=str(file_path),
                workspace_folders=[str(root), str(nested), str(root / "other")],
                line=0,
                character=0,
            )
            self.assertEqual(invocation.arguments, [str(nested.resolve())])
            self.assertEqual(invocation.workspace_path, str(nested.resolve()))

    def test_position_preview_uses_the_active_document_and_cursor(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            file_path = Path(directory) / "lib" / "Example.pm"
            invocation = surface.prepare_invocation(
                "preview_safe_delete",
                surface.command_ids(),
                file_path=str(file_path),
                workspace_folders=[directory],
                line=7,
                character=11,
            )
            self.assertEqual(
                invocation.arguments,
                [
                    {
                        "textDocument": {"uri": file_path.resolve().as_uri()},
                        "position": {"line": 7, "character": 11},
                    }
                ],
            )

    def test_unsaved_nonowning_and_unadvertised_commands_fail_closed(self) -> None:
        with self.assertRaisesRegex(surface.CommandSurfaceError, "Save the active Perl buffer"):
            surface.prepare_invocation(
                "run_current_file",
                surface.command_ids(),
                file_path=None,
                workspace_folders=[],
                line=0,
                character=0,
            )

        with tempfile.TemporaryDirectory() as directory:
            file_path = Path(directory) / "outside.pl"
            with self.assertRaisesRegex(surface.CommandSurfaceError, "not owned"):
                surface.prepare_invocation(
                    "run_workspace_tests",
                    surface.command_ids(),
                    file_path=str(file_path),
                    workspace_folders=[str(Path(directory) / "other")],
                    line=0,
                    character=0,
                )

            advertised = surface.command_ids() - {"perl.runFile"}
            with self.assertRaisesRegex(surface.CommandSurfaceError, "did not advertise"):
                surface.prepare_invocation(
                    "run_current_file",
                    advertised,
                    file_path=str(file_path),
                    workspace_folders=[directory],
                    line=0,
                    character=0,
                )

    def test_result_and_error_rendering_are_bounded_and_preserve_failures(self) -> None:
        result = surface.format_result(
            "Perl: Run Current File",
            {"success": False, "stdout": "x" * (surface.MAX_OUTPUT_CHARS + 100), "error": "boom"},
        )
        self.assertIn("Success: no", result)
        self.assertIn("Error: boom", result)
        self.assertIn("omitted by LSP-perllsp", result)

        error = type("ServerError", (), {"message": "interpreter unavailable"})()
        rendered = surface.format_error("Perl: Run Current File", "perl.runFile", error)
        self.assertIn("Status: failed", rendered)
        self.assertIn("perl.runFile", rendered)
        self.assertIn("interpreter unavailable", rendered)

    def test_navigation_target_is_deliberate(self) -> None:
        self.assertEqual(
            surface.navigation_target({"found": True, "path": "/workspace/t/example.t"}),
            "/workspace/t/example.t",
        )
        self.assertIsNone(surface.navigation_target({"found": False, "candidates": []}))
        self.assertEqual(
            surface.navigation_target({"candidates": [{"uri": "file:///workspace/t/example.t"}]}),
            "file:///workspace/t/example.t",
        )

    def test_plugin_selects_the_active_named_session_and_never_shells_out(self) -> None:
        source = (PACKAGE / "plugin.py").read_text(encoding="utf-8")
        self.assertIn('session_name = "LSP-perllsp"', source)
        self.assertIn("self.session_by_name(self.session_name)", source)
        self.assertIn("No active LSP-perllsp session owns the current Perl view", source)
        self.assertIn('session.get_capability("executeCommandProvider.commands"', source)
        self.assertIn("isinstance(response, Error)", source)
        self.assertIn("format_error", source)
        for forbidden in ("subprocess", "os.system", "Popen(", "shell=True", "perlcritic ", "prove "):
            self.assertNotIn(forbidden, source)

    def test_deterministic_export_contains_the_command_runtime(self) -> None:
        exporter = (PACKAGE.parent / "export_lsp_perllsp.py").read_text(encoding="utf-8")
        self.assertIn('"Default.sublime-commands"', exporter)
        self.assertIn('"command_surface.py"', exporter)


if __name__ == "__main__":
    unittest.main()
