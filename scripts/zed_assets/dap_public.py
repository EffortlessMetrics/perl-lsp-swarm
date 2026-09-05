"""Fail-closed validation for the official-registry Zed perl-dap journey receipt.

This is the D05 (#9487) consumer surface. It binds the checked #9516
managed-download contract and the committed aggregate asset receipt instead of
re-deriving public asset selection, and it binds the DU01 registry acceptance
manifest (`.ci/fixtures/zed-perl-upstream/registry/manifest.toml`) as the only
authority for a merged-and-released official registry subject.

Separation rules mirrors the asset stage:

- an exact-source dev-extension receipt can never satisfy this stage;
- a #9516 public asset receipt can never satisfy this stage (bytes/process are
  not real Zed debugger behavior);
- a clean official-registry profile with no prior managed perl-dap cache, no
  PATH candidate, and no explicit override is required for a pass;
- the stopped frame must be proven against the workspace root, so a wrong-root
  same-basename source mapping cannot pass;
- adapter exit must leave no debuggee or adapter orphan, and a restart must
  reuse the exact verified known-good managed subject.
"""

from __future__ import annotations

import datetime as dt
import json
import tomllib
from pathlib import Path
from typing import Any

from .common import ReceiptError, parse_digest, sha256_file
from .dap_contract import DAP_ADAPTER_ID, DAP_MANAGED_PREFIX, validate_dap_contract
from .dap_validation import validate_dap_receipt

PUBLIC_RECEIPT_SCHEMA = "zed_perl_dap_public_registry_receipt.v1"
PUBLIC_STAGE = "public_registry_install"
ASSET_STAGE = "public_perl_dap_asset"
EXACT_SOURCE_STAGE = "exact_source_dev_extension"

# The committed #9486 exact-source receipt family: only a file carrying this
# schema and evidence stage counts as exact-source DAP evidence, so a
# pass-shaped JSON file from any other stage caught by the receipts glob can
# never satisfy the D02 gate.
EXACT_SOURCE_RECEIPT_SCHEMA = "zed_host_compat.v1"
EXACT_SOURCE_EVIDENCE_STAGE = "exact_source_dev_extension"

CONTRACT_RELATIVE_PATH = ".ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json"
REGISTRY_MANIFEST_RELATIVE_PATH = ".ci/fixtures/zed-perl-upstream/registry/manifest.toml"

PUBLIC_RESULTS = {
    "not_run",
    "blocked_external",
    "pass",
    "fail",
    "not_proven",
    "instrument_failed",
    "contract_stale",
}

# The bounded public journey from the #9487 spec: prove the host, install,
# resolve, launch, stop, inspect, step, terminate, clean up, and restart.
JOURNEY_CELLS = [
    "zed_host_identity",
    "fixture_open",
    "adapter_selection",
    "managed_resolution",
    "adapter_process_identity",
    "initialize_launch",
    "breakpoint_verified",
    "stopped_event",
    "frame_source_identity",
    "scopes_variables",
    "continue_step",
    "terminate_disconnect",
    "cleanup_bounded",
    "restart_reuse",
]

# Clean public-install preconditions: every cell must be true for a pass.
PROFILE_CELLS = [
    "development_extension_absent",
    "fork_or_copied_extension_absent",
    "prior_public_extension_state_absent",
    "prior_managed_perl_dap_cache_absent",
    "explicit_debugger_binary_override_absent",
    "path_candidate_satisfying_managed_row_absent",
    "other_perl_debugger_selected_absent",
    "relabeled_receipt_absent",
]

# Entry-gate accounting retained with the receipt. `matching_host_asset_receipt`
# is live-bound to the checked #9516 surfaces; the registry gates are
# live-bound to the DU01 acceptance manifest; the exact-source gate is
# live-bound to the committed exact-source receipts directory.
GATE_CELLS = [
    "released_zed_build",
    "official_registry_entry",
    "extension_upstream_release",
    "matching_host_asset_receipt",
    "exact_source_zed_dap_receipt",
    "routing_final_check_authority",
]

GATE_STATES = {"absent", "stale", "current"}

CELL_RESULTS = {"pass", "fail", "not_proven", "unsupported", "instrument_failed"}

ADAPTER_BINARY_ROUTES = {"managed_public_artifact", "path_override", "worktree_candidate"}


def load_registry_manifest(path: Path) -> dict[str, Any]:
    """Load the DU01 registry acceptance manifest."""
    with path.open("rb") as handle:
        manifest = tomllib.load(handle)
    if not isinstance(manifest, dict):
        raise ReceiptError(f"{path} must contain a TOML table")
    return manifest


def registry_subject(manifest: dict[str, Any]) -> dict[str, Any]:
    """Evaluate the DU01 merged-and-released acceptance predicate.

    The predicate is the one the train contract names: a changed upstream
    commit/version on a reachable branch, contained by a named released Zed
    build, with manifest-version equality. It is evaluated over the acceptance
    manifest only — submission or merge metadata alone never satisfies it.
    """
    extension = manifest.get("extension")
    validation = manifest.get("validation")
    zed_defaults = manifest.get("zed_defaults")
    registry = manifest.get("registry")
    if not isinstance(extension, dict) or not isinstance(validation, dict):
        raise ReceiptError("registry acceptance manifest lacks its tables")
    if not isinstance(zed_defaults, dict) or not isinstance(registry, dict):
        raise ReceiptError("registry acceptance manifest lacks its tables")

    new_commit = str(extension.get("new_commit") or "")
    new_version = str(extension.get("new_version") or "")
    branch = str(extension.get("upstream_branch_containing_commit") or "")
    released_build = str(zed_defaults.get("released_build") or "")
    upstream_released = bool(
        new_commit
        and new_commit != str(extension.get("current_commit") or "")
        and new_version
        and new_version != str(extension.get("current_version") or "")
        and branch
        and released_build
        and validation.get("submodule_commit_branch_reachable") is True
        and validation.get("manifest_version_matches") is True
        and validation.get("released_build_contains_commit") is True
    )
    return {
        "accepted": upstream_released,
        "repository": str(registry.get("repository") or ""),
        "entry": str(extension.get("id") or ""),
        "submodule_path": str(extension.get("submodule_path") or ""),
        "extension_commit": new_commit,
        "extension_version": new_version,
        "upstream_branch": branch,
        "released_build": released_build,
    }


def exact_source_receipt_current(receipts_dir: Path) -> bool:
    """Whether a committed exact-source Zed DAP receipt currently records a pass.

    Only a genuine #9486 exact-source observation counts: the file must carry
    the exact-source receipt schema and evidence stage and a `pass` result. An
    unrelated LSP host receipt or a malformed pass-shaped JSON file caught by
    the same glob never satisfies the D02 gate.
    """
    if not receipts_dir.is_dir():
        return False
    for path in sorted(receipts_dir.glob("exact-source*.json")):
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if (
            isinstance(value, dict)
            and value.get("schema_version") == EXACT_SOURCE_RECEIPT_SCHEMA
            and value.get("evidence_stage") == EXACT_SOURCE_EVIDENCE_STAGE
            and value.get("result") == "pass"
        ):
            return True
    return False


def _object(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ReceiptError(f"{context} must be an object")
    return value


def _nonempty(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ReceiptError(f"{context} must be a non-empty string")
    return value


def _require_gate(gates: dict[str, Any], cell: str, expected: str, reason: str) -> None:
    if gates.get(cell) != expected:
        raise ReceiptError(reason)


def _selected_row(contract: dict[str, Any], target: str) -> dict[str, Any]:
    for row in contract.get("targets", []):
        if isinstance(row, dict) and row.get("target") == target:
            return row
    raise ReceiptError(f"selected target {target!r} is absent from the checked contract")


def _aggregate_row(asset_receipt: dict[str, Any], target: str) -> dict[str, Any]:
    for row in asset_receipt.get("targets", []):
        if isinstance(row, dict) and row.get("target") == target:
            return row
    raise ReceiptError(
        f"selected target {target!r} is absent from the bound #9516 aggregate receipt"
    )


def validate_dap_public_receipt(
    receipt: dict[str, Any],
    contract_path: Path,
    contract: dict[str, Any],
    asset_receipt_path: Path,
    asset_receipt: dict[str, Any],
    manifest_path: Path,
    manifest: dict[str, Any],
    receipts_dir: Path | None = None,
) -> None:
    """Validate one official-registry perl-dap journey receipt.

    Every retained receipt (passing or blocked) is bound to the exact checked
    #9516 contract and aggregate receipt bytes, so drift on either surface
    invalidates this receipt offline. Passing receipts are additionally bound
    to an accepted DU01 registry subject, a clean official-registry profile,
    the managed public artifact route, and the complete bounded journey.
    Blocked receipts are valid only while the external subjects they name are
    actually absent.
    """
    if receipt.get("schema_version") != PUBLIC_RECEIPT_SCHEMA:
        raise ReceiptError("unexpected perl-dap public registry receipt schema")
    if receipt.get("stage") != PUBLIC_STAGE:
        raise ReceiptError(
            "perl-dap public registry receipt must name the public_registry_install stage; "
            f"{EXACT_SOURCE_STAGE!r} and {ASSET_STAGE!r} receipts cannot satisfy this stage"
        )
    if receipt.get("evidence_stage") is not None:
        raise ReceiptError(
            "public registry receipt must not carry an exact-source evidence_stage field"
        )
    result = receipt.get("result")
    if result not in PUBLIC_RESULTS:
        raise ReceiptError(f"unknown public registry receipt result {result!r}")

    journey = _object(receipt.get("journey"), "journey")
    for cell in JOURNEY_CELLS:
        entry = _object(journey.get(cell), f"journey.{cell}")
        if entry.get("result") not in CELL_RESULTS:
            raise ReceiptError(f"journey cell {cell!r} has an invalid result")
        if result != "pass" and entry.get("result") == "pass":
            raise ReceiptError(
                f"non-passing receipt cannot claim a proven journey cell {cell!r}"
            )

    _validate_asset_evidence(receipt, contract_path, contract, asset_receipt_path, asset_receipt)

    if result == "blocked_external":
        _validate_blocked_gates(receipt, manifest, receipts_dir)
        return
    if result != "pass":
        return

    _validate_pass(receipt, contract, asset_receipt, manifest, manifest_path, receipts_dir)


def _validate_asset_evidence(
    receipt: dict[str, Any],
    contract_path: Path,
    contract: dict[str, Any],
    asset_receipt_path: Path,
    asset_receipt: dict[str, Any],
) -> None:
    evidence = _object(receipt.get("asset_evidence"), "asset_evidence")

    contract_block = _object(evidence.get("contract"), "asset_evidence.contract")
    if contract_block.get("relative_path") != CONTRACT_RELATIVE_PATH:
        raise ReceiptError("asset_evidence.contract must bind the canonical checked contract")
    parse_digest(contract_block.get("sha256"), "asset_evidence.contract.sha256")
    validate_dap_contract(contract)
    if contract_block["sha256"] != sha256_file(contract_path):
        raise ReceiptError(
            "asset_evidence.contract.sha256 does not match the checked perl-dap contract; "
            "the receipt was not produced against this contract"
        )

    aggregate = _object(evidence.get("aggregate_receipt"), "asset_evidence.aggregate_receipt")
    recorded_path = str(aggregate.get("relative_path") or "")
    provided = asset_receipt_path.as_posix()
    if recorded_path != provided and recorded_path != asset_receipt_path.resolve().as_posix():
        raise ReceiptError(
            "asset_evidence.aggregate_receipt does not bind the provided #9516 receipt"
        )
    parse_digest(aggregate.get("sha256"), "asset_evidence.aggregate_receipt.sha256")
    if aggregate.get("result") != "pass":
        raise ReceiptError("the bound #9516 aggregate receipt must be a current pass")

    # Bind #9516 rather than reconstructing public asset selection: the
    # aggregate receipt is validated by the #9516 validator itself against
    # the same checked contract, then cross-bound below.
    validate_dap_receipt(asset_receipt, contract_path, contract)
    if aggregate["sha256"] != sha256_file(asset_receipt_path):
        raise ReceiptError(
            "asset_evidence.aggregate_receipt.sha256 does not match the committed "
            "#9516 receipt bytes; the binding is stale"
        )

    selected = _object(evidence.get("selected_target"), "asset_evidence.selected_target")
    target = _nonempty(selected.get("target"), "asset_evidence.selected_target.target")
    row = _selected_row(contract, target)
    if row.get("disposition") != "managed":
        raise ReceiptError(f"selected target {target!r} is not a managed contract row")
    source = contract["source"]
    if (
        selected.get("release_repository") != source.get("repository")
        or selected.get("release_id") != source.get("release_id")
        or selected.get("release_tag") != source.get("tag")
        or selected.get("release_version") != source.get("version")
        or selected.get("release_producer") != source.get("producer")
        or selected.get("os") != row.get("os")
        or selected.get("architecture") != row.get("architecture")
        or selected.get("asset_id") != row.get("asset_id")
        or selected.get("asset_name") != row.get("asset_name")
        or selected.get("asset_archive_type") != row.get("archive_type")
        or selected.get("asset_sha256") != row.get("asset_digest")
        or selected.get("archive_member") != row.get("archive_member")
        or selected.get("member_sha256") != row.get("member_sha256")
        or selected.get("installed_path") != row.get("installed_path")
    ):
        raise ReceiptError(
            "asset_evidence.selected_target does not match the exact checked contract row; "
            "public asset selection cannot be re-derived or quoted approximately"
        )

    aggregate_row = _aggregate_row(asset_receipt, target)
    aggregate_asset = _object(aggregate_row.get("asset"), "aggregate asset")
    aggregate_archive = _object(aggregate_row.get("archive"), "aggregate archive")
    aggregate_binary = _object(aggregate_row.get("binary"), "aggregate binary")
    if (
        aggregate_row.get("result") not in {"managed_executed", "managed_extracted_not_executed"}
        or aggregate_asset.get("id") != row.get("asset_id")
        or aggregate_asset.get("name") != row.get("asset_name")
        or aggregate_asset.get("sha256") != row.get("asset_digest")
        or aggregate_archive.get("required_member") != row.get("archive_member")
        or aggregate_archive.get("installed_path") != row.get("installed_path")
        or aggregate_binary.get("sha256") != row.get("member_sha256")
    ):
        raise ReceiptError(
            "asset_evidence.selected_target does not match the bound #9516 receipt row"
        )


def _validate_blocked_gates(
    receipt: dict[str, Any],
    manifest: dict[str, Any],
    receipts_dir: Path | None,
) -> None:
    subject = registry_subject(manifest)
    if subject["accepted"]:
        raise ReceiptError(
            "the registry acceptance manifest now records a merged-and-released subject; "
            "this blocked receipt is stale and the public journey must be re-attempted"
        )

    gates = _object(receipt.get("gates"), "gates")
    for cell in GATE_CELLS:
        if gates.get(cell) not in GATE_STATES:
            raise ReceiptError(f"gates.{cell} must record absent, stale, or current")

    # Gate honesty is live, not prose: the receipt may not claim a registry
    # subject the acceptance manifest does not record, may not deny the
    # current #9516 matching-host evidence it is bound to, and may not claim
    # an exact-source receipt that no committed fixture records.
    _require_gate(
        gates,
        "official_registry_entry",
        "absent",
        "gates.official_registry_entry cannot claim a subject the acceptance "
        "manifest does not record",
    )
    _require_gate(
        gates,
        "extension_upstream_release",
        "absent",
        "gates.extension_upstream_release cannot claim a merged-and-released "
        "subject the acceptance manifest does not accept",
    )
    _require_gate(
        gates,
        "matching_host_asset_receipt",
        "current",
        "the bound #9516 aggregate receipt is a current pass; the asset gate "
        "cannot deny it",
    )
    # The released-build gate is bound to the acceptance manifest, not to the
    # enum alone: while the manifest records no released build at all the gate
    # can only be `absent`, and while the manifest does not accept the subject
    # the gate can never be `current`, so a blocked receipt cannot overclaim
    # external Zed release progress the manifest denies.
    if not subject["released_build"]:
        _require_gate(
            gates,
            "released_zed_build",
            "absent",
            "gates.released_zed_build cannot claim a released build the "
            "acceptance manifest does not record",
        )
    elif gates.get("released_zed_build") == "current":
        raise ReceiptError(
            "gates.released_zed_build cannot be current while the acceptance "
            "manifest does not accept the subject"
        )
    if receipts_dir is not None:
        recorded = exact_source_receipt_current(receipts_dir)
        claimed = gates.get("exact_source_zed_dap_receipt")
        if recorded and claimed != "current":
            raise ReceiptError(
                "a committed exact-source receipt records a pass; "
                "gates.exact_source_zed_dap_receipt cannot deny it"
            )
        if not recorded and claimed == "current":
            raise ReceiptError(
                "gates.exact_source_zed_dap_receipt claims a current exact-source "
                "receipt that no committed fixture records"
            )

    blockers = gates.get("blockers")
    if not isinstance(blockers, list) or not blockers:
        raise ReceiptError("blocked receipt must name its absent external subjects")
    if not all(isinstance(item, str) and item.strip() for item in blockers):
        raise ReceiptError("gates.blockers must contain non-empty strings")


def _validate_pass(
    receipt: dict[str, Any],
    contract: dict[str, Any],
    asset_receipt: dict[str, Any],
    manifest: dict[str, Any],
    manifest_path: Path,
    receipts_dir: Path | None,
) -> None:
    subject = registry_subject(manifest)
    if not subject["accepted"]:
        raise ReceiptError(
            "public pass requires an accepted merged-and-released registry subject; "
            f"the acceptance manifest {manifest_path.name} still blocks the journey"
        )

    # The D05 entry gates are prerequisites of a pass, not decoration: a pass
    # must record every gate as `current`, and the D02 exact-source gate is
    # live-bound to the committed receipts directory exactly as a blocked
    # receipt's is, so the public journey can never outrun an absent or stale
    # prerequisite and feed downstream projection on unproven gates.
    gates = _object(receipt.get("gates"), "gates")
    for cell in GATE_CELLS:
        _require_gate(
            gates,
            cell,
            "current",
            f"a public pass requires gates.{cell} to be current; the journey "
            "cannot outrun an absent or stale entry gate",
        )
    if receipts_dir is None:
        raise ReceiptError(
            "a public pass must be validated with its exact-source receipts "
            "directory; the D02 gate cannot be verified without it"
        )
    if not exact_source_receipt_current(receipts_dir):
        raise ReceiptError(
            "a public pass requires a committed exact-source receipt recording "
            "a pass; the D02 prerequisite is not current"
        )

    registry = _object(receipt.get("registry"), "registry")
    if (
        registry.get("repository") != subject["repository"]
        or registry.get("repository") != "zed-industries/extensions"
        or registry.get("entry") != subject["entry"]
        or registry.get("entry") != "perl"
        or registry.get("submodule_path") != subject["submodule_path"]
        or registry.get("extension_commit") != subject["extension_commit"]
        or registry.get("extension_version") != subject["extension_version"]
        or registry.get("upstream_branch") != subject["upstream_branch"]
        or registry.get("released_build") != subject["released_build"]
    ):
        raise ReceiptError(
            "receipt registry identities disagree with the accepted registry subject; "
            "a dev fork, copied package, or wrong registry cannot satisfy the public stage"
        )

    zed = _object(receipt.get("zed"), "zed")
    platform = _object(receipt.get("platform"), "platform")
    if zed.get("product") != "Zed":
        raise ReceiptError("zed.product must be `Zed`")
    _nonempty(zed.get("version"), "zed.version")
    _nonempty(zed.get("channel"), "zed.channel")
    if zed.get("build") != subject["released_build"]:
        raise ReceiptError("zed.build must be the released build the subject names")
    if platform.get("os") not in {"linux", "macos", "windows"}:
        raise ReceiptError("platform.os must be canonical")
    if platform.get("architecture") not in {"x86_64", "aarch64"}:
        raise ReceiptError("platform.architecture must be canonical")

    extension = _object(receipt.get("extension"), "extension")
    if extension.get("install_route") != "official_registry":
        raise ReceiptError(
            "public pass requires extension.install_route=official_registry; a "
            "development extension cannot satisfy the official-registry stage"
        )
    if extension.get("upstream_commit") != subject["extension_commit"]:
        raise ReceiptError("extension.upstream_commit must equal the accepted subject commit")
    if extension.get("manifest_version") != subject["extension_version"]:
        raise ReceiptError("extension.manifest_version must equal the accepted subject version")
    expected_package = f"perl@{subject['extension_version']}"
    if extension.get("package_identity") != expected_package:
        raise ReceiptError(
            f"extension.package_identity must be the exact {expected_package!r} identity"
        )

    profile = _object(receipt.get("profile"), "profile")
    for cell in PROFILE_CELLS:
        if profile.get(cell) is not True:
            raise ReceiptError(
                f"clean official-registry profile precondition failed: {cell} "
                "must be observed true"
            )

    adapter = _object(receipt.get("adapter"), "adapter")
    if adapter.get("adapter_id") != DAP_ADAPTER_ID:
        raise ReceiptError(
            "adapter.adapter_id must be the exact perl-dap product; another "
            "adapter or a perllsp process cannot satisfy the route"
        )
    if adapter.get("binary_route") != "managed_public_artifact":
        raise ReceiptError(
            "public pass requires adapter.binary_route=managed_public_artifact; "
            "a PATH candidate, worktree binary, or explicit override cannot satisfy it"
        )
    selected = _object(
        receipt.get("asset_evidence", {}).get("selected_target"),
        "asset_evidence.selected_target",
    )
    expected_version_line = f"perl-dap {contract['source']['version']}"
    if adapter.get("version_output") != expected_version_line:
        raise ReceiptError(
            "adapter.version_output must be the exact canonical "
            f"{expected_version_line!r} line"
        )
    if adapter.get("binary_sha256") != selected.get("member_sha256"):
        raise ReceiptError(
            "adapter.binary_sha256 must equal the selected #9516 member digest"
        )
    process_path = _nonempty(adapter.get("process_path"), "adapter.process_path")
    normalized = process_path.replace("\\", "/")
    installed = str(selected.get("installed_path") or "")
    if not normalized.endswith(installed) or DAP_MANAGED_PREFIX not in normalized:
        raise ReceiptError(
            "adapter.process_path is not the managed public artifact; a PATH, "
            "explicit-override, or prior-cache binary cannot satisfy the managed route"
        )
    argv = adapter.get("process_argv")
    if not isinstance(argv, list) or not all(isinstance(item, str) for item in argv):
        raise ReceiptError("adapter.process_argv must be an array of strings")
    for item in argv:
        basename = item.replace("\\", "/").rsplit("/", 1)[-1].lower()
        if basename in {"perllsp", "perllsp.exe"}:
            raise ReceiptError(
                "adapter.process_argv names the perllsp product; another "
                "product cannot satisfy the perl-dap route"
            )

    workspace = _object(receipt.get("workspace"), "workspace")
    _nonempty(workspace.get("fixture_id"), "workspace.fixture_id")
    parse_digest(workspace.get("fixture_sha256"), "workspace.fixture_sha256")
    _nonempty(workspace.get("root_identity"), "workspace.root_identity")
    configuration = _object(receipt.get("configuration"), "configuration")
    for cell in ("config_sha256", "driver_sha256", "instrument_sha256"):
        parse_digest(configuration.get(cell), f"configuration.{cell}")

    journey = _object(receipt.get("journey"), "journey")
    for cell in JOURNEY_CELLS:
        entry = _object(journey.get(cell), f"journey.{cell}")
        if entry.get("result") != "pass" or not str(entry.get("evidence") or "").strip():
            raise ReceiptError(f"required journey cell {cell!r} is not proven")

    discriminators = _object(receipt.get("discriminators"), "discriminators")
    if discriminators.get("wrong_root_same_basename_rejected") is not True:
        raise ReceiptError(
            "wrong-root same-basename source mapping must be observed rejected; "
            "a wrong-source stop cannot pass"
        )

    cache = _object(receipt.get("managed_cache"), "managed_cache")
    if cache.get("before") != []:
        raise ReceiptError(
            "managed_cache.before must be an empty inventory; a prior managed "
            "perl-dap cache violates the clean profile"
        )
    after = cache.get("after")
    version_directory = f"{DAP_MANAGED_PREFIX}{contract['source']['version']}-{selected.get('target')}"
    if (
        not isinstance(after, list)
        or len(after) != 1
        or after[0] != version_directory
    ):
        raise ReceiptError(
            "managed_cache.after must be exactly the bounded selected managed subject"
        )
    restart = _object(cache.get("restart"), "managed_cache.restart")
    if restart.get("same_subject") is not True or restart.get("second_provider_absent") is not True:
        raise ReceiptError(
            "restart must reuse the exact verified known-good managed subject "
            "without a second provider"
        )

    cleanup = _object(receipt.get("cleanup"), "cleanup")
    for cell in ("adapter_orphans", "debuggee_orphans"):
        if cleanup.get(cell) != []:
            raise ReceiptError(
                f"cleanup.{cell} must be empty; an adapter exit with a surviving "
                "process cannot pass"
            )

    boundary = _object(receipt.get("claim_boundary"), "claim_boundary")
    if boundary.get("lsp_support_rows") != "unchanged":
        raise ReceiptError("claim_boundary.lsp_support_rows must record unchanged")
    for cell in ("dap_support_projection", "all_platforms"):
        if boundary.get(cell) != "not_proven":
            raise ReceiptError(f"claim_boundary.{cell} must stay not_proven")

    # A passing observation must carry its observation time and honest
    # limitations/currentness accounting, not just the proven cells.
    observed_at = _nonempty(receipt.get("observed_at"), "observed_at")
    try:
        parsed = dt.datetime.fromisoformat(observed_at.replace("Z", "+00:00"))
    except ValueError as error:
        raise ReceiptError(f"observed_at is not an RFC 3339 timestamp: {error}") from error
    if parsed.tzinfo is None:
        raise ReceiptError("observed_at must carry a timezone")
    limitations = receipt.get("limitations")
    if (
        not isinstance(limitations, list)
        or not limitations
        or not all(isinstance(item, str) and item.strip() for item in limitations)
    ):
        raise ReceiptError("limitations must be a non-empty list of non-empty strings")
    currentness = _object(receipt.get("currentness"), "currentness")
    invalidators = currentness.get("invalidators")
    if not isinstance(invalidators, list) or not invalidators or not all(
        isinstance(item, str) and item.strip() for item in invalidators
    ):
        raise ReceiptError(
            "currentness.invalidators must name the conditions that stale this receipt"
        )

    # The D05 platform must be a managed contract row, and the selected target
    # must be that row: a Windows ARM64 host is unsupported by inference, and
    # a cross-target selection cannot authorize the journey's process claims.
    matching = [
        row
        for row in contract.get("targets", [])
        if isinstance(row, dict)
        and row.get("os") == platform.get("os")
        and row.get("architecture") == platform.get("architecture")
        and row.get("disposition") == "managed"
    ]
    if not matching:
        raise ReceiptError(
            "the journey platform has no managed contract row; an unsupported "
            "platform cannot be promoted by inference"
        )
    if selected.get("target") != matching[0].get("target"):
        raise ReceiptError(
            "the selected target must be the managed row matching the journey platform"
        )
    aggregate_row = _aggregate_row(asset_receipt, str(selected.get("target")))
    if aggregate_row.get("result") not in {"managed_executed", "managed_extracted_not_executed"}:
        raise ReceiptError("the bound #9516 row for the journey target lacks byte evidence")
