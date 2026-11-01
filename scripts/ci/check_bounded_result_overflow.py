#!/usr/bin/env python3
"""Check bounded-result overflow invariants that draft-07 cannot express.

Wired into the Policy Validators workflow (.github/workflows/policy-validators.yml)
and triggered on changes to the schema, fixtures, or this script.
"""

from __future__ import annotations

import json
from decimal import Decimal
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "docs/agents/bounded-subagent-result-v1.schema.json"
FIXTURES = ROOT / "docs/agents/bounded-subagent-result-v1.fixtures.json"


def integer_count(value: Any) -> int | None:
    """Normalize a decoded JSON integer without rounding during addition."""
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, Decimal) and value == value.to_integral_value():
        return int(value)
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return None


def overflow_errors(packet: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    for index, overflow in enumerate(packet.get("overflow", [])):
        counts = overflow.get("counts")
        if not isinstance(counts, dict):
            continue
        selected = integer_count(counts.get("selected"))
        omitted = integer_count(counts.get("omitted"))
        total = integer_count(counts.get("total"))
        if overflow.get("truncated") and omitted == 0:
            errors.append(f"overflow[{index}].counts.omitted must be greater than zero")
        if (
            selected is not None
            and omitted is not None
            and total is not None
            and selected + omitted != total
        ):
            errors.append(f"overflow[{index}].counts selected + omitted must equal total")
    return errors


def main() -> int:
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    fixtures = json.loads(
        FIXTURES.read_text(encoding="utf-8"),
        parse_float=Decimal,
    )
    invariant = schema["definitions"]["overflow_counts"]["x-bounded-result-invariants"][0]
    if invariant != {"id": "overflow-counts-sum", "expression": "selected + omitted == total"}:
        raise SystemExit("overflow sum invariant declaration drifted")

    cases = {case["name"]: case for case in fixtures["cases"]}
    expected_errors = {
        "invalid-truncated-with-zero-omitted": "must be greater than zero",
        "invalid-truncated-count-sum-mismatch": "must equal total",
    }
    for name, expected in expected_errors.items():
        errors = overflow_errors(cases[name]["packet"])
        if not any(expected in error for error in errors):
            raise SystemExit(f"{name}: expected overflow invariant was not rejected")

    for case in fixtures["cases"]:
        if case["valid"] and (errors := overflow_errors(case["packet"])):
            raise SystemExit(f"{case['name']}: unexpected overflow errors: {errors}")
    print(f"checked {len(fixtures['cases'])} bounded-result fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
