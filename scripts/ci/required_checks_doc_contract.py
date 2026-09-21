#!/usr/bin/env python3
"""Validate that documentation prose mentioning `required checks` matches the
canonical policy file.

The original defect (#16127): three reference documents stated that the main
branch ruleset enforces two required checks when it actually enforces five.
After #16125 corrected the prose, the same drift can re-occur unless a contract
test binds the prose to the policy file. This module is that contract.

It does not enforce specific wording. It enforces one fact:

1. Any prose that gives a count for `required checks` matches the count of
   `[[checks]]` rows with `required = true` in
   `.ci/policies/required-checks.toml`.

The check ignores text inside fenced code blocks, since a doc may legitimately
quote a different repository's policy in a code block.

The scope is narrow on purpose: the existing fix already updated the
``docs/reference/`` corpus, and a wider scope would fire on legitimate
historical/forward-looking prose in agent and design docs. A name-mention
check is deliberately omitted — it would have to read surrounding context to
distinguish "the required checks include **ripr+ New Gap Gate**" from "the
``ripr+ New Gap Gate`` job uploaded its receipt", which is closer to NLP than
to a contract ratchet.
"""

from __future__ import annotations

import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path


POLICY_FILENAME = "required-checks.toml"
# Filesystem path components that mark archived/historical docs.
ARCHIVE_PATH_PARTS = ("archive", "ARCHIVE", ".archive")
# Substring patterns (lowercased) that indicate a line is inside an example,
# historical, or otherwise non-current prose block. Used for line-level skip
# rather than block-level skip; the regex for count extraction is conservative
# enough that we keep the surface small.
HISTORICAL_LINE_MARKERS = (
    "the previous ruleset",
    "the previous policy",
    "before #16125",
    "before this fix",
    "the historic",
    "the historical",
)
# The default scope: only top-level reference docs and the focused files
# flagged in #16127. A wider scope is exposed via ``--doc-root`` for tests.
DEFAULT_DOC_ROOTS = (
    "docs/reference",
)

NUMBER_WORDS: dict[str, int] = {
    "one": 1,
    "two": 2,
    "three": 3,
    "four": 4,
    "five": 5,
    "six": 6,
    "seven": 7,
    "eight": 8,
    "nine": 9,
    "ten": 10,
}

# Match `<count> required (status )?check(s)`. The count is either a digit
# string or a spelled-out small number. The match must be inside prose, so we
# reject matches preceded by `$` (shell), `\` (markdown code-span close), or
# inside a fenced code block (handled at the line level). We also reject
# fraction patterns like "2 of 3 required checks" — the contract is about
# the total count, not a quota phrase.
COUNT_RE = re.compile(
    r"""
    (?<![\w$\\])              # not preceded by word char / $ / backslash
    (?P<count>\d+|
        one|two|three|four|five|
        six|seven|eight|nine|ten)
    \s+required\s+
    (?:status\s+)?
    checks?
    """,
    re.IGNORECASE | re.VERBOSE,
)

# Words that, when they appear within 8 characters before the count, signal a
# fraction pattern rather than a total-count claim. "2 of 3 required checks"
# is a quota, not a count of how many required checks exist.
FRACTION_PRECEDING = re.compile(
    r"(?:\bof\b|\bout of\b|\bamong\b)\s+(?:\S+\s+){0,3}$",
    re.IGNORECASE,
)


@dataclass(frozen=True)
class DriftFinding:
    """A single drift finding the validator reports."""

    path: Path
    line_number: int
    line_text: str
    detail: str


def extract_required_checks(
    toml_text: str,
) -> tuple[int, tuple[str, ...]]:
    """Return ``(count, sorted_names)`` for ``[[checks]]`` rows with
    ``required = true``.

    The singular ``[[check]]`` blocks are workflow-trigger-lint input and are
    not authoritative for the rule we are validating: they describe whether a
    workflow receives the required-workflow trigger contract, not whether its
    status context blocks the merge. The plural ``[[checks]]`` block carries
    the authoritative ``enforcement = "github-ruleset"`` binding that #16127
    measured against the live ruleset.
    """
    data = tomllib.loads(toml_text)
    blocks = data.get("checks", [])
    required = sorted(
        block["name"]
        for block in blocks
        if isinstance(block, dict) and block.get("required") is True
    )
    return len(required), tuple(required)


def _word_to_int(token: str) -> int:
    token = token.lower()
    if token.isdigit():
        return int(token)
    if token in NUMBER_WORDS:
        return NUMBER_WORDS[token]
    raise ValueError(f"unparseable count token: {token!r}")


def _inside_code_span(line: str, match_start: int, match_end: int) -> bool:
    """Return True when ``line[match_start:match_end]`` sits between two
    backticks (inline code span) or between an unbalanced pair that opens a
    multi-backtick span on the same line.
    """
    before = line[:match_start].count("`")
    after = line[match_end:].count("`")
    # An odd number of backticks on either side means the match is inside
    # (or extends) an unclosed inline code span.
    return (before % 2 == 1) or (after % 2 == 1)


def find_count_mentions(line: str) -> list[tuple[int, str]]:
    """Return ``[(count, matched_text), ...]`` for a single line of prose.

    Lines that are obviously historical or that contain backticks framing the
    match (inline code spans) are filtered before extraction.
    """
    stripped = line.strip()
    if not stripped:
        return []
    lowered = stripped.lower()
    if any(marker in lowered for marker in HISTORICAL_LINE_MARKERS):
        return []
    # Skip pure code-fence lines. In-line code spans (`like this`) are common
    # in markdown; ``COUNT_RE`` already rejects matches preceded by `\`, but a
    # bullet like "the named check is `Perl LSP Rust Small Result`" should
    # also be excluded. The simplest signal: if the line starts with a code
    # fence (`` ``` ``) or is the payload of one we are not parsing, drop it.
    if stripped.startswith("```") or stripped.startswith("~~~"):
        return []

    findings: list[tuple[int, str]] = []
    for match in COUNT_RE.finditer(line):
        if _inside_code_span(line, match.start(), match.end()):
            continue
        # Fraction patterns ("2 of 3 required checks") describe a quota,
        # not the total count. The contract is about the total.
        prefix = line[: match.start()]
        if FRACTION_PRECEDING.search(prefix):
            continue
        findings.append((_word_to_int(match.group("count")), match.group(0)))
    return findings


def check_doc(
    doc_path: Path,
    required_count: int,
    required_names: tuple[str, ...],
    *,
    in_code_block: bool = False,
) -> tuple[list[DriftFinding], bool]:
    """Walk ``doc_path`` line by line and return ``(findings, ends_in_block)``.

    ``in_code_block`` carries the fence state from the previous chunk so that
    a code block that starts mid-line does not leak. The contract ignores
    fenced code blocks because they may legitimately quote another repository.
    """
    findings: list[DriftFinding] = []
    inside_block = in_code_block
    try:
        text = doc_path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        findings.append(
            DriftFinding(
                path=doc_path,
                line_number=0,
                line_text="",
                detail=f"could not read doc: {exc}",
            )
        )
        return findings, False

    for line_no, line in enumerate(text.splitlines(), start=1):
        # Track fenced code blocks; opening and closing fences are themselves
        # matched by ``find_count_mentions``'s own ``startswith("```")`` guard.
        stripped = line.strip()
        if stripped.startswith("```") or stripped.startswith("~~~"):
            inside_block = not inside_block
            continue
        if inside_block:
            continue

        for count, matched in find_count_mentions(line):
            if count != required_count:
                findings.append(
                    DriftFinding(
                        path=doc_path,
                        line_number=line_no,
                        line_text=line,
                        detail=(
                            f"mentions {count!r} required checks but the "
                            f"policy file lists {required_count}; "
                            f"matched text: {matched!r}"
                        ),
                    )
                )

    return findings, inside_block


def iter_doc_paths(
    root: Path,
    doc_roots: tuple[str, ...] = DEFAULT_DOC_ROOTS,
) -> list[Path]:
    """Yield active (non-archived) markdown paths under ``doc_roots``.

    The archive/ subtree is excluded because it carries historical prose that
    legitimately describes superseded policy. The exclusion is path-based, so
    any nested archive folder is also skipped.
    """
    paths: list[Path] = []
    for rel_root in doc_roots:
        root_path = root / rel_root
        if not root_path.exists():
            continue
        for path in sorted(root_path.rglob("*.md")):
            rel_parts = path.relative_to(root).parts
            if any(part in ARCHIVE_PATH_PARTS for part in rel_parts):
                continue
            paths.append(path)
    return paths


def run(
    root: Path,
    policy_path: Path,
    doc_roots: tuple[str, ...] = DEFAULT_DOC_ROOTS,
) -> list[DriftFinding]:
    """Run the contract end-to-end: parse the policy, walk the docs, return drift."""
    if not policy_path.exists():
        return [
            DriftFinding(
                path=policy_path,
                line_number=0,
                line_text="",
                detail=f"policy file not found at {policy_path}",
            )
        ]
    toml_text = policy_path.read_text(encoding="utf-8")
    count, names = extract_required_checks(toml_text)
    findings: list[DriftFinding] = []
    for doc in iter_doc_paths(root, doc_roots):
        doc_findings, _ = check_doc(doc, count, names)
        findings.extend(doc_findings)
    return findings


def main(argv: list[str]) -> int:
    """CLI entry point: exit 0 if the contract holds, 1 with a report otherwise."""
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root",
        type=Path,
        default=Path("."),
        help="Repository root to scan.",
    )
    parser.add_argument(
        "--policy",
        type=Path,
        default=Path(".ci/policies") / POLICY_FILENAME,
        help="Path to the required-checks TOML policy file.",
    )
    parser.add_argument(
        "--doc-root",
        action="append",
        default=None,
        help=(
            "Add a doc root to scan (relative to --root). May be passed "
            "multiple times. Defaults to %(default)s."
        ),
    )
    parser.add_argument(
        "--receipt",
        type=Path,
        default=None,
        help="Optional path to write a JSON receipt.",
    )
    args = parser.parse_args(argv)

    doc_roots = tuple(args.doc_root) if args.doc_root else DEFAULT_DOC_ROOTS
    findings = run(args.root, args.policy, doc_roots)

    receipt = {
        "policy_path": str(args.policy),
        "doc_roots": list(doc_roots),
        "findings": [
            {
                "path": str(finding.path.relative_to(args.root)),
                "line_number": finding.line_number,
                "line_text": finding.line_text,
                "detail": finding.detail,
            }
            for finding in findings
        ],
    }
    if args.receipt is not None:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        import json

        args.receipt.write_text(
            json.dumps(receipt, indent=2, sort_keys=True), encoding="utf-8"
        )

    if findings:
        print(f"required_checks_doc_contract: {len(findings)} drift finding(s):")
        for finding in findings:
            rel = finding.path.relative_to(args.root)
            print(f"  {rel}:{finding.line_number}: {finding.detail}")
            print(f"    | {finding.line_text.strip()[:200]}")
        return 1
    print("required_checks_doc_contract: clean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))