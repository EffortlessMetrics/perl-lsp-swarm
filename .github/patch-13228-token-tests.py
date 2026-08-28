#!/usr/bin/env python3
"""Update manual TokenKind test tables without changing their match shape."""

from __future__ import annotations

import re
from pathlib import Path


def insert_variant(anchor: str, variant: str, minimum: int) -> int:
    pattern = re.compile(
        rf"^(?P<indent>\s*)(?P<prefix>\|\s+)?TokenKind::{anchor},$",
        re.MULTILINE,
    )
    total = 0
    for path in Path("crates/perl-token/tests").rglob("*.rs"):
        text = path.read_text()

        def replacement(match: re.Match[str]) -> str:
            nonlocal total
            total += 1
            indent = match.group("indent")
            prefix = match.group("prefix") or ""
            return (
                f"{indent}{prefix}TokenKind::{anchor},\n"
                f"{indent}{prefix}TokenKind::{variant},"
            )

        updated = pattern.sub(replacement, text)
        path.write_text(updated)

    if total < minimum:
        raise SystemExit(
            f"expected at least {minimum} TokenKind::{anchor} test-table entries, found {total}"
        )
    return total


assignment_entries = insert_variant("LogicalOrAssign", "LogicalXorAssign", minimum=2)
logical_entries = insert_variant("Or", "LogicalXor", minimum=2)
print(
    "patched token-test tables: "
    f"assignment_entries={assignment_entries}, logical_entries={logical_entries}"
)
