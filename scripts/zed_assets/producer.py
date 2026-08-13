"""Execute the checked Zed public asset matrix and write one receipt."""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path
from typing import Any

from .archive import extract_expected
from .common import RECEIPT_SCHEMA, ReceiptError, expected_host, now_utc, sha256_file, verifier_identity, write_receipt
from .contract import validate_contract
from .github_io import asset_index, download_asset, fetch_json, release_version
from .process import run_stdio_smoke


def base_receipt(contract_path: Path, contract: dict[str, Any]) -> dict[str, Any]:
    source = contract["source"]
    return {
        "schema_version": RECEIPT_SCHEMA,
        "result": "not_run",
        "observed_at": None,
        "contract": {
            "relative_path": str(contract_path).replace("\\", "/"),
            "sha256": sha256_file(contract_path),
            "schema_version": contract["schema_version"],
        },
        "release": {
            "repository": source["repository"],
            "id": None,
            "tag": None,
            "version": None,
            "prerelease": None,
            "draft": None,
            "published_at": None,
        },
        "verifier": verifier_identity(),
        "targets": [],
        "limitations": [],
        "claim_boundary": {
            "asset_bytes": "not_run",
            "archive_layout": "not_run",
            "host_process": "not_run",
            "actual_zed": "not_proven",
            "public_registry": "not_proven",
        },
    }


def _new_target(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "target": row["target"],
        "os": row["os"],
        "architecture": row["architecture"],
        "disposition": row["disposition"],
        "result": "not_run",
        "asset": None,
        "archive": None,
        "binary": None,
        "stdio_smoke": None,
        "errors": [],
    }


def _execute_target(
    row: dict[str, Any],
    by_id: dict[int, dict[str, Any]],
    by_name: dict[str, list[dict[str, Any]]],
    verifier: dict[str, str],
    work_dir: Path,
    token: str | None,
    target_result: dict[str, Any],
) -> None:
    name = row["asset_name"]
    matches = by_name.get(name, [])
    if len(matches) != 1:
        raise ReceiptError(f"expected exactly one release asset named {name!r}, found {len(matches)}")
    asset = matches[0]
    if by_id.get(row["asset_id"]) is not asset:
        raise ReceiptError(f"asset id/name identity mismatch for {name}")
    if asset.get("size") != row["asset_size"]:
        raise ReceiptError(f"asset size drift for {name}")
    api_digest = asset.get("digest")
    if api_digest is not None and api_digest != row["asset_digest"]:
        raise ReceiptError(f"GitHub immutable digest drift for {name}")
    download_url = asset.get("url")
    if not isinstance(download_url, str):
        raise ReceiptError(f"asset {name} lacks API download URL")

    target_dir = work_dir / row["target"]
    shutil.rmtree(target_dir, ignore_errors=True)
    target_dir.mkdir(parents=True)
    archive_path = target_dir / name
    download_asset(download_url, archive_path, token)
    actual_size = archive_path.stat().st_size
    actual_digest = sha256_file(archive_path)
    if actual_size != row["asset_size"]:
        raise ReceiptError(f"downloaded asset size mismatch for {name}")
    if actual_digest != row["asset_digest"]:
        raise ReceiptError(f"downloaded asset digest mismatch for {name}")

    binary, members_digest = extract_expected(
        archive_path,
        row["archive_type"],
        row["archive_member"],
        target_dir / "installed",
        bool(row["make_executable"]),
    )
    target_result["asset"] = {
        "id": row["asset_id"],
        "name": name,
        "url": asset.get("browser_download_url"),
        "size": actual_size,
        "sha256": actual_digest,
        "archive_type": row["archive_type"],
    }
    target_result["archive"] = {
        "members_sha256": members_digest,
        "required_member": row["archive_member"],
        "installed_name": binary.name,
        "safe": True,
    }
    target_result["binary"] = {
        "name": binary.name,
        "sha256": sha256_file(binary),
        "executable": os.access(binary, os.X_OK) if os.name != "nt" else True,
    }
    if expected_host(row, verifier):
        target_result["stdio_smoke"] = run_stdio_smoke(binary.resolve(), target_dir)
        target_result["result"] = "managed_executed"
    else:
        target_result["stdio_smoke"] = {"result": "not_executed"}
        target_result["result"] = "managed_extracted_not_executed"


def execute(
    contract_path: Path,
    contract: dict[str, Any],
    output_path: Path,
    work_dir: Path,
    token: str | None,
) -> int:
    validate_contract(contract)
    receipt = base_receipt(contract_path, contract)
    receipt["observed_at"] = now_utc()
    work_dir.mkdir(parents=True, exist_ok=True)

    repository = contract["source"]["repository"]
    latest_url = f"https://api.github.com/repos/{repository}/releases/latest"
    try:
        release = fetch_json(latest_url, token)
        if release.get("draft") is not False or release.get("prerelease") is not False:
            raise ReceiptError("GitHub latest release is draft or prerelease")
        tag = release.get("tag_name")
        if not isinstance(tag, str) or not tag:
            raise ReceiptError("latest release lacks tag_name")
        receipt["release"] = {
            "repository": repository,
            "id": release.get("id"),
            "tag": tag,
            "version": release_version(tag),
            "prerelease": release.get("prerelease"),
            "draft": release.get("draft"),
            "published_at": release.get("published_at"),
        }

        source = contract["source"]
        if (
            release.get("id") != source.get("release_id")
            or tag != source.get("tag")
            or release_version(tag) != source.get("version")
        ):
            receipt["result"] = "contract_stale"
            receipt["limitations"].append(
                "The current stable release no longer matches the checked managed-download projection."
            )
            return write_receipt(output_path, receipt, exit_code=3)

        by_id, by_name = asset_index(release)
        verifier = receipt["verifier"]
        any_failed = False
        for row in contract["targets"]:
            target_result = _new_target(row)
            receipt["targets"].append(target_result)
            if row["disposition"] != "managed":
                target_result["result"] = row["disposition"]
                target_result["errors"].append(row.get("reason", "not managed"))
                continue
            try:
                _execute_target(row, by_id, by_name, verifier, work_dir, token, target_result)
            except ReceiptError as error:
                any_failed = True
                target_result["result"] = "fail"
                target_result["errors"].append(str(error))

        if any_failed:
            receipt["result"] = "fail"
            receipt["limitations"].append(
                "One or more managed target rows failed; passing rows remain bounded to their exact subjects."
            )
            return write_receipt(output_path, receipt, exit_code=1)

        receipt["result"] = "pass"
        receipt["claim_boundary"]["asset_bytes"] = "proven"
        receipt["claim_boundary"]["archive_layout"] = "proven"
        receipt["claim_boundary"]["host_process"] = (
            "proven_for_matching_host_only"
            if any(row["result"] == "managed_executed" for row in receipt["targets"])
            else "not_executed_on_this_verifier"
        )
        return write_receipt(output_path, receipt, exit_code=0)
    except ReceiptError as error:
        receipt["result"] = "fail"
        receipt["limitations"].append(str(error))
        return write_receipt(output_path, receipt, exit_code=1)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        receipt["result"] = "instrument_failed"
        receipt["limitations"].append(f"receipt instrument failed: {error}")
        return write_receipt(output_path, receipt, exit_code=2)
