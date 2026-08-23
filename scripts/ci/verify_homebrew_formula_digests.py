#!/usr/bin/env python3
"""Bind each verified release digest to the formula stanza that ships it.

The Homebrew bump job verifies every release archive against the release's own
``SHA256SUMS`` before generating the formula. Checking only that each verified
digest appears *somewhere* in the generated Ruby is not enough: a generator that
swapped the macOS and Linux digests, attached one to the wrong URL, or left a
digest behind in a comment would still pass, and `brew install` would then hand
users a valid-looking checksum for the wrong archive.

This module pairs each ``url``/``sha256`` stanza in the formula and requires the
mapping to agree exactly with the verified manifest: every expected asset
present, each carrying its own digest, each appearing exactly once, and no
unverified archive carried alongside them.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# The generated formula pairs each archive URL with the digest on the very next
# line (see Formula/perllsp.rb). Anchoring the digest to the URL is the whole
# point: a free-floating digest search is what this check exists to replace.
STANZA_PATTERN = re.compile(
    r'url\s+"[^"]*/(?P<asset>[^"/]+\.tar\.gz)"\s*\n\s*sha256\s+"(?P<digest>[0-9a-fA-F]{64})"'
)


class FormulaMismatch(ValueError):
    """Raised when the formula's digests do not match the verified manifest."""


def parse_formula(text: str) -> dict[str, str]:
    """Return ``{asset: digest}`` for each url/sha256 stanza in the formula."""
    found: dict[str, str] = {}
    for match in STANZA_PATTERN.finditer(text):
        asset = match.group("asset")
        digest = match.group("digest").lower()
        if asset in found:
            raise FormulaMismatch(f"formula lists {asset} more than once")
        found[asset] = digest
    return found


def parse_manifest(text: str) -> dict[str, str]:
    """Return ``{asset: digest}`` from the verified-checksum manifest."""
    expected: dict[str, str] = {}
    for lineno, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        parts = line.split()
        if len(parts) != 2:
            raise FormulaMismatch(f"malformed manifest line {lineno}: {line!r}")
        asset, digest = parts
        expected[asset] = digest.lower()
    if not expected:
        raise FormulaMismatch("verified checksum manifest is empty")
    return expected


def check(formula_text: str, manifest_text: str) -> int:
    """Verify the formula against the manifest. Returns the number bound."""
    found = parse_formula(formula_text)
    expected = parse_manifest(manifest_text)

    for asset, digest in sorted(expected.items()):
        actual = found.get(asset)
        if actual is None:
            raise FormulaMismatch(f"formula has no url/sha256 stanza for {asset}")
        if actual != digest:
            raise FormulaMismatch(
                f"formula digest for {asset} does not match the verified archive"
            )

    unverified = sorted(set(found) - set(expected))
    if unverified:
        raise FormulaMismatch(
            f"formula carries archives that were never verified: {', '.join(unverified)}"
        )

    return len(expected)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--formula", required=True, help="Generated .rb formula")
    parser.add_argument("--manifest", required=True, help="Verified 'asset digest' lines")
    args = parser.parse_args(argv)

    try:
        bound = check(
            Path(args.formula).read_text(encoding="utf-8"),
            Path(args.manifest).read_text(encoding="utf-8"),
        )
    except (FormulaMismatch, OSError) as error:
        print(f"::error::{error}", file=sys.stdout)
        return 1

    print(f"bound {bound} verified digests to their exact formula stanzas")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
