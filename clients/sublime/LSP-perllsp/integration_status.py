from __future__ import annotations

import json
import os
import shutil
from pathlib import Path
from typing import Any, Callable

try:
    from .compatibility import (
        CompatibilityError,
        assert_managed_install_allowed,
        load_record,
        summary as compatibility_summary,
    )
    from .dap_support import DapPathError, resolve_dap_path, sha256_file as dap_sha256_file
    from .release import (
        install_server,
        installed_binary_is_current,
        load_manifest,
        platform_key,
        select_asset,
        sha256_file,
    )
except ImportError:
    from compatibility import (  # type: ignore
        CompatibilityError,
        assert_managed_install_allowed,
        load_record,
        summary as compatibility_summary,
    )
    from dap_support import DapPathError, resolve_dap_path, sha256_file as dap_sha256_file  # type: ignore
    from release import (  # type: ignore
        install_server,
        installed_binary_is_current,
        load_manifest,
        platform_key,
        select_asset,
        sha256_file,
    )

AUTO = "auto"
MAX_STATUS_CHARS = 64 * 1024


class IntegrationStatusError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise IntegrationStatusError(message)


def _contains_separator(value: str) -> bool:
    return "/" in value or "\\" in value


def _resolve_user_executable(
    configured: str,
    *,
    which: Callable[[str], str | None],
) -> Path:
    expanded = os.path.expandvars(os.path.expanduser(configured))
    candidate = Path(expanded)
    if candidate.is_absolute():
        resolved = candidate
    elif _contains_separator(expanded):
        raise IntegrationStatusError(
            "External server paths must be absolute or bare executable names resolved through PATH."
        )
    else:
        found = which(expanded)
        if not found:
            raise IntegrationStatusError(f"Configured perllsp executable was not found: {configured}")
        resolved = Path(found)
    if not resolved.is_file():
        raise IntegrationStatusError(f"Configured perllsp executable was not found: {resolved}")
    return resolved.resolve()


def _managed_paths(
    storage_path: Path,
    platform: str,
    arch: str,
) -> tuple[dict[str, Any], dict[str, str], Path, Path]:
    manifest = load_manifest()
    asset = select_asset(manifest, platform, arch)
    storage_root = storage_path.resolve()
    install_dir = (storage_root / manifest["version"] / platform_key(platform, arch)).resolve()
    require(
        os.path.commonpath([str(storage_root), str(install_dir)]) == str(storage_root),
        "managed server path escaped Package Storage",
    )
    binary_path = install_dir / asset["binary"]
    return manifest, asset, install_dir, binary_path


def _managed_server_status(storage_path: Path, platform: str, arch: str) -> dict[str, Any]:
    try:
        manifest, asset, install_dir, binary_path = _managed_paths(storage_path, platform, arch)
    except (CompatibilityError, IntegrationStatusError, RuntimeError, KeyError, ValueError) as error:
        return {
            "mode": "managed",
            "state": "unsupported_or_invalid_manifest",
            "error": str(error),
        }

    metadata_path = binary_path.with_name("install.json")
    binary_exists = binary_path.is_file()
    metadata_exists = metadata_path.is_file()
    # An unreadable or vanishing managed binary is an invalid cache, not a
    # crashed health command: hashing inside the currency check raises for
    # those, so verify defensively and surface the failure in the payload.
    verify_error: str | None = None
    try:
        verified = installed_binary_is_current(binary_path, asset)
    except OSError as error:
        verified = False
        verify_error = str(error)
    if verified:
        state = "verified_cache"
    elif binary_exists or metadata_exists:
        state = "invalid_cache"
    else:
        state = "missing"

    result: dict[str, Any] = {
        "mode": "managed",
        "state": state,
        "version": manifest["version"],
        "release_tag": manifest["release_tag"],
        "target": asset["target"],
        "asset": asset["asset"],
        "archive_sha256": asset["sha256"],
        "install_dir": str(install_dir),
        "binary_path": str(binary_path),
        "binary_exists": binary_exists,
        "metadata_exists": metadata_exists,
        "verified": verified,
    }
    if verify_error is not None:
        result["verify_error"] = verify_error
    if binary_exists:
        try:
            result["binary_sha256"] = sha256_file(binary_path)
        except OSError as error:
            result["binary_error"] = str(error)
    return result


def _external_server_status(
    configured: str,
    *,
    which: Callable[[str], str | None],
) -> dict[str, Any]:
    result: dict[str, Any] = {
        "mode": "external_user_managed",
        "configured": configured,
        "support_disposition": "not_proven",
    }
    try:
        path = _resolve_user_executable(configured, which=which)
    except IntegrationStatusError as error:
        result.update({"state": "missing", "error": str(error)})
        return result
    try:
        digest = sha256_file(path)
    except OSError as error:
        result.update({"state": "unreadable", "path": str(path), "error": str(error)})
        return result
    result.update(
        {
            "state": "resolved",
            "path": str(path),
            "sha256": digest,
        }
    )
    return result


def _dap_status(
    configured: str,
    *,
    server_path: str,
    which: Callable[[str], str | None],
    debugger_registered: bool | None,
) -> dict[str, Any]:
    result: dict[str, Any] = {
        "mode": "external_user_managed",
        "configured": configured,
        "support_disposition": "not_proven",
        "debugger_registered": debugger_registered,
    }
    try:
        path = resolve_dap_path(configured, server_path=server_path, which=which)
    except DapPathError as error:
        result.update({"state": "unavailable", "error": str(error)})
        return result
    try:
        digest = dap_sha256_file(path)
    except OSError as error:
        result.update({"state": "unreadable", "path": str(path), "error": str(error)})
        return result
    result.update(
        {
            "state": "resolved",
            "path": str(path),
            "sha256": digest,
        }
    )
    return result


def collect_status(
    storage_path: Path,
    platform: str,
    arch: str,
    *,
    server_path: str = AUTO,
    dap_path: str = AUTO,
    which: Callable[[str], str | None] = shutil.which,
    debugger_registered: bool | None = None,
) -> dict[str, Any]:
    require(isinstance(server_path, str) and server_path, "server_path must be a non-empty string")
    require(isinstance(dap_path, str) and dap_path, "dap_path must be a non-empty string")

    compatibility_payload: dict[str, Any]
    compatibility_error: str | None = None
    try:
        compatibility_payload = compatibility_summary(load_record())
    except CompatibilityError as error:
        compatibility_payload = {
            "compatibility": "invalid",
            "currentness": "invalid",
        }
        compatibility_error = str(error)

    if server_path == AUTO:
        server = _managed_server_status(storage_path, platform, arch)
    else:
        server = _external_server_status(server_path, which=which)
    dap = _dap_status(
        dap_path,
        server_path=server_path,
        which=which,
        debugger_registered=debugger_registered,
    )

    reason_tokens: list[str] = []
    if compatibility_error:
        reason_tokens.append("compatibility_record_invalid")
    elif compatibility_payload.get("compatibility") == "not_proven":
        reason_tokens.append("compatibility_not_proven")
    elif compatibility_payload.get("compatibility") == "incompatible":
        reason_tokens.append("compatibility_incompatible")

    server_state = server.get("state")
    if server_state == "missing":
        reason_tokens.append("managed_server_missing" if server_path == AUTO else "external_server_missing")
    elif server_state == "invalid_cache":
        reason_tokens.append("managed_server_invalid_cache")
    elif server_state == "unsupported_or_invalid_manifest":
        reason_tokens.append("managed_server_unsupported")
    elif server_path != AUTO:
        reason_tokens.append("external_server_user_owned")

    if dap.get("state") != "resolved":
        reason_tokens.append("dap_unavailable")
    elif dap.get("support_disposition") == "not_proven":
        reason_tokens.append("dap_external_not_proven")
    if debugger_registered is False:
        reason_tokens.append("debugger_adapter_not_registered")
    reason_tokens.append("semantic_support_not_assessed")
    reason_tokens = sorted(set(reason_tokens))

    blocking = {
        "compatibility_record_invalid",
        "compatibility_incompatible",
        "managed_server_missing",
        "managed_server_invalid_cache",
        "managed_server_unsupported",
        "external_server_missing",
    }
    # A record that vouches compatibility but carries an unsupported or
    # withdrawn currentness must demand action: the install gate and the
    # plugin refuse that record, so reporting it as merely usable would lie.
    # A not_proven record stays a usable-but-unproven candidate, matching
    # the verified-cache contract the health surface pins.
    currentness = compatibility_payload.get("currentness")
    if compatibility_payload.get("compatibility") == "compatible" and currentness not in {
        "current",
        "stale_supported",
    }:
        blocking.add("compatibility_currentness_unsupported")
        reason_tokens.append("compatibility_currentness_unsupported")
        reason_tokens = sorted(set(reason_tokens))
    structural_state = "action_required" if blocking.intersection(reason_tokens) else "usable_candidate"
    if (
        structural_state == "usable_candidate"
        and compatibility_payload.get("compatibility") == "compatible"
        and currentness in {"current", "stale_supported"}
        and server.get("state") in {"verified_cache", "resolved"}
        # A user-owned external executable stays a usable but unproven
        # candidate even when the pinned pair is compatible: the record
        # never verified this binary.
        and "external_server_user_owned" not in reason_tokens
    ):
        structural_state = "ready"

    return {
        "schema_version": 1,
        "mutated": False,
        "structural_state": structural_state,
        "semantic_support": "not_assessed",
        "platform": platform,
        "architecture": arch,
        "compatibility": compatibility_payload,
        "compatibility_error": compatibility_error,
        "server": server,
        "dap": dap,
        "reason_tokens": reason_tokens,
    }


def clear_invalid_managed_cache(storage_path: Path, platform: str, arch: str) -> dict[str, Any]:
    manifest, asset, install_dir, binary_path = _managed_paths(storage_path, platform, arch)
    if installed_binary_is_current(binary_path, asset):
        raise IntegrationStatusError("The managed perllsp cache is verified; refusing to remove it.")
    existed = install_dir.exists()
    if existed:
        shutil.rmtree(install_dir)
    return {
        "schema_version": 1,
        "action": "clear_invalid_managed_cache",
        "mutated": existed,
        "version": manifest["version"],
        "target": asset["target"],
        "install_dir": str(install_dir),
        "result": "removed" if existed else "already_absent",
    }


def repair_managed_server(
    storage_path: Path,
    platform: str,
    arch: str,
    *,
    opener: Callable[..., Any],
) -> dict[str, Any]:
    record = assert_managed_install_allowed()
    before = _managed_server_status(storage_path, platform, arch)
    binary = install_server(storage_path, platform, arch, opener=opener)
    manifest, asset, _install_dir, expected_binary = _managed_paths(storage_path, platform, arch)
    require(binary.resolve() == expected_binary.resolve(), "repair installed an unexpected binary path")
    require(installed_binary_is_current(binary, asset), "repair did not produce a verified managed cache")
    return {
        "schema_version": 1,
        "action": "repair_managed_server",
        "mutated": before.get("state") != "verified_cache",
        "compatibility": record["compatibility"],
        "currentness": record["currentness"],
        "version": manifest["version"],
        "target": asset["target"],
        "binary_path": str(binary),
        "binary_sha256": sha256_file(binary),
        "result": "verified",
    }


def format_status(payload: dict[str, Any]) -> str:
    lines = [
        "Perl LSP Integration Status",
        "===========================",
        "",
        f"Structural state: {payload.get('structural_state', 'unknown')}",
        "Semantic support: not assessed by this structural command",
        f"Platform: {payload.get('platform', 'unknown')} / {payload.get('architecture', 'unknown')}",
        "",
    ]
    compatibility = payload.get("compatibility", {})
    lines.extend(
        [
            "Compatibility",
            "-------------",
            f"Result: {compatibility.get('compatibility', 'unknown')}",
            f"Currentness: {compatibility.get('currentness', 'unknown')}",
        ]
    )
    if payload.get("compatibility_error"):
        lines.append(f"Error: {payload['compatibility_error']}")
    lines.extend(["", "Server", "------"])
    server = payload.get("server", {})
    for field in ("mode", "state", "version", "target", "binary_path", "sha256", "binary_sha256", "error"):
        value = server.get(field)
        if value is not None:
            lines.append(f"{field.replace('_', ' ').title()}: {value}")
    lines.extend(["", "Debugger / perl-dap", "-------------------"])
    dap = payload.get("dap", {})
    for field in ("mode", "state", "path", "sha256", "debugger_registered", "error"):
        value = dap.get(field)
        if value is not None:
            lines.append(f"{field.replace('_', ' ').title()}: {value}")
    lines.extend(["", "Next-action reasons", "-------------------"])
    reasons = payload.get("reason_tokens", [])
    if reasons:
        lines.extend(f"- {reason}" for reason in reasons)
    else:
        lines.append("- none")
    text = "\n".join(lines) + "\n"
    if len(text) > MAX_STATUS_CHARS:
        omitted = len(text) - MAX_STATUS_CHARS
        text = text[:MAX_STATUS_CHARS] + f"\n... {omitted} character(s) omitted.\n"
    return text


def status_json(payload: dict[str, Any]) -> str:
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"
