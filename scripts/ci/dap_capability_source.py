"""Exhaustive parser for the canonical production initialize capability object."""

from __future__ import annotations

import json
import re
from typing import Mapping

from dap_capability_common import EXPRESSION, PRODUCTION_ANCHOR, MatrixError

IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def _skip_space_and_comments(text: str, index: int, end: int) -> int:
    while index < end:
        if text[index].isspace():
            index += 1
            continue
        if text.startswith("//", index):
            newline = text.find("\n", index + 2, end)
            return end if newline < 0 else _skip_space_and_comments(text, newline + 1, end)
        if text.startswith("/*", index):
            depth = 1
            index += 2
            while index < end and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            if depth:
                raise MatrixError("unterminated block comment in production capability object")
            continue
        break
    return index


def _scan_quoted(text: str, index: int, quote: str, end: int) -> int:
    index += 1
    while index < end:
        character = text[index]
        if character == "\\":
            index += 2
            continue
        if character == quote:
            return index + 1
        index += 1
    raise MatrixError("unterminated quoted literal in production capability object")


def _split_entries(source_text: str) -> list[str]:
    occurrences = source_text.count(PRODUCTION_ANCHOR)
    if occurrences != 1:
        raise MatrixError(
            f"production capability anchor must occur exactly once, observed {occurrences}"
        )
    anchor = source_text.find(PRODUCTION_ANCHOR)
    body_start = anchor + len(PRODUCTION_ANCHOR)
    entries: list[str] = []
    entry_start = body_start
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    index = body_start
    end = len(source_text)
    while index < end:
        index = _skip_space_and_comments(source_text, index, end)
        if index >= end:
            break
        character = source_text[index]
        if character in {'"', "'"}:
            index = _scan_quoted(source_text, index, character, end)
            continue
        if character in "([{":
            stack.append(character)
            index += 1
            continue
        if character in ")]} ".replace(" ", ""):
            if character == "}" and not stack:
                entries.append(source_text[entry_start:index])
                tail = _skip_space_and_comments(source_text, index + 1, end)
                if tail >= end or source_text[tail] != ")":
                    raise MatrixError(
                        "production capability object must close the json! macro with ')'"
                    )
                tail = _skip_space_and_comments(source_text, tail + 1, end)
                if tail >= end or source_text[tail] != ";":
                    raise MatrixError(
                        "production capability object must end with a semicolon"
                    )
                return entries
            expected = pairs[character]
            if not stack or stack[-1] != expected:
                raise MatrixError(
                    f"unmatched delimiter {character!r} in production capability object"
                )
            stack.pop()
            index += 1
            continue
        if character == "," and not stack:
            entries.append(source_text[entry_start:index])
            entry_start = index + 1
        index += 1
    raise MatrixError("production capability object is unterminated")


def _parse_field_name(fragment: str, index: int, end: int) -> tuple[str, int]:
    if index >= end or fragment[index] != '"':
        raise MatrixError(f"unclassified production capability fragment: {fragment.strip()!r}")
    stop = _scan_quoted(fragment, index, '"', end)
    literal = fragment[index:stop]
    try:
        name = json.loads(literal)
    except json.JSONDecodeError as exc:
        raise MatrixError(f"invalid capability field string {literal!r}") from exc
    if not isinstance(name, str) or re.fullmatch(r"[A-Za-z][A-Za-z0-9]*", name) is None:
        raise MatrixError(f"invalid production capability wire name: {name!r}")
    return name, stop


def _parse_entry(fragment: str) -> tuple[str, str] | None:
    end = len(fragment)
    index = _skip_space_and_comments(fragment, 0, end)
    if index >= end:
        return None
    name, index = _parse_field_name(fragment, index, end)
    index = _skip_space_and_comments(fragment, index, end)
    if index >= end or fragment[index] != ":":
        raise MatrixError(f"capability {name} is missing a top-level colon")
    index = _skip_space_and_comments(fragment, index + 1, end)
    match = IDENTIFIER.match(fragment, index)
    if match is None:
        raise MatrixError(f"capability {name} has no canonical simple expression")
    expression = match.group(0)
    if EXPRESSION.fullmatch(expression) is None:
        raise MatrixError(
            f"production capability {name} uses an unclassified expression {expression!r}"
        )
    index = _skip_space_and_comments(fragment, match.end(), end)
    if index != end:
        raise MatrixError(
            f"unclassified syntax after capability {name}: {fragment[index:].strip()!r}"
        )
    return name, expression


def extract_production_capabilities(
    source_text: str, anchor: str = PRODUCTION_ANCHOR
) -> dict[str, str]:
    if anchor != PRODUCTION_ANCHOR:
        raise MatrixError(
            f"production anchor is independently fixed as {PRODUCTION_ANCHOR!r}, got {anchor!r}"
        )
    extracted: dict[str, str] = {}
    for fragment in _split_entries(source_text):
        parsed = _parse_entry(fragment)
        if parsed is None:
            continue
        name, expression = parsed
        if name in extracted:
            raise MatrixError(f"production capability object repeats {name}")
        extracted[name] = expression
    if not extracted:
        raise MatrixError("production capability object contains no fields")
    return extracted


def compare_inventory(
    matrix_rows: Mapping[str, Mapping[str, object]],
    production: Mapping[str, str],
) -> None:
    declared = set(matrix_rows)
    emitted = set(production)
    if declared != emitted:
        raise MatrixError(
            "capability inventory drift; "
            f"missing_rows={sorted(emitted - declared)}, stale_rows={sorted(declared - emitted)}"
        )
    for name, expression in production.items():
        declared_expression = matrix_rows[name].get("expression")
        if declared_expression != expression:
            raise MatrixError(
                f"capability expression drift for {name}: "
                f"matrix={declared_expression!r}, production={expression!r}"
            )
