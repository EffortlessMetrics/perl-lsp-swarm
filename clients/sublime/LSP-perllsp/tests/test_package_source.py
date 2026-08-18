from __future__ import annotations

import copy
import json
import shutil
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

SUBLIME_ROOT = Path(__file__).resolve().parents[2]
PACKAGE_ROOT = Path(__file__).resolve().parents[1]
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
            receipt_a = package_source.export_atomically(
                first,
                source_commit="0" * 40,
            )
            receipt_b = package_source.export_atomically(
                second,
                source_commit="0" * 40,
            )
            self.assertEqual(receipt_a, receipt_b)
            self.assertEqual(package_source.check_export(first), receipt_a["destination_tree_sha256"])
            self.assertEqual(package_source.check_export(second), receipt_b["destination_tree_sha256"])
            for relative in package_source.load_manifest()["source_files"]:
                self.assertEqual((first / relative).read_bytes(), (second / relative).read_bytes())

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
