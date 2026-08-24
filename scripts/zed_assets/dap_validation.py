"""Fail-closed validation for Zed perl-dap public asset receipts."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from .common import ReceiptError, parse_digest, sha256_file
from .dap_contract import DAP_MANAGED_PREFIX, validate_dap_contract

DAP_RECEIPT_SCHEMA = "zed_perl_dap_asset_receipt.v1"


def _verifier_matches(verifier: dict[str, Any], row: dict[str, Any]) -> bool:
    os_name = str(row.get("os", "")).lower()
    verifier_os = str(verifier.get("os", "")).lower()
    if verifier_os == "darwin":
        verifier_os = "macos"
    return os_name == verifier_os and row.get("architecture") == verifier.get("architecture")


def validate_dap_receipt(
    receipt: dict[str, Any],
    contract_path: Path,
    contract: dict[str, Any],
) -> None:
    """Validate one perl-dap receipt against the exact contract it checks.

    The checked contract path is required so the receipt's self-reported
    contract digest is recomputed, and passing receipts are bound to the
    contract target set, the verifier host, and the unproven higher stages.
    A receipt may not attest to its own subject, and a cross-built row can
    never appear executed for a verifier it does not match.
    """
    if receipt.get("schema_version") != DAP_RECEIPT_SCHEMA:
        raise ReceiptError("unexpected perl-dap asset receipt schema")
    if receipt.get("stage") != "public_perl_dap_asset":
        raise ReceiptError("perl-dap receipt must name its evidence stage")
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

    validate_dap_contract(contract)
    if contract_block.get("sha256") is not None:
        checked_digest = sha256_file(contract_path)
        if contract_block.get("sha256") != checked_digest:
            raise ReceiptError(
                "receipt contract sha256 does not match the checked perl-dap contract; "
                "the receipt was not produced against this contract"
            )

    boundary = receipt.get("claim_boundary")
    if not isinstance(boundary, dict):
        raise ReceiptError("receipt.claim_boundary must be an object")
    for cell in ("actual_zed", "public_registry", "real_zed_debug_session"):
        if boundary.get(cell) != "not_proven":
            raise ReceiptError(f"perl-dap asset receipt may not claim {cell} proof")
    if result != "pass":
        return

    release = receipt.get("release")
    if (
        not isinstance(release, dict)
        or release.get("prerelease") is not False
        or release.get("draft") is not False
    ):
        raise ReceiptError("passing receipt must bind a stable public release")

    verifier = receipt.get("verifier")
    if not isinstance(verifier, dict) or not verifier.get("os") or not verifier.get("architecture"):
        raise ReceiptError("passing receipt must bind its verifier host")

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
        if row.get("result") not in {"managed_executed", "managed_extracted_not_executed"}:
            raise ReceiptError(
                "managed perl-dap target lacks an executed or extracted-not-executed result"
            )
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
        if binary.get("product") != "perl-dap":
            raise ReceiptError("receipt binary identity must be the exact perl-dap product")
        installed = archive.get("installed_path")
        if not isinstance(installed, str) or not installed.startswith(DAP_MANAGED_PREFIX):
            raise ReceiptError(
                "receipt installed path must stay inside the perl-dap-managed- boundary"
            )

        if row.get("result") == "managed_executed":
            executed += 1
            if not _verifier_matches(verifier, row):
                raise ReceiptError(
                    f"target {target_name} claims execution on a verifier it does not "
                    "match; cross-built adapters must stay extracted-not-executed"
                )
            smoke = row.get("stdio_smoke")
            if not isinstance(smoke, dict) or smoke.get("result") != "pass":
                raise ReceiptError("executed target lacks passing DAP stdio smoke")
            if smoke.get("stdout_pure") is not True or smoke.get("orphan_result") != "no_orphans":
                raise ReceiptError("executed target lacks stdout-purity or orphan proof")

    if managed == 0:
        raise ReceiptError("passing receipt contains no managed perl-dap target evidence")
    if actual_targets != expected_targets:
        raise ReceiptError(
            "passing receipt target rows do not match the checked contract target set"
        )

    cache = receipt.get("cache_recovery")
    if not isinstance(cache, dict) or cache.get("result") != "pass":
        raise ReceiptError("passing receipt lacks a passing managed-DAP cache recovery suite")

    dap_process = boundary.get("dap_process")
    if executed and dap_process != "proven_for_matching_host_only":
        raise ReceiptError("passing executed row lacks the matching-host boundary")
    if not executed and dap_process != "not_executed_on_this_verifier":
        raise ReceiptError("cross-extraction receipt overclaims host execution")
