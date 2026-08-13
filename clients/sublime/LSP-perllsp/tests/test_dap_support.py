from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("perllsp_dap_support", PACKAGE / "dap_support.py")
assert SPEC and SPEC.loader
support = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = support
SPEC.loader.exec_module(support)


class DapSupportContractTests(unittest.TestCase):
    def test_explicit_absolute_binary_is_user_authority(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / support.DAP_EXECUTABLE
            binary.write_bytes(b"exact")
            resolved = support.resolve_dap_path(str(binary), which=lambda _: None)
            self.assertEqual(resolved, binary.resolve())
            self.assertEqual(support.dap_command(resolved), [str(binary.resolve()), "--stdio"])

    def test_explicit_bare_name_uses_path_but_relative_path_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / support.DAP_EXECUTABLE
            binary.write_bytes(b"exact")
            resolved = support.resolve_dap_path(
                "company-perl-dap",
                which=lambda name: str(binary) if name == "company-perl-dap" else None,
            )
            self.assertEqual(resolved, binary.resolve())

        with self.assertRaisesRegex(support.DapPathError, "absolute path or a bare executable"):
            support.resolve_dap_path("tools/perl-dap", which=lambda _: None)

    def test_auto_prefers_sibling_of_explicit_server_before_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            server = root / ("perllsp.exe" if os.name == "nt" else "perllsp")
            sibling = root / support.DAP_EXECUTABLE
            other = root / "path-perl-dap"
            server.write_bytes(b"server")
            sibling.write_bytes(b"sibling")
            other.write_bytes(b"path")
            resolved = support.resolve_dap_path(
                "auto",
                server_path=str(server),
                which=lambda name: str(other) if name == "perl-dap" else None,
            )
            self.assertEqual(resolved, sibling.resolve())

    def test_auto_uses_path_and_missing_binary_is_actionable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / support.DAP_EXECUTABLE
            binary.write_bytes(b"path")
            resolved = support.resolve_dap_path(
                "auto",
                which=lambda name: str(binary) if name == "perl-dap" else None,
            )
            self.assertEqual(resolved, binary.resolve())

        with self.assertRaisesRegex(support.DapPathError, "managed perllsp release"):
            support.resolve_dap_path("auto", which=lambda _: None)

    def test_settings_and_project_example_define_the_bounded_contract(self) -> None:
        settings = json.loads((PACKAGE / "LSP-perllsp.sublime-settings").read_text(encoding="utf-8"))
        schema = json.loads((PACKAGE / "sublime-package.json").read_text(encoding="utf-8"))
        example = json.loads((PACKAGE / "Perl.sublime-project.example").read_text(encoding="utf-8"))

        self.assertEqual(settings["dap_path"], "auto")
        properties = schema["contributions"]["settings"][0]["schema"]["definitions"]["PluginConfig"]["properties"]
        self.assertEqual(properties["dap_path"]["default"], "auto")
        self.assertIn("does not include a verified perl-dap artifact", properties["dap_path"]["markdownDescription"])

        config = example["debugger_configurations"][0]
        self.assertEqual(config["type"], "perl")
        self.assertEqual(config["request"], "launch")
        self.assertEqual(config["program"], "${file}")
        self.assertEqual(config["cwd"], "${workspaceFolder}")
        self.assertNotIn("adapterPath", config)
        self.assertNotIn("dap_path", config)

    def test_adapter_registration_is_direct_stdio_and_project_cannot_replace_binary(self) -> None:
        source = (PACKAGE / "debugger_adapter.py").read_text(encoding="utf-8")
        plugin = (PACKAGE / "plugin.py").read_text(encoding="utf-8")

        self.assertIn('type = "perl"', source)
        self.assertIn("StdioTransport(command=command, cwd=cwd)", source)
        self.assertIn('return [str(path.resolve()), "--stdio"]', (PACKAGE / "dap_support.py").read_text(encoding="utf-8"))
        self.assertIn('settings.get("dap_path", "auto")', source)
        self.assertIn('settings.get("server_path", "auto")', source)
        self.assertNotIn('configuration.get("dap_path"', source)
        self.assertNotIn('configuration.get("adapterPath"', source)
        self.assertNotIn("SocketTransport", source)
        self.assertNotIn("subprocess", source)
        self.assertNotIn("perl.debugFile", (PACKAGE / "Default.sublime-commands").read_text(encoding="utf-8"))
        self.assertIn("register_debugger_adapter()", plugin)
        self.assertIn("PerllspDebuggerRegistrationListener", plugin)

    def test_adapter_launch_snippet_matches_canonical_perl_dap_launch_shape(self) -> None:
        source = (PACKAGE / "debugger_adapter.py").read_text(encoding="utf-8")
        for field in (
            '"program": r"\\${file}"',
            '"cwd": r"\\${workspaceFolder}"',
            '"perlPath": "perl"',
            '"args": []',
            '"includePaths": [r"\\${workspaceFolder}/lib"]',
            '"env": {}',
        ):
            self.assertIn(field, source)

    def test_exporter_contains_the_dap_runtime(self) -> None:
        exporter = (PACKAGE.parent / "export_lsp_perllsp.py").read_text(encoding="utf-8")
        for relative in ("dap_support.py", "debugger_adapter.py", "Perl.sublime-project.example"):
            self.assertIn(f'"{relative}"', exporter)


if __name__ == "__main__":
    unittest.main()
