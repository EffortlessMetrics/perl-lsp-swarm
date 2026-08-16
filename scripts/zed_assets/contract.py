"""Validation for the checked Zed managed-download projection."""

from __future__ import annotations

from typing import Any

from .common import (
    CONTRACT_SCHEMA,
    ReceiptError,
    parse_digest,
    validate_relative_member,
    validate_single_component,
)


def validate_contract(contract: dict[str, Any]) -> None:
    if contract.get("schema_version") != CONTRACT_SCHEMA:
        raise ReceiptError("unexpected Zed managed-download contract schema")
    identity = contract.get("identity")
    if not isinstance(identity, dict):
        raise ReceiptError("contract.identity must be an object")
    if identity.get("server_id") != "perllsp" or identity.get("executable") != "perllsp":
        raise ReceiptError("managed assets must retain exact perllsp identity")
    if identity.get("arguments") != ["--stdio"]:
        raise ReceiptError("managed assets must retain exact ['--stdio'] arguments")

    source = contract.get("source")
    if not isinstance(source, dict):
        raise ReceiptError("contract.source must be an object")
    if source.get("repository") != "EffortlessMetrics/perl-lsp":
        raise ReceiptError("unexpected release repository")
    if source.get("prerelease") is not False:
        raise ReceiptError("managed route cannot target a prerelease")
    version = source.get("version")
    if not isinstance(version, str) or not version:
        raise ReceiptError("contract source version is missing")
    validate_single_component(version, "contract source version")

    rows = contract.get("targets")
    if not isinstance(rows, list) or not rows:
        raise ReceiptError("contract.targets must be a non-empty array")
    seen: set[str] = set()
    managed = 0
    windows_arm64 = False
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            raise ReceiptError(f"target row {index} must be an object")
        target = row.get("target")
        disposition = row.get("disposition")
        if not isinstance(target, str) or not target:
            raise ReceiptError(f"target row {index} lacks target")
        validate_single_component(target, f"target row {index} target")
        if target in seen:
            raise ReceiptError(f"duplicate target row: {target}")
        seen.add(target)
        if target == "aarch64-pc-windows-msvc":
            windows_arm64 = disposition == "unsupported"
        if disposition != "managed":
            if disposition not in {"path_only", "deferred", "unsupported"}:
                raise ReceiptError(f"target {target} has unknown disposition {disposition!r}")
            continue

        managed += 1
        archive_type = row.get("archive_type")
        if archive_type not in {"tar.gz", "zip"}:
            raise ReceiptError(f"target {target} has unsupported archive type")
        validate_single_component(row.get("asset_name"), f"target {target} asset_name")
        suffix = ".tar.gz" if archive_type == "tar.gz" else ".zip"
        if row.get("asset_name") != f"perllsp-{version}-{target}{suffix}":
            raise ReceiptError(f"target {target} asset name does not match the contract")
        if not isinstance(row.get("asset_id"), int) or row["asset_id"] <= 0:
            raise ReceiptError(f"target {target} lacks a positive asset id")
        if not isinstance(row.get("asset_size"), int) or row["asset_size"] <= 0:
            raise ReceiptError(f"target {target} lacks a positive asset size")
        parse_digest(row.get("asset_digest"), f"target {target} asset_digest")
        member = row.get("archive_member")
        installed = row.get("installed_path")
        if not isinstance(member, str) or not isinstance(installed, str):
            raise ReceiptError(f"target {target} lacks archive/install path")
        validate_relative_member(member)
        validate_relative_member(installed)
        if "perl-lsp" in member or "perl-lsp" in installed:
            raise ReceiptError(f"target {target} references another executable")
        if archive_type == "zip" and member != "perllsp.exe":
            raise ReceiptError("Windows archive must expose root-level perllsp.exe")
        if archive_type == "tar.gz" and member != f"perllsp-{version}-{target}/perllsp":
            raise ReceiptError(f"Unix target {target} has an unexpected member layout")

    if managed == 0:
        raise ReceiptError("contract contains no managed target")
    if not windows_arm64:
        raise ReceiptError("Windows ARM64 must remain explicitly unsupported")

    boundary = contract.get("claim_boundary")
    if not isinstance(boundary, dict):
        raise ReceiptError("contract.claim_boundary must be an object")
    for cell in (
        "archive_extraction",
        "perllsp_version_execution",
        "stdio_initialize_shutdown",
        "actual_zed_host",
    ):
        if boundary.get(cell) != "not_proven":
            raise ReceiptError(f"static contract overclaims {cell}")
