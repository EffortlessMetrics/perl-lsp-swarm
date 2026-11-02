#!/usr/bin/env python3
"""Fail-closed SweepReport reader for CPAN corpus workflow summaries (#15315).

Mirrors the Rust producer envelope in `xtask/src/tasks/parser_corpus_sweep.rs`:
integer `schema_version`, object envelope, integer counters (booleans excluded),
nonzero `total_files`, and `0 <= clean_files <= total_files`.

Usage:
    read-cpan-corpus-sweep-summary.py REPORT.json --heading '### Title'
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

# Keep in sync with parser_corpus_sweep::SCHEMA_VERSION.
ACCEPTED_SCHEMA_VERSION = 1


def _require_int_counter(report: dict, field: str) -> int:
    if field not in report:
        raise ValueError(f"missing {field}")
    value = report[field]
    if type(value) is not int:
        raise ValueError(f"{field} must be an integer counter, found {value!r}")
    return value


def parse_sweep_report(raw: object) -> tuple[int, int]:
    """Validate envelope and return (total_files, clean_files)."""
    if not isinstance(raw, dict):
        raise ValueError("report must be a JSON object")

    if "schema_version" not in raw:
        raise ValueError("missing schema_version")

    schema_version = raw["schema_version"]
    if type(schema_version) is not int or schema_version != ACCEPTED_SCHEMA_VERSION:
        raise ValueError(
            "unsupported SweepReport schema_version: "
            f"expected {ACCEPTED_SCHEMA_VERSION}, found {schema_version!r}"
        )

    total = _require_int_counter(raw, "total_files")
    clean = _require_int_counter(raw, "clean_files")

    if total <= 0:
        raise ValueError(f"total_files must be positive, found {total}")
    if clean < 0 or clean > total:
        raise ValueError(f"clean_files must satisfy 0 <= clean <= total_files; got clean={clean}, total={total}")

    return total, clean


def render_step_summary(heading: str, total: int, clean: int) -> str:
    pct = clean / total * 100
    lines = [
        heading,
        "| Metric | Value |",
        "| --- | ---: |",
        f"| Total .pm files | {total} |",
        f"| Parse-clean | {clean} ({pct:.1f}%) |",
        f"| Parse failures | {total - clean} |",
        "",
    ]
    return "\n".join(lines)


def load_report(path: Path) -> tuple[int, int]:
    try:
        raw_text = path.read_text(encoding="utf-8")
    except OSError as error:
        raise ValueError(f"cannot read report: {error}") from error
    try:
        raw = json.loads(raw_text)
    except json.JSONDecodeError as error:
        raise ValueError(f"report is not valid JSON: {error}") from error
    return parse_sweep_report(raw)


def append_step_summary(markdown: str) -> None:
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not summary_path:
        raise ValueError("GITHUB_STEP_SUMMARY is not set")
    with Path(summary_path).open("a", encoding="utf-8") as handle:
        handle.write(markdown)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="Path to SweepReport JSON")
    parser.add_argument(
        "--heading",
        required=True,
        help="Markdown heading written above the summary table",
    )
    args = parser.parse_args(argv)

    try:
        total, clean = load_report(args.report)
        append_step_summary(render_step_summary(args.heading, total, clean))
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
