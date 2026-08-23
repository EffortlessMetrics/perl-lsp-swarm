#!/usr/bin/env python3
"""Stable digest for the visible material PR claim/review subject."""

from __future__ import annotations

import hashlib
import re
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


def _visible_structure_text(
    line: str,
    in_comment: bool,
    code_span_ticks: int | None,
) -> tuple[str, bool, int | None]:
    """Remove HTML comments while preserving literal Markdown code spans."""

    visible: list[str] = []
    cursor = 0
    while cursor < len(line):
        if in_comment:
            end = line.find("-->", cursor)
            if end < 0:
                return "".join(visible), True, code_span_ticks
            cursor = end + 3
            in_comment = False
            continue

        if code_span_ticks is not None:
            delimiter = "`" * code_span_ticks
            end = line.find(delimiter, cursor)
            if end < 0:
                visible.append(line[cursor:])
                return "".join(visible), False, code_span_ticks
            visible.append(line[cursor : end + code_span_ticks])
            cursor = end + code_span_ticks
            code_span_ticks = None
            continue

        if line.startswith("<!--", cursor):
            cursor += 4
            in_comment = True
            continue

        if line[cursor] == "`":
            end = cursor
            while end < len(line) and line[end] == "`":
                end += 1
            code_span_ticks = end - cursor
            visible.append(line[cursor:end])
            cursor = end
            continue

        visible.append(line[cursor])
        cursor += 1

    return "".join(visible), in_comment, code_span_ticks


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


def _is_indented_code(line: str) -> bool:
    return line.startswith("    ") or line.startswith("\t")


def canonical_material_claim(body: str) -> tuple[str, str]:
    """Return canonical visible material text and extraction mode.

    Only visible level-two headings delimit material sections. Heading-shaped
    text inside fenced/indented code, inline code spans, or HTML comments remains
    ordinary visible or hidden content according to GitHub Markdown semantics.
    HTML comments outside literal code do not affect currentness and cannot
    satisfy an otherwise-empty material section.
    """

    normalized = body.replace("\r\n", "\n").replace("\r", "\n")
    sections: dict[str, list[str]] = {}
    visible_document: list[str] = []
    current: str | None = None
    fence: tuple[str, int] | None = None
    in_comment = False
    code_span_ticks: int | None = None

    for source_line in normalized.split("\n"):
        if fence is not None:
            visible_document.append(source_line)
            if current is not None:
                sections[current].append(source_line)
            if _is_fence_closing(source_line, fence[0], fence[1]):
                fence = None
            continue

        if not in_comment and code_span_ticks is None:
            opening = _fence_opening(source_line)
            if opening is not None:
                visible_document.append(source_line)
                if current is not None:
                    sections[current].append(source_line)
                fence = opening
                continue
            if _is_indented_code(source_line):
                visible_document.append(source_line)
                if current is not None:
                    sections[current].append(source_line)
                continue

        was_in_comment = in_comment
        visible_line, in_comment, code_span_ticks = _visible_structure_text(
            source_line,
            in_comment,
            code_span_ticks,
        )
        comment_only_line = (
            not visible_line.strip()
            and (was_in_comment or "<!--" in source_line or "-->" in source_line)
        )
        if not comment_only_line:
            visible_document.append(visible_line)

        match = _HEADING.match(visible_line)
        if match and code_span_ticks is None:
            current = match.group(1).strip().casefold()
            sections.setdefault(current, [])
            continue

        if current is not None and not comment_only_line:
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


if __name__ == "__main__":
    raise SystemExit(
        "RETIRED: scripts/reviews/claim_digest.py is import-only; it does not read live PRs"
    )
