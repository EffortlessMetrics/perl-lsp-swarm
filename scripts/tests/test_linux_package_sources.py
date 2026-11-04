"""Exercise rendered Linux scaffold sources and release-shaped package staging.

This does not run makepkg/rpmbuild or establish public package-channel support.
"""

from __future__ import annotations

import hashlib
import io
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
VERSION = "0.17.0"
# A candidate mirror catches recipes that hardcode the public GitHub prefix
# instead of honoring the selected --download-base. No network is needed.
DOWNLOAD_BASE = "https://downloads.example.invalid/candidates/fixture-17772"


class LinuxPackageSourcesTest(unittest.TestCase):
    def render(self, arch: str, output: Path, digest: str) -> None:
        subprocess.run(
            [
                sys.executable, str(ROOT / "scripts/render-linux-packages.py"),
                "--version", VERSION, "--download-base", DOWNLOAD_BASE + "/",
                "--download-sha256", digest, "--arch", arch,
                "--output-dir", str(output),
            ],
            check=True, capture_output=True, text=True,
        )

    def test_rpm_source_uses_selected_release_url(self) -> None:
        for arch in ("x86_64", "aarch64"):
            with self.subTest(arch=arch), tempfile.TemporaryDirectory() as temporary:
                output = Path(temporary)
                self.render(arch, output, "a" * 64)
                spec = (output / "dnf/perl-lsp.spec").read_text()
                source = next(line.split(None, 1)[1] for line in spec.splitlines()
                              if line.startswith("Source0:"))
                asset = f"perllsp-{VERSION}-{arch}-unknown-linux-gnu.tar.gz"
                self.assertEqual(source, f"{DOWNLOAD_BASE}/{asset}")
                self.assertIn(f"%autosetup -n {asset.removesuffix('.tar.gz')}", spec)
                self.assertIn(f"BuildArch:      {arch}", spec)

    def test_pacman_source_and_staged_product_match_archive(self) -> None:
        for arch in ("x86_64", "aarch64"):
            with self.subTest(arch=arch), tempfile.TemporaryDirectory() as temporary:
                work = Path(temporary)
                asset = f"perllsp-{VERSION}-{arch}-unknown-linux-gnu.tar.gz"
                source_dir = asset.removesuffix(".tar.gz")
                members = {
                    "perllsp": b"#!/bin/sh\necho perllsp-fixture\n",
                    "perl-dap": b"#!/bin/sh\necho perl-dap-fixture\n",
                    "README.md": b"release-shaped fixture, not a public binary\n",
                    "LICENSE-MIT": (ROOT / "LICENSE-MIT").read_bytes(),
                    "LICENSE-APACHE": (ROOT / "LICENSE-APACHE").read_bytes(),
                }
                archive = work / asset
                with tarfile.open(archive, "w:gz") as bundle:
                    for name, data in members.items():
                        info = tarfile.TarInfo(f"{source_dir}/{name}")
                        info.size = len(data)
                        info.mode = 0o755 if name in ("perllsp", "perl-dap") else 0o644
                        bundle.addfile(info, io.BytesIO(data))
                digest = hashlib.sha256(archive.read_bytes()).hexdigest()
                output = work / "rendered"
                self.render(arch, output, digest)
                recipe = output / "pacman/PKGBUILD"
                metadata = subprocess.run(
                    ["bash", "-euc", 'source "$1"; printf "%s\\0" '
                     '"${source[@]}" "${sha256sums[@]}" "${arch[@]}"',
                     "bash", str(recipe)],
                    check=True, capture_output=True,
                ).stdout.decode().split("\0")[:-1]
                self.assertEqual(metadata, [f"{DOWNLOAD_BASE}/{asset}", digest, arch])
                # The recipe lives in a clean directory without a local source
                # archive. Unpack the controlled workflow-shaped input separately,
                # as makepkg does before calling the real package() function.
                self.assertFalse((recipe.parent / asset).exists())
                srcdir = work / "src"
                with tarfile.open(archive) as bundle:
                    bundle.extractall(srcdir, filter="data")
                pkgdir = work / "pkg"
                subprocess.run(
                    ["bash", "-euc", 'source "$1"; srcdir="$2"; pkgdir="$3"; package',
                     "bash", str(recipe), str(srcdir), str(pkgdir)],
                    check=True, capture_output=True, text=True,
                )
                expected = {
                    "usr/bin/perllsp": members["perllsp"],
                    "usr/bin/perl-dap": members["perl-dap"],
                    "usr/share/doc/perl-lsp/README.md": members["README.md"],
                    "usr/share/licenses/perl-lsp/LICENSE-MIT": members["LICENSE-MIT"],
                    "usr/share/licenses/perl-lsp/LICENSE-APACHE": members["LICENSE-APACHE"],
                }
                actual = {str(path.relative_to(pkgdir)): path.read_bytes()
                          for path in pkgdir.rglob("*") if path.is_file()}
                self.assertEqual(actual, expected)
                for name in ("perllsp", "perl-dap"):
                    self.assertEqual((pkgdir / "usr/bin" / name).stat().st_mode & 0o777, 0o755)


if __name__ == "__main__":
    unittest.main()
