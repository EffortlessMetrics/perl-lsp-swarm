"""Execute the checked Zed perl-dap public asset matrix and write one receipt.

The producer is read-only against GitHub: it resolves the current stable
release, downloads the exact shared archives the contract binds, verifies
every byte against two independent authorities (the checked contract and the
release's consolidated SHA256SUMS asset, plus each archive's in-member sums
manifest when present), extracts exactly the `perl-dap` member, executes the
DAP process boundary on matching hosts, runs the offline known-good cache
recovery suite, and writes one fail-closed receipt.
"""

from __future__ import annotations

import os
import shutil
from pathlib import Path
from typing import Any

from .common import (
    ReceiptError,
    expected_host,
    now_utc,
    sha256_file,
    verifier_identity,
    write_receipt,
)
from .dap_archive import extract_expected_member, verify_member_against_archive_sums
from .dap_cache import run_recovery_scenarios
from .dap_contract import validate_dap_contract
from .dap_process import run_dap_stdio_smoke
from .github_io import asset_index, download_asset, fetch_json, release_version

DAP_RECEIPT_SCHEMA = "zed_perl_dap_asset_receipt.v1"


def base_receipt(contract_path: Path, contract: dict[str, Any]) -> dict[str, Any]:
    source = contract["source"]
    return {
        "schema_version": DAP_RECEIPT_SCHEMA,
        "stage": "public_perl_dap_asset",
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
            "producer": None,
        },
        "topology": {
            "subject": source["release_topology"],
            "sha256": contract["bindings"]["topology"]["sha256"],
        },
        "projection": {
            "zed_adapter_subject": contract["bindings"]["zed_adapter_projection"]["path"],
            "sha256": contract["bindings"]["zed_adapter_projection"]["sha256"],
            "debug_adapter_id": contract["bindings"]["zed_adapter_projection"]["debug_adapter_id"],
        },
        "verifier": verifier_identity(),
        "targets": [],
        "cache_recovery": None,
        "limitations": [],
        "claim_boundary": {
            "asset_bytes": "not_run",
            "archive_layout": "not_run",
            "dap_process": "not_run",
            "cache_recovery": "not_run",
            "real_zed_debug_session": "not_proven",
            "actual_zed": "not_proven",
            "public_registry": "not_proven",
        },
        "currentness": {
            "invalidators": [
                "release id/tag/version change",
                "topology or projection binding digest change",
                "contract sha256 change",
                "verifier os/architecture change",
            ],
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


def _verify_consolidated_sums(
    release: dict[str, Any],
    contract: dict[str, Any],
    work_dir: Path,
    token: str | None,
) -> dict[str, str]:
    """Verify every managed asset digest against the release SHA256SUMS asset.

    The consolidated checksums file is a second, release-published authority
    independent of the checked contract. A disagreement fails the matrix
    before any archive is trusted.
    """
    sums_name = contract["source"].get("consolidated_checksums_asset")
    _, by_name = asset_index(release)
    matches = by_name.get(str(sums_name), [])
    if len(matches) != 1:
        raise ReceiptError(
            f"release lacks exactly one consolidated checksums asset named {sums_name!r}"
        )
    destination = work_dir / str(sums_name)
    download_asset(str(matches[0].get("url")), destination, token)
    sums: dict[str, str] = {}
    for line in destination.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        digest, _, name = line.replace("*", " ").partition(" ")
        if not name.strip():
            raise ReceiptError(f"consolidated checksums line is malformed: {line!r}")
        sums[name.strip()] = f"sha256:{digest.strip()}"
    for row in contract["targets"]:
        if row.get("disposition") != "managed":
            continue
        recorded = sums.get(str(row["asset_name"]))
        if recorded is None:
            raise ReceiptError(
                f"consolidated checksums asset does not list {row['asset_name']!r}"
            )
        if recorded != row["asset_digest"]:
            raise ReceiptError(
                "consolidated checksums disagree with the checked contract for "
                f"{row['asset_name']!r}: {recorded} != {row['asset_digest']}"
            )
    return sums


def _execute_target(
    row: dict[str, Any],
    by_id: dict[int, dict[str, Any]],
    by_name: dict[str, list[dict[str, Any]]],
    verifier: dict[str, str],
    work_dir: Path,
    token: str | None,
    expected_version: str,
    target_result: dict[str, Any],
) -> None:
    name = row["asset_name"]
    matches = by_name.get(name, [])
    if len(matches) != 1:
        raise ReceiptError(
            f"expected exactly one release asset named {name!r}, found {len(matches)}"
        )
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

    binary, members_digest, sums = extract_expected_member(
        archive_path,
        row["archive_type"],
        row["archive_member"],
        target_dir / "installed",
        bool(row["make_executable"]),
    )
    archive_sums_recorded = verify_member_against_archive_sums(
        sums, binary, row["member_sha256"]
    )
    binary_digest = sha256_file(binary)
    if binary_digest != row["member_sha256"]:
        raise ReceiptError(
            f"extracted perl-dap member digest mismatch for {name}: "
            f"{binary_digest} != {row['member_sha256']}"
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
        "installed_path": row["installed_path"],
        "installed_name": binary.name,
        "safe": True,
    }
    target_result["binary"] = {
        "product": "perl-dap",
        "name": binary.name,
        "sha256": binary_digest,
        "archive_sums_sha256": archive_sums_recorded,
        # Observed only where the verifier applies POSIX mode semantics; a
        # Windows verifier never fabricates an executable-bit observation.
        "executable": os.access(binary, os.X_OK) if os.name != "nt" else None,
    }
    if expected_host(row, verifier):
        target_result["stdio_smoke"] = run_dap_stdio_smoke(
            binary.resolve(), target_dir, expected_version
        )
        target_result["result"] = "managed_executed"
    else:
        target_result["stdio_smoke"] = {"result": "not_executed"}
        target_result["result"] = "managed_extracted_not_executed"


def execute_dap(
    contract_path: Path,
    contract: dict[str, Any],
    output_path: Path,
    work_dir: Path,
    token: str | None,
    repo_root: Path | None = None,
) -> int:
    validate_dap_contract(contract, repo_root)
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
        author = release.get("author") or {}
        receipt["release"] = {
            "repository": repository,
            "id": release.get("id"),
            "tag": tag,
            "version": release_version(tag),
            "prerelease": release.get("prerelease"),
            "draft": release.get("draft"),
            "published_at": release.get("published_at"),
            "producer": author.get("login"),
        }

        source = contract["source"]
        if (
            release.get("id") != source.get("release_id")
            or tag != source.get("tag")
            or release_version(tag) != source.get("version")
        ):
            receipt["result"] = "contract_stale"
            receipt["limitations"].append(
                "The current stable release no longer matches the checked perl-dap "
                "managed-download projection."
            )
            return write_receipt(output_path, receipt, exit_code=3)
        if receipt["release"]["producer"] != source.get("producer"):
            receipt["limitations"].append(
                f"release producer drifted: contract {source.get('producer')!r}, "
                f"live {receipt['release']['producer']!r}"
            )

        by_id, by_name = asset_index(release)
        _verify_consolidated_sums(release, contract, work_dir, token)
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
                _execute_target(
                    row,
                    by_id,
                    by_name,
                    verifier,
                    work_dir,
                    token,
                    source["version"],
                    target_result,
                )
            except ReceiptError as error:
                any_failed = True
                target_result["result"] = "fail"
                target_result["errors"].append(str(error))

        receipt["cache_recovery"] = run_recovery_scenarios(work_dir)

        if any_failed:
            receipt["result"] = "fail"
            receipt["limitations"].append(
                "One or more managed perl-dap target rows failed; passing rows remain "
                "bounded to their exact subjects."
            )
            return write_receipt(output_path, receipt, exit_code=1)
        if receipt["cache_recovery"]["result"] != "pass":
            receipt["result"] = "fail"
            receipt["limitations"].append(
                "The managed-DAP cache recovery suite failed; known-good preservation "
                "is not proven."
            )
            return write_receipt(output_path, receipt, exit_code=1)

        receipt["result"] = "pass"
        receipt["claim_boundary"]["asset_bytes"] = "proven"
        receipt["claim_boundary"]["archive_layout"] = "proven"
        receipt["claim_boundary"]["dap_process"] = (
            "proven_for_matching_host_only"
            if any(row["result"] == "managed_executed" for row in receipt["targets"])
            else "not_executed_on_this_verifier"
        )
        # The cache-recovery suite proves the isolated managed-DAP cache model
        # the receipt lane owns; the production Zed downloader in the extension
        # fixture is a different implementation surface owned by #9485.
        receipt["claim_boundary"]["cache_recovery"] = "proven_isolated_cache_model_only"
        receipt["cache_recovery"]["limitations"].append(
            "The production Zed extension downloader is not exercised by this "
            "suite; the fixture route is owned by #9485 and remains unproven here."
        )
        return write_receipt(output_path, receipt, exit_code=0)
    except ReceiptError as error:
        receipt["result"] = "fail"
        receipt["limitations"].append(str(error))
        return write_receipt(output_path, receipt, exit_code=1)
    except (OSError, ValueError) as error:
        receipt["result"] = "instrument_failed"
        receipt["limitations"].append(f"receipt instrument failed: {error}")
        return write_receipt(output_path, receipt, exit_code=2)
