#!/usr/bin/env python3
"""Build and fan in rolling installed-public-beta observation receipts.

This tool composes existing packaged VSIX/current-source smoke evidence. It does
not reinterpret source tests as installed behavior. Missing Critic, text-sync,
DAP, concrete host-version, topology-target, exactness, or cleanup evidence
remains explicitly not proven.

The fan-in emits ``rolling_installed_public_beta_fan_in.v1``: upstream rolling
evidence for the canonical ``pre_freeze_public_beta_acceptance.v1`` packet. It
deliberately does not use the canonical schema name, because the canonical
packet has a fixed candidate-bound shape (macOS platform evidence, mechanism
dispositions, the full journey denominator) that a three-row rolling
observation cannot populate. Consumers that expect the canonical shape must
keep validating it with ``xtask/examples/pre_freeze_public_beta_acceptance.rs``.
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
import zlib
from collections.abc import Iterable, Mapping, Sequence
from typing import Any

ROW_SCHEMA = "rolling_installed_public_beta_row.v1"
FAN_IN_SCHEMA = "rolling_installed_public_beta_fan_in.v1"
CANONICAL_PACKET_SCHEMA = "pre_freeze_public_beta_acceptance.v1"
PRODUCT_UNIT_SCHEMA = "rolling_release_artifact_unit.v1"
VERDICTS = {
    "pass",
    "product_defect",
    "instrument_defect",
    "unsupported_or_withdrawn",
    "not_proven",
}
# The canonical row table: each required row id is bound to exactly one
# platform, architecture, host role, and host-version selector kind. A row
# that keeps its id but drifts on any of these axes is not that row.
#
# `behavioral` records the product's candidate-bound platform policy: the
# packaged first-hour behavioral journey is candidate-bound and therefore
# Linux-only (assertCandidateBoundPlatform in runPublishedSmoke.ts). On a
# policy-restricted row the journey can never run, so its cell is honestly
# unsupported_or_withdrawn; an observed pass there would contradict the
# policy and is an instrument defect (policy drift), and a failure cannot be
# a product defect because the journey never executed.
ROW_SPECS: Mapping[str, Mapping[str, str]] = {
    "linux-minimum": {
        "platform": "linux",
        "architecture": "x64",
        "host_role": "minimum_supported",
        "host_selector": "concrete",
        "behavioral": "observed",
    },
    "linux-current": {
        "platform": "linux",
        "architecture": "x64",
        "host_role": "current_stable",
        "host_selector": "stable",
        "behavioral": "observed",
    },
    "windows-current": {
        "platform": "windows",
        "architecture": "x64",
        "host_role": "current_stable",
        "host_selector": "stable",
        "behavioral": "policy_linux_only",
    },
}
REQUIRED_ROWS = tuple(ROW_SPECS)
# The fixed cell denominator every row must report. An empty or partial cell
# set cannot claim a pass at fan-in.
REQUIRED_CELLS = (
    "artifact_identity",
    "package_creation",
    "package_inventory",
    "packaged_provider_edit_journey",
    "activation_failure_recovery",
    "crash_recovery",
    "host_version_exactness",
    "source_generation_exactness",
    "native_critic_installed",
    "full_document_utf16_installed",
    "dap_preview_installed",
    "process_cleanup",
)
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
CONCRETE_SELECTOR = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
# Host-resolution failures are instrument failures, never product defects.
# The smoke instrument reports them with these reason prefixes and already
# distinguishes `unavailable` (not proven) from network/cache/runner (failed).
HOST_RESOLUTION_REASON = "vscode_host_resolution_"


class ObservationError(RuntimeError):
    """The observation packet or its exact subject is malformed."""


def read_json(path: pathlib.Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ObservationError(f"cannot read JSON {path}: {error}") from error


def write_json(path: pathlib.Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f"{path.name}.{os.getpid()}.tmp")
    temporary.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
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


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def exact_regular_file(path: pathlib.Path, role: str) -> None:
    if not path.exists():
        raise ObservationError(f"{role} does not exist: {path}")
    if path.is_symlink() or not path.is_file():
        raise ObservationError(f"{role} must be a regular non-symlink file: {path}")
    if path.stat().st_size <= 0:
        raise ObservationError(f"{role} is empty: {path}")


def require_sha(value: str) -> str:
    normalized = value.strip().lower()
    if not HEX40.fullmatch(normalized):
        raise ObservationError(
            "source SHA must be exactly 40 lowercase hexadecimal characters"
        )
    return normalized


def row_spec(row_id: str) -> Mapping[str, str]:
    spec = ROW_SPECS.get(row_id)
    if spec is None:
        raise ObservationError(
            f"unknown rolling row {row_id!r}; required rows are {', '.join(REQUIRED_ROWS)}"
        )
    return spec


def check_row_axes(
    row_id: str,
    *,
    platform: str,
    architecture: str,
    host_role: str,
    vscode_version: str,
    findings: list[str] | None = None,
) -> bool:
    """Bind a row id to its canonical platform/architecture/host tuple.

    Returns True when every axis matches; otherwise records one finding per
    drifted axis (or raises when no findings sink is supplied).
    """

    problems: list[str] = []
    spec = ROW_SPECS.get(row_id)
    if spec is None:
        problems.append(f"row {row_id!r} is not a required rolling row")
    else:
        for axis, observed in (
            ("platform", platform),
            ("architecture", architecture),
            ("host_role", host_role),
        ):
            if observed != spec[axis]:
                problems.append(
                    f"row {row_id} must have {axis} {spec[axis]!r}, got {observed!r}"
                )
        selector_kind = spec["host_selector"]
        if selector_kind == "stable" and vscode_version != "stable":
            problems.append(
                f"row {row_id} must use the current-stable VS Code selector, "
                f"got {vscode_version!r}"
            )
        if selector_kind == "concrete" and not CONCRETE_SELECTOR.fullmatch(
            vscode_version
        ):
            problems.append(
                f"row {row_id} must use a concrete minimum VS Code version, "
                f"got {vscode_version!r}"
            )
    if problems:
        if findings is None:
            raise ObservationError("; ".join(problems))
        findings.extend(problems)
        return False
    return True


def package_artifacts(args: argparse.Namespace) -> int:
    source_sha = require_sha(args.source_sha)
    server = pathlib.Path(args.server).resolve()
    dap = pathlib.Path(args.dap).resolve()
    output = pathlib.Path(args.output)
    exact_regular_file(server, "perllsp")
    exact_regular_file(dap, "perl-dap")
    if server.name == dap.name:
        raise ObservationError(
            "perllsp and perl-dap must have distinct archive member names, "
            f"got {server.name!r} for both"
        )

    manifest = {
        "schema": PRODUCT_UNIT_SCHEMA,
        "source_sha": source_sha,
        "source_version": args.source_version,
        "platform": args.platform,
        "architecture": args.architecture,
        "members": [
            {
                "role": "perllsp",
                "name": server.name,
                "size": server.stat().st_size,
                "sha256": sha256(server),
            },
            {
                "role": "perl-dap",
                "name": dap.name,
                "size": dap.stat().st_size,
                "sha256": sha256(dap),
            },
        ],
    }
    manifest_bytes = (
        json.dumps(manifest, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")

    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(
        output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as archive:
        for path in (server, dap):
            info = zipfile.ZipInfo(path.name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100755 << 16
            archive.writestr(info, path.read_bytes())
        info = zipfile.ZipInfo(
            "artifact-manifest.json", date_time=(1980, 1, 1, 0, 0, 0)
        )
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o100644 << 16
        archive.writestr(info, manifest_bytes)

    exact_regular_file(output, "release-shaped product-unit archive")
    receipt = {
        **manifest,
        "archive": {
            "name": output.name,
            "size": output.stat().st_size,
            "sha256": sha256(output),
        },
    }
    manifest_output = (
        pathlib.Path(args.manifest_output)
        if args.manifest_output
        else output.with_suffix(".manifest.json")
    )
    write_json(manifest_output, receipt)
    return 0


def verify_product_unit(
    archive: pathlib.Path,
    *,
    server: pathlib.Path,
    dap: pathlib.Path,
    server_hash: str | None,
    dap_hash: str | None,
    source_sha: str,
    source_version: str,
    platform: str,
    architecture: str,
    findings: list[str],
) -> str | None:
    """Parse the product-unit archive and bind it to the current binaries.

    Arbitrary nonempty bytes, a wrong member set, a manifest from another
    source/version/platform, or member bytes that do not match the built
    release binaries all fail: the function records findings and returns None.
    """

    archive_hash = safe_hash(archive, findings, "release-shaped product-unit archive")
    if archive_hash is None:
        return None
    try:
        with zipfile.ZipFile(archive) as unit:
            names = sorted(unit.namelist())
            expected_names = sorted(
                {server.name, dap.name, "artifact-manifest.json"}
            )
            if names != expected_names:
                findings.append(
                    "product-unit archive members are "
                    f"{names}, expected exactly {expected_names}"
                )
                return None
            member_bytes = {
                name: unit.read(name) for name in (server.name, dap.name)
            }
            try:
                manifest = json.loads(unit.read("artifact-manifest.json"))
            except (KeyError, UnicodeError, json.JSONDecodeError) as error:
                findings.append(
                    f"product-unit manifest is unreadable: {error}"
                )
                return None
    except (zipfile.BadZipFile, EOFError, OSError, zlib.error) as error:
        findings.append(f"product-unit archive is not a readable zip: {error}")
        return None

    if not isinstance(manifest, dict):
        findings.append("product-unit manifest is not a JSON object")
        return None
    if manifest.get("schema") != PRODUCT_UNIT_SCHEMA:
        findings.append("product-unit manifest has an unsupported schema")
        return None
    for field, expected in (
        ("source_sha", source_sha),
        ("source_version", source_version),
        ("platform", platform),
        ("architecture", architecture),
    ):
        if manifest.get(field) != expected:
            findings.append(
                f"product-unit manifest {field} does not match this row subject"
            )
            return None

    members = manifest.get("members")
    if not isinstance(members, list):
        findings.append("product-unit manifest has no member list")
        return None
    by_role: dict[str, Mapping[str, Any]] = {}
    for member in members:
        if not isinstance(member, dict) or not isinstance(member.get("role"), str):
            findings.append("product-unit manifest member lacks a role")
            return None
        if member["role"] in by_role:
            findings.append("product-unit manifest repeats a member role")
            return None
        by_role[member["role"]] = member
    for role, binary, binary_hash in (
        ("perllsp", server, server_hash),
        ("perl-dap", dap, dap_hash),
    ):
        member = by_role.get(role)
        if member is None:
            findings.append(f"product-unit manifest lacks the {role} member")
            return None
        data = member_bytes[binary.name]
        if member.get("name") != binary.name:
            findings.append(f"product-unit {role} member name does not match")
            return None
        if member.get("size") != len(data):
            findings.append(f"product-unit {role} member size does not match")
            return None
        if not isinstance(member.get("sha256"), str) or not HEX64.fullmatch(
            member["sha256"]
        ):
            findings.append(f"product-unit {role} member hash is malformed")
            return None
        if sha256_bytes(data) != member["sha256"]:
            findings.append(
                f"product-unit {role} bytes do not match the manifest hash"
            )
            return None
        if binary_hash is not None and member["sha256"] != binary_hash:
            findings.append(
                f"product-unit {role} bytes are not the built release binary"
            )
            return None
    return archive_hash


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
        findings.append(
            "no exact current-source orchestration receipt matched the row subject"
        )
        return None, None, findings
    path, value = matches[0]
    return path, value, findings


def stage_verdict(stage: Any) -> str:
    """Classify one smoke stage without laundering instrument failures.

    A stage that failed because the VS Code host could not be resolved or
    reached (network, cache, or runner infrastructure) is an instrument
    defect. ``product_defect`` is reserved for failures observed after the
    exact intended host actually ran the packaged extension.
    """

    if not isinstance(stage, dict):
        return "not_proven"
    status = stage.get("status")
    reason = stage.get("reason")
    host_resolution = (
        isinstance(reason, str) and reason.startswith(HOST_RESOLUTION_REASON)
    )
    if status == "pass":
        return "pass"
    if status == "failed":
        if host_resolution:
            return "instrument_defect"
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
    expected_receipt_platform = {"linux": "linux", "windows": "win32"}.get(
        args.platform
    )
    if expected_receipt_platform is None:
        raise ObservationError(f"unsupported full-row platform: {args.platform}")
    # A row built from drifted matrix arguments is not evidence for the row it
    # claims to be; fail the row job instead of emitting a mislabelled row.
    check_row_axes(
        args.row_id,
        platform=args.platform,
        architecture=args.architecture,
        host_role=args.host_role,
        vscode_version=args.vscode_version,
    )

    findings: list[str] = []
    server = pathlib.Path(args.server).resolve()
    dap = pathlib.Path(args.dap).resolve()
    archive = pathlib.Path(args.archive).resolve()
    server_hash = safe_hash(server, findings, "perllsp")
    dap_hash = safe_hash(dap, findings, "perl-dap")
    archive_hash = verify_product_unit(
        archive,
        server=server,
        dap=dap,
        server_hash=server_hash,
        dap_hash=dap_hash,
        source_sha=source_sha,
        source_version=args.source_version,
        platform=args.platform,
        architecture=args.architecture,
        findings=findings,
    )

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

        instrument_failure = receipt.get("instrument_failure")
        if isinstance(instrument_failure, str) and instrument_failure.strip():
            findings.append(
                "current-source smoke instrument reported a failure; affected "
                "cells are instrument evidence, not product evidence"
            )

        observed_server = receipt.get("server")
        if not isinstance(observed_server, dict):
            findings.append("current-source smoke receipt has no server identity")
            identity_ok = False
        else:
            if observed_server.get("source_sha") != source_sha:
                findings.append("smoke server source SHA does not match row subject")
                identity_ok = False
            if server_hash and observed_server.get("sha256") != server_hash:
                findings.append(
                    "smoke server hash does not match the built release binary"
                )
                identity_ok = False

        observed_vsix = receipt.get("vsix")
        if isinstance(observed_vsix, dict) and isinstance(
            observed_vsix.get("sha256"), str
        ):
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
            findings.append("smoke receipt VS Code selector does not match the row")
            identity_ok = False

    cells: dict[str, str] = {
        "artifact_identity": (
            "pass" if identity_ok and archive_hash else "instrument_defect"
        ),
        "package_creation": stage_verdict(stages.get("package_creation")),
        "package_inventory": stage_verdict(stages.get("package_inventory")),
        "packaged_provider_edit_journey": stage_verdict(
            stages.get("behavioral_smoke")
        ),
        "activation_failure_recovery": stage_verdict(
            stages.get("activation_failure_journey")
        ),
        "crash_recovery": stage_verdict(stages.get("crash_recovery_journey")),
        # A concrete selector is exact host evidence only when a bound smoke
        # receipt actually ran against that same version; a receipt from
        # another host version or a missing receipt stays not_proven.
        "host_version_exactness": (
            "pass"
            if (
                args.vscode_version != "stable"
                and receipt is not None
                and receipt.get("vscode_version") == args.vscode_version
            )
            else "not_proven"
        ),
        "source_generation_exactness": "not_proven",
        "native_critic_installed": "not_proven",
        "full_document_utf16_installed": "not_proven",
        "dap_preview_installed": "not_proven",
    }

    if receipt is None and args.smoke_outcome == "failure":
        cells["package_creation"] = "instrument_defect"
        cells["package_inventory"] = "instrument_defect"
        cells["packaged_provider_edit_journey"] = "instrument_defect"
    elif (
        receipt is not None
        and args.smoke_outcome == "failure"
        and receipt.get("overall") == "pass"
    ):
        findings.append("smoke process failed while its receipt claimed pass")
        cells["packaged_provider_edit_journey"] = "instrument_defect"

    behavioral_stage = stages.get("behavioral_smoke")
    behavioral_status = (
        behavioral_stage.get("status")
        if isinstance(behavioral_stage, dict)
        else None
    )

    # Candidate-bound behavioral journeys are Linux-only by product policy
    # (assertCandidateBoundPlatform). On a policy-restricted row the journey
    # can never execute: a pass would contradict the policy (instrument
    # defect), and a failure is the guard boundary, never product evidence.
    if receipt is not None and row_spec(args.row_id)["behavioral"] != "observed":
        findings.append(
            "candidate-bound behavioral journey is policy-restricted to Linux; "
            "this row's packaged_provider_edit_journey is unsupported_or_withdrawn"
        )
        if behavioral_status == "pass":
            findings.append(
                "candidate-bound behavioral stage passed on a policy-restricted "
                "platform; the product policy drifted and this row must be "
                "reclassified"
            )
            cells["packaged_provider_edit_journey"] = "instrument_defect"
        else:
            cells["packaged_provider_edit_journey"] = "unsupported_or_withdrawn"

    # An absent cleanup_failure key is unobserved evidence, never proof of
    # clean cleanup; only an explicitly null observed value can pass.
    cleanup_reported = receipt is not None and "cleanup_failure" in receipt
    cleanup_failure = receipt.get("cleanup_failure") if cleanup_reported else "not_observed"
    if cleanup_reported and cleanup_failure is None and behavioral_status == "pass":
        cells["process_cleanup"] = "pass"
    elif cleanup_failure not in (None, "not_observed"):
        cells["process_cleanup"] = "instrument_defect"
    else:
        cells["process_cleanup"] = "not_proven"

    zero_budget_counts: dict[str, int | None] = {
        key: None for key in ZERO_BUDGET_KEYS
    }
    zero_budget_counts["wrong_binary_or_artifact"] = (
        0 if cells["artifact_identity"] == "pass" else 1
    )
    zero_budget_counts["partial_or_checksum_invalid_install"] = (
        0
        if cells["package_inventory"] == "pass"
        else 1
        if cells["package_inventory"] == "product_defect"
        else None
    )
    zero_budget_counts["orphaned_candidate_process"] = (
        0
        if cells["process_cleanup"] == "pass"
        else 1
        if cells["process_cleanup"] == "product_defect"
        else None
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
            "vscode_selector": args.vscode_version,
            "vscode_concrete_version": (
                None if args.vscode_version == "stable" else args.vscode_version
            ),
        },
        "artifacts": {
            "perllsp": {"name": server.name, "sha256": server_hash},
            "perl_dap": {"name": dap.name, "sha256": dap_hash},
            "product_unit_archive": {
                "name": archive.name,
                "sha256": archive_hash,
            },
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
            "Rolling release-shaped installed observation only. A stable selector "
            "does not establish a concrete host version, and missing native-Critic, "
            "FULL/UTF-16, DAP, generation, and topology-target evidence remains "
            "not_proven."
        ),
    }
    write_json(pathlib.Path(args.output), row)
    return 0


def aggregate_status(values: Iterable[str]) -> str:
    observed = list(values)
    if "blocked" in observed:
        return "blocked"
    if "not_proven" in observed or not observed:
        return "not_proven"
    return "pass"


def validate_row(
    row_id: str,
    row: Mapping[str, Any],
    *,
    source_sha: str,
    source_version: str,
    malformed: list[str],
) -> Mapping[str, str] | None:
    """Validate one row against the canonical row table and cell denominator.

    Returns the validated cells on success. Any drift in subject identity,
    artifact shape, or the required cell set makes the row malformed: it can
    never satisfy its required-row slot.
    """

    spec = ROW_SPECS.get(row_id)
    if spec is None:
        malformed.append(f"unexpected row {row_id!r} is not in the required row table")
        return None

    subject = row.get("subject")
    if not isinstance(subject, dict):
        malformed.append(f"row {row_id} lacks a subject identity")
        return None
    if subject.get("kind") != "exact_current_main":
        malformed.append(f"row {row_id} subject kind is not exact_current_main")
        return None
    if subject.get("repository_sha") != source_sha:
        malformed.append(f"row {row_id} is bound to another source SHA")
        return None
    if subject.get("source_version") != source_version:
        malformed.append(f"row {row_id} is bound to another source version")
        return None
    subject_problems: list[str] = []
    axes_ok = check_row_axes(
        row_id,
        platform=str(subject.get("platform")),
        architecture=str(subject.get("architecture")),
        host_role=str(subject.get("host_role")),
        vscode_version=str(subject.get("vscode_selector")),
        findings=subject_problems,
    )
    if not axes_ok:
        malformed.extend(subject_problems)
        return None
    selector = subject.get("vscode_selector")
    concrete = subject.get("vscode_concrete_version")
    if selector == "stable" and concrete is not None:
        malformed.append(f"row {row_id} stable selector carries a concrete version")
        return None
    if (
        isinstance(selector, str)
        and CONCRETE_SELECTOR.fullmatch(selector)
        and concrete != selector
    ):
        malformed.append(f"row {row_id} concrete selector/version disagree")
        return None

    artifacts = row.get("artifacts")
    if not isinstance(artifacts, dict):
        malformed.append(f"row {row_id} lacks an artifact identity block")
        return None
    for artifact_key in ("perllsp", "perl_dap", "product_unit_archive"):
        entry = artifacts.get(artifact_key)
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            malformed.append(f"row {row_id} artifact {artifact_key} is malformed")
            return None
        digest = entry.get("sha256")
        if digest is not None and not (
            isinstance(digest, str) and HEX64.fullmatch(digest)
        ):
            malformed.append(
                f"row {row_id} artifact {artifact_key} hash is malformed"
            )
            return None

    mechanism = row.get("mechanism_receipt")
    if not isinstance(mechanism, dict) or not (
        mechanism.get("sha256") is None
        or (
            isinstance(mechanism.get("sha256"), str)
            and HEX64.fullmatch(mechanism["sha256"])
        )
    ):
        malformed.append(f"row {row_id} mechanism receipt identity is malformed")
        return None

    counts = row.get("zero_budget_counts")
    if not isinstance(counts, dict) or set(counts) != set(ZERO_BUDGET_KEYS):
        malformed.append(f"row {row_id} zero-budget denominator is malformed")
        return None
    for key, value in counts.items():
        # bool is an int subclass; True/False and negative counts are
        # malformed evidence, not totals.
        if value is not None and (type(value) is not int or value < 0):
            malformed.append(f"row {row_id} zero-budget count {key} is malformed")
            return None

    row_cells = row.get("cells")
    if not isinstance(row_cells, dict):
        malformed.append(f"row {row_id} has no cell verdicts")
        return None
    missing = [name for name in REQUIRED_CELLS if name not in row_cells]
    extra = [name for name in row_cells if name not in REQUIRED_CELLS]
    if missing or extra:
        malformed.append(
            f"row {row_id} cell denominator drifted: missing={sorted(missing)} "
            f"unexpected={sorted(extra)}"
        )
        return None
    normalized: dict[str, str] = {}
    for name in REQUIRED_CELLS:
        verdict = row_cells[name]
        if verdict not in VERDICTS:
            malformed.append(
                f"row {row_id} cell {name} has invalid verdict {verdict!r}"
            )
            return None
        normalized[name] = str(verdict)

    # A claimed artifact_identity pass must be backed by exact identities:
    # null digests are admissible only on non-passing evidence.
    if normalized.get("artifact_identity") == "pass":
        identity_digests = {
            key: artifacts[key].get("sha256")
            for key in ("perllsp", "perl_dap", "product_unit_archive")
        }
        vsix = artifacts.get("vsix")
        identity_digests["vsix"] = (
            vsix.get("sha256") if isinstance(vsix, dict) else None
        )
        identity_digests["mechanism_receipt"] = mechanism.get("sha256")
        missing = [
            name for name, digest in identity_digests.items() if digest is None
        ]
        if missing:
            malformed.append(
                f"row {row_id} claims artifact_identity pass with null "
                f"identities: {sorted(missing)}"
            )
            return None

    # Never trust the child's declared status: derive it from validated cells
    # and flag any disagreement as an instrument defect.
    derived = summarize_row(normalized)
    declared = row.get("status")
    if declared != derived:
        malformed.append(
            f"row {row_id} declares status {declared!r} but its cells derive "
            f"{derived!r}"
        )
    return normalized


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
        observed[row_id] = value

    validated_cells: dict[str, Mapping[str, str]] = {}
    for row_id, row in sorted(observed.items()):
        cells = validate_row(
            row_id,
            row,
            source_sha=source_sha,
            source_version=args.source_version,
            malformed=malformed,
        )
        if cells is not None:
            validated_cells[row_id] = cells

    missing_rows = [
        row_id for row_id in REQUIRED_ROWS if row_id not in validated_cells
    ]
    # Row status is derived from the validated cells, never from the child's
    # declared status field.
    row_statuses = {
        row_id: (
            summarize_row(validated_cells[row_id])
            if row_id in validated_cells
            else "not_proven"
        )
        for row_id in REQUIRED_ROWS
    }

    cells: dict[str, dict[str, str]] = {}
    product_blockers: list[str] = []
    instrument_defects: list[str] = []
    not_proven_cells: list[str] = ["topology:other_retained_targets"]
    for row_id in REQUIRED_ROWS:
        normalized = dict(validated_cells.get(row_id, {}))
        cells[row_id] = normalized
        if not normalized:
            not_proven_cells.append(f"{row_id}:row_missing")
            continue
        for name, verdict in sorted(normalized.items()):
            qualified = f"{row_id}:{name}"
            if verdict == "product_defect":
                product_blockers.append(qualified)
            elif verdict == "instrument_defect":
                instrument_defects.append(qualified)
            elif verdict == "not_proven":
                not_proven_cells.append(qualified)

    aggregate_counts: dict[str, int | None] = {}
    for key in ZERO_BUDGET_KEYS:
        values: list[int] = []
        incomplete = False
        for row_id in REQUIRED_ROWS:
            row = observed.get(row_id)
            counts = row.get("zero_budget_counts") if isinstance(row, dict) else None
            value = counts.get(key) if isinstance(counts, dict) else None
            if row_id in validated_cells and isinstance(value, int):
                values.append(value)
            else:
                incomplete = True
        aggregate_counts[key] = None if incomplete else sum(values)

    topology_path = pathlib.Path(args.topology)
    topology_digest = None
    try:
        exact_regular_file(topology_path, "release topology projection")
        topology = read_json(topology_path)
        if not isinstance(topology, dict):
            raise ObservationError("release topology projection is not an object")
        if topology.get("release") != args.source_version:
            raise ObservationError(
                "release topology projection is for release "
                f"{topology.get('release')!r}, not {args.source_version!r}"
            )
        if topology.get("frozen_product_sha") != source_sha:
            raise ObservationError(
                "release topology projection is bound to a different source SHA"
            )
        topology_digest = sha256(topology_path)
    except ObservationError as error:
        malformed.append(str(error))

    # Row-level findings (including smoke instrument failures) must remain
    # visible after fan-in; they are not verdicts, but they are evidence.
    row_findings: dict[str, list[str]] = {}
    for row_id in REQUIRED_ROWS:
        row = observed.get(row_id)
        if row_id not in validated_cells or not isinstance(row, dict):
            continue
        row_finding_list = row.get("findings")
        if isinstance(row_finding_list, list):
            row_findings[row_id] = sorted(
                {str(item) for item in row_finding_list}
            )

    if product_blockers:
        recommendation = "blocked"
    elif missing_rows or malformed or instrument_defects or not_proven_cells:
        recommendation = "not_proven"
    else:
        recommendation = aggregate_status(row_statuses.values())

    packet = {
        "schema_version": FAN_IN_SCHEMA,
        "canonical_packet_schema": CANONICAL_PACKET_SCHEMA,
        "subject_kind": "exact_current_main",
        "repository_sha": source_sha,
        "source_version": args.source_version,
        # The rolling target is the observed source version itself; a
        # hard-coded release would disagree with the generated topology
        # whenever main's version moves.
        "target_release": args.source_version,
        "release_topology_digest": topology_digest,
        "artifact_hashes": {
            row_id: (
                observed[row_id].get("artifacts")
                if row_id in validated_cells
                else None
            )
            for row_id in REQUIRED_ROWS
        },
        "mechanism_receipts": {
            row_id: (
                observed[row_id].get("mechanism_receipt")
                if row_id in validated_cells
                else None
            )
            for row_id in REQUIRED_ROWS
        },
        "vs_code_hosts": {
            "minimum_supported": {
                "row": "linux-minimum",
                "status": row_statuses["linux-minimum"],
            },
            "current_stable_linux": {
                "row": "linux-current",
                "status": row_statuses["linux-current"],
            },
            "current_stable_windows": {
                "row": "windows-current",
                "status": row_statuses["windows-current"],
            },
        },
        "platforms": {
            "linux": aggregate_status(
                [row_statuses["linux-minimum"], row_statuses["linux-current"]]
            ),
            "windows": row_statuses["windows-current"],
            "other_retained_targets": "not_proven",
        },
        "journey_cells": cells,
        "row_findings": row_findings,
        "zero_budget_counts": aggregate_counts,
        "product_blockers": sorted(product_blockers),
        "instrument_defects": sorted(set(instrument_defects + malformed)),
        "not_proven_cells": sorted(set(not_proven_cells)),
        "missing_rows": missing_rows,
        "expected_beta_limitations": [
            "Current-stable selectors do not establish concrete VS Code versions.",
            "DAP remains preview and requires exact installed evidence from #6694.",
            "Native Critic requires exact installed evidence from #6992.",
            "FULL/UTF-16 installed behavior requires the #9378-#9389 train.",
            "Other retained topology targets require bounded archive/launch rows.",
        ],
        "freeze_recommendation": recommendation,
        "claim_boundary": (
            "Rolling evidence only: freezes_product=false, closes_6056=false, "
            "closes_4346=false, can_discover_blockers=true. This is the upstream "
            f"{FAN_IN_SCHEMA} packet, not the canonical {CANONICAL_PACKET_SCHEMA} "
            "packet validated by xtask/examples/pre_freeze_public_beta_acceptance.rs."
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
    row.add_argument("--row-id", required=True, choices=REQUIRED_ROWS)
    row.add_argument("--platform", required=True, choices=("linux", "windows"))
    row.add_argument("--architecture", required=True)
    row.add_argument("--host-role", required=True)
    row.add_argument("--vscode-version", required=True)
    row.add_argument("--server", required=True)
    row.add_argument("--dap", required=True)
    row.add_argument("--archive", required=True)
    row.add_argument("--receipts-root", required=True)
    row.add_argument(
        "--smoke-outcome",
        choices=("success", "failure", "skipped"),
        required=True,
    )
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
