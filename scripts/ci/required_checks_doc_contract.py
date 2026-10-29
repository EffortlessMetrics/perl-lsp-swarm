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


def _code_span_regions(line: str) -> list[tuple[int, int]]:
    """Return ``[(start, end)]`` inline code-span regions on ``line``.

    Markdown code spans pair backtick delimiter runs: a run opens a span and
    the next run of at least the same length closes it, so a valid
    ```` two-backtick ```` span has even counts on both sides and must not be
    scanned as prose (#16127 review). An unclosed run extends its span to the
    end of the line.
    """
    regions: list[tuple[int, int]] = []
    in_span = False
    open_len = 0
    open_start = 0
    i = 0
    n = len(line)
    while i < n:
        if line[i] == "`":
            j = i
            while j < n and line[j] == "`":
                j += 1
            run = j - i
            if in_span:
                if run >= open_len:
                    regions.append((open_start, j))
                    in_span = False
            else:
                in_span = True
                open_len = run
                open_start = i
            i = j
        else:
            i += 1
    if in_span:
        regions.append((open_start, n))
    return regions


def _inside_code_span(line: str, match_start: int, match_end: int) -> bool:
    """Return True when the match overlaps an inline code span on ``line``."""
    return any(
        start < match_end and end > match_start
        for start, end in _code_span_regions(line)
    )


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


def _fence_marker(stripped: str) -> tuple[str, int] | None:
    """Return ``(fence_char, run_length)`` when ``stripped`` is a fence line.

    A fenced code block opens with a run of at least three backticks or
    tildes (optionally followed by an info string) and closes only with a run
    of the same character at least as long as the opener (#16127 review).
    """
    if len(stripped) < 3 or stripped[0] not in ("`", "~"):
        return None
    char = stripped[0]
    run = len(stripped) - len(stripped.lstrip(char))
    if run < 3:
        return None
    return char, run


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

    Two fidelity rules close the #16127 review findings: a fenced block only
    closes on the same fence character with a run at least as long as its
    opener, and consecutive non-blank prose lines form one logical paragraph
    (Markdown renders the soft line break as a space), so a count phrase
    wrapped across lines is still presented to ``COUNT_RE`` in full.
    """
    findings: list[DriftFinding] = []
    inside_block = in_code_block
    # ``(char, run)`` of the fence that opened the current block. ``None``
    # while resuming a chunk that was already inside a block, where the opener
    # is unknown and any fence line closes (the pre-existing degraded edge).
    open_fence: tuple[str, int] | None = None if inside_block else None
    # ``None`` while resuming a chunk that was already inside a block (opener
    # unknown: any fence line closes); set to the opener when a fence opens.
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

    def report(count: int, matched: str, line_no: int, line: str) -> None:
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

    paragraph: list[tuple[str, int]] = []

    def flush_paragraph() -> None:
        if not paragraph:
            return
        reported: set[tuple[int, int]] = set()
        # Per-line pass (unchanged behavior): every physical line is scanned.
        for line_text, line_no in paragraph:
            for count, matched in find_count_mentions(line_text):
                if count == required_count or (count, line_no) in reported:
                    continue
                reported.add((count, line_no))
                report(count, matched, line_no, line_text)
        # Paragraph pass: soft-wrapped phrases render as one prose line, so
        # scan the joined paragraph and attribute each new finding to the
        # line where the match starts. Already-reported (count, line) pairs
        # from the per-line pass are not duplicated.
        if len(paragraph) > 1:
            parts: list[str] = []
            offsets: list[tuple[int, int, int, str]] = []
            cursor = 0
            for line_text, line_no in paragraph:
                stripped_text = line_text.strip()
                parts.append(stripped_text)
                offsets.append((cursor, cursor + len(stripped_text), line_no, line_text))
                cursor += len(stripped_text) + 1
            joined = " ".join(parts)
            for match in COUNT_RE.finditer(joined):
                if _inside_code_span(joined, match.start(), match.end()):
                    continue
                if FRACTION_PRECEDING.search(joined[: match.start()]):
                    continue
                count = _word_to_int(match.group("count"))
                if count == required_count:
                    continue
                target = offsets[0]
                for start, end, line_no, line_text in offsets:
                    if match.start() < end:
                        target = (start, end, line_no, line_text)
                        break
                _, _, line_no, line_text = target
                if (count, line_no) in reported:
                    continue
                reported.add((count, line_no))
                report(count, match.group(0), line_no, line_text)
        paragraph.clear()

    for line_no, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        fence = _fence_marker(stripped)
        if inside_block:
            # A closing fence uses the opener's character with a run at
            # least as long; anything else is literal block content.
            if fence is not None and (
                open_fence is None
                or (fence[0] == open_fence[0] and fence[1] >= open_fence[1])
            ):
                inside_block = False
                open_fence = None
            continue
        if fence is not None:
            inside_block = True
            open_fence = fence
            flush_paragraph()
            continue
        if not stripped:
            flush_paragraph()
            continue
        paragraph.append((line, line_no))
    flush_paragraph()

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
        if not root_path.is_dir():
            # Fail closed (#16127 review): a missing or non-directory
            # configured root means the contract validated nothing; silently
            # returning no files would report clean while the validation
            # subject disappeared (rename, typo).
            raise FileNotFoundError(f"documentation root not found: {root_path}")
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
    try:
        findings = run(args.root, args.policy, doc_roots)
    except FileNotFoundError as exc:
        print(f"required_checks_doc_contract: {exc}")
        return 1

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