"""Validation for the checked Zed perl-dap managed-download projection.

The perl-dap contract is deliberately a sibling of the perllsp contract, not
a generalization of it: both products share one release archive family, but
every identity, member, cache, and proof row stays product-specific. A
perllsp member, executable, or asset family can never satisfy a DAP row.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from .common import (
    ReceiptError,
    parse_digest,
    validate_relative_member,
    validate_single_component,
)

DAP_CONTRACT_SCHEMA = "zed_perl_dap_managed_downloads.v1"
DAP_ADAPTER_ID = "perl-dap"
DAP_MANAGED_PREFIX = "perl-dap-managed-"
RELEASE_REPOSITORY = "EffortlessMetrics/perl-lsp"
RELEASE_TOPOLOGY_PATH = "docs/reference/downstream-dap-integrations.json"
PROJECTION_MANIFEST_PATH = ".ci/fixtures/zed-perl-upstream/manifest.toml"

KNOWN_SHARED_BINARIES = {"perllsp", "perllsp.exe", "perl-dap", "perl-dap.exe"}

PRESERVE_KNOWN_GOOD_ON = [
    "missing_asset",
    "duplicate_asset",
    "wrong_target_asset",
    "wrong_product_member",
    "digest_mismatch",
    "member_digest_mismatch",
    "unsafe_archive_path",
    "duplicate_member",
    "foreign_executable_member",
    "missing_member",
    "partial_download",
    "extraction_failure",
    "launch_failure",
    "protocol_impurity",
    "promote_failure",
]


def expected_member(version: str, target: str, archive_type: str) -> str:
    """The exact `perl-dap` member inside a canonical shared release archive.

    Every captured public archive — including the Windows zips — carries the
    binaries inside the `perllsp-{version}-{triple}` directory. A root-level
    member is therefore always a wrong projection, never a layout variant.
    """
    suffix = ".exe" if archive_type == "zip" else ""
    return f"perllsp-{version}-{target}/perl-dap{suffix}"


def expected_installed_path(version: str, target: str, archive_type: str) -> str:
    return f"{DAP_MANAGED_PREFIX}{version}-{target}/{expected_member(version, target, archive_type)}"


def _validate_identity(identity: dict[str, Any]) -> None:
    if identity.get("adapter_id") != DAP_ADAPTER_ID:
        raise ReceiptError("contract.identity.adapter_id must be the exact perl-dap adapter id")
    if identity.get("executable") != DAP_ADAPTER_ID:
        raise ReceiptError(
            "managed DAP assets must retain the exact perl-dap executable identity; "
            f"got {identity.get('executable')!r}"
        )
    if identity.get("transport_arguments") != ["--stdio"]:
        raise ReceiptError("the DAP smoke must run the exact ['--stdio'] transport")
    if identity.get("release_asset_prefix") != "perllsp-":
        raise ReceiptError(
            "perl-dap must consume the shared perllsp- release asset family; "
            "a private perl-dap- asset family is not a second release authority"
        )
    if identity.get("managed_cache_prefix") != DAP_MANAGED_PREFIX:
        raise ReceiptError("the debugger-specific managed cache boundary drifted")
    never = set(identity.get("never_satisfies_rows") or [])
    for forbidden in ("perllsp", "perl-lsp", "perlnavigator-server"):
        if forbidden not in never:
            raise ReceiptError(f"contract must record that {forbidden!r} can never satisfy a DAP row")


def _validate_source(source: dict[str, Any]) -> None:
    if source.get("repository") != RELEASE_REPOSITORY:
        raise ReceiptError("unexpected DAP release repository")
    if source.get("prerelease") is not False:
        raise ReceiptError("the managed DAP route cannot target a prerelease")
    version = source.get("version")
    if not isinstance(version, str) or not version:
        raise ReceiptError("contract source version is missing")
    validate_single_component(version, "contract source version")
    if not isinstance(source.get("release_id"), int) or source["release_id"] <= 0:
        raise ReceiptError("contract source lacks a positive release id")
    if not isinstance(source.get("tag"), str) or not version in str(source["tag"]):
        raise ReceiptError("contract source tag does not carry the recorded version")
    if source.get("release_topology") != RELEASE_TOPOLOGY_PATH:
        raise ReceiptError("contract must bind the canonical release topology path")
    if source.get("consolidated_checksums_asset") != "SHA256SUMS":
        raise ReceiptError("contract must name the release consolidated checksums authority")
    producer = source.get("producer")
    if not isinstance(producer, str) or not producer:
        raise ReceiptError("contract source lacks the release producer identity")


def _validate_bindings(bindings: dict[str, Any], repo_root: Path | None) -> None:
    from .common import sha256_file

    topology = bindings.get("topology")
    if not isinstance(topology, dict) or topology.get("path") != RELEASE_TOPOLOGY_PATH:
        raise ReceiptError("contract.bindings.topology must bind the canonical release topology")
    projection = bindings.get("zed_adapter_projection")
    if not isinstance(projection, dict) or projection.get("path") != PROJECTION_MANIFEST_PATH:
        raise ReceiptError("contract must bind the #9485 zed adapter projection manifest")
    if projection.get("debug_adapter_id") != DAP_ADAPTER_ID:
        raise ReceiptError("projection binding must name the perl-dap adapter id")
    if projection.get("debug_binary") != DAP_ADAPTER_ID:
        raise ReceiptError("projection binding must name the perl-dap debug binary")

    # The binding digests are structural identity, not optional context: they
    # must be exact sha256 values even when the caller has no repository tree
    # to recompute them against.
    for key in ("topology", "zed_adapter_projection"):
        parse_digest(bindings[key].get("sha256"), f"contract bindings {key} sha256")

    if repo_root is None:
        return
    for key, path in (
        ("topology", topology.get("path")),
        ("zed_adapter_projection", projection.get("path")),
    ):
        recorded = bindings[key].get("sha256")
        subject = repo_root / str(path)
        if not subject.is_file():
            raise ReceiptError(
                f"contract binding {key} subject {path} is absent from this tree; "
                "the projection must be re-captured against the current authority"
            )
        actual = sha256_file(subject)
        if actual != recorded:
            raise ReceiptError(
                f"contract binding {key} drifted from the current {path}: "
                f"recorded {recorded}, actual {actual}; the receipt matrix must be re-derived"
            )


def _validate_divergence(contract: dict[str, Any], windows_member: str) -> None:
    divergence = contract.get("projection_divergence")
    if not isinstance(divergence, dict):
        raise ReceiptError(
            "contract lacks the recorded #9485 Windows member divergence; a silent gap "
            "between the adapter projection and the actual public archive is not accepted"
        )
    if divergence.get("target") != "x86_64-pc-windows-msvc":
        raise ReceiptError("the recorded projection divergence must name the Windows x86_64 target")
    if divergence.get("zed_adapter_projection_member") != "perl-dap.exe":
        raise ReceiptError(
            "the recorded divergence must restate the #9485 root-level Windows member"
        )
    if divergence.get("actual_public_archive_member") != windows_member:
        raise ReceiptError(
            "the recorded divergence actual member must equal the Windows target archive_member"
        )
    note = divergence.get("note")
    if not isinstance(note, str) or len(note) < 40:
        raise ReceiptError("the recorded divergence must carry an explanatory note")


def _validate_row(row: dict[str, Any], index: int, version: str) -> None:
    target = row.get("target")
    disposition = row.get("disposition")
    if not isinstance(target, str) or not target:
        raise ReceiptError(f"target row {index} lacks target")
    validate_single_component(target, f"target row {index} target")

    # The os/architecture projection is load-bearing for the matching-host
    # proof, so every row must carry exact canonical values — a missing or
    # free-form value can never authorize an execution claim later.
    if row.get("os") not in {"linux", "macos", "windows"}:
        raise ReceiptError(f"target row {index} lacks a canonical os projection")
    if row.get("architecture") not in {"x86_64", "aarch64"}:
        raise ReceiptError(f"target row {index} lacks a canonical architecture projection")

    if disposition != "managed":
        if disposition not in {"path_only", "deferred", "unsupported"}:
            raise ReceiptError(f"target {target} has unknown disposition {disposition!r}")
        reason = row.get("reason")
        if not isinstance(reason, str) or not reason.strip():
            raise ReceiptError(f"non-managed target {target} must record a reason")
        return

    archive_type = row.get("archive_type")
    if archive_type not in {"tar.gz", "zip"}:
        raise ReceiptError(f"target {target} has unsupported archive type")
    validate_single_component(row.get("asset_name"), f"target {target} asset_name")
    suffix = ".tar.gz" if archive_type == "tar.gz" else ".zip"
    if row.get("asset_name") != f"perllsp-{version}-{target}{suffix}":
        raise ReceiptError(
            f"target {target} asset name does not match the shared perllsp- release family"
        )
    if not isinstance(row.get("asset_id"), int) or row["asset_id"] <= 0:
        raise ReceiptError(f"target {target} lacks a positive asset id")
    if not isinstance(row.get("asset_size"), int) or row["asset_size"] <= 0:
        raise ReceiptError(f"target {target} lacks a positive asset size")
    parse_digest(row.get("asset_digest"), f"target {target} asset_digest")
    parse_digest(row.get("member_sha256"), f"target {target} member_sha256")

    member = row.get("archive_member")
    installed = row.get("installed_path")
    if not isinstance(member, str) or not isinstance(installed, str):
        raise ReceiptError(f"target {target} lacks archive/install path")
    validate_relative_member(member)
    validate_relative_member(installed)
    if "perl-lsp" in member or "perl-lsp" in installed:
        raise ReceiptError(f"target {target} references the retired perl-lsp executable")

    member_basename = member.rsplit("/", 1)[-1].lower()
    if member_basename in {"perllsp", "perllsp.exe"}:
        raise ReceiptError(
            f"target {target} archive member names the perllsp language-server product; "
            "a perllsp member can never satisfy the perl-dap row"
        )
    if member_basename not in {"perl-dap", "perl-dap.exe"}:
        raise ReceiptError(f"target {target} archive member does not name the perl-dap product")
    if member != expected_member(version, target, archive_type):
        raise ReceiptError(
            f"target {target} member must be the canonical nested perl-dap member "
            f"{expected_member(version, target, archive_type)!r}; a root-level member "
            "does not match any captured public archive layout"
        )
    if installed != expected_installed_path(version, target, archive_type):
        raise ReceiptError(
            f"target {target} installed path must stay inside the {DAP_MANAGED_PREFIX} "
            "debugger-specific cache boundary"
        )
    if not isinstance(row.get("make_executable"), bool):
        raise ReceiptError(f"target {target} lacks the make_executable flag")


def _validate_cache_contract(cache: dict[str, Any]) -> None:
    if cache.get("version_directory") != "perl-dap-managed-{version}-{target}":
        raise ReceiptError("cache contract version directory drifted from the DAP managed prefix")
    preserve = list(cache.get("preserve_known_good_on") or [])
    for scenario in PRESERVE_KNOWN_GOOD_ON:
        if scenario not in preserve:
            raise ReceiptError(
                f"cache contract must preserve known-good state on {scenario!r}"
            )
    cleanup = cache.get("cleanup_scope")
    if not isinstance(cleanup, str) or DAP_MANAGED_PREFIX not in cleanup:
        raise ReceiptError("cache contract cleanup scope must stay inside the DAP managed prefix")
    staging = cache.get("staging_is_private")
    if not isinstance(staging, str) or ".tmp" not in staging:
        raise ReceiptError("cache contract must require private staging before promotion")


def validate_dap_contract(
    contract: dict[str, Any], repo_root: Path | None = None
) -> None:
    if contract.get("schema_version") != DAP_CONTRACT_SCHEMA:
        raise ReceiptError("unexpected Zed perl-dap managed-download contract schema")

    identity = contract.get("identity")
    if not isinstance(identity, dict):
        raise ReceiptError("contract.identity must be an object")
    _validate_identity(identity)

    source = contract.get("source")
    if not isinstance(source, dict):
        raise ReceiptError("contract.source must be an object")
    _validate_source(source)

    bindings = contract.get("bindings")
    if not isinstance(bindings, dict):
        raise ReceiptError("contract.bindings must be an object")
    _validate_bindings(bindings, repo_root)

    rows = contract.get("targets")
    if not isinstance(rows, list) or not rows:
        raise ReceiptError("contract.targets must be a non-empty array")
    version = source["version"]
    seen: set[str] = set()
    managed = 0
    windows_arm64_unsupported = False
    windows_member = ""
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            raise ReceiptError(f"target row {index} must be an object")
        target = row.get("target")
        if target in seen:
            raise ReceiptError(f"duplicate target row: {target}")
        seen.add(str(target))
        if target == "aarch64-pc-windows-msvc":
            windows_arm64_unsupported = row.get("disposition") == "unsupported"
        if row.get("disposition") == "managed":
            managed += 1
            if target == "x86_64-pc-windows-msvc":
                member = row.get("archive_member")
                windows_member = member if isinstance(member, str) else ""
        _validate_row(row, index, version)

    if managed == 0:
        raise ReceiptError("contract contains no managed DAP target")
    if not windows_arm64_unsupported:
        raise ReceiptError("Windows ARM64 must remain explicitly unsupported")
    _validate_divergence(contract, windows_member)

    cache = contract.get("cache_contract")
    if not isinstance(cache, dict):
        raise ReceiptError("contract.cache_contract must be an object")
    _validate_cache_contract(cache)

    boundary = contract.get("claim_boundary")
    if not isinstance(boundary, dict):
        raise ReceiptError("contract.claim_boundary must be an object")
    if boundary.get("public_asset_metadata") != "captured":
        raise ReceiptError("static contract must record captured asset metadata only")
    for cell in (
        "archive_extraction",
        "perl_dap_version_execution",
        "dap_stdio_lifecycle",
        "cache_recovery",
        "real_zed_debug_session",
        "actual_zed_host",
    ):
        if boundary.get(cell) != "not_proven":
            raise ReceiptError(f"static contract overclaims {cell}")
