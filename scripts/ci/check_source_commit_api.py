#!/usr/bin/env python3
"""Validate the #11301 source-commit API caller ledger.

This is a source-backed structural check: it identifies Rust method-call
surfaces in every tracked Rust source, requires exact ledger identity for every
caller file, checks that each caller has one owner row, and rejects compatibility
growth beyond the ledger's explicit accepted baseline. It deliberately does not
claim lifecycle/provider semantics.
"""

from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / ".spec/11301-source-commit-api-and-caller-ledger/caller-ledger.toml"
CALL_RE = re.compile(
    r"\.(?P<method>index_(?:initial_)?file(?:_str|_with_generation)?|index_(?:initial_)?files_batch)\("
)
COMPATIBLE = {"index_file", "index_file_str", "index_file_with_generation", "index_files_batch"}


def normalized_source(path: Path) -> str:
    source = path.read_text(encoding="utf-8")
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.DOTALL)
    source = re.sub(r"//[^\n]*", "", source)
    return re.sub(r"\s+", "", source)


def main() -> int:
    ledger = tomllib.loads(LEDGER.read_text(encoding="utf-8"))
    expected = set(ledger["caller_paths"])
    owners = ledger["owner"]
    compatibility_baseline = ledger["compatibility_baseline"]
    if len(expected) != len(ledger["caller_paths"]):
        raise SystemExit("duplicate caller identity in caller-ledger.toml")

    actual: dict[str, list[str]] = {}
    compatibility: list[tuple[str, str]] = []
    tracked_sources = subprocess.check_output(
        ["git", "ls-files", "-z", "--", "*.rs"], cwd=ROOT
    ).decode("utf-8").split("\0")
    for relative in filter(None, tracked_sources):
        path = ROOT / relative
        methods = [match.group("method") for match in CALL_RE.finditer(normalized_source(path))]
        if methods:
            actual[relative] = methods
            compatibility.extend((relative, method) for method in methods if method in COMPATIBLE)

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

    if len(compatibility) != compatibility_baseline:
        raise SystemExit(
            f"compatibility baseline changed: ledger={compatibility_baseline}, "
            f"source={len(compatibility)}; update the ledger only with explicit proof"
        )

    print(
        f"source-commit-api ledger valid: {len(actual_paths)} caller files, "
        f"{len(compatibility)} compatibility calls, {len(owners)} owner rows\n"
        "caller files:\n" + "\n".join(f"- {path}" for path in sorted(actual_paths))
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
