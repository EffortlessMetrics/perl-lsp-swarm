"""I/O and deterministic Markdown projection for the security ledger."""

from __future__ import annotations

import json
from collections import Counter
from pathlib import Path
from typing import Any

from security_reconciliation_model import (
    ALLOWED_VERDICTS,
    LedgerError,
    normalize_ledger,
    require,
    validate_ledger,
)


def load_ledger(path: Path) -> dict[str, Any]:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise LedgerError(f"ledger not found: {path}") from exc
    except json.JSONDecodeError as exc:
        raise LedgerError(f"invalid JSON in {path}: {exc}") from exc
    require(isinstance(raw, dict), "ledger root must be an object")
    return normalize_ledger(raw)


def render_markdown(data: dict[str, Any]) -> str:
    validate_ledger(data)
    findings = data["findings"]
    counts = Counter(row["verdict"] for row in findings)
    observation = data["current_main_observation"]
    lines = [
        "# May 13, 2026 security scan reconciliation", "",
        "> This is an audit ledger for the original 60 report rows. It is not a security score,",
        "> a claim that the scan covered every repository surface, or an automatic issue-closure mechanism.",
        "", "## Observation boundary", "",
        f"- Repository: `{observation['repository']}`",
        f"- Current-main observation: `{observation['main_sha']}` on `{observation['observed_at']}`",
        f"- Source report: `{data['report']['date']}`; {data['report']['files_analyzed']} files analyzed; {data['report']['total_findings']} findings",
        "", "## Verdict counts", "", "| Verdict | Count |", "| --- | ---: |",
    ]
    for verdict in sorted(ALLOWED_VERDICTS):
        lines.append(f"| `{verdict}` | {counts.get(verdict, 0)} |")
    lines.extend([
        "", "Aggregate counts describe ledger state only. They do not establish repository security.",
        "", "## Findings", "",
        "| ID | Severity | Source | Finding | Canonical owner | Candidate / landed state | Verdict | Residual owner |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ])
    for row in findings:
        source = row["source"]
        line_text = ",".join(str(item) for item in source["lines"])
        issue = row["github"]["canonical_issue"] or "—"
        pr = row["github"]["canonical_pr"] or "—"
        relation = row["github"]["pr_relationship"]
        landed = row["landed_commit"][:12] if row["landed_commit"] else "—"
        residual = row["residual_owner"] or "—"
        title = row["title"].replace("|", "\\|")
        lines.append(
            f"| `{row['id']}` | `{row['severity']}` | `{source['path']}:{line_text}` | "
            f"{title} | {issue} | {pr} / `{relation}` / `{landed}` | "
            f"`{row['verdict']}` | {residual} |"
        )
    lines.extend([
        "", "## Update contract", "",
        "A row moves to `proven_closed` only when the accepted PR is merged, the landed commit is",
        "observed on the recorded current-main SHA, the current source seam is inspected, and",
        "discriminating proof is cited. A closed issue or an existing PR is not closure evidence.",
        "", "Regenerate with:", "", "```bash",
        "python3 scripts/ci/check_security_reconciliation.py --write", "```", "",
    ])
    return "\n".join(lines)


def check_or_write(ledger_path: Path, markdown_path: Path, write: bool) -> None:
    rendered = render_markdown(load_ledger(ledger_path))
    if write:
        markdown_path.parent.mkdir(parents=True, exist_ok=True)
        markdown_path.write_text(rendered, encoding="utf-8")
        return
    try:
        actual = markdown_path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise LedgerError(f"generated Markdown not found: {markdown_path}") from exc
    require(actual == rendered,
            "generated Markdown is stale: run scripts/ci/check_security_reconciliation.py --write")
