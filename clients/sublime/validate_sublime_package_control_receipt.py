#!/usr/bin/env python3
"""Fail-closed Package Control public-install receipt law for LSP-perllsp.

This module owns the #8003 evidence stage, not a live Sublime host run.
A pass requires a listed default-channel subject and independently proven
cells. Exact-source, custom-channel, local checkout, listing-only, and
blocked-subject evidence cannot satisfy that pass.
"""
from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any, Iterable, Mapping

SUBJECT_RELATIVE_PATH = "clients/sublime/package-control-subject.v1.json"
SUBJECT_SCHEMA = "sublime_package_control_subject.v1"
PASS_INSTALL_ROUTE = "package_control_default_channel"
PASS_HELPER_SOURCE = "package_control_default_channel"
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
RECEIPT_RESULTS = {"pass", "fail", "not_proven", "not_run"}
CELL_RESULTS = {"pass", "fail", "not_proven", "not_run"}
ACTIVATION_CELLS = ("pl", "pm", "t")
REQUIRED_JOURNEY_FOR_PASS = (
    "default_channel_install",
    "lsp_dependency_resolved",
    "public_packed_or_unpacked_layout",
    "managed_perllsp_verified",
    "initialize",
    "stdio_process_identity",
    "pull_diagnostics_open",
    "pull_diagnostics_after_edit",
    "completion",
    "hover",
    "definition",
    "workspace_edit_applied",
    "semantic_tokens",
    "custom_semantic_mapping",
    "restart",
    "clean_shutdown",
)
OPTIONAL_JOURNEY = ("command_7704",)
CLEAN_PROFILE_FIELDS = (
    "isolated",
    "prior_package_absent",
    "prior_managed_server_absent",
    "manual_perl_server_absent",
    "custom_package_source_absent",
    "prior_cache_absent",
    "competing_perl_provider_absent",
    "workspace_does_not_replace_server_authority",
)
FORBIDDEN_PASS_SOURCES = {
    "clients/sublime/LSP-perllsp",
    "extra_packages",
    "local_checkout",
    "custom_channel",
    "manual_copy",
    "exact_source_local",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def _mapping(payload: Any, field: str) -> dict[str, Any]:
    require(isinstance(payload, dict), f"{field} must be an object")
    return payload


def _cell(group: Mapping[str, Any], name: str) -> dict[str, Any]:
    cell = group.get(name)
    require(isinstance(cell, dict), f"{name} cell must be an object")
    result = cell.get("result")
    require(result in CELL_RESULTS, f"{name} cell result is invalid")
    return cell


def _cell_pass(group: Mapping[str, Any], name: str) -> bool:
    cell = _cell(group, name)
    evidence = cell.get("evidence")
    return cell.get("result") == "pass" and isinstance(evidence, str) and evidence != ""


def _require_cells(group: Mapping[str, Any], names: Iterable[str], message: str) -> None:
    failed = [name for name in names if not _cell_pass(group, name)]
    require(not failed, message)


def not_run_template() -> dict[str, Any]:
    pending = {"result": "not_proven", "evidence": None}
    return {
        "schema_version": 1,
        "stage": "package_control_public",
        "result": "not_run",
        "recorded_at": None,
        "listing": {
            "channel": "package_control_default",
            "package_name": "LSP-perllsp",
            "observable": False,
            "status": "blocked_pending_listing",
        },
        "install_route": None,
        "host": {"name": None, "version": None, "platform": None, "arch": None},
        "lsp_package": {"repository": "sublimelsp/LSP", "version": None, "ref": None},
        "helper_package": {
            "name": "LSP-perllsp",
            "source": None,
            "version": None,
            "tag": None,
            "tree_sha256": None,
            "installed_digest": None,
        },
        "binary": {
            "path": None,
            "sha256": None,
            "command": [None, "--stdio"],
            "release": None,
            "archive_sha256": None,
        },
        "resolution_route": None,
        "compatibility": {"result": "not_proven", "record_id": None},
        "fixtures": {"pl": None, "pm": None, "t": None, "digest": None},
        "clean_profile": {field: None for field in CLEAN_PROFILE_FIELDS},
        "activation": {name: dict(pending) for name in ACTIVATION_CELLS},
        "journey": {
            name: dict(pending) for name in (*REQUIRED_JOURNEY_FOR_PASS, *OPTIONAL_JOURNEY)
        },
        "promotion": {
            "public_artifact_installed": "not_proven",
            "package_control_leading_setup": "not_proven",
        },
        "public_subject": {"relative_path": SUBJECT_RELATIVE_PATH, "sha256": None},
    }


def validate_subject(payload: dict[str, Any]) -> None:
    require(payload.get("schema_version") == SUBJECT_SCHEMA, "public subject schema mismatch")
    require(payload.get("status") in {"blocked_pending_listing", "listed"}, "invalid subject status")
    channel = _mapping(payload.get("channel"), "channel")
    require(channel.get("name") == "package_control_default", "subject channel must be the default")
    require(channel.get("package_name") == "LSP-perllsp", "unexpected Package Control package name")
    package = _mapping(payload.get("package"), "package")
    require(package.get("name") == "LSP-perllsp", "unexpected package name")
    require(
        package.get("public_repository") == "EffortlessMetrics/LSP-perllsp",
        "unexpected public package repository",
    )
    promotion = _mapping(payload.get("promotion"), "promotion")
    require(
        promotion.get("package_control_leading_setup") == "not_proven",
        "leading Package Control setup remains owned by a later docs stage",
    )
    if payload.get("status") == "blocked_pending_listing":
        require(channel.get("observable") is False, "blocked subject cannot be observable")
        require(channel.get("listing_url") is None, "blocked subject cannot invent a listing URL")
        require(
            promotion.get("public_artifact_installed") == "not_proven",
            "promotion cannot be claimed on a blocked subject",
        )
        for field in ("version", "tag", "tree_sha256", "installed_digest"):
            require(package.get(field) is None, "blocked subject cannot invent a public package identity")
        return
    require(channel.get("observable") is True, "listed subject must be observable")
    require(isinstance(channel.get("listing_url"), str) and channel["listing_url"], "listed subject missing listing URL")
    for field in ("version", "tag", "tree_sha256", "installed_digest"):
        value = package.get(field)
        require(isinstance(value, str) and value, f"listed subject missing package.{field}")
    require(SHA256.fullmatch(str(package.get("tree_sha256"))), "listed tree digest is invalid")
    require(SHA256.fullmatch(str(package.get("installed_digest"))), "listed installed digest is invalid")


def validate_shape(payload: dict[str, Any]) -> None:
    require(payload.get("schema_version") == 1, "schema_version must be 1")
    require(payload.get("stage") == "package_control_public", "stage must be package_control_public")
    require(payload.get("result") in RECEIPT_RESULTS, "result is invalid")
    listing = _mapping(payload.get("listing"), "listing")
    require(listing.get("channel") == "package_control_default", "listing.channel must be the default")
    require(listing.get("package_name") == "LSP-perllsp", "listing.package_name must be LSP-perllsp")
    helper = _mapping(payload.get("helper_package"), "helper_package")
    require(helper.get("name") == "LSP-perllsp", "unexpected helper package name")
    activation = _mapping(payload.get("activation"), "activation")
    for name in ACTIVATION_CELLS:
        _cell(activation, name)
    journey = _mapping(payload.get("journey"), "journey")
    for name in (*REQUIRED_JOURNEY_FOR_PASS, *OPTIONAL_JOURNEY):
        _cell(journey, name)
    promotion = _mapping(payload.get("promotion"), "promotion")
    require(
        promotion.get("package_control_leading_setup") == "not_proven",
        "leading Package Control setup remains owned by a later docs stage",
    )
    require(
        promotion.get("public_artifact_installed") in {"not_proven", "eligible"},
        "public_artifact_installed promotion is invalid",
    )
    if promotion.get("public_artifact_installed") == "eligible":
        require(
            payload.get("result") == "pass",
            "public_artifact_installed cannot be eligible without a pass",
        )
    subject = _mapping(payload.get("public_subject"), "public_subject")
    require(
        subject.get("relative_path") == SUBJECT_RELATIVE_PATH,
        "public receipt binds the wrong subject path",
    )


def _validate_pass_identities(payload: dict[str, Any], subject: dict[str, Any]) -> None:
    listing = _mapping(payload.get("listing"), "listing")
    require(listing.get("observable") is True, "listing is not a behavior proof")
    require(listing.get("status") == "listed", "listing is not a behavior proof")
    require(payload.get("install_route") == PASS_INSTALL_ROUTE, "install route must be package_control_default_channel")
    helper = _mapping(payload.get("helper_package"), "helper_package")
    source = helper.get("source")
    require(source not in FORBIDDEN_PASS_SOURCES, "exact-source helper source cannot satisfy a Package Control pass")
    require(source == PASS_HELPER_SOURCE, "install route must be package_control_default_channel")
    host = _mapping(payload.get("host"), "host")
    require(host.get("name") == "Sublime Text", "host.name must be Sublime Text")
    require(str(host.get("version", "")).isdigit(), "host.version must be a Sublime build number")
    require(host.get("platform") in {"linux", "osx", "windows"}, "unsupported host.platform")
    require(host.get("arch") in {"x64", "arm64"}, "unsupported host.arch")
    lsp = _mapping(payload.get("lsp_package"), "lsp_package")
    require(lsp.get("repository") == "sublimelsp/LSP", "unexpected LSP repository")
    require(FULL_SHA.fullmatch(str(lsp.get("ref", ""))), "lsp_package.ref must be a full commit SHA")
    require(lsp.get("version") == subject["lsp_package"]["version"], "public receipt identities disagree with the bound public subject")
    require(lsp.get("ref") == subject["lsp_package"]["ref"], "public receipt identities disagree with the bound public subject")
    for field in ("version", "tag", "tree_sha256", "installed_digest"):
        require(
            helper.get(field) == subject["package"][field],
            "public receipt identities disagree with the bound public subject",
        )
    binary = _mapping(payload.get("binary"), "binary")
    command = binary.get("command")
    require(isinstance(command, list) and len(command) == 2, "binary.command must launch managed perllsp --stdio")
    require(command[1] == "--stdio", "binary.command must launch managed perllsp --stdio")
    require(
        Path(str(command[0])).name in {"perllsp", "perllsp.exe"},
        "binary.command must launch managed perllsp --stdio",
    )
    require(SHA256.fullmatch(str(binary.get("sha256", ""))), "binary.sha256 must be a lowercase SHA-256 digest")
    require(
        binary.get("sha256") == subject["perllsp_asset"]["asset_sha256"],
        "public receipt identities disagree with the bound public subject",
    )
    require(binary.get("release") == subject["perllsp_asset"]["release"], "public receipt identities disagree with the bound public subject")
    require(
        SHA256.fullmatch(str(binary.get("archive_sha256", ""))),
        "binary.archive_sha256 must be a lowercase SHA-256 digest",
    )
    require(payload.get("resolution_route") == "managed_download", "managed perllsp resolution_route must be managed_download")
    compatibility = _mapping(payload.get("compatibility"), "compatibility")
    require(compatibility.get("result") == "compatible", "compatibility result must be compatible")
    require(
        compatibility.get("record_id") == subject["compatibility"]["record_id"],
        "public receipt identities disagree with the bound public subject",
    )
    fixtures = _mapping(payload.get("fixtures"), "fixtures")
    require(str(fixtures.get("pl", "")).endswith(".pl"), "pl fixture identity is invalid")
    require(str(fixtures.get("pm", "")).endswith(".pm"), "pm fixture identity is invalid")
    require(str(fixtures.get("t", "")).endswith(".t"), "t fixture identity is invalid")
    require(SHA256.fullmatch(str(fixtures.get("digest", ""))), "fixture digest is invalid")


def _validate_pass_profile_and_cells(payload: dict[str, Any]) -> None:
    profile = _mapping(payload.get("clean_profile"), "clean_profile")
    missing = [
        field
        for field in CLEAN_PROFILE_FIELDS
        if field != "workspace_does_not_replace_server_authority"
        and profile.get(field) is not True
    ]
    require(not missing, "clean-profile preconditions are not proven")
    require(
        profile.get("workspace_does_not_replace_server_authority") is True,
        "workspace configuration cannot replace package-owned server authority",
    )
    activation = _mapping(payload.get("activation"), "activation")
    _require_cells(
        activation,
        ACTIVATION_CELLS,
        "activation cells cannot inherit; pl, pm, and t must be independently proven",
    )
    journey = _mapping(payload.get("journey"), "journey")
    if not _cell_pass(journey, "default_channel_install"):
        raise ValueError("listing is not a behavior proof")
    if not _cell_pass(journey, "pull_diagnostics_after_edit"):
        raise ValueError("post-edit diagnostics are not proven current")
    if not _cell_pass(journey, "workspace_edit_applied"):
        raise ValueError("workspace edit was returned but not applied")
    if not _cell_pass(journey, "clean_shutdown"):
        raise ValueError("shutdown left an orphaned server process")
    remaining = [
        name
        for name in REQUIRED_JOURNEY_FOR_PASS
        if name
        not in {
            "default_channel_install",
            "pull_diagnostics_after_edit",
            "workspace_edit_applied",
            "clean_shutdown",
        }
    ]
    _require_cells(journey, remaining, "required first-use journey cells are not proven")
    command = _cell(journey, "command_7704")
    require(
        command.get("result") in {"pass", "not_proven"},
        "command cells are promoted only for the public package version that contains them",
    )
    if command.get("result") == "pass":
        require(isinstance(command.get("evidence"), str) and command["evidence"], "command_7704 pass needs evidence")
    promotion = _mapping(payload.get("promotion"), "promotion")
    require(
        promotion.get("public_artifact_installed") in {"not_proven", "eligible"},
        "this stage cannot prove public_artifact_installed",
    )


def validate_pass(payload: dict[str, Any], subject_bytes: bytes | None) -> None:
    validate_shape(payload)
    require(payload.get("result") == "pass", "receipt is not a pass candidate")
    require(subject_bytes is not None, "public package-control pass requires the bound public subject")
    try:
        subject = json.loads(subject_bytes)
    except json.JSONDecodeError as error:
        raise ValueError(f"public subject parse failed: {error}") from error
    require(isinstance(subject, dict), "public subject must be an object")
    validate_subject(subject)
    require(subject.get("status") == "listed", "public subject is not listed")
    bound = _mapping(payload.get("public_subject"), "public_subject")
    digest = bound.get("sha256")
    require(SHA256.fullmatch(str(digest or "")), "public receipt missing content-addressed public_subject.sha256")
    require(digest == sha256_bytes(subject_bytes), "public_subject.sha256 does not match the bound subject bytes")
    _validate_pass_identities(payload, subject)
    _validate_pass_profile_and_cells(payload)


def validate(payload: dict[str, Any], subject_bytes: bytes | None = None) -> None:
    validate_shape(payload)
    if payload.get("result") == "pass":
        validate_pass(payload, subject_bytes)


def main(argv: list[str]) -> int:
    if len(argv) not in {2, 3}:
        print(
            "usage: validate_sublime_package_control_receipt.py RECEIPT.json [SUBJECT.json]",
            file=sys.stderr,
        )
        return 2
    path = Path(argv[1])
    subject_bytes = Path(argv[2]).read_bytes() if len(argv) == 3 else None
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
        require(isinstance(payload, dict), "receipt root must be an object")
        validate(payload, subject_bytes)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"{path}: {error}", file=sys.stderr)
        return 1
    print(f"validated {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
