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
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ModuleNotFoundError:
        tomllib = None  # type: ignore[assignment]

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / ".spec/11301-source-commit-api-and-caller-ledger/caller-ledger.toml"


def normalized_source(path: Path) -> str:
    source = path.read_text(encoding="utf-8")
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.DOTALL)
    source = re.sub(r"//[^\n]*", "", source)
    return re.sub(r"\s+", "", source)


def declared_apis(ledger: dict[str, object]) -> tuple[set[str], set[str], re.Pattern[str]]:
    canonical_keys = (
        "canonical_initial_api",
        "canonical_initial_batch_api",
        "canonical_live_api",
    )
    canonical = {ledger.get(key) for key in canonical_keys}
    compatibility = ledger.get("compatibility_apis")
    if not all(isinstance(api, str) and api for api in canonical):
        raise SystemExit("canonical API metadata must contain non-empty strings")
    if not isinstance(compatibility, list) or not all(
        isinstance(api, str) and api for api in compatibility
    ):
        raise SystemExit("compatibility_apis metadata must be a non-empty string list")
    compatibility_set = set(compatibility)
    if len(canonical) != len(canonical_keys):
        raise SystemExit("canonical API metadata contains duplicates")
    if len(compatibility_set) != len(compatibility):
        raise SystemExit("compatibility_apis metadata contains duplicates")
    if canonical & compatibility_set:
        raise SystemExit("canonical and compatibility API metadata overlap")
    # The string/path initial entry point is the explicitly named companion of
    # the canonical single-file initial API; keep that family relationship
    # derived from the ledger instead of duplicating a second canonical list.
    canonical_surface = canonical | {f"{ledger['canonical_initial_api']}_str"}
    api_names = sorted(canonical_surface | compatibility_set, key=lambda api: (-len(api), api))
    call_re = re.compile(r"\.(?P<method>" + "|".join(map(re.escape, api_names)) + r")\(")
    return canonical, compatibility_set, call_re


def main() -> int:
    if tomllib is None:
        raise SystemExit("Python 3.11+ or the 'tomli' package is required to parse TOML")
    ledger = tomllib.loads(LEDGER.read_text(encoding="utf-8"))
    canonical, compatible, call_re = declared_apis(ledger)
    expected = set(ledger["caller_paths"])
    owners = ledger["owner"]
    compatibility_baseline = ledger["compatibility_baseline"]
    if not isinstance(compatibility_baseline, int) or compatibility_baseline < 0:
        raise SystemExit("compatibility_baseline metadata must be a non-negative integer")
    if len(expected) != len(ledger["caller_paths"]):
        raise SystemExit("duplicate caller identity in caller-ledger.toml")

    actual: dict[str, list[str]] = {}
    compatibility: list[tuple[str, str]] = []
    tracked_sources = subprocess.check_output(
        ["git", "ls-files", "-z", "--", "*.rs"], cwd=ROOT
    ).decode("utf-8").split("\0")
    for relative in filter(None, tracked_sources):
        path = ROOT / relative
        methods = [match.group("method") for match in call_re.finditer(normalized_source(path))]
        if methods:
            actual[relative] = methods
            compatibility.extend((relative, method) for method in methods if method in compatible)

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
        f"{len(compatibility)} compatibility calls, {len(owners)} owner rows; "
        f"canonical APIs: {', '.join(sorted(canonical))}; "
        f"compatibility APIs: {', '.join(sorted(compatible))}\n"
        "caller files:\n" + "\n".join(f"- {path}" for path in sorted(actual_paths))
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
