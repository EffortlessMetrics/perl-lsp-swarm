from __future__ import annotations

import copy
import json
import shutil
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

SUBLIME_ROOT = Path(__file__).resolve().parents[1]
PACKAGE_ROOT = SUBLIME_ROOT / "LSP-perllsp"
if str(SUBLIME_ROOT) not in sys.path:
    sys.path.insert(0, str(SUBLIME_ROOT))

import export_lsp_perllsp  # noqa: E402
import package_source  # noqa: E402


class PackageSourceAuthorityTests(unittest.TestCase):
    def test_manifest_covers_the_exact_source_tree(self) -> None:
        manifest = package_source.load_manifest()
        self.assertEqual(
            package_source.validate_source_tree(manifest, PACKAGE_ROOT),
            tuple(manifest["source_files"]),
        )
        self.assertTrue(set(manifest["package_files"]).issubset(manifest["source_files"]))

    def test_source_export_is_deterministic_and_complete(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = root / "first"
            second = root / "second"
            source_commit = package_source.resolve_source_commit()
            receipt_a = package_source.export_atomically(
                first,
                source_commit=source_commit,
            )
            receipt_b = package_source.export_atomically(
                second,
                source_commit=source_commit,
            )
            self.assertEqual(receipt_a, receipt_b)
            self.assertEqual(package_source.check_export(first), receipt_a["destination_tree_sha256"])
            self.assertEqual(package_source.check_export(second), receipt_b["destination_tree_sha256"])
            for relative in package_source.load_manifest()["source_files"]:
                self.assertEqual((first / relative).read_bytes(), (second / relative).read_bytes())

    def test_spoofed_source_commit_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(package_source.AuthorityError, "does not match"):
                package_source.export_atomically(
                    Path(directory) / "export",
                    source_commit="0" * 40,
                )

    def test_stale_source_commit_is_rejected(self) -> None:
        actual = package_source.resolve_source_commit()
        stale = ("1" if actual[0] != "1" else "2") + actual[1:]
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(package_source.AuthorityError, "does not match"):
                package_source.export_atomically(
                    Path(directory) / "export",
                    source_commit=stale,
                )

    def test_public_authority_phase_is_rejected_until_pinned_resolution_exists(self) -> None:
        payload = copy.deepcopy(package_source.load_manifest())
        payload["authority_phase"] = "public_repository_authoritative"
        payload["editable_authority"] = "public_repository"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(package_source.AuthorityError, "source resolution is not implemented"):
                package_source.load_manifest(path)

    def test_existing_checkout_is_restored_when_install_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "checkout"
            destination.mkdir()
            (destination / ".git").mkdir()
            (destination / ".git" / "HEAD").write_text("old\n", encoding="utf-8")
            staged = Path(directory) / "staged"
            staged.mkdir()
            (staged / "new.txt").write_text("new\n", encoding="utf-8")

            original_replace = package_source.os.replace
            calls = 0

            def fail_install(source: Path, target: Path) -> None:
                nonlocal calls
                calls += 1
                if calls == 2:
                    raise OSError("simulated install failure")
                original_replace(source, target)

            with mock.patch.object(package_source.os, "replace", side_effect=fail_install):
                with self.assertRaisesRegex(OSError, "simulated install failure"):
                    package_source._replace_directory_atomically(staged, destination)

            self.assertTrue((destination / ".git" / "HEAD").exists())
            self.assertEqual((destination / ".git" / "HEAD").read_text(encoding="utf-8"), "old\n")
            self.assertFalse((destination / "new.txt").exists())

    def test_existing_checkout_is_removed_only_after_install_succeeds(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "checkout"
            destination.mkdir()
            (destination / ".git").mkdir()
            (destination / ".git" / "HEAD").write_text("old\n", encoding="utf-8")
            staged = Path(directory) / "staged"
            staged.mkdir()
            (staged / "new.txt").write_text("new\n", encoding="utf-8")

            package_source._replace_directory_atomically(staged, destination)

            self.assertEqual((destination / "new.txt").read_text(encoding="utf-8"), "new\n")
            self.assertFalse((destination / ".git").exists())

    def test_package_export_uses_the_authoritative_package_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "LSP-perllsp.sublime-package"
            export_lsp_perllsp.build(output)
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(
                    sorted(archive.namelist()),
                    list(package_source.load_manifest()["package_files"]),
                )

    def test_undeclared_source_file_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            copied = Path(directory) / "package"
            shutil.copytree(PACKAGE_ROOT, copied)
            (copied / "unexpected.py").write_text("pass\n", encoding="utf-8")
            with self.assertRaisesRegex(package_source.AuthorityError, "undeclared"):
                package_source.validate_source_tree(package_source.load_manifest(), copied)

    def test_parent_traversal_is_rejected(self) -> None:
        payload = copy.deepcopy(package_source.load_manifest())
        payload["source_files"] = sorted([*payload["source_files"], "../escape.py"])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(package_source.AuthorityError, "unsafe manifest path"):
                package_source.load_manifest(path)

    @unittest.skipUnless(hasattr(Path, "symlink_to"), "symlinks are not supported")
    def test_symlink_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "package"
            package.mkdir()
            target = root / "outside.py"
            target.write_text("pass\n", encoding="utf-8")
            link = package / "link.py"
            try:
                link.symlink_to(target)
            except OSError as error:
                self.skipTest(f"symlink unavailable: {error}")
            with self.assertRaisesRegex(package_source.AuthorityError, "symlinks"):
                package_source.discover_files(package)


if __name__ == "__main__":
    unittest.main()
