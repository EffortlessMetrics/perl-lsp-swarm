import json
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path
from unittest.mock import patch

from scripts.zed_host import prepare


class ZedHostPrepareTests(unittest.TestCase):
    def test_prepare_writes_only_verified_identity_values(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            extension = root / "extension"
            extension.mkdir()
            (extension / "extension.toml").write_text(
                "id = 'perl'\n"
                "version = '0.5.0'\n"
                "repository = 'https://github.com/tree-sitter-perl/zed-perl'\n",
                encoding="utf-8",
            )
            wasm = extension / "target" / "zed_perl.wasm"
            wasm.parent.mkdir()
            wasm.write_bytes(b"wasm")
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "fixture.pl").write_text(
                "my $answer = 42;\n", encoding="utf-8"
            )
            zed_cli = root / "zed-cli"
            zed_app = root / "zed"
            perllsp = root / "perllsp"
            for path in (zed_cli, zed_app, perllsp):
                path.write_bytes(b"test binary")
            (root / ".ci" / "fixtures" / "zed-perl-upstream" / "receipts").mkdir(
                parents=True
            )
            (
                root
                / ".ci"
                / "fixtures"
                / "zed-perl-upstream"
                / "receipts"
                / "exact-source-observations-template.json"
            ).write_text("{}\n", encoding="utf-8")
            args = Namespace(
                run_dir=root / "run",
                zed_cli=zed_cli,
                zed_app=zed_app,
                zed_version="0.224.8",
                zed_channel="stable",
                zed_build="169",
                extension_dir=extension,
                extension_base="b" * 40,
                extension_candidate="c" * 40,
                extension_version="0.5.0",
                wasm=wasm,
                perllsp=perllsp,
                perllsp_version="0.17.0",
                perllsp_build="a" * 40,
                resolution_route="explicit_binary_path",
                workspace=workspace,
                fixture_id="fixture-v1",
                root_identity=None,
                perl_settings=None,
            )

            def fake_run_checked(command: list[str], cwd: Path | None = None) -> str:
                del cwd
                if command[0] == str(zed_cli):
                    return f"Zed 0.224.8 – {zed_app.resolve()}"
                if command[0] == str(zed_app):
                    return "Zed: v0.224.8+stable.169 (Zed)\n"
                if command[0] == str(perllsp):
                    return "perllsp 0.17.0\nGit commit: aaaaaaa\n"
                raise AssertionError(f"unexpected command: {command}")

            with (
                patch.object(prepare, "require_clean_git_checkout"),
                patch.object(prepare, "run_checked", side_effect=fake_run_checked),
            ):
                self.assertEqual(prepare.prepare(args, root), 0)

            manifest = json.loads(
                (root / "run" / "manifest.json").read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["zed"]["build"], "stable.169")
            self.assertEqual(manifest["perllsp"]["embedded_revision"], "aaaaaaa")
            self.assertEqual(
                manifest["extension"]["wasm_relative_path"], "target/zed_perl.wasm"
            )
            self.assertTrue(
                manifest["extension"]["manifest_sha256"].startswith("sha256:")
            )

    def test_zed_identity_is_bound_to_cli_path_and_system_specs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            app = (Path(temporary) / "zed").resolve()
            identity = prepare._parse_zed_identity(
                f"Zed 0.224.8 – {app}\n",
                "Zed: v0.224.8+stable.169 (Zed)\nOS: test\n",
                app,
                "0.224.8",
                "stable",
                "169",
            )

        self.assertEqual(identity["version"], "0.224.8")
        self.assertEqual(identity["channel"], "stable")
        self.assertEqual(identity["build"], "stable.169")
        self.assertEqual(identity["build_id"], "169")

    def test_zed_identity_rejects_mismatched_channel_or_build(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            app = (Path(temporary) / "zed").resolve()
            with self.assertRaisesRegex(prepare.HostReceiptError, "channel"):
                prepare._parse_zed_identity(
                    f"Zed 0.224.8 – {app}\n",
                    "Zed: v0.224.8+preview.169 (Zed Preview)\n",
                    app,
                    "0.224.8",
                    "stable",
                    "169",
                )
            with self.assertRaisesRegex(prepare.HostReceiptError, "build"):
                prepare._parse_zed_identity(
                    f"Zed 0.224.8 – {app}\n",
                    "Zed: v0.224.8+stable.170 (Zed)\n",
                    app,
                    "0.224.8",
                    "stable",
                    "169",
                )

    def test_perllsp_identity_requires_embedded_matching_full_commit(self) -> None:
        commit = "a" * 40
        embedded = commit[:7]
        revision = prepare._parse_perllsp_identity(
            f"perllsp 0.17.0\nGit commit: {embedded}\nPerl LSP using perl-parser v3\n",
            "0.17.0",
            commit,
        )
        self.assertEqual(revision, embedded)

        with self.assertRaisesRegex(prepare.HostReceiptError, "does not match"):
            prepare._parse_perllsp_identity(
                "perllsp 0.17.0\nGit commit: deadbee\n",
                "0.17.0",
                commit,
            )

    def test_manifest_errors_are_receipt_errors(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            manifest = Path(temporary) / "extension.toml"
            manifest.write_text("id = [", encoding="utf-8")
            with self.assertRaisesRegex(prepare.HostReceiptError, "not valid TOML"):
                prepare._load_extension_manifest(manifest)

    def test_wasm_must_resolve_inside_extension_checkout(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            extension = root / "extension"
            extension.mkdir()
            wasm = extension / "target" / "zed_perl.wasm"
            wasm.parent.mkdir()
            wasm.write_bytes(b"wasm")

            relative, digest = prepare._bound_wasm(extension.resolve(), wasm.resolve())
            self.assertEqual(relative.as_posix(), "target/zed_perl.wasm")
            self.assertTrue(digest.startswith("sha256:"))

            outside = root / "outside.wasm"
            outside.write_bytes(b"wasm")
            with self.assertRaisesRegex(prepare.HostReceiptError, "inside"):
                prepare._bound_wasm(extension.resolve(), outside.resolve())


if __name__ == "__main__":
    unittest.main()
