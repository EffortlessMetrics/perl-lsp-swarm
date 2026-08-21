#!/usr/bin/env python3
"""Validate the #11301 source-commit API caller ledger.

This is a source-backed structural check: it identifies Rust method-call
surfaces, requires exact ledger identity for every caller file, checks that each
caller has one owner row, and rejects compatibility growth beyond the accepted
baseline. It deliberately does not claim lifecycle/provider semantics.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / ".spec/11301-source-commit-api-and-caller-ledger/caller-ledger.toml"
CALL_RE = re.compile(
    r"\.(?P<method>index_(?:initial_)?file(?:_str|_with_generation)?|index_(?:initial_)?files_batch)\s*\("
)
COMPATIBLE = {"index_file", "index_file_str", "index_file_with_generation", "index_files_batch"}
MAX_COMPATIBILITY_CALLS = 692


def source_lines(path: Path) -> list[str]:
    lines = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        code = raw.split("//", 1)[0]
        if code.strip():
            lines.append(code)
    return lines


def main() -> int:
    ledger = tomllib.loads(LEDGER.read_text(encoding="utf-8"))
    expected = set(ledger["caller_paths"])
    owners = ledger["owner"]
    if len(expected) != len(ledger["caller_paths"]):
        raise SystemExit("duplicate caller identity in caller-ledger.toml")

    actual: dict[str, list[str]] = {}
    compatibility: list[tuple[str, str]] = []
    for root_name in ("crates", "tests"):
        root = ROOT / root_name
        if not root.exists():
            continue
        for path in sorted(root.rglob("*.rs")):
            rel = path.relative_to(ROOT).as_posix()
            methods = [match.group("method") for line in source_lines(path) for match in CALL_RE.finditer(line)]
            if methods:
                actual[rel] = methods
                compatibility.extend((rel, method) for method in methods if method in COMPATIBLE)

    actual_paths = set(actual)
    missing = sorted(actual_paths - expected)
    stale = sorted(expected - actual_paths)
    if missing:
        raise SystemExit("unledgered caller identity: " + ", ".join(missing))
    if stale:
        raise SystemExit("stale ledger identity: " + ", ".join(stale))

    for path in sorted(actual_paths):
        matches = [
            row
            for row in owners
            if path == row["prefix"] or path.startswith(row["prefix"] + "/")
        ]
        if not matches:
            raise SystemExit(f"caller role drift for {path}: no owner row")
        longest = max(len(row["prefix"]) for row in matches)
        owners_at_anchor = [row for row in matches if len(row["prefix"]) == longest]
        if len(owners_at_anchor) != 1:
            raise SystemExit(
                f"caller role drift for {path}: duplicate owner anchor ({len(owners_at_anchor)})"
            )
        row = owners_at_anchor[0]
        for field in ("role", "owner", "successor", "removal_condition"):
            if not row.get(field):
                raise SystemExit(f"missing {field} for {path}")

    if len(compatibility) > MAX_COMPATIBILITY_CALLS:
        raise SystemExit(
            f"compatibility growth: {len(compatibility)} calls exceeds {MAX_COMPATIBILITY_CALLS}"
        )

    print(
        f"source-commit-api ledger valid: {len(actual_paths)} caller files, "
        f"{len(compatibility)} compatibility calls, {len(owners)} owner rows"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
