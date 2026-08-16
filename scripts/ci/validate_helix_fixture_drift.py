#!/usr/bin/env python3
"""Validate that embedded Helix registrations match the canonical fixture.

`docs/examples/helix/languages.toml` is the reviewed, safe manual Helix
registration for `perllsp`. It deliberately narrows Helix's combined `perl`
language entry to Perl 5 file families so Raku/NQP/P6 files do not launch the
Perl 5 server. Setup guides embed that same registration inline so a reader can
copy it without opening a second file.

Inline copies drift. Parsing a snippet proves only that it is syntactically
valid TOML -- not that it still matches the reviewed fixture, and not that it
still carries the Perl 5 narrowing. A guide that silently loses `file-types`
reintroduces the exact Raku/NQP/P6 leak the fixture exists to prevent (#7724).

This validator enforces three invariants.

1. **Safety, repository-wide.** Every fenced ``toml`` block anywhere under
   `docs/` or `book/src/` that registers a Helix `perl` language entry backed by
   the `perllsp` command must carry the Perl 5 narrowing (`file-types`) and use
   the standardized Helix-local server ID. Detection is *structural* -- the
   block is parsed and inspected -- rather than a literal search for
   ``[language-server.perllsp]``. A literal search silently misses the exact
   drift this check exists to catch: a guide that renames the server ID back to
   `perl-lsp` no longer contains the searched string and would escape entirely.
   `perl-lsp` is additionally a different crates.io project, so the stale ID is
   itself a documentation defect.
2. **Canonical drift.** The documents registered in ``CANONICAL_COPY_SITES``
   embed the fixture as a plain copy/paste block. Those blocks must reproduce
   the canonical fixture body byte for byte. Guides that legitimately extend the
   registration (extra CLI args, `config` subtables) are covered by invariant 1
   only -- forcing them to match verbatim would be wrong.
3. **Parse.** Every fenced ``toml`` block in a governed copy/paste surface must
   parse. Those documents are what a reader pastes into their config, so a block
   that does not parse is a user-facing defect. The parse check is deliberately
   scoped to those files: other documents legitimately contain illustrative
   fragments that are not standalone TOML.

Usage:
  scripts/ci/validate_helix_fixture_drift.py [--repo-root PATH]

Exit codes:
  0 - the canonical fixture and every embedded copy agree
  1 - a copy drifted, a governed block failed to parse, or extraction broke
"""
from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

# The reviewed registration. Everything else is a copy of this.
CANONICAL_FIXTURE = Path("docs/examples/helix/languages.toml")

# Trees searched for embedded full registrations. `book/src` is included
# because populate-book.sh commits generated copies of `docs/` pages, so a
# stale generated copy is real drift a reader can still find.
DISCOVERY_ROOTS = (Path("docs"), Path("book/src"))

# Documents whose every TOML block must parse. These are the copy/paste
# surfaces for Helix setup.
GOVERNED_PARSE_TARGETS = (
    Path("docs/EDITORS/HELIX_SETUP.md"),
    Path("docs/how-to/EDITOR_SETUP.md"),
    Path("docs/tutorials/GETTING_STARTED.md"),
    Path("book/src/reference/editor-setup-canonical.md"),
)

# Documents that embed the fixture as a plain copy/paste block and must
# therefore reproduce it byte for byte (invariant 2).
CANONICAL_COPY_SITES = (
    Path("docs/EDITORS/HELIX_SETUP.md"),
    Path("docs/how-to/EDITOR_SETUP.md"),
    Path("docs/tutorials/GETTING_STARTED.md"),
    Path("book/src/reference/editor-setup-canonical.md"),
)

# The binary this project ships. A Helix `perl` entry wired to this command is
# our registration regardless of the Helix-local server ID chosen for it.
PERLLSP_COMMAND = "perllsp"

# The standardized Helix-local server ID (#7724). `perl-lsp` is a different
# crates.io project and must not be used as the ID for this server.
CANONICAL_SERVER_ID = "perllsp"

# Minimum number of Helix registrations expected repository-wide. Guards
# against a regex or layout change that makes this validator pass by finding
# nothing at all.
MIN_EXPECTED_REGISTRATIONS = 6

TOML_BLOCK = re.compile(r"^```toml[^\n]*\n(.*?)^```", re.DOTALL | re.MULTILINE)


def canonical_body(fixture_text: str) -> str:
    """Return the fixture's TOML body with its comment header removed.

    Guides embed the registration without the fixture's explanatory header, so
    the comparable form is the fixture minus whole-line comments.
    """
    kept = [
        line
        for line in fixture_text.splitlines()
        if not line.lstrip().startswith("#")
    ]
    return "\n".join(kept).strip()


def toml_blocks(markdown_text: str) -> list[str]:
    """Return every fenced ``toml`` block body in a Markdown document."""
    return [match.group(1) for match in TOML_BLOCK.finditer(markdown_text)]


def helix_perl_registration(block: str) -> dict | None:
    """Return the parsed Helix `perl` registration in a block, or None.

    A block counts as a registration when it declares a `[[language]]` entry
    named `perl` *and* defines at least one language server whose command is
    the `perllsp` binary. Inspecting the parsed structure -- rather than
    grepping for a server-ID string -- is what lets this catch a guide that
    renamed the ID back to `perl-lsp`.
    """
    try:
        parsed = tomllib.loads(block)
    except tomllib.TOMLDecodeError:
        # Unparseable blocks cannot be classified here. Governed copy/paste
        # surfaces catch those separately via the parse invariant.
        return None

    languages = parsed.get("language")
    if not isinstance(languages, list):
        return None
    perl_entries = [
        entry
        for entry in languages
        if isinstance(entry, dict) and entry.get("name") == "perl"
    ]
    if not perl_entries:
        return None

    servers = parsed.get("language-server")
    if not isinstance(servers, dict):
        return None
    our_ids = sorted(
        server_id
        for server_id, definition in servers.items()
        if isinstance(definition, dict)
        and definition.get("command") == PERLLSP_COMMAND
    )
    if not our_ids:
        return None

    return {"perl_entry": perl_entries[0], "server_ids": our_ids}


def iter_markdown(repo_root: Path) -> list[Path]:
    found: list[Path] = []
    for root in DISCOVERY_ROOTS:
        base = repo_root / root
        if base.is_dir():
            found.extend(sorted(base.rglob("*.md")))
    return found


def validate(repo_root: Path) -> list[str]:
    """Return a list of human-readable failures; empty means the check passed."""
    failures: list[str] = []

    fixture_path = repo_root / CANONICAL_FIXTURE
    if not fixture_path.is_file():
        return [f"canonical fixture is missing: {CANONICAL_FIXTURE}"]

    fixture_text = fixture_path.read_text(encoding="utf-8")
    try:
        tomllib.loads(fixture_text)
    except tomllib.TOMLDecodeError as exc:
        failures.append(f"{CANONICAL_FIXTURE}: fixture does not parse: {exc}")
        # A broken fixture makes every comparison meaningless.
        return failures

    expected = canonical_body(fixture_text)
    if helix_perl_registration(expected) is None:
        return [
            f"{CANONICAL_FIXTURE}: fixture is no longer a recognizable Helix "
            f"`perl` registration backed by the `{PERLLSP_COMMAND}` command"
        ]

    # Invariant 1: every Helix perl registration is safe and uses the
    # standardized server ID, wherever in the documentation it lives.
    registrations_found = 0
    canonical_sites_seen: set[Path] = set()
    for path in iter_markdown(repo_root):
        rel = path.relative_to(repo_root)
        is_canonical_site = rel in CANONICAL_COPY_SITES
        for index, block in enumerate(toml_blocks(path.read_text(encoding="utf-8"))):
            registration = helix_perl_registration(block)
            if registration is None:
                continue
            registrations_found += 1
            where = f"{rel}: toml block {index}"

            if "file-types" not in registration["perl_entry"]:
                failures.append(
                    f"{where} registers the Perl 5 server on Helix's combined "
                    f"`perl` entry without a `file-types` narrowing, so "
                    f"Raku/NQP/P6 files would launch it. Copy the narrowing "
                    f"from {CANONICAL_FIXTURE}."
                )

            stale_ids = [
                server_id
                for server_id in registration["server_ids"]
                if server_id != CANONICAL_SERVER_ID
            ]
            if stale_ids:
                failures.append(
                    f"{where} uses Helix-local server ID(s) "
                    f"{', '.join(repr(i) for i in stale_ids)} for the "
                    f"`{PERLLSP_COMMAND}` command; the standardized ID is "
                    f"'{CANONICAL_SERVER_ID}'."
                )

            # Invariant 2: plain copy/paste sites must match the fixture.
            if is_canonical_site:
                canonical_sites_seen.add(rel)
                if block.strip() != expected:
                    failures.append(
                        f"{where} is a canonical copy site that does not match "
                        f"{CANONICAL_FIXTURE}. Copy the fixture body verbatim, "
                        f"or update the fixture and every copy together."
                    )

    for site in CANONICAL_COPY_SITES:
        if site not in canonical_sites_seen:
            failures.append(
                f"{site}: expected a canonical Helix registration block, found "
                f"none. Either the guide lost its copy/paste block or this "
                f"validator stopped finding it."
            )

    if registrations_found < MIN_EXPECTED_REGISTRATIONS:
        failures.append(
            f"expected at least {MIN_EXPECTED_REGISTRATIONS} Helix "
            f"registrations, found {registrations_found}. Either the guides "
            f"lost their configuration blocks or this validator stopped "
            f"finding them."
        )

    # Invariant 3: every TOML block in a governed copy/paste surface parses.
    for target in GOVERNED_PARSE_TARGETS:
        path = repo_root / target
        if not path.is_file():
            failures.append(f"governed document is missing: {target}")
            continue
        blocks = toml_blocks(path.read_text(encoding="utf-8"))
        if not blocks:
            failures.append(
                f"{target}: no toml blocks found; this document is expected to "
                f"carry at least one configuration block."
            )
        for index, block in enumerate(blocks):
            try:
                tomllib.loads(block)
            except tomllib.TOMLDecodeError as exc:
                failures.append(f"{target}: toml block {index} does not parse: {exc}")

    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="repository root to validate (defaults to this checkout)",
    )
    args = parser.parse_args(argv)

    failures = validate(args.repo_root.resolve())
    if failures:
        print("Helix fixture drift check FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print(f"OK: embedded Helix registrations match {CANONICAL_FIXTURE}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
