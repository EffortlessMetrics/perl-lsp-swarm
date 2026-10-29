#!/usr/bin/env python3
"""Publish the PR-fast failing-gate list where the check-runs API can read it.

Issue #15492. The PR Smoke lane already rendered a gate table into
``GITHUB_STEP_SUMMARY``, but that file is not served by the REST API: anyone
diagnosing the red through the API saw a failed check run naming no gate, and
no job-log tail could surface the table either, because nothing in the step
reached stdout. This module renders the same table and then publishes it three
ways, in rising order of reachability:

1. ``GITHUB_STEP_SUMMARY`` — unchanged, for the web UI;
2. stdout — so the table lands in the job-log tail, the remedy #16026 applied
   to the UX regression receipt in the same workflow;
3. one ``::error`` annotation per failing gate — reachable through
   ``GET /repos/{owner}/{repo}/check-runs/{id}/annotations`` even when the
   job-log archive download is not.

Three properties of the annotation path are not obvious and are each covered by
``test_summarize_pr_fast_gates.py``:

``skip`` is not a failure
    A short-circuiting required gate marks every remaining planned gate
    ``skip`` (``xtask/src/tasks/gates.rs``). Those never ran, so they belong in
    the table but must not be annotated — otherwise one real failure publishes
    a dozen annotations naming gates that were never executed.

Workflow-command escaping
    ``rendered`` carries gate names straight out of the receipt and nothing
    validates that file before this module reads it. Message data escapes
    ``%``/CR/LF; a property value additionally treats ``:`` and ``,`` as
    delimiters, so an unescaped gate name containing either would truncate
    ``title`` and inject a further property. ``.ci/receipt.schema.json``
    constrains ``gate_name`` to ``^[a-z][a-z0-9_-]*$``, so a conformant
    producer cannot reach these branches; the escaping is defence in depth
    because this module does not schema-validate the receipt it reads.

The ten-annotation cap
    GitHub publishes at most ten annotations of a level per check run: measured
    on this repository, ``Workflow Trigger Lint`` records one
    ``RECORDED_LEGACY_PROVENANCE_DEBT`` per pinned action and the tree carries
    23, yet its check run exposes exactly 10. One annotation per failing gate
    would therefore drop the tail on a bad enough receipt, reintroducing for
    the worst runs the unreadability this module exists to remove. The first
    nine stay one-per-gate and the remainder folds into the tenth, so every
    failing gate name still reaches the annotations API.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import uuid
from pathlib import Path

# Statuses meaning a gate ran and did not pass.
BLOCKING_STATUSES = ("fail", "timeout", "error")
# Statuses meaning a gate ran and passed; anything else is listed in the table.
PASSING_STATUSES = ("pass", "passed", "success")
MAX_ANNOTATIONS = 10

DEFAULT_RECEIPT = Path("target/receipts/receipt.json")


def summarize(receipt: Path) -> tuple[str, list[tuple[str, str]], str | None]:
    """Render the gate table and select the gates worth annotating.

    Returns the rendered markdown, the ``(name, detail)`` pairs for gates that
    ran and did not pass, and a receipt-level problem string when the receipt
    itself was missing or unreadable.
    """
    lines = ["### PR Smoke gate summary", ""]
    failing: list[tuple[str, str]] = []
    receipt_problem: str | None = None

    if not receipt.is_file():
        lines.append(
            "- Receipt: unavailable (the PR-fast runner did not produce a final receipt.)"
        )
        receipt_problem = "the PR-fast runner did not produce a final receipt"
    else:
        try:
            data = json.loads(receipt.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            lines.append(f"- Receipt: unreadable ({error})")
            receipt_problem = f"receipt unreadable: {error}"
        else:
            gates = data.get("gates", [])
            failures = [
                gate
                for gate in gates
                if gate.get("status") not in PASSING_STATUSES
            ]
            lines.append(f"- Receipt: `{receipt}`")
            if failures:
                lines.append("")
                lines.append("| Gate | Status | Exit |")
                lines.append("| --- | --- | ---: |")
                for gate in failures:
                    name = gate.get("gate_name", "unknown")
                    status = gate.get("status", "unknown")
                    exit_code = gate.get("exit_code", "unknown")
                    lines.append(f"| `{name}` | `{status}` | `{exit_code}` |")
                    if status in BLOCKING_STATUSES:
                        failing.append((name, f"status {status}, exit {exit_code}"))
            else:
                lines.append("- Non-success gates: none recorded.")

    return "\n".join(lines) + "\n", failing, receipt_problem


def escape_data(text: object) -> str:
    """Escape workflow-command message data."""
    return (
        str(text)
        .replace("%", "%25")
        .replace("\r", "%0D")
        .replace("\n", "%0A")
    )


def escape_property(text: object) -> str:
    """Escape a workflow-command property value.

    A property value additionally treats ``:`` and ``,`` as delimiters.
    """
    return escape_data(text).replace(":", "%3A").replace(",", "%2C")


def fold_annotations(
    failing: list[tuple[str, str]], limit: int = MAX_ANNOTATIONS
) -> list[tuple[str, str]]:
    """Bound the annotation list to ``limit`` without losing a gate name."""
    if len(failing) <= limit:
        return list(failing)
    annotations = list(failing[: limit - 1])
    rest = failing[limit - 1 :]
    annotations.append(
        (
            f"{len(rest)} further failing gates",
            "; ".join(f"{name} ({detail})" for name, detail in rest),
        )
    )
    return annotations


def emit(
    rendered: str,
    failing: list[tuple[str, str]],
    receipt_problem: str | None,
    stop_token: str,
    stream=None,
) -> None:
    """Write the stdout mirror and the annotations.

    The mirror is fenced by ``::stop-commands::``: a crafted gate name
    containing a newline and a ``::`` directive is printed rather than
    executed. The token must be random per run, because a fixed one could be
    reproduced by the very content being fenced.
    """
    out = sys.stdout if stream is None else stream
    print(f"::stop-commands::{stop_token}", file=out)
    print(rendered, file=out)
    print(f"::{stop_token}::", file=out)

    for name, detail in fold_annotations(failing):
        print(
            f"::error title=pr-fast gate {escape_property(name)}::{escape_data(detail)}",
            file=out,
        )
    if receipt_problem is not None:
        print(
            f"::warning title=pr-fast receipt::{escape_data(receipt_problem)}",
            file=out,
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--receipt",
        type=Path,
        default=DEFAULT_RECEIPT,
        help="path to the PR-fast gate receipt (default: %(default)s)",
    )
    parser.add_argument(
        "--summary",
        type=Path,
        default=None,
        help="path to write the markdown summary (default: $GITHUB_STEP_SUMMARY)",
    )
    args = parser.parse_args(argv)

    summary = args.summary
    if summary is None:
        location = os.environ.get("GITHUB_STEP_SUMMARY")
        if not location:
            parser.error("--summary is required when GITHUB_STEP_SUMMARY is unset")
        summary = Path(location)

    rendered, failing, receipt_problem = summarize(args.receipt)
    summary.write_text(rendered, encoding="utf-8")
    emit(rendered, failing, receipt_problem, f"pr-fast-summary-{uuid.uuid4().hex}")
    # Advisory: the gate verdict belongs to the runner, never to its reporter.
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
