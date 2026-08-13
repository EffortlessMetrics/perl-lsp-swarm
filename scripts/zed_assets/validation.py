"""Fail-closed validation for Zed managed public asset receipts."""

from __future__ import annotations

from typing import Any

from .common import RECEIPT_SCHEMA, ReceiptError, parse_digest


def validate_receipt(receipt: dict[str, Any]) -> None:
    if receipt.get("schema_version") != RECEIPT_SCHEMA:
        raise ReceiptError("unexpected managed asset receipt schema")
    result = receipt.get("result")
    if result not in {"not_run", "pass", "fail", "instrument_failed", "contract_stale"}:
        raise ReceiptError(f"unknown receipt result {result!r}")

    contract = receipt.get("contract")
    if not isinstance(contract, dict):
        raise ReceiptError("receipt.contract must be an object")
    if result == "not_run":
        if contract.get("sha256") is not None:
            parse_digest(contract.get("sha256"), "receipt contract sha256")
    else:
        parse_digest(contract.get("sha256"), "receipt contract sha256")

    boundary = receipt.get("claim_boundary")
    if not isinstance(boundary, dict):
        raise ReceiptError("receipt.claim_boundary must be an object")
    if boundary.get("actual_zed") != "not_proven" or boundary.get("public_registry") != "not_proven":
        raise ReceiptError("asset receipt may not claim Zed host or registry proof")
    if result != "pass":
        return

    release = receipt.get("release")
    if (
        not isinstance(release, dict)
        or release.get("prerelease") is not False
        or release.get("draft") is not False
    ):
        raise ReceiptError("passing receipt must bind a stable public release")
    rows = receipt.get("targets")
    if not isinstance(rows, list) or not rows:
        raise ReceiptError("passing receipt must contain target rows")

    executed = 0
    for row in rows:
        if not isinstance(row, dict):
            raise ReceiptError("receipt target row is not an object")
        if row.get("disposition") != "managed":
            continue
        if row.get("result") not in {
            "managed_executed",
            "managed_extracted_not_executed",
        }:
            raise ReceiptError("managed target lacks an executable or extracted disposition")
        asset = row.get("asset")
        binary = row.get("binary")
        archive = row.get("archive")
        if not isinstance(asset, dict) or not isinstance(binary, dict) or not isinstance(archive, dict):
            raise ReceiptError("managed target lacks asset/archive/binary identity")
        parse_digest(asset.get("sha256"), "receipt asset sha256")
        parse_digest(binary.get("sha256"), "receipt binary sha256")
        parse_digest(archive.get("members_sha256"), "receipt members sha256")
        if archive.get("safe") is not True:
            raise ReceiptError("passing managed target did not prove safe archive handling")

        if row.get("result") == "managed_executed":
            executed += 1
            smoke = row.get("stdio_smoke")
            if not isinstance(smoke, dict) or smoke.get("result") != "pass":
                raise ReceiptError("executed target lacks passing stdio smoke")
            if smoke.get("stdout_pure") is not True or smoke.get("process_group_clean") is not True:
                raise ReceiptError("executed target lacks stdout or cleanup proof")

    host_process = boundary.get("host_process")
    if executed and host_process != "proven_for_matching_host_only":
        raise ReceiptError("passing executed row lacks matching host boundary")
    if not executed and host_process != "not_executed_on_this_verifier":
        raise ReceiptError("cross-extraction receipt overclaims host execution")
