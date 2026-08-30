#!/usr/bin/env python3
"""Update manual TokenKind tables and the explicit public cardinality contract."""

from __future__ import annotations

import re
from pathlib import Path


def replace_exact(
    path: str | Path,
    old: str,
    new: str,
    *,
    expected: int = 1,
) -> int:
    file_path = Path(path)
    text = file_path.read_text()
    actual = text.count(old)
    if actual != expected:
        raise SystemExit(
            f"{file_path}: expected {expected} occurrences, found {actual}: {old!r}"
        )
    file_path.write_text(text.replace(old, new))
    return actual


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


def update_cardinality_contract() -> None:
    replace_exact(
        "crates/perl-token/src/lib.rs",
        """    fn all_returns_132_variants() {\n        assert_eq!(TokenKind::all().len(), 132);\n        assert_eq!(TokenKind::metadata_count(), 132);\n    }\n""",
        """    fn all_returns_134_variants() {\n        assert_eq!(TokenKind::all().len(), 134);\n        assert_eq!(TokenKind::metadata_count(), 134);\n    }\n""",
    )

    conformance = "crates/perl-token/tests/conformance_guards.rs"
    replace_exact(
        conformance,
        "const EXPECTED_TOKEN_KIND_COUNT: usize = 132;",
        "const EXPECTED_TOKEN_KIND_COUNT: usize = 134;",
    )
    replace_exact(
        conformance,
        '            "LogicalOrAssign",\n            "DefinedOrAssign",',
        '            "LogicalOrAssign",\n            "LogicalXorAssign",\n            "DefinedOrAssign",',
    )
    replace_exact(
        conformance,
        '            "Or",\n            "Not",',
        '            "Or",\n            "LogicalXor",\n            "Not",',
    )
    replace_exact(
        conformance,
        "silently under-count distinct kinds while keeping the length at 132",
        "silently under-count distinct kinds while keeping the length at 134",
    )

    for doc in (
        "crates/perl-token/README.md",
        "crates/perl-token/ROADMAP.md",
    ):
        replace_exact(doc, "TokenKind variants: 132", "TokenKind variants: 134")


def verify_old_cardinality_is_gone() -> None:
    checks = {
        Path("crates/perl-token/src/lib.rs"): (
            "all_returns_132_variants",
            "metadata_count(), 132",
        ),
        Path("crates/perl-token/tests/conformance_guards.rs"): (
            "EXPECTED_TOKEN_KIND_COUNT: usize = 132",
            '"LogicalOrAssign",\n            "DefinedOrAssign"',
            '"Or",\n            "Not"',
        ),
        Path("crates/perl-token/README.md"): ("TokenKind variants: 132",),
        Path("crates/perl-token/ROADMAP.md"): ("TokenKind variants: 132",),
    }
    for path, stale_fragments in checks.items():
        text = path.read_text()
        for fragment in stale_fragments:
            if fragment in text:
                raise SystemExit(f"{path}: stale 132-variant contract remains: {fragment!r}")


assignment_entries = insert_variant("LogicalOrAssign", "LogicalXorAssign", minimum=2)
logical_entries = insert_variant("Or", "LogicalXor", minimum=2)
update_cardinality_contract()
verify_old_cardinality_is_gone()
print(
    "patched token test/public contract: "
    f"assignment_entries={assignment_entries}, logical_entries={logical_entries}, variants=134"
)
