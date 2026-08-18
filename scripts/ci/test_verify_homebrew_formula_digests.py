#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("verify_homebrew_formula_digests.py")
SPEC = importlib.util.spec_from_file_location("verify_homebrew_formula_digests", MODULE_PATH)
assert SPEC and SPEC.loader
verify = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verify
SPEC.loader.exec_module(verify)

MACOS_ARM = "a" * 64
MACOS_X86 = "b" * 64
LINUX_ARM = "c" * 64
LINUX_X86 = "d" * 64

BASE = "https://github.com/EffortlessMetrics/perl-lsp/releases/download/v1.2.3"


def formula(macos_arm=MACOS_ARM, macos_x86=MACOS_X86, linux_arm=LINUX_ARM, linux_x86=LINUX_X86):
    return f"""class Perllsp < Formula
  on_macos do
    on_arm do
      url "{BASE}/perllsp-1.2.3-aarch64-apple-darwin.tar.gz"
      sha256 "{macos_arm}"
    end
    on_intel do
      url "{BASE}/perllsp-1.2.3-x86_64-apple-darwin.tar.gz"
      sha256 "{macos_x86}"
    end
  end
  on_linux do
    on_arm do
      url "{BASE}/perllsp-1.2.3-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "{linux_arm}"
    end
    on_intel do
      url "{BASE}/perllsp-1.2.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "{linux_x86}"
    end
  end
end
"""


MANIFEST = "\n".join(
    [
        f"perllsp-1.2.3-aarch64-apple-darwin.tar.gz {MACOS_ARM}",
        f"perllsp-1.2.3-x86_64-apple-darwin.tar.gz {MACOS_X86}",
        f"perllsp-1.2.3-aarch64-unknown-linux-gnu.tar.gz {LINUX_ARM}",
        f"perllsp-1.2.3-x86_64-unknown-linux-gnu.tar.gz {LINUX_X86}",
    ]
) + "\n"


class VerifyFormulaDigestsTests(unittest.TestCase):
    def test_correct_formula_binds_all_four(self) -> None:
        self.assertEqual(verify.check(formula(), MANIFEST), 4)

    def test_swapped_digests_are_rejected(self) -> None:
        """The negative control: every digest is still present, but mispaired."""
        swapped = formula(macos_arm=LINUX_ARM, linux_arm=MACOS_ARM)
        # Both digests still appear somewhere, so a substring search would pass.
        self.assertIn(MACOS_ARM, swapped)
        self.assertIn(LINUX_ARM, swapped)
        with self.assertRaisesRegex(verify.FormulaMismatch, "does not match"):
            verify.check(swapped, MANIFEST)

    def test_platform_swap_across_os_is_rejected(self) -> None:
        swapped = formula(macos_x86=LINUX_X86, linux_x86=MACOS_X86)
        with self.assertRaisesRegex(verify.FormulaMismatch, "does not match"):
            verify.check(swapped, MANIFEST)

    def test_digest_only_in_a_comment_is_rejected(self) -> None:
        text = formula(macos_arm="e" * 64) + f"\n# stale digest {MACOS_ARM}\n"
        with self.assertRaisesRegex(verify.FormulaMismatch, "does not match"):
            verify.check(text, MANIFEST)

    def test_missing_stanza_is_rejected(self) -> None:
        text = formula()
        text = text.replace(
            f'      url "{BASE}/perllsp-1.2.3-x86_64-unknown-linux-gnu.tar.gz"\n'
            f'      sha256 "{LINUX_X86}"\n',
            "",
        )
        with self.assertRaisesRegex(verify.FormulaMismatch, "no url/sha256 stanza"):
            verify.check(text, MANIFEST)

    def test_unverified_extra_archive_is_rejected(self) -> None:
        text = formula() + (
            f'  url "{BASE}/perllsp-1.2.3-x86_64-unknown-freebsd.tar.gz"\n'
            f'  sha256 "{"f" * 64}"\n'
        )
        with self.assertRaisesRegex(verify.FormulaMismatch, "never verified"):
            verify.check(text, MANIFEST)

    def test_duplicate_stanza_is_rejected(self) -> None:
        text = formula() + (
            f'  url "{BASE}/perllsp-1.2.3-x86_64-apple-darwin.tar.gz"\n'
            f'  sha256 "{MACOS_X86}"\n'
        )
        with self.assertRaisesRegex(verify.FormulaMismatch, "more than once"):
            verify.check(text, MANIFEST)

    def test_digest_comparison_is_case_insensitive(self) -> None:
        """Only the hex digests differ in case; keywords and asset names do not."""
        upper = formula(
            macos_arm=MACOS_ARM.upper(),
            macos_x86=MACOS_X86.upper(),
            linux_arm=LINUX_ARM.upper(),
            linux_x86=LINUX_X86.upper(),
        )
        self.assertEqual(verify.check(upper, MANIFEST), 4)

    def test_empty_manifest_is_rejected(self) -> None:
        with self.assertRaisesRegex(verify.FormulaMismatch, "empty"):
            verify.check(formula(), "\n")

    def test_malformed_manifest_line_is_rejected(self) -> None:
        with self.assertRaisesRegex(verify.FormulaMismatch, "malformed"):
            verify.check(formula(), "only-one-field\n")


if __name__ == "__main__":
    unittest.main()
