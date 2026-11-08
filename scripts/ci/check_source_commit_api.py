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
LEDGER_RELATIVE = ".spec/11301-source-commit-api-and-caller-ledger/caller-ledger.toml"
LEDGER = ROOT / LEDGER_RELATIVE


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


def owner_row_for_path(
    path: str, owners: object
) -> dict[str, object] | None:
    if not isinstance(owners, list):
        return None
    matches = [
        row
        for row in owners
        if isinstance(row, dict)
        and isinstance(row.get("prefix"), str)
        and (path == row["prefix"] or path.startswith(str(row["prefix"]) + "/"))
    ]
    if not matches:
        return None
    longest = max(len(str(row["prefix"])) for row in matches)
    owners_at_anchor = [row for row in matches if len(str(row["prefix"])) == longest]
    if len(owners_at_anchor) != 1:
        return None
    return owners_at_anchor[0]


def compatibility_baseline_mismatch_message(
    *,
    ledger_path: str,
    ledger_baseline: int,
    compatibility: list[tuple[str, str]],
    owners: object,
) -> str:
    """Name the drifted ledger field and per-file counts; do not recount source.

    The caller already collected ``compatibility`` while scanning tracked
    files. This formats that existing list so a post-merge red names the
    ledger file/value and the files that currently contribute to the count,
    plus each owner row's ``removal_condition`` (prefer burn-down).
    """
    per_file: dict[str, int] = {}
    for path, _method in compatibility:
        per_file[path] = per_file.get(path, 0) + 1
    lines = [
        (
            f"compatibility baseline changed: ledger={ledger_baseline}, "
            f"source={len(compatibility)}"
        ),
        f"ledger file: {ledger_path}",
        f"ledger field: compatibility_baseline={ledger_baseline}",
        "per-file compatibility calls:",
    ]
    for path in sorted(per_file):
        lines.append(f"- {path}: {per_file[path]}")
    conditions: list[str] = []
    seen_prefixes: set[str] = set()
    for path in sorted(per_file):
        row = owner_row_for_path(path, owners)
        if row is None:
            continue
        prefix = str(row.get("prefix", ""))
        condition = row.get("removal_condition")
        if not prefix or not isinstance(condition, str) or not condition:
            continue
        if prefix in seen_prefixes:
            continue
        seen_prefixes.add(prefix)
        conditions.append(f"- {prefix}: {condition}")
    if conditions:
        lines.append(
            "owner removal_condition (prefer burn-down over raising "
            "compatibility_baseline):"
        )
        lines.extend(conditions)
    lines.append(
        "update the ledger only with explicit proof; prefer the owner row's "
        "removal_condition over raising compatibility_baseline"
    )
    return "\n".join(lines)


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
    banned_prefixes = ledger.get("compatibility_banned_prefixes", [])
    if not isinstance(banned_prefixes, list) or not all(
        isinstance(prefix, str) and prefix for prefix in banned_prefixes
    ):
        raise SystemExit("compatibility_banned_prefixes must be a list of non-empty strings")
    tracked_sources = subprocess.check_output(
        ["git", "ls-files", "-z", "--", "*.rs"], cwd=ROOT
    ).decode("utf-8").split("\0")
    for relative in filter(None, tracked_sources):
        path = ROOT / relative
        methods = [match.group("method") for match in call_re.finditer(normalized_source(path))]
        if methods:
            actual[relative] = methods
            compatibility.extend((relative, method) for method in methods if method in compatible)

    # Structural recurrence control (#11305): banned source regions must never
    # call a compatibility API at all, regardless of the global baseline.
    # Prefix matching fails closed: any tracked file whose path starts with a
    # banned prefix is covered.
    for relative, method in compatibility:
        for prefix in banned_prefixes:
            if relative.startswith(prefix):
                raise SystemExit(
                    f"compatibility ban violated: {relative} calls {method} "
                    f"(banned prefix {prefix})"
                )

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
            compatibility_baseline_mismatch_message(
                ledger_path=LEDGER_RELATIVE,
                ledger_baseline=compatibility_baseline,
                compatibility=compatibility,
                owners=owners,
            )
        )

    print(
        f"source-commit-api ledger valid: {len(actual_paths)} caller files, "
        f"{len(compatibility)} compatibility calls, {len(owners)} owner rows, "
        f"{len(banned_prefixes)} banned prefixes; "
        f"canonical APIs: {', '.join(sorted(canonical))}; "
        f"compatibility APIs: {', '.join(sorted(compatible))}\n"
        "caller files:\n" + "\n".join(f"- {path}" for path in sorted(actual_paths))
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
