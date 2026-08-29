"""Deterministic parser for the landed #11178 Lite XL journey ledger.

The landed spec ledger (`.spec/11178-lite-xl-bdd-journeys/acceptance.md`) is
consumed strictly as DATA: this module derives the exact scenario-ID inventory
(baseline plus optional regions, in family order) from the bytes in the tree.
No scenario identity, count, wording, or profile membership is duplicated
here, so a #11178 revision instantly invalidates any replay receipt that was
bound to the previous ledger generation instead of silently drifting.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

BASELINE_HEADING = "## §Behavior — baseline journey ledger"
OPTIONAL_HEADING = "## §Behavior — optional and stronger-profile inputs"
SCENARIO_ROW = re.compile(r"^\|\s*`(lite_xl\.bdd\.[a-z0-9_]+\.[0-9]+)`")


class LedgerError(RuntimeError):
    """A bounded, user-actionable ledger-parsing failure."""


def _scenario_ids(lines: list[str]) -> list[str]:
    ids: list[str] = []
    for line in lines:
        match = SCENARIO_ROW.match(line.strip())
        if match:
            ids.append(match.group(1))
    return ids


def load_ledger_inventory(path: Path) -> dict[str, Any]:
    """Return the baseline and optional scenario inventories in file order."""
    text = read_ledger(path)
    lines = text.splitlines()
    baseline_start = None
    optional_start = None
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith(BASELINE_HEADING):
            if baseline_start is not None:
                raise LedgerError(f"{path} repeats the baseline journey ledger heading")
            baseline_start = index + 1
        elif stripped.startswith(OPTIONAL_HEADING):
            if optional_start is not None:
                raise LedgerError(f"{path} repeats the optional-input heading")
            optional_start = index + 1
    if baseline_start is None:
        raise LedgerError(f"{path} lacks the baseline journey ledger heading")
    if optional_start is None:
        raise LedgerError(f"{path} lacks the optional-input heading")
    if optional_start <= baseline_start:
        raise LedgerError(f"{path} orders the optional section before the baseline ledger")

    baseline = _scenario_ids(lines[baseline_start : optional_start - 1])
    rest = lines[optional_start:]
    end = len(rest)
    for offset, line in enumerate(rest):
        if line.startswith("## ") and not line.strip().startswith(OPTIONAL_HEADING):
            end = offset
            break
    optional = _scenario_ids(rest[:end])

    overlap = sorted(set(baseline) & set(optional))
    if overlap:
        raise LedgerError(f"{path} duplicates scenario IDs across regions: {overlap}")
    repeated_baseline = sorted({cid for cid in baseline if baseline.count(cid) > 1})
    if repeated_baseline:
        raise LedgerError(
            f"{path} repeats scenario IDs within the baseline ledger: {repeated_baseline[0]}"
        )
    repeated_optional = sorted({cid for cid in optional if optional.count(cid) > 1})
    if repeated_optional:
        raise LedgerError(
            f"{path} repeats scenario IDs within the optional table: {repeated_optional[0]}"
        )
    if not baseline:
        raise LedgerError(f"{path} carries no baseline scenarios")
    return {
        "baseline": baseline,
        "optional": optional,
        "baseline_set": frozenset(baseline),
        "optional_set": frozenset(optional),
    }


def read_ledger(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise LedgerError(f"cannot read {path}: {error}") from error
