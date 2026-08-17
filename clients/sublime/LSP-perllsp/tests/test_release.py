from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("perllsp_release", PACKAGE / "release.py")
assert SPEC and SPEC.loader
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ReleaseContractTests(unittest.TestCase):
    def test_manifest_is_pinned_and_complete(self) -> None:
        manifest = release.load_manifest(PACKAGE / "server-manifest.json")
        self.assertEqual(manifest["version"], "0.17.0")
        self.assertEqual(manifest["tested_lsp_package"], "2.13.0")
        # Each reviewed asset record is locked to its exact expected values:
        # a changed target, filename, digest, or binary name must fail here,
        # not merely a changed version substring in the filename.
        expected_assets = {
            "osx-x64": (
                "x86_64-apple-darwin",
                "perllsp-0.17.0-x86_64-apple-darwin.tar.gz",
                "b117abfe29bb9840f21433d5ffff8c47a2fda1f6815cf477230d3f3b3ec22e74",
                "perllsp",
            ),
            "osx-arm64": (
                "aarch64-apple-darwin",
                "perllsp-0.17.0-aarch64-apple-darwin.tar.gz",
                "cb00d92395e94caee6e565b2e74f18cbbcd2b9366ddd1e13a6b5df7660f77ffd",
                "perllsp",
            ),
            "windows-x64": (
                "x86_64-pc-windows-msvc",
                "perllsp-0.17.0-x86_64-pc-windows-msvc.zip",
                "679cd43780d36cb2a980a42804905d24d2c76b0a3e2deaa3cd344daece87f7dd",
                "perllsp.exe",
            ),
            "linux-x64": (
                "x86_64-unknown-linux-gnu",
                "perllsp-0.17.0-x86_64-unknown-linux-gnu.tar.gz",
                "23e742925e8d517230c93ca0674772c6dfa480ad6a571c9b1fb2ab031bb97d88",
                "perllsp",
            ),
            "linux-arm64": (
                "aarch64-unknown-linux-gnu",
                "perllsp-0.17.0-aarch64-unknown-linux-gnu.tar.gz",
                "a83e085590e1cf071a5c2ae6e8bea28e491d767d108ea2464bab6c653dc4acca",
                "perllsp",
            ),
        }
        for platform, arch in [
            ("osx", "x64"),
            ("osx", "arm64"),
            ("windows", "x64"),
            ("linux", "x64"),
            ("linux", "arm64"),
        ]:
            asset = release.select_asset(manifest, platform, arch)
            self.assertIn(manifest["version"], asset["asset"])
            self.assertTrue(release.release_url(manifest, asset).startswith(
                "https://github.com/EffortlessMetrics/perl-lsp/releases/download/v0.17.0/"
            ))
            key = f"{platform}-{arch}"
            target, filename, digest, binary = expected_assets[key]
            self.assertEqual(asset["target"], target, key)
            self.assertEqual(asset["asset"], filename, key)
            self.assertEqual(asset["sha256"], digest, key)
            self.assertEqual(asset["binary"], binary, key)

    def test_unknown_platform_fails_closed(self) -> None:
        manifest = release.load_manifest(PACKAGE / "server-manifest.json")
        with self.assertRaises(release.UnsupportedPlatform):
            release.select_asset(manifest, "windows", "arm64")

    def test_checksum_mismatch_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "asset"
            path.write_bytes(b"not the expected archive")
            with self.assertRaises(release.ManifestError):
                release.verify_sha256(path, "0" * 64)

    def test_extracts_one_nested_binary_from_zip(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "perllsp.zip"
            with zipfile.ZipFile(archive, "w") as package:
                package.writestr("perllsp-0.17.0-x86_64-pc-windows-msvc/perllsp.exe", b"binary")
            output = root / "perllsp.exe"
            release.extract_binary(archive, "perllsp.exe", output)
            self.assertEqual(output.read_bytes(), b"binary")

    def test_extracts_one_nested_binary_from_tar(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "perllsp.tar.gz"
            payload = b"binary"
            with tarfile.open(archive, "w:gz") as package:
                info = tarfile.TarInfo("perllsp-0.17.0-x86_64-unknown-linux-gnu/perllsp")
                info.size = len(payload)
                package.addfile(info, io.BytesIO(payload))
            output = root / "perllsp"
            release.extract_binary(archive, "perllsp", output)
            self.assertEqual(output.read_bytes(), payload)

    def test_ambiguous_archive_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "perllsp.zip"
            with zipfile.ZipFile(archive, "w") as package:
                package.writestr("one/perllsp", b"one")
                package.writestr("two/perllsp", b"two")
            with self.assertRaises(release.ManifestError):
                release.extract_binary(archive, "perllsp", root / "perllsp")

    def test_installed_binary_metadata_detects_tampering(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "perllsp"
            binary.write_bytes(b"trusted")
            binary.chmod(0o755)
            digest = hashlib.sha256(b"trusted").hexdigest()
            binary.with_name("install.json").write_text(
                json.dumps({"archive_sha256": "a" * 64, "binary_sha256": digest}),
                encoding="utf-8",
            )
            asset = {"sha256": "a" * 64}
            self.assertTrue(release.installed_binary_is_current(binary, asset))
            binary.write_bytes(b"tampered")
            self.assertFalse(release.installed_binary_is_current(binary, asset))
            # A binary whose execute permission was lost is not a usable
            # install even when its content still matches.
            binary.write_bytes(b"trusted")
            binary.chmod(0o644)
            self.assertFalse(release.installed_binary_is_current(binary, asset))

    def test_settings_bind_perl_and_custom_tokens(self) -> None:
        settings = json.loads((PACKAGE / "LSP-perllsp.sublime-settings").read_text(encoding="utf-8"))
        self.assertEqual(settings["selector"], "source.perl")
        self.assertEqual(settings["command"], ["${server_path}", "--stdio"])
        self.assertEqual(settings["server_path"], "auto")
        self.assertEqual(settings["syntax_map"]["perldoc"], "Packages/Perl/Perl.sublime-syntax")
        for token in [
            "sql_string",
            "sql_heredoc_keyword",
            "json_heredoc_key",
            "variable.scalarVariable",
            "variable.arrayVariable",
            "variable.hashVariable",
        ]:
            self.assertIn(token, settings["semantic_tokens"])

    def test_plugin_reconstructs_launch_authority(self) -> None:
        source = (PACKAGE / "plugin.py").read_text(encoding="utf-8")
        self.assertIn('context.configuration.command = ["${server_path}", "--stdio"]', source)
        self.assertIn("context.configuration.tcp_port = None", source)
        self.assertIn("context.configuration.env = dict(trusted_env)", source)
        self.assertNotIn('root_settings.get("server_path")', source)


if __name__ == "__main__":
    unittest.main()
