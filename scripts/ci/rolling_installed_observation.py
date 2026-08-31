#!/usr/bin/env python3
"""Build and fan in rolling installed-public-beta observation receipts.

This tool composes existing packaged VSIX/current-source smoke evidence. It does
not reinterpret source tests as installed behavior. Missing Critic, text-sync,
DAP, exactness, or cleanup evidence remains explicitly not proven.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import sys
import zipfile
from collections.abc import Iterable, Mapping, Sequence
from typing import Any

ROW_SCHEMA = "rolling_installed_public_beta_row.v1"
FAN_IN_SCHEMA = "pre_freeze_public_beta_acceptance.v1"
VERDICTS = {
    "pass",
    "product_defect",
    "instrument_defect",
    "unsupported_or_withdrawn",
    "not_proven",
}
REQUIRED_ROWS = ("linux-minimum", "linux-current", "windows-current")
ZERO_BUDGET_KEYS = (
    "wrong_binary_or_artifact",
    "partial_or_checksum_invalid_install",
    "false_exact",
    "stale_exact",
    "unsafe_edit",
    "unexplained_successful_empty",
    "mixed_generation_result",
    "cross_root_leakage",
    "false_repair_diagnosis",
    "optional_tool_false_requirement",
    "orphaned_candidate_process",
    "silent_product_failure",
)
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


class ObservationError(RuntimeError):
    """The observation packet or its exact subject is malformed."""


def read_json(path: pathlib.Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ObservationError(f"cannot read JSON {path}: {error}") from error


def write_json(path: pathlib.Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f"{path.name}.{os.getpid()}.tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as error:
        raise ObservationError(f"cannot hash {path}: {error}") from error
    return digest.hexdigest()


def exact_regular_file(path: pathlib.Path, role: str) -> None:
    if not path.exists():
        raise ObservationError(f"{role} does not exist: {path}")
    if path.is_symlink() or not path.is_file():
        raise ObservationError(f"{role} must be a regular non-symlink file: {path}")
    if path.stat().st_size <= 0:
        raise ObservationError(f"{role} is empty: {path}")


def package_artifacts(args: argparse.Namespace) -> int:
    source_sha = require_sha(args.source_sha)
    server = pathlib.Path(args.server).resolve()
    dap = pathlib.Path(args.dap).resolve()
    output = pathlib.Path(args.output)
    exact_regular_file(server, "perllsp")
    exact_regular_file(dap, "perl-dap")

    server_hash = sha256(server)
    dap_hash = sha256(dap)
    manifest = {
        "schema": "rolling_release_artifact_unit.v1",
        "source_sha": source_sha,
        "source_version": args.source_version,
        "platform": args.platform,
        "architecture": args.architecture,
        "members": [
            {"role": "perllsp", "name": server.name, "size": server.stat().st_size, "sha256": server_hash},
            {"role": "perl-dap", "name": dap.name, "size": dap.stat().st_size, "sha256": dap_hash},
        ],
    }
    manifest_bytes = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode("utf-8")

    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for role, path in (("perllsp", server), ("perl-dap", dap)):
            info = zipfile.ZipInfo(path.name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100755 << 16
            archive.writestr(info, path.read_bytes())
        manifest_info = zipfile.ZipInfo(
            "artifact-manifest.json", date_time=(1980, 1, 1, 0, 0, 0)
        )
        manifest_info.compress_type = zipfile.ZIP_DEFLATED
        manifest_info.external_attr = 0o100644 << 16
        archive.writestr(manifest_info, manifest_bytes)

    exact_regular_file(output, "release-shaped product-unit archive")
    receipt = {
        **manifest,
        "archive": {
            "name": output.name,
            "size": output.stat().st_size,
            "sha256": sha256(output),
        },
    }
    manifest_output = pathlib.Path(args.manifest_output) if args.manifest_output else output.with_suffix(".manifest.json")
    write_json(manifest_output, receipt)
    return 0


def require_sha(value: str) -> str:
    normalized = value.strip().lower()
    if not HEX40.fullmatch(normalized):
        raise ObservationError("source SHA must be exactly 40 lowercase hexadecimal characters")
    return normalized


def find_smoke_receipt(
    root: pathlib.Path, source_sha: str, expected_platform: str
) -> tuple[pathlib.Path | None, Mapping[str, Any] | None, list[str]]:
    findings: list[str] = []
    matches: list[tuple[pathlib.Path, Mapping[str, Any]]] = []
    if not root.exists():
        return None, None, [f"receipt root does not exist: {root}"]
    for path in sorted(root.rglob("*.json")):
        try:
            value = read_json(path)
        except ObservationError as error:
            findings.append(str(error))
            continue
        if not isinstance(value, dict):
            continue
        if value.get("receipt_kind") != "vscode_current_source_smoke":
            continue
        if value.get("schema_version") != "vscode_current_source_smoke.v1":
            findings.append(f"unsupported current-source smoke schema at {path.name}")
            continue
        if value.get("repository_sha") != source_sha:
            continue
        if value.get("platform") != expected_platform:
            continue
        matches.append((path, value))
    if len(matches) > 1:
        return None, None, [
            "multiple exact current-source orchestration receipts matched: "
            + ", ".join(path.name for path, _ in matches)
        ]
    if not matches:
        findings.append("no exact current-source orchestration receipt matched the row subject")
        return None, None, findings
    path, value = matches[0]
    return path, value, findings


def stage_verdict(stage: Any) -> str:
    if not isinstance(stage, dict):
        return "not_proven"
    status = stage.get("status")
    if status == "pass":
        return "pass"
    if status == "failed":
        return "product_defect"
    if status in {"not_proven", "not_run", None}:
        return "not_proven"
    return "instrument_defect"


def summarize_row(cells: Mapping[str, str]) -> str:
    values = set(cells.values())
    if "product_defect" in values:
        return "blocked"
    if "instrument_defect" in values or "not_proven" in values:
        return "not_proven"
    if values <= {"pass", "unsupported_or_withdrawn"}:
        return "pass"
    raise ObservationError(f"unknown row verdict set: {sorted(values)}")


def safe_hash(path: pathlib.Path, findings: list[str], role: str) -> str | None:
    try:
        exact_regular_file(path, role)
        return sha256(path)
    except ObservationError as error:
        findings.append(str(error))
        return None


def build_row(args: argparse.Namespace) -> int:
    source_sha = require_sha(args.source_sha)
    expected_receipt_platform = {"linux": "linux", "windows": "win32"}.get(args.platform)
    if expected_receipt_platform is None:
        raise ObservationError(f"unsupported full-row platform: {args.platform}")

    findings: list[str] = []
    server = pathlib.Path(args.server).resolve()
    dap = pathlib.Path(args.dap).resolve()
    archive = pathlib.Path(args.archive).resolve()
    server_hash = safe_hash(server, findings, "perllsp")
    dap_hash = safe_hash(dap, findings, "perl-dap")
    archive_hash = safe_hash(archive, findings, "release-shaped product-unit archive")

    receipt_path, receipt, receipt_findings = find_smoke_receipt(
        pathlib.Path(args.receipts_root), source_sha, expected_receipt_platform
    )
    findings.extend(receipt_findings)

    identity_ok = receipt is not None and server_hash is not None and dap_hash is not None
    vsix_hash: str | None = None
    receipt_digest: str | None = None
    stages: Mapping[str, Any] = {}
    if receipt is not None and receipt_path is not None:
        receipt_digest = sha256(receipt_path)
        stages_value = receipt.get("stages")
        if isinstance(stages_value, dict):
            stages = stages_value
        else:
            findings.append("current-source smoke receipt has no stages object")
            identity_ok = False

        observed_server = receipt.get("server")
        if not isinstance(observed_server, dict):
            findings.append("current-source smoke receipt has no server identity")
            identity_ok = False
        else:
            if observed_server.get("source_sha") != source_sha:
                findings.append("smoke server source SHA does not match row subject")
                identity_ok = False
            if server_hash and observed_server.get("sha256") != server_hash:
                findings.append("smoke server hash does not match the built release binary")
                identity_ok = False

        observed_vsix = receipt.get("vsix")
        if isinstance(observed_vsix, dict) and isinstance(observed_vsix.get("sha256"), str):
            candidate = observed_vsix["sha256"].lower()
            if HEX64.fullmatch(candidate):
                vsix_hash = candidate
            else:
                findings.append("smoke VSIX SHA-256 is malformed")
                identity_ok = False
        else:
            findings.append("smoke receipt lacks an exact VSIX SHA-256")
            identity_ok = False

        if receipt.get("architecture") != args.architecture:
            findings.append("smoke receipt architecture does not match the row")
            identity_ok = False
        if receipt.get("vscode_version") != args.vscode_version:
            findings.append("smoke receipt VS Code host version does not match the row")
            identity_ok = False

    cells: dict[str, str] = {
        "artifact_identity": "pass" if identity_ok and archive_hash else "instrument_defect",
        "package_creation": stage_verdict(stages.get("package_creation")),
        "package_inventory": stage_verdict(stages.get("package_inventory")),
        "packaged_provider_edit_journey": stage_verdict(stages.get("behavioral_smoke")),
        "activation_failure_recovery": stage_verdict(stages.get("activation_failure_journey")),
        "crash_recovery": stage_verdict(stages.get("crash_recovery_journey")),
        "source_generation_exactness": "not_proven",
        "native_critic_installed": "not_proven",
        "full_document_utf16_installed": "not_proven",
        "dap_preview_installed": "not_proven",
    }

    if receipt is None and args.smoke_outcome == "failure":
        cells["package_creation"] = "instrument_defect"
        cells["package_inventory"] = "instrument_defect"
        cells["packaged_provider_edit_journey"] = "instrument_defect"
    elif receipt is not None and args.smoke_outcome == "failure" and receipt.get("overall") == "pass":
        findings.append("smoke process failed while its receipt claimed pass")
        cells["packaged_provider_edit_journey"] = "instrument_defect"

    cleanup_failure = receipt.get("cleanup_failure") if receipt else "not_observed"
    cells["process_cleanup"] = (
        "pass"
        if receipt is not None and cleanup_failure is None and stages.get("behavioral_smoke", {}).get("status") == "pass"
        else "instrument_defect" if cleanup_failure not in (None, "not_observed") else "not_proven"
    )

    zero_budget_counts: dict[str, int | None] = {key: None for key in ZERO_BUDGET_KEYS}
    zero_budget_counts["wrong_binary_or_artifact"] = 0 if cells["artifact_identity"] == "pass" else 1
    zero_budget_counts["partial_or_checksum_invalid_install"] = (
        0 if cells["package_inventory"] == "pass" else 1 if cells["package_inventory"] == "product_defect" else None
    )
    zero_budget_counts["orphaned_candidate_process"] = (
        0 if cells["process_cleanup"] == "pass" else 1 if cells["process_cleanup"] == "product_defect" else None
    )
    zero_budget_counts["silent_product_failure"] = (
        1 if any(value == "product_defect" for value in cells.values()) else 0
    )

    row = {
        "schema_version": ROW_SCHEMA,
        "row_id": args.row_id,
        "subject": {
            "kind": "exact_current_main",
            "repository_sha": source_sha,
            "source_version": args.source_version,
            "platform": args.platform,
            "architecture": args.architecture,
            "host_role": args.host_role,
            "vscode_version": args.vscode_version,
        },
        "artifacts": {
            "perllsp": {"name": server.name, "sha256": server_hash},
            "perl_dap": {"name": dap.name, "sha256": dap_hash},
            "product_unit_archive": {"name": archive.name, "sha256": archive_hash},
            "vsix": {"sha256": vsix_hash, "retained": False},
        },
        "mechanism_receipt": {
            "kind": "vscode_current_source_smoke.v1",
            "sha256": receipt_digest,
            "logical_name": receipt_path.name if receipt_path else None,
            "overall": receipt.get("overall") if receipt else None,
        },
        "cells": cells,
        "zero_budget_counts": zero_budget_counts,
        "findings": sorted(set(findings)),
        "status": summarize_row(cells),
        "claim_boundary": (
            "Rolling release-shaped installed observation only. Missing native-Critic, "
            "FULL/UTF-16, DAP, and exact generation evidence remains not_proven."
        ),
    }
    write_json(pathlib.Path(args.output), row)
    return 0


def aggregate_status(values: Iterable[str]) -> str:
    values = list(values)
    if "blocked" in values:
        return "blocked"
    if "not_proven" in values or not values:
        return "not_proven"
    return "pass"


def fan_in(args: argparse.Namespace) -> int:
    source_sha = require_sha(args.source_sha)
    rows_root = pathlib.Path(args.rows_root)
    observed: dict[str, Mapping[str, Any]] = {}
    malformed: list[str] = []
    for path in sorted(rows_root.rglob("*.json")) if rows_root.exists() else []:
        try:
            value = read_json(path)
        except ObservationError as error:
            malformed.append(str(error))
            continue
        if not isinstance(value, dict) or value.get("schema_version") != ROW_SCHEMA:
            continue
        row_id = value.get("row_id")
        if not isinstance(row_id, str):
            malformed.append(f"row at {path} lacks row_id")
            continue
        if row_id in observed:
            malformed.append(f"duplicate row_id {row_id}")
            continue
        subject = value.get("subject")
        if not isinstance(subject, dict) or subject.get("repository_sha") != source_sha:
            malformed.append(f"row {row_id} is bound to another source SHA")
            continue
        observed[row_id] = value

    missing_rows = [row_id for row_id in REQUIRED_ROWS if row_id not in observed]
    row_statuses = {
        row_id: (observed[row_id].get("status") if row_id in observed else "not_proven")
        for row_id in REQUIRED_ROWS
    }
    for row_id, status in row_statuses.items():
        if status not in {"pass", "blocked", "not_proven"}:
            malformed.append(f"row {row_id} has invalid status {status!r}")
            row_statuses[row_id] = "not_proven"

    cells: dict[str, dict[str, str]] = {}
    product_blockers: list[str] = []
    instrument_defects: list[str] = []
    not_proven_cells: list[str] = []
    for row_id in REQUIRED_ROWS:
        row = observed.get(row_id)
        row_cells = row.get("cells") if isinstance(row, dict) else None
        normalized: dict[str, str] = {}
        if isinstance(row_cells, dict):
            for name, verdict in sorted(row_cells.items()):
                if verdict not in VERDICTS:
                    malformed.append(f"row {row_id} cell {name} has invalid verdict {verdict!r}")
                    verdict = "instrument_defect"
                normalized[str(name)] = str(verdict)
                qualified = f"{row_id}:{name}"
                if verdict == "product_defect":
                    product_blockers.append(qualified)
                elif verdict == "instrument_defect":
                    instrument_defects.append(qualified)
                elif verdict == "not_proven":
                    not_proven_cells.append(qualified)
        else:
            not_proven_cells.append(f"{row_id}:row_missing")
        cells[row_id] = normalized

    aggregate_counts: dict[str, int | None] = {}
    for key in ZERO_BUDGET_KEYS:
        values: list[int] = []
        incomplete = False
        for row_id in REQUIRED_ROWS:
            row = observed.get(row_id)
            counts = row.get("zero_budget_counts") if isinstance(row, dict) else None
            value = counts.get(key) if isinstance(counts, dict) else None
            if isinstance(value, int):
                values.append(value)
            else:
                incomplete = True
        aggregate_counts[key] = None if incomplete else sum(values)

    topology_path = pathlib.Path(args.topology)
    topology_digest = None
    try:
        exact_regular_file(topology_path, "release topology projection")
        topology_digest = sha256(topology_path)
    except ObservationError as error:
        malformed.append(str(error))

    preliminary = aggregate_status(row_statuses.values())
    if product_blockers:
        recommendation = "blocked"
    elif missing_rows or malformed or instrument_defects or not_proven_cells:
        recommendation = "not_proven"
    else:
        recommendation = preliminary

    packet = {
        "schema_version": FAN_IN_SCHEMA,
        "subject_kind": "exact_current_main",
        "repository_sha": source_sha,
        "source_version": args.source_version,
        "target_release": "0.18.0",
        "release_topology_digest": topology_digest,
        "artifact_hashes": {
            row_id: (observed[row_id].get("artifacts") if row_id in observed else None)
            for row_id in REQUIRED_ROWS
        },
        "mechanism_receipts": {
            row_id: (observed[row_id].get("mechanism_receipt") if row_id in observed else None)
            for row_id in REQUIRED_ROWS
        },
        "vs_code_hosts": {
            "minimum_supported": {"row": "linux-minimum", "status": row_statuses["linux-minimum"]},
            "current_stable_linux": {"row": "linux-current", "status": row_statuses["linux-current"]},
            "current_stable_windows": {"row": "windows-current", "status": row_statuses["windows-current"]},
        },
        "platforms": {
            "linux": aggregate_status(
                [row_statuses["linux-minimum"], row_statuses["linux-current"]]
            ),
            "windows": row_statuses["windows-current"],
            "other_retained_targets": "not_proven",
        },
        "journey_cells": cells,
        "zero_budget_counts": aggregate_counts,
        "product_blockers": sorted(product_blockers),
        "instrument_defects": sorted(set(instrument_defects + malformed)),
        "not_proven_cells": sorted(set(not_proven_cells)),
        "missing_rows": missing_rows,
        "expected_beta_limitations": [
            "DAP remains preview and requires exact installed evidence from #6694.",
            "Native Critic requires exact installed evidence from #6992.",
            "FULL/UTF-16 installed behavior requires the #9378-#9389 train.",
        ],
        "freeze_recommendation": recommendation,
        "claim_boundary": (
            "Rolling evidence only: freezes_product=false, closes_6056=false, "
            "closes_4346=false, can_discover_blockers=true."
        ),
        "freezes_product": False,
        "closes_6056": False,
        "closes_4346": False,
        "can_discover_blockers": True,
    }
    write_json(pathlib.Path(args.output), packet)
    if args.require_ready and recommendation != "pass":
        return 1
    return 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    commands = root.add_subparsers(dest="command", required=True)

    package = commands.add_parser("package")
    package.add_argument("--source-sha", required=True)
    package.add_argument("--source-version", required=True)
    package.add_argument("--platform", required=True, choices=("linux", "windows"))
    package.add_argument("--architecture", required=True)
    package.add_argument("--server", required=True)
    package.add_argument("--dap", required=True)
    package.add_argument("--output", required=True)
    package.add_argument("--manifest-output")
    package.set_defaults(handler=package_artifacts)

    row = commands.add_parser("row")
    row.add_argument("--source-sha", required=True)
    row.add_argument("--source-version", required=True)
    row.add_argument("--row-id", required=True)
    row.add_argument("--platform", required=True, choices=("linux", "windows"))
    row.add_argument("--architecture", required=True)
    row.add_argument("--host-role", required=True)
    row.add_argument("--vscode-version", required=True)
    row.add_argument("--server", required=True)
    row.add_argument("--dap", required=True)
    row.add_argument("--archive", required=True)
    row.add_argument("--receipts-root", required=True)
    row.add_argument("--smoke-outcome", choices=("success", "failure", "skipped"), required=True)
    row.add_argument("--output", required=True)
    row.set_defaults(handler=build_row)

    joined = commands.add_parser("fan-in")
    joined.add_argument("--source-sha", required=True)
    joined.add_argument("--source-version", required=True)
    joined.add_argument("--rows-root", required=True)
    joined.add_argument("--topology", required=True)
    joined.add_argument("--output", required=True)
    joined.add_argument("--require-ready", action="store_true")
    joined.set_defaults(handler=fan_in)
    return root


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        return int(args.handler(args))
    except ObservationError as error:
        print(f"rolling installed observation failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
