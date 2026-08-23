#!/usr/bin/env python3
"""Falsifiers for the Helix canonical-fixture drift check.

Each negative case mutates a synthetic repository in one realistic way and
asserts the validator rejects it. A drift check that cannot fail is not proof,
so the silent-Raku-leak case below is the one that matters most: it is the exact
regression `docs/examples/helix/languages.toml` exists to prevent (#7724).

The final test runs the validator against the real checkout, so the committed
guides must genuinely agree with the committed fixture.
"""
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from validate_helix_fixture_drift import (  # noqa: E402
    CANONICAL_COPY_SITES,
    CANONICAL_FIXTURE,
    GOVERNED_PARSE_TARGETS,
    canonical_body,
    validate,
)

REPO_ROOT = Path(__file__).resolve().parents[2]

FIXTURE = """\
# Canonical manual Helix registration for perllsp.
#
# This deliberately narrows Helix's combined `perl` entry to reviewed Perl 5
# file families so Raku/NQP/P6 files do not launch the Perl 5 server.

[language-server.perllsp]
command = "perllsp"
args = ["--stdio"]

[[language]]
name = "perl"
language-servers = ["perllsp"]
roots = [".perl-lsp.toml", "Makefile.PL", "Build.PL", "cpanfile", "dist.ini"]
file-types = [
  "pl",
  "pm",
  "t",
  "psgi",
  { glob = "latexmkrc" },
  { glob = ".latexmkrc" },
]
shebangs = ["perl"]
"""

# The unsafe override this check exists to catch: a valid, plausible-looking
# registration that drops the Perl 5 file-family narrowing and therefore leaves
# Helix's Raku/NQP/P6 extensions attached to the Perl 5 server.
UNSAFE_OVERRIDE = """\
[[language]]
name = "perl"
language-servers = ["perllsp"]

[language-server.perllsp]
command = "perllsp"
args = ["--stdio"]
"""


class HelixFixtureDriftTests(unittest.TestCase):
    def setUp(self) -> None:
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.write(CANONICAL_FIXTURE, FIXTURE)
        self.canonical_block = canonical_body(FIXTURE)
        # Give every canonical site a conforming copy so the baseline is green
        # and each test isolates exactly one mutation.
        for target in CANONICAL_COPY_SITES:
            self.write_guide(target, self.canonical_block)
        for target in GOVERNED_PARSE_TARGETS:
            if not (self.root / target).exists():
                self.write_guide(target, self.canonical_block)
        # Extending guides are covered by the safety invariant only, and make
        # up the repository-wide registration count.
        self.write_guide("docs/reference/CONFIG.md", self.extended_block())
        self.write_guide("docs/specs/PACKAGING_INSTALL_SPEC.md", self.canonical_block)

    def extended_block(self) -> str:
        """A legitimate extension: canonical registration plus a config subtable."""
        return (
            self.canonical_block
            + "\n\n[language-server.perllsp.config.perl.inlayHints]\nenabled = true\n"
        )

    def write(self, relative: Path | str, text: str) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def write_guide(self, relative: Path | str, block: str) -> None:
        self.write(relative, f"# Guide\n\nSetup:\n\n```toml\n{block}\n```\n")

    def assert_fails(self, needle: str) -> None:
        failures = validate(self.root)
        self.assertTrue(failures, "expected the validator to report a failure")
        joined = "\n".join(failures)
        self.assertIn(needle, joined)

    def test_conforming_repository_passes(self) -> None:
        self.assertEqual(validate(self.root), [])

    def test_silent_raku_leak_is_rejected(self) -> None:
        # The regression that motivates the whole check.
        self.write_guide(CANONICAL_COPY_SITES[0], UNSAFE_OVERRIDE)
        self.assert_fails("without a `file-types` narrowing")

    def test_dropped_file_types_is_rejected(self) -> None:
        drifted = self.canonical_block.replace('  "psgi",\n', "")
        self.write_guide(CANONICAL_COPY_SITES[1], drifted)
        self.assert_fails("does not match")

    def test_renamed_server_id_is_rejected(self) -> None:
        # Structural detection is what makes this catchable: after the rename
        # the block no longer contains the literal `[language-server.perllsp]`.
        drifted = self.canonical_block.replace("perllsp", "perl-lsp").replace(
            'command = "perl-lsp"', 'command = "perllsp"'
        )
        self.write_guide(CANONICAL_COPY_SITES[2], drifted)
        self.assert_fails("the standardized ID is 'perllsp'")

    def test_renamed_server_id_in_extending_guide_is_rejected(self) -> None:
        # Extending guides are exempt from byte-equality but not from the ID
        # and narrowing invariants.
        drifted = self.extended_block().replace("perllsp", "perl-lsp").replace(
            'command = "perl-lsp"', 'command = "perllsp"'
        )
        self.write_guide("docs/reference/CONFIG.md", drifted)
        self.assert_fails("the standardized ID is 'perllsp'")

    def test_extending_guide_is_not_forced_to_match_verbatim(self) -> None:
        # A guide may legitimately add args or config subtables.
        self.assertEqual(validate(self.root), [])

    def test_new_undeclared_copy_is_discovered(self) -> None:
        # A brand-new guide nobody registered anywhere still gets checked.
        self.write_guide("docs/tutorials/BRAND_NEW_GUIDE.md", UNSAFE_OVERRIDE)
        self.assert_fails("BRAND_NEW_GUIDE.md")

    def test_generated_book_copy_is_checked(self) -> None:
        self.write_guide("book/src/reference/editor-setup-canonical.md", UNSAFE_OVERRIDE)
        self.assert_fails("editor-setup-canonical.md")

    def test_missing_canonical_block_is_rejected(self) -> None:
        self.write(CANONICAL_COPY_SITES[0], "# Guide\n\nNo block here.\n")
        self.assert_fails("expected a canonical Helix registration block")

    def test_partial_override_is_exempt(self) -> None:
        # A config-only or environment-only snippet is an intentional partial
        # override, not a copy of the fixture, and must not be forced to match.
        self.write(
            "docs/EDITORS/EXTRA.md",
            "```toml\n[language-server.perllsp]\n"
            'command = "perllsp"\n'
            'environment = { PERL5LIB = "lib" }\n```\n',
        )
        self.assertEqual(validate(self.root), [])

    def test_unparseable_block_in_governed_document_is_rejected(self) -> None:
        path = self.root / GOVERNED_PARSE_TARGETS[0]
        path.write_text(
            path.read_text(encoding="utf-8") + "\n```toml\nthis is not = = toml\n```\n",
            encoding="utf-8",
        )
        self.assert_fails("does not parse")

    def test_missing_fixture_is_rejected(self) -> None:
        (self.root / CANONICAL_FIXTURE).unlink()
        self.assert_fails("canonical fixture is missing")

    def test_broken_fixture_is_rejected(self) -> None:
        self.write(CANONICAL_FIXTURE, "not = = valid toml\n")
        self.assert_fails("does not parse")

    def test_vanishing_copies_are_rejected(self) -> None:
        # If extraction breaks, or the guides lose their copy/paste blocks, the
        # check must fail loudly rather than pass by finding nothing.
        for target in (*GOVERNED_PARSE_TARGETS, *CANONICAL_COPY_SITES):
            self.write(target, "# Guide\n\nNo configuration block here.\n")
        self.write("docs/reference/CONFIG.md", "# C\n")
        self.write("docs/specs/PACKAGING_INSTALL_SPEC.md", "# P\n")
        self.assert_fails("expected at least")

    def test_real_repository_is_conforming(self) -> None:
        self.assertEqual(validate(REPO_ROOT), [])


if __name__ == "__main__":
    unittest.main()
