"""Schema and invariants for the May 2026 security reconciliation ledger."""

from __future__ import annotations

import hashlib
import re
from collections import Counter
from datetime import date
from typing import Any

EXPECTED_PROJECT = "perl-lsp"
EXPECTED_REPORT_DATE = "2026-05-13T13:32:50.221Z"
EXPECTED_FINDINGS = 60
EXPECTED_FILES_ANALYZED = 59
EXPECTED_SEVERITIES = {"HIGH": 22, "MEDIUM": 30, "HIGH_BUG": 2, "BUG": 6}
ALLOWED_VERDICTS = {
    "open", "partially_landed", "landed_not_proven", "proven_closed",
    "false_or_stale_premise", "transferred",
}
ALLOWED_PR_RELATIONSHIPS = {
    "none", "candidate", "merged", "superseded", "duplicate", "abandoned",
}
ALLOWED_ANCESTRY_STATES = {
    "not_checked", "observed_ancestor", "not_ancestor", "not_applicable",
}
FINDING_FIELDS = [
    "id", "severity", "title", "slug", "path", "lines", "source_text_digest",
    "threat", "reachability", "current_reachability_correction",
    "canonical_issue", "issue_state", "canonical_pr", "pr_relationship", "pr_state",
    "landed_commit", "ancestry_state", "ancestry_main_sha", "ancestry_observed_at",
    "current_source_seam", "proof_refs", "verdict", "residual_owner", "limitations",
]
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
FINDING_ID_RE = re.compile(r"^MAY2026-(HIGH|MEDIUM|HIGH_BUG|BUG)-\d{3}$")
ISSUE_REF_RE = re.compile(r"^#\d+$")
PROOF_REF_RE = re.compile(r"^(#\d+|[0-9a-f]{40}|https://github\.com/.+)$")


class LedgerError(ValueError):
    """Raised when the ledger violates its audit contract."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise LedgerError(message)


def normalize_ledger(raw: dict[str, Any]) -> dict[str, Any]:
    """Expand compact on-disk row arrays into named audit objects."""
    findings = raw.get("findings")
    if not isinstance(findings, list) or not findings or isinstance(findings[0], dict):
        return raw
    require(raw.get("finding_fields") == FINDING_FIELDS,
            "finding_fields does not match schema version 1")
    normalized = dict(raw)
    normalized_rows: list[dict[str, Any]] = []
    for index, values in enumerate(findings, start=1):
        require(isinstance(values, list), f"compact finding {index} must be an array")
        require(len(values) == len(FINDING_FIELDS),
                f"compact finding {index} has {len(values)} values; expected {len(FINDING_FIELDS)}")
        item = dict(zip(FINDING_FIELDS, values, strict=True))
        normalized_rows.append({
            "id": item["id"],
            "severity": item["severity"],
            "title": item["title"],
            "slug": item["slug"],
            "source": {
                "project": raw.get("report", {}).get("project"),
                "report_date": raw.get("report", {}).get("date"),
                "path": item["path"],
                "lines": item["lines"],
            },
            "source_text_digest": item["source_text_digest"],
            "original": {"threat": item["threat"], "reachability": item["reachability"]},
            "current_reachability_correction": item["current_reachability_correction"],
            "github": {
                "canonical_issue": item["canonical_issue"],
                "issue_state": item["issue_state"],
                "canonical_pr": item["canonical_pr"],
                "pr_relationship": item["pr_relationship"],
                "pr_state": item["pr_state"],
            },
            "landed_commit": item["landed_commit"],
            "current_main_ancestry": {
                "state": item["ancestry_state"],
                "observed_main_sha": item["ancestry_main_sha"],
                "observed_at": item["ancestry_observed_at"],
            },
            "current_source_seam": item["current_source_seam"],
            "proof_refs": item["proof_refs"],
            "verdict": item["verdict"],
            "residual_owner": item["residual_owner"],
            "limitations": item["limitations"],
        })
    normalized["findings"] = normalized_rows
    return normalized


def canonical_source_text(row: dict[str, Any]) -> str:
    source = row["source"]
    return "\n".join([
        source["project"], source["report_date"], row["severity"], row["title"],
        source["path"], ",".join(str(item) for item in source["lines"]), row["slug"],
        row["original"]["threat"], row["original"]["reachability"],
    ])


def source_digest(row: dict[str, Any]) -> str:
    return "sha256:" + hashlib.sha256(canonical_source_text(row).encode("utf-8")).hexdigest()


def _require_ref(value: Any, field: str, finding_id: str) -> None:
    require(isinstance(value, str) and ISSUE_REF_RE.fullmatch(value) is not None,
            f"{finding_id}: {field} must be an issue/PR reference such as #6124")


def _validate_report(data: dict[str, Any]) -> dict[str, Any]:
    require(data.get("schema_version") == 1, "schema_version must be 1")
    report = data.get("report")
    require(isinstance(report, dict), "report must be an object")
    require(report.get("project") == EXPECTED_PROJECT,
            f"report.project must be {EXPECTED_PROJECT}")
    require(report.get("date") == EXPECTED_REPORT_DATE,
            "report.date does not match the source report")
    require(report.get("files_analyzed") == EXPECTED_FILES_ANALYZED,
            f"report.files_analyzed must be {EXPECTED_FILES_ANALYZED}")
    require(report.get("total_findings") == EXPECTED_FINDINGS,
            f"report.total_findings must be {EXPECTED_FINDINGS}")
    require(report.get("severity_counts") == EXPECTED_SEVERITIES,
            "report.severity_counts does not match the source report")
    return report


def _validate_observation(data: dict[str, Any]) -> dict[str, Any]:
    observation = data.get("current_main_observation")
    require(isinstance(observation, dict), "current_main_observation must be an object")
    require(isinstance(observation.get("observed_at"), str),
            "current_main_observation.observed_at must be a date")
    try:
        date.fromisoformat(observation["observed_at"])
    except (TypeError, ValueError) as exc:
        raise LedgerError("current_main_observation.observed_at must be YYYY-MM-DD") from exc
    require(isinstance(observation.get("main_sha"), str)
            and SHA_RE.fullmatch(observation["main_sha"]) is not None,
            "current_main_observation.main_sha must be a full lowercase commit SHA")
    require(observation.get("repository") == "EffortlessMetrics/perl-lsp-swarm",
            "current_main_observation.repository must name this repository")
    return observation


def _validate_source(row: dict[str, Any], finding_id: str,
                     source_keys: set[tuple[Any, ...]]) -> None:
    source = row.get("source")
    require(isinstance(source, dict), f"{finding_id}: source must be an object")
    require(source.get("project") == EXPECTED_PROJECT, f"{finding_id}: source.project mismatch")
    require(source.get("report_date") == EXPECTED_REPORT_DATE,
            f"{finding_id}: source.report_date mismatch")
    require(isinstance(source.get("path"), str) and source["path"].strip(),
            f"{finding_id}: source.path is required")
    lines = source.get("lines")
    require(isinstance(lines, list) and lines
            and all(isinstance(item, int) and item > 0 for item in lines),
            f"{finding_id}: source.lines must be a non-empty positive-integer array")
    require(lines == sorted(set(lines)), f"{finding_id}: source.lines must be sorted and unique")
    source_key = (source["report_date"], source["path"], tuple(lines), row["severity"], row["slug"])
    require(source_key not in source_keys, f"{finding_id}: duplicate source finding identity")
    source_keys.add(source_key)
    require(row.get("source_text_digest") == source_digest(row),
            f"{finding_id}: source_text_digest is stale or incorrect")


def _validate_github(row: dict[str, Any], finding_id: str) -> tuple[str, str | None]:
    github = row.get("github")
    require(isinstance(github, dict), f"{finding_id}: github must be an object")
    issue = github.get("canonical_issue")
    if issue is not None:
        _require_ref(issue, "github.canonical_issue", finding_id)
    require(github.get("issue_state") in {"open", "closed", "unknown", "none"},
            f"{finding_id}: invalid issue_state")
    pr = github.get("canonical_pr")
    if pr is not None:
        _require_ref(pr, "github.canonical_pr", finding_id)
    relationship = github.get("pr_relationship")
    require(relationship in ALLOWED_PR_RELATIONSHIPS,
            f"{finding_id}: invalid pr_relationship {relationship!r}")
    pr_state = github.get("pr_state")
    require(pr_state in {"open", "closed", "merged", "unknown", "none"},
            f"{finding_id}: invalid pr_state")
    if relationship == "merged":
        require(pr is not None, f"{finding_id}: merged relationship requires canonical_pr")
        require(pr_state == "merged",
                f"{finding_id}: merged relationship cannot be inferred from {pr_state!r} PR state")
    if pr is None:
        require(relationship == "none", f"{finding_id}: missing canonical_pr requires relationship=none")
        require(pr_state == "none", f"{finding_id}: missing canonical_pr requires pr_state=none")
    return relationship, pr


def _validate_disposition(row: dict[str, Any], finding_id: str, relationship: str,
                          observation: dict[str, Any]) -> None:
    landed = row.get("landed_commit")
    require(landed is None or (isinstance(landed, str) and SHA_RE.fullmatch(landed) is not None),
            f"{finding_id}: landed_commit must be null or a full lowercase SHA")
    ancestry = row.get("current_main_ancestry")
    require(isinstance(ancestry, dict), f"{finding_id}: current_main_ancestry must be an object")
    require(ancestry.get("state") in ALLOWED_ANCESTRY_STATES,
            f"{finding_id}: invalid ancestry state")
    if ancestry.get("state") == "observed_ancestor":
        require(landed is not None, f"{finding_id}: observed ancestry requires landed_commit")
        require(ancestry.get("observed_main_sha") == observation["main_sha"],
                f"{finding_id}: ancestry observation must bind to current_main_observation.main_sha")
        require(ancestry.get("observed_at") == observation["observed_at"],
                f"{finding_id}: ancestry observation date must bind to current_main_observation.observed_at")
    seam = row.get("current_source_seam")
    require(seam is None or isinstance(seam, str),
            f"{finding_id}: current_source_seam must be null or text")
    proof_refs = row.get("proof_refs")
    require(isinstance(proof_refs, list) and all(isinstance(item, str) for item in proof_refs),
            f"{finding_id}: proof_refs must be an array of strings")
    for proof_ref in proof_refs:
        require(PROOF_REF_RE.fullmatch(proof_ref) is not None,
                f"{finding_id}: unsupported proof reference {proof_ref!r}")
    verdict = row.get("verdict")
    require(verdict in ALLOWED_VERDICTS, f"{finding_id}: invalid verdict {verdict!r}")
    residual = row.get("residual_owner")
    require(residual is None or (isinstance(residual, str) and ISSUE_REF_RE.fullmatch(residual) is not None),
            f"{finding_id}: residual_owner must be null or an issue reference")
    limitations = row.get("limitations")
    require(isinstance(limitations, list)
            and all(isinstance(item, str) and item.strip() for item in limitations),
            f"{finding_id}: limitations must be an array of non-empty strings")
    correction = row.get("current_reachability_correction")
    require(correction is None or isinstance(correction, str),
            f"{finding_id}: current_reachability_correction must be null or text")
    if verdict == "proven_closed":
        require(relationship == "merged", f"{finding_id}: proven_closed requires merged canonical PR")
        require(landed is not None, f"{finding_id}: proven_closed requires landed_commit")
        require(ancestry.get("state") == "observed_ancestor",
                f"{finding_id}: proven_closed requires current-main ancestry proof")
        require(bool(proof_refs), f"{finding_id}: proven_closed requires discriminating proof")
        require(residual is None, f"{finding_id}: proven_closed cannot retain a residual owner")
        require(isinstance(seam, str) and bool(seam.strip()),
                f"{finding_id}: proven_closed requires a current source seam")
    else:
        require(residual is not None, f"{finding_id}: non-closed verdict requires one residual owner")
    if verdict == "false_or_stale_premise":
        require(bool(correction and correction.strip()),
                f"{finding_id}: false_or_stale_premise requires an explicit current correction")


def validate_ledger(data: dict[str, Any]) -> None:
    _validate_report(data)
    observation = _validate_observation(data)
    findings = data.get("findings")
    require(isinstance(findings, list), "findings must be an array")
    require(len(findings) == EXPECTED_FINDINGS,
            f"expected exactly {EXPECTED_FINDINGS} findings, found {len(findings)}")
    ids: set[str] = set()
    source_keys: set[tuple[Any, ...]] = set()
    severities: Counter[str] = Counter()
    for index, row in enumerate(findings, start=1):
        require(isinstance(row, dict), f"finding {index} must be an object")
        finding_id = row.get("id")
        require(isinstance(finding_id, str) and FINDING_ID_RE.fullmatch(finding_id) is not None,
                f"finding {index}: malformed id")
        require(finding_id not in ids, f"duplicate finding id {finding_id}")
        ids.add(finding_id)
        severity = row.get("severity")
        require(severity in EXPECTED_SEVERITIES, f"{finding_id}: unsupported severity {severity!r}")
        require(finding_id.startswith(f"MAY2026-{severity}-"),
                f"{finding_id}: id severity disagrees with severity field")
        severities[severity] += 1
        require(isinstance(row.get("title"), str) and row["title"].strip(),
                f"{finding_id}: title is required")
        require(isinstance(row.get("slug"), str) and row["slug"].strip(),
                f"{finding_id}: slug is required")
        original = row.get("original")
        require(isinstance(original, dict), f"{finding_id}: original must be an object")
        require(isinstance(original.get("threat"), str) and original["threat"].strip(),
                f"{finding_id}: original.threat is required")
        require(isinstance(original.get("reachability"), str) and original["reachability"].strip(),
                f"{finding_id}: original.reachability is required")
        _validate_source(row, finding_id, source_keys)
        relationship, _ = _validate_github(row, finding_id)
        _validate_disposition(row, finding_id, relationship, observation)
    require(dict(severities) == EXPECTED_SEVERITIES,
            f"finding severity counts mismatch: expected {EXPECTED_SEVERITIES}, got {dict(severities)}")
    expected = sorted(findings,
                      key=lambda row: (list(EXPECTED_SEVERITIES).index(row["severity"]), row["id"]))
    require(findings == expected,
            "findings must be deterministically ordered by report severity and id")
