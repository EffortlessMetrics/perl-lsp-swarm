"""Fail-closed validation for Zed managed public asset receipts."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from .common import RECEIPT_SCHEMA, ReceiptError, parse_digest, sha256_file
from .contract import validate_contract


def validate_receipt(
    receipt: dict[str, Any],
    contract_path: Path,
    contract: dict[str, Any],
) -> None:
    """Validate one receipt against the exact contract it claims to check.

    The checked contract path is required so the receipt's self-reported
    contract digest is recomputed and compared, and (for passing receipts)
    the receipt target rows are bound to the contract target set. A receipt
    may not attest to its own subject.
    """
    if receipt.get("schema_version") != RECEIPT_SCHEMA:
        raise ReceiptError("unexpected managed asset receipt schema")
    result = receipt.get("result")
    if result not in {"not_run", "pass", "fail", "instrument_failed", "contract_stale"}:
        raise ReceiptError(f"unknown receipt result {result!r}")

    contract_block = receipt.get("contract")
    if not isinstance(contract_block, dict):
        raise ReceiptError("receipt.contract must be an object")
    if result == "not_run":
        if contract_block.get("sha256") is not None:
            parse_digest(contract_block.get("sha256"), "receipt contract sha256")
    else:
        parse_digest(contract_block.get("sha256"), "receipt contract sha256")

    validate_contract(contract)
    if contract_block.get("sha256") is not None:
        checked_digest = sha256_file(contract_path)
        if contract_block.get("sha256") != checked_digest:
            raise ReceiptError(
                "receipt contract sha256 does not match the checked contract; "
                "the receipt was not produced against this contract"
            )

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

    expected_targets = {row["target"]: row.get("disposition") for row in contract["targets"]}
    actual_targets: dict[str, Any] = {}
    executed = 0
    managed = 0
    for row in rows:
        if not isinstance(row, dict):
            raise ReceiptError("receipt target row is not an object")
        target_name = row.get("target")
        if not isinstance(target_name, str) or not target_name:
            raise ReceiptError("receipt target row lacks a target name")
        if target_name in actual_targets:
            raise ReceiptError(f"duplicate receipt target row: {target_name}")
        actual_targets[target_name] = row.get("disposition")
        if row.get("disposition") != "managed":
            continue
        managed += 1
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

    if managed == 0:
        raise ReceiptError("passing receipt contains no managed target evidence")
    if actual_targets != expected_targets:
        raise ReceiptError(
            "passing receipt target rows do not match the checked contract target set"
        )

    host_process = boundary.get("host_process")
    if executed and host_process != "proven_for_matching_host_only":
        raise ReceiptError("passing executed row lacks matching host boundary")
    if not executed and host_process != "not_executed_on_this_verifier":
        raise ReceiptError("cross-extraction receipt overclaims host execution")
