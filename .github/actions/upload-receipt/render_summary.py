#!/usr/bin/env python3
"""Render a gate receipt as a GitHub step summary, honestly.

The receipt contract is `.ci/receipt.schema.json`, produced by
`xtask/src/tasks/gates.rs`. That contract is the authority here; this renderer
only reports it. Two consequences shape this module:

* **Gate status vocabulary is `pass` / `fail` / `skip` / `timeout` / `error`.**
  A status this renderer does not recognise is not a pass — it means the
  renderer and the receipt disagree about the contract, which is exactly the
  condition under which a summary must not claim success.
* **`fail`, `timeout` and `error` all count as failing**, matching
  `is_blocking_gate_status` in `gates.rs`. A gate that timed out is not a gate
  that passed. Whether a run is *merge*-blocking further depends on each gate's
  `required` flag, and that verdict stays with the receipt's own
  `summary.overall_status` — this renderer reports statuses, not merge
  decisions.

The counts must reconcile: every gate carries one recognised status, so
``passed + failed + skipped + timeout + error == len(gates)``. When they do
not, or when the gate set is absent, malformed or empty, or when a gate is
missing the fields the contract requires of one, the summary reports
``NOT_PROVEN`` rather than a status it cannot support. The headline also
accounts for every gate: skips are reported alongside failures rather than
dropped.

Every rendering carries exactly one ``**Status**`` line, including the ones
where no receipt could be read at all — a missing or unparseable file is the
most unusable input there is, and reporting no verdict for it would leave a
consumer scanning for a status with nothing to find.

**How deep the checking goes**, deliberately: presence for structure (the
`required` lists in the schema), the contract enum for `status` — the one field
the verdict rests on — and visible degradation for everything else, so an
unusable `duration_ms` renders as a missing cell rather than a wrong number.
Type-level validation would reimplement `jsonschema` (unavailable on the
runner) by hand, with its own risk of drifting from the schema — the very
failure this module exists to prevent. Enforcing full receipt shape belongs to
a validator that consumes `.ci/receipt.schema.json` directly.

Caller-supplied receipt content reaches a Markdown table that GitHub renders,
so every interpolated value is escaped (`summary_text` / `summary_code`).
"""

from __future__ import annotations

import html
import json
import math
import os
from pathlib import Path
from typing import Sequence

# Gate statuses, from `.ci/receipt.schema.json` `$defs.gate_result.status`.
PASS_STATUS = "pass"
SKIP_STATUS = "skip"
# Statuses `is_blocking_gate_status` in gates.rs treats as blocking. Whether a
# run is *merge*-blocking additionally depends on each gate's `required` flag,
# and that verdict belongs to the receipt's own `summary.overall_status` /
# `summary.blocking_failures`. This renderer reports gate statuses; it does not
# restate the merge decision.
BLOCKING_STATUSES: tuple[str, ...] = ("fail", "timeout", "error")
RECOGNIZED_STATUSES: tuple[str, ...] = (PASS_STATUS, *BLOCKING_STATUSES, SKIP_STATUS)

# Contract-required fields, from `.ci/receipt.schema.json`: `required` on the
# root object and on `$defs.gate_result`. Presence is checked, not full schema
# validation — a status word alone is not a gate, and a summary must not report
# a pass for a receipt it cannot actually read.
RECEIPT_REQUIRED_FIELDS: tuple[str, ...] = ("schema_version", "metadata", "gates", "summary")
GATE_REQUIRED_FIELDS: tuple[str, ...] = ("gate_name", "tier", "status", "duration_ms", "command")

# How a non-passing status reads in the status line.
BLOCKING_LABELS: dict[str, str] = {
    "fail": "failed",
    "timeout": "timed out",
    "error": "errored",
}

MISSING = "—"


def summary_text(value: object) -> str:
    """Escape `value` for interpolation into a Markdown table cell."""
    sanitized = str(value).replace("\r", " ").replace("\n", " ")
    return html.escape(sanitized, quote=True).replace("|", "&#124;")


def summary_code(value: object) -> str:
    """Escape `value` and wrap it as inline code."""
    return f"<code>{summary_text(value)}</code>"


def gate_name(gate: dict) -> str:
    """The gate's identity.

    `gate_name` is the contract field; `name` is accepted as a fallback so a
    hand-written or legacy receipt still renders an identity rather than
    `unknown`.
    """
    for key in ("gate_name", "name"):
        value = gate.get(key)
        if isinstance(value, str) and value:
            return value
    return "unknown"


def gate_status(gate: dict) -> str:
    """The gate's raw status string, or the empty string when absent."""
    status = gate.get("status")
    return status if isinstance(status, str) else ""


def milliseconds_as_seconds(value: object) -> str | None:
    """`value` milliseconds as a seconds string, or `None` when unusable.

    The schema bounds `duration_ms` below but not above, and JSON integers are
    unbounded, so `value / 1000` can raise `OverflowError` on a perfectly valid
    receipt. A duration the renderer cannot represent degrades to a missing
    cell like any other unusable value — it must never abort the summary, since
    a crashed step publishes no verdict at all.
    """
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    try:
        seconds = value / 1000
    except (OverflowError, ValueError):
        return None
    if not math.isfinite(seconds):
        return None
    return f"{seconds:.1f}"


def format_duration(gate: dict) -> str:
    """Render `duration_ms` (the contract field) as seconds."""
    seconds = milliseconds_as_seconds(gate.get("duration_ms"))
    return MISSING if seconds is None else f"{seconds}s"


def format_exit_code(gate: dict) -> str:
    """Render `exit_code`, which the contract allows to be null."""
    exit_code = gate.get("exit_code")
    if exit_code is None:
        return MISSING
    return summary_text(exit_code)


def missing_receipt_fields(data: dict) -> list[str]:
    """Contract-required top-level fields absent from `data`."""
    return [field for field in RECEIPT_REQUIRED_FIELDS if field not in data]


def gate_is_complete(gate: dict) -> bool:
    """Whether `gate` carries every contract-required field.

    Presence only — this is not schema validation. The identity requirement
    accepts the legacy `name` spelling that `gate_name` also honours; every
    other required field must appear under its contract name.
    """
    for field in GATE_REQUIRED_FIELDS:
        if field == "gate_name":
            # Presence of an identity, not the sentinel `gate_name` falls back
            # to: `unknown` is itself a legal gate name under the contract.
            if not any(
                isinstance(gate.get(key), str) and gate.get(key)
                for key in ("gate_name", "name")
            ):
                return False
        elif field not in gate:
            return False
    return True


def count_statuses(gates: Sequence[dict]) -> dict[str, int]:
    """Count each recognised status, plus `unrecognized`, across `gates`."""
    counts = {status: 0 for status in RECOGNIZED_STATUSES}
    counts["unrecognized"] = 0
    for gate in gates:
        status = gate_status(gate)
        if status in RECOGNIZED_STATUSES:
            counts[status] += 1
        else:
            counts["unrecognized"] += 1
    return counts


def status_line(gates: Sequence[dict]) -> str:
    """The `**Status**` line for a well-formed, non-empty gate list."""
    counts = count_statuses(gates)
    total = len(gates)

    if counts["unrecognized"]:
        return (
            f"**Status**: NOT_PROVEN — {counts['unrecognized']}/{total} gates carry a "
            "status outside the receipt contract "
            f"({', '.join(summary_code(s) for s in RECOGNIZED_STATUSES)})"
        )

    failing = sum(counts[status] for status in BLOCKING_STATUSES)
    if failing:
        detail = ", ".join(
            f"{counts[status]} {BLOCKING_LABELS[status]}"
            for status in BLOCKING_STATUSES
            if counts[status]
        )
        # Lead with the failures, then account for every remaining gate: a
        # skipped gate did not pass either, and dropping it here would let the
        # headline describe fewer gates than the receipt contains.
        parts = [f"**Status**: {failing}/{total} gates failing ({detail})"]
        if counts[SKIP_STATUS]:
            parts.append(f"{counts[SKIP_STATUS]} skipped")
        if counts[PASS_STATUS]:
            parts.append(f"{counts[PASS_STATUS]} passed")
        return ", ".join(parts)

    if counts[SKIP_STATUS]:
        return (
            f"**Status**: {counts[PASS_STATUS]}/{total} gates passed, "
            f"{counts[SKIP_STATUS]} skipped"
        )

    return f"**Status**: All {counts[PASS_STATUS]}/{total} gates passed"


def status_cell(status: str) -> str:
    """The status column for one gate row."""
    if status == PASS_STATUS:
        return PASS_STATUS
    if status in BLOCKING_STATUSES:
        return f"**{status.upper()}**"
    if status == SKIP_STATUS:
        return SKIP_STATUS
    return f"**UNKNOWN** ({summary_text(status or MISSING)})"


def header_lines(data: dict) -> list[str]:
    """Receipt provenance, read from the contract's `metadata` object."""
    metadata = data.get("metadata")
    if not isinstance(metadata, dict):
        metadata = {}
    timestamp = metadata.get("timestamp")
    commit = metadata.get("git_sha")
    return [
        f"**Generated**: {summary_text(timestamp if timestamp else 'unknown')}",
        f"**Commit**: {summary_code(str(commit)[:12] if commit else 'unknown')}",
    ]


def total_duration_lines(data: dict) -> list[str]:
    """The receipt-wide duration, read from `summary.total_duration_ms`."""
    summary = data.get("summary")
    if not isinstance(summary, dict):
        return []
    seconds = milliseconds_as_seconds(summary.get("total_duration_ms"))
    if seconds is None:
        return []
    return ["", f"**Total duration**: {seconds}s"]


def gate_table(gates: Sequence[dict]) -> list[str]:
    """The per-gate table."""
    lines = ["| Gate | Status | Exit | Duration |", "|------|--------|------|----------|"]
    for gate in gates:
        lines.append(
            f"| {summary_text(gate_name(gate))} | {status_cell(gate_status(gate))} "
            f"| {format_exit_code(gate)} | {format_duration(gate)} |"
        )
    return lines


def render(data: object) -> str:
    """Render a parsed receipt as step-summary Markdown."""
    lines = ["### Gate Receipt", ""]

    if not isinstance(data, dict):
        lines.append("**Status**: NOT_PROVEN — receipt is not a JSON object")
        return "\n".join(lines) + "\n"

    lines.extend(header_lines(data))

    raw_gates = data.get("gates")
    if raw_gates is None:
        lines.append("**Status**: NOT_PROVEN — receipt carries no `gates` array")
        return "\n".join(lines) + "\n"
    if not isinstance(raw_gates, list) or not all(
        isinstance(gate, dict) for gate in raw_gates
    ):
        lines.append("**Status**: NOT_PROVEN — `gates` must be an array of objects")
        return "\n".join(lines) + "\n"
    if not raw_gates:
        lines.append("**Status**: NOT_PROVEN — receipt reports no gates")
        return "\n".join(lines) + "\n"

    missing = missing_receipt_fields(data)
    if missing:
        lines.append(
            "**Status**: NOT_PROVEN — receipt is missing contract-required fields "
            f"({', '.join(summary_code(field) for field in missing)})"
        )
        return "\n".join(lines) + "\n"

    incomplete = sum(1 for gate in raw_gates if not gate_is_complete(gate))
    if incomplete:
        lines.append(
            f"**Status**: NOT_PROVEN — {incomplete}/{len(raw_gates)} gates are missing "
            "contract-required fields "
            f"({', '.join(summary_code(field) for field in GATE_REQUIRED_FIELDS)})"
        )
        lines.append("")
        lines.extend(gate_table(raw_gates))
        return "\n".join(lines) + "\n"

    lines.append(status_line(raw_gates))
    lines.append("")
    lines.extend(gate_table(raw_gates))
    lines.extend(total_duration_lines(data))
    return "\n".join(lines) + "\n"


def render_receipt_file(receipt_path_raw: str) -> str:
    """Read the receipt at `receipt_path_raw` and render it."""
    receipt_path = Path(receipt_path_raw)
    if not receipt_path.is_file():
        return (
            "### Gate Receipt\n\n"
            "**Status**: NOT_PROVEN — receipt file is missing\n\n"
            f"> Receipt not found at {summary_code(receipt_path_raw)}\n"
        )
    try:
        data = json.loads(receipt_path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError, UnicodeDecodeError) as error:
        return (
            "### Gate Receipt\n\n"
            "**Status**: NOT_PROVEN — receipt could not be read\n\n"
            f"> Failed to read receipt: {summary_code(error)}\n"
        )
    return render(data)


def main() -> int:
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not summary_path:
        return 0
    rendered = render_receipt_file(os.environ["RECEIPT_PATH"])
    Path(summary_path).write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
