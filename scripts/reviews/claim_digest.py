#!/usr/bin/env python3
"""Stable digest for the visible material PR claim/review subject."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Final

MATERIAL_SECTIONS: Final[tuple[str, ...]] = (
    "Claim",
    "What this establishes",
    "What this does not establish",
    "Risk and rollback",
    "Review index",
)

# The first alias is the current-generation canonical heading. Later aliases
# preserve meaningful review identity for PRs created from the repository's
# existing template while that template migration lands separately.
SECTION_ALIASES: Final[dict[str, tuple[str, ...]]] = {
    "Claim": ("Claim", "Claim Boundary"),
    "What this establishes": ("What this establishes", "Behavior", "Changes"),
    "What this does not establish": (
        "What this does not establish",
        "Non-goals",
        "Remaining Work",
    ),
    "Risk and rollback": (
        "Risk and rollback",
        "Risk",
        "Rollback",
        "Risks",
        "Risk Surfaces",
    ),
    "Review index": ("Review index",),
}

_HEADING = re.compile(r"^ {0,3}##(?!#)[ \t]+(.+?)(?:[ \t]+#+[ \t]*)?$")
_FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})(.*)$")


def normalize_pr_body(value: object) -> str:
    """Normalize GitHub's nullable body field without inventing text."""

    return value if isinstance(value, str) else ""


def _normalize_text(text: str) -> str:
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    normalized = [line.rstrip() for line in lines]
    while normalized and normalized[-1] == "":
        normalized.pop()
    return "\n".join(normalized).strip()


def _visible_structure_text(line: str, in_comment: bool) -> tuple[str, bool]:
    """Remove HTML comments from rendered material outside fenced code."""

    visible: list[str] = []
    cursor = 0
    while cursor < len(line):
        if in_comment:
            end = line.find("-->", cursor)
            if end < 0:
                return "".join(visible), True
            cursor = end + 3
            in_comment = False
            continue

        start = line.find("<!--", cursor)
        if start < 0:
            visible.append(line[cursor:])
            break
        visible.append(line[cursor:start])
        cursor = start + 4
        in_comment = True

    return "".join(visible), in_comment


def _fence_opening(line: str) -> tuple[str, int] | None:
    match = _FENCE.match(line)
    if not match:
        return None
    sequence = match.group(1)
    return sequence[0], len(sequence)


def _is_fence_closing(line: str, marker: str, minimum: int) -> bool:
    stripped = line.lstrip(" ")
    if len(line) - len(stripped) > 3 or not stripped.startswith(marker * minimum):
        return False
    count = 0
    while count < len(stripped) and stripped[count] == marker:
        count += 1
    return count >= minimum and stripped[count:].strip() == ""


def canonical_material_claim(body: str) -> tuple[str, str]:
    """Return canonical visible material text and extraction mode.

    Only visible level-two headings delimit material sections. Heading-shaped
    text inside fenced code blocks or HTML comments remains ordinary content and
    cannot switch the digest from full-body fallback into material-section mode.
    HTML comments outside fenced code are not rendered material, so they do not
    affect currentness and cannot satisfy an otherwise-empty material section.
    """

    normalized = body.replace("\r\n", "\n").replace("\r", "\n")
    sections: dict[str, list[str]] = {}
    visible_document: list[str] = []
    current: str | None = None
    fence: tuple[str, int] | None = None
    in_comment = False

    for source_line in normalized.split("\n"):
        if fence is not None:
            visible_document.append(source_line)
            if current is not None:
                sections[current].append(source_line)
            if _is_fence_closing(source_line, fence[0], fence[1]):
                fence = None
            continue

        visible_line, in_comment = _visible_structure_text(source_line, in_comment)
        visible_document.append(visible_line)
        opening = _fence_opening(visible_line)
        if opening is not None:
            if current is not None:
                sections[current].append(visible_line)
            fence = opening
            continue

        match = _HEADING.match(visible_line)
        if match:
            current = match.group(1).strip().casefold()
            sections.setdefault(current, [])
            continue

        if current is not None:
            sections[current].append(visible_line)

    recognized_keys = {
        alias.casefold()
        for aliases in SECTION_ALIASES.values()
        for alias in aliases
    }
    if not recognized_keys.intersection(sections):
        canonical = _normalize_text("\n".join(visible_document))
        if not canonical:
            raise ValueError("empty PR body has no material claim to review")
        return canonical, "full_body_fallback"

    parts: list[str] = []
    has_material_content = False
    for canonical_name in MATERIAL_SECTIONS:
        values: list[str] = []
        for alias in SECTION_ALIASES[canonical_name]:
            key = alias.casefold()
            if key not in sections:
                continue
            value = _normalize_text("\n".join(sections[key]))
            if value:
                has_material_content = True
            values.append(f"### {alias}\n{value}")

        material = "\n\n".join(values) if values else "<missing>"
        parts.append(f"## {canonical_name}\n{material}")

    if not has_material_content:
        raise ValueError("recognized PR headings contain no material claim content")

    return "\n\n".join(parts), "material_sections"


def claim_digest(body: str) -> dict[str, object]:
    canonical, mode = canonical_material_claim(body)
    digest = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    return {
        "algorithm": "sha256",
        "digest": digest,
        "mode": mode,
        "sections": list(MATERIAL_SECTIONS),
        "canonical_bytes": len(canonical.encode("utf-8")),
    }


def _read_live_pr_body(pr: str, repo: str | None) -> str:
    command = [
        "gh",
        "pr",
        "view",
        pr,
        "--json",
        "body",
        "--jq",
        '.body // ""',
    ]
    if repo:
        command.extend(["--repo", repo])
    completed = subprocess.run(command, check=False, capture_output=True, text=True)
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or "gh pr view failed")
    return normalize_pr_body(completed.stdout)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--pr", help="GitHub pull-request number or URL")
    source.add_argument("--body-file", type=Path, help="Read PR body from a fixture/file")
    source.add_argument("--stdin", action="store_true", help="Read PR body from stdin")
    parser.add_argument("--repo", help="owner/repo for --pr")
    parser.add_argument("--json", action="store_true", help="Emit the full digest record")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.pr:
            body = _read_live_pr_body(args.pr, args.repo)
        elif args.body_file:
            body = args.body_file.read_text(encoding="utf-8")
        else:
            body = sys.stdin.read()
        record = claim_digest(body)
    except (OSError, RuntimeError, ValueError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(record, sort_keys=True))
    else:
        print(record["digest"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
