#!/usr/bin/env python3
"""Update manual TokenKind test tables and cardinality invariants."""

from __future__ import annotations

import re
from pathlib import Path


def insert_variant(anchor: str, variant: str, minimum: int) -> int:
    pattern = re.compile(
        rf"^(?P<indent>\s*)(?P<prefix>\|\s+)?TokenKind::{anchor}(?P<comma>,?)$",
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
            comma = match.group("comma")

            if prefix:
                # OR-pattern alternatives may be either intermediate lines
                # without punctuation or the final, comma-terminated line.
                # Preserve that shape while inserting the new alternative.
                if comma:
                    return (
                        f"{indent}{prefix}TokenKind::{anchor}\n"
                        f"{indent}{prefix}TokenKind::{variant},"
                    )
                return (
                    f"{indent}{prefix}TokenKind::{anchor}\n"
                    f"{indent}{prefix}TokenKind::{variant}"
                )

            if comma:
                return (
                    f"{indent}TokenKind::{anchor},\n"
                    f"{indent}TokenKind::{variant},"
                )

            raise SystemExit(
                f"{path}: TokenKind::{anchor} table entry has neither OR-pattern prefix nor comma"
            )

        updated = pattern.sub(replacement, text)
        path.write_text(updated)

    if total < minimum:
        raise SystemExit(
            f"expected at least {minimum} TokenKind::{anchor} test-table entries, found {total}"
        )
    return total


def update_cardinality_invariant() -> None:
    path = Path("crates/perl-token/src/lib.rs")
    text = path.read_text()
    old = """    fn all_returns_132_variants() {\n        assert_eq!(TokenKind::all().len(), 132);\n        assert_eq!(TokenKind::metadata_count(), 132);\n    }\n"""
    new = """    fn all_returns_134_variants() {\n        assert_eq!(TokenKind::all().len(), 134);\n        assert_eq!(TokenKind::metadata_count(), 134);\n    }\n"""
    if text.count(old) != 1:
        raise SystemExit("expected exactly one 132-variant TokenKind cardinality invariant")
    path.write_text(text.replace(old, new))


assignment_entries = insert_variant("LogicalOrAssign", "LogicalXorAssign", minimum=2)
logical_entries = insert_variant("Or", "LogicalXor", minimum=2)
update_cardinality_invariant()
print(
    "patched token-test tables/cardinality: "
    f"assignment_entries={assignment_entries}, logical_entries={logical_entries}, variants=134"
)
