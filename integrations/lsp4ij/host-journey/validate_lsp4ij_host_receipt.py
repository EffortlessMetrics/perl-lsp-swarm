#!/usr/bin/env python3
"""Validate one LSP4IJ real-host session receipt against its declared launch spec.

A receipt is admissible only if every recorded observation carries
``live_wire_capture`` origin plus a digest of the captured bytes, the exact
current-source binary identity is present, the maintained LSP4IJ line is
respected, and the supervised process ledger records an orderly shutdown.
The receipt must also be bound to its precondition: the validator recomputes
the canonical digest of the supplied launch spec, requires the recorded
``launch_spec_digest`` to match it exactly, and rejects every drift between
the declared subject (source SHA, IDE, plugin, binary) and what the session
observed.

Receipts stamped with any other origin are synthetic; synthetic receipts are
forbidden for production closure.

Exit codes: 0 valid, 1 invalid receipt or spec binding, 2 usage error.
"""
from __future__ import annotations

import json
import re
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

import validate_lsp4ij_launch_spec as launch_spec_contract

# docs/EDITORS/INTELLIJ_IDEA_SETUP.md declares 0.20.0 and newer as the
# maintained LSP4IJ line; an observed subject below it is not reviewable here.
MIN_LSP4IJ_VERSION = (0, 20, 0)

CAPTURE_ORIGIN = "live_wire_capture"
VALID_PLATFORMS = {"linux", "macos", "windows"}
VALID_ARCHES = {"x64", "arm64"}
VALID_PROVIDERS = {
    "completion",
    "diagnostic",
    "documentSymbol",
    "formatting",
    "hover",
    "references",
    "semanticTokens",
    "signatureHelp",
}
VALID_FILE_SUFFIXES = {".pl", ".pm", ".t"}

REQUIRED_CAPABILITY_KEYS = {"completion", "hover", "diagnostic"}
REQUIRED_FILE_SUFFIXES = {".pl"}

KNOWN_KEYS = {
    "schema_version",
    "stage",
    "source_sha",
    "recorded_at",
    "launch_spec_digest",
    "host",
    "lsp4ij_plugin",
    "server_binary",
    "session_initialize",
    "repro_readiness",
    "provider_taps",
    "process_ledger",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def _is_sha256(value: Any) -> bool:
    return isinstance(value, str) and bool(re.fullmatch(r"[0-9a-f]{64}", value))


def _is_source_sha(value: Any) -> bool:
    return isinstance(value, str) and bool(re.fullmatch(r"[0-9a-f]{40}", value))


def _is_build_number(value: Any) -> bool:
    return isinstance(value, str) and bool(re.fullmatch(r"[A-Z]{2}-[0-9]{3,4}(\.[0-9]+)+", value))


def _is_plugin_version(value: Any) -> bool:
    return isinstance(value, str) and bool(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", value))


def _validate_plugin(plugin: dict[str, Any]) -> None:
    require(plugin.get("id") == "com.redhat.devtools.lsp4ij", "unexpected LSP4IJ plugin id")
    require(plugin.get("upstream_repository") == "redhat-developer/lsp4ij", "unexpected LSP4IJ upstream repository")
    require(_is_plugin_version(plugin.get("version")), "lsp4ij_plugin.version must be a three-part semver string")
    version = tuple(int(part) for part in str(plugin["version"]).split("."))
    require(version >= MIN_LSP4IJ_VERSION,
            f"lsp4ij_plugin.version {plugin['version']} is below the maintained line "
            f"{'.'.join(str(p) for p in MIN_LSP4IJ_VERSION)} (docs/EDITORS/INTELLIJ_IDEA_SETUP.md)")
    if plugin.get("pinned_commit") is not None:
        require(_is_source_sha(plugin["pinned_commit"]), "lsp4ij_plugin.pinned_commit must be a full Git commit SHA")


def validate(payload: dict[str, Any]) -> None:
    unexpected = sorted(set(payload) - KNOWN_KEYS)
    require(not unexpected, f"receipt contains keys outside the v1 contract: {unexpected}")
    require(payload.get("schema_version") == 1, "schema_version must be 1")
    require(payload.get("stage") == "exact_source_local", "stage must remain exact_source_local")
    require(_is_source_sha(payload.get("source_sha")), "source_sha must be a full lowercase Git commit SHA")

    recorded_at = payload.get("recorded_at")
    require(isinstance(recorded_at, str), "recorded_at must be an RFC 3339 timestamp string")
    try:
        parsed = datetime.fromisoformat(str(recorded_at).replace("Z", "+00:00"))
    except ValueError as error:
        raise ValueError(f"recorded_at is not a parseable timestamp: {recorded_at!r}") from error
    require(parsed.tzinfo is not None, "recorded_at must carry a UTC offset")

    require(_is_sha256(payload.get("launch_spec_digest")),
            "launch_spec_digest must bind this session to its declared launch spec via a SHA-256 digest")

    host = payload.get("host")
    require(isinstance(host, dict), "host must be an object")
    require(set(host) == {"product", "build_number", "platform", "arch", "os_version", "jbr_version"},
            "host keys drifted from the v1 contract")
    require(isinstance(host["product"], str) and host["product"].strip() != "", "host.product must be non-empty")
    require(_is_build_number(host["build_number"]), 'host.build_number must look like "IC-241.18034.62"')
    require(host["platform"] in VALID_PLATFORMS, f"host.platform must be in {sorted(VALID_PLATFORMS)}")
    require(host["arch"] in VALID_ARCHES, f"host.arch must be in {sorted(VALID_ARCHES)}")
    for text_field in ("os_version", "jbr_version"):
        require(isinstance(host[text_field], str) and host[text_field].strip() != "",
                f"host.{text_field} must be non-empty")

    plugin = payload.get("lsp4ij_plugin")
    require(isinstance(plugin, dict), "lsp4ij_plugin must be an object")
    _validate_plugin(plugin)

    binary = payload.get("server_binary")
    require(isinstance(binary, dict), "server_binary must be an object")
    require(
        set(binary) == {"path", "sha256", "command"},
        "server_binary keys drifted from the v1 contract",
    )
    require(_is_sha256(binary["sha256"]), "server_binary.sha256 must be a lowercase SHA-256 digest")
    command = binary["command"]
    require(isinstance(command, list) and len(command) == 2, "server_binary.command must have two entries")
    require(command[1] == "--stdio", "server_binary.command must use stdio")
    require(Path(str(command[0])).name in {"perllsp", "perllsp.exe"}, "server_binary.command must launch perllsp")
    require(launch_spec_contract.same_file_reference(binary["path"], command[0]),
            "server_binary.command[0] must target exactly the recorded server_binary.path; "
            "otherwise the identity would be attributed to a different executable")

    init = payload.get("session_initialize")
    require(isinstance(init, dict), "session_initialize must be an object")
    require(
        set(init) == {"origin", "request_sha256", "response_sha256", "observed_capabilities"},
        "session_initialize keys drifted from the v1 contract",
    )
    require(init["origin"] == CAPTURE_ORIGIN,
            f"session_initialize.origin must be {CAPTURE_ORIGIN}; synthetic sessions never close production claims")
    require(_is_sha256(init["request_sha256"]), "session_initialize.request_sha256 must capture the request bytes")
    require(_is_sha256(init["response_sha256"]), "session_initialize.response_sha256 must capture the response bytes")
    capabilities = init["observed_capabilities"]
    require(isinstance(capabilities, dict) and len(capabilities) >= 1,
            "observed_capabilities must record at least one capability presence entry")
    for name, present in capabilities.items():
        require(bool(re.fullmatch(r"[a-zA-Z][A-Za-z]*", str(name))), f"capability name {name!r} is not well formed")
        require(isinstance(present, bool), f"capability {name} must be a boolean presence value")
    present_capabilities = {str(name) for name, present in capabilities.items() if present}
    missing_core = REQUIRED_CAPABILITY_KEYS.difference(capabilities)
    require(not missing_core, f"observed_capabilities must judge the core surface: {sorted(missing_core)}")
    failed_core = sorted(REQUIRED_CAPABILITY_KEYS - present_capabilities)
    require(not failed_core, f"core capabilities were observed absent: {failed_core}")

    readiness = payload.get("repro_readiness")
    require(isinstance(readiness, dict), "repro_readiness must be an object")
    require(
        set(readiness) == {"origin", "fixture_opened", "first_diagnostics_settled", "evidence_sha256"},
        "repro_readiness keys drifted from the v1 contract",
    )
    require(readiness["origin"] == CAPTURE_ORIGIN, "repro_readiness.origin must be a live wire capture")
    require(readiness["fixture_opened"] is True, "fixture_opened must be true in an admissible receipt")
    require(readiness["first_diagnostics_settled"] is True,
            "first_diagnostics_settled must be true in an admissible receipt")
    require(_is_sha256(readiness["evidence_sha256"]), "repro_readiness.evidence_sha256 must digest the settle capture")

    taps = payload.get("provider_taps")
    require(isinstance(taps, list) and len(taps) >= 1, "provider_taps must be a non-empty array")
    seen_providers: set[str] = set()
    seen_suffixes: set[str] = set()
    for index, tap in enumerate(taps):
        label = f"provider_taps[{index}]"
        require(isinstance(tap, dict), f"{label} must be an object")
        require(
            set(tap) <= {"provider", "file_suffix", "origin", "result_sha256", "latency_ms"}
            and {"provider", "file_suffix", "origin", "result_sha256"} <= set(tap),
            f"{label} keys drifted from the v1 contract",
        )
        require(tap["origin"] == CAPTURE_ORIGIN, f"{label}.origin must be {CAPTURE_ORIGIN}")
        require(tap["provider"] in VALID_PROVIDERS, f"{label}.provider {tap['provider']!r} is not an admitted provider")
        require(tap["file_suffix"] in VALID_FILE_SUFFIXES, f"{label}.file_suffix must cover one admitted family")
        require(_is_sha256(tap["result_sha256"]), f"{label}.result_sha256 must digest the captured response bytes")
        if tap.get("latency_ms") is not None:
            latency = tap["latency_ms"]
            require(isinstance(latency, int) and not isinstance(latency, bool) and latency >= 0,
                    f"{label}.latency_ms must be a non-negative integer")
        seen_providers.add(str(tap["provider"]))
        seen_suffixes.add(str(tap["file_suffix"]))
    missing_providers = REQUIRED_CAPABILITY_KEYS.difference(seen_providers)
    require(not missing_providers, f"provider taps must observe the core surface: {sorted(missing_providers)}")
    missing_families = REQUIRED_FILE_SUFFIXES.difference(seen_suffixes)
    require(not missing_families, f"provider taps must exercise the .pl subject at minimum: {sorted(missing_families)}")

    ledger = payload.get("process_ledger")
    require(isinstance(ledger, dict), "process_ledger must be an object")
    require(set(ledger) == {"spawned_server_pids", "all_orderly_exited"},
            "process_ledger keys drifted from the v1 contract")
    pids = ledger["spawned_server_pids"]
    require(isinstance(pids, list) and len(pids) >= 1, "spawned_server_pids must list every spawned server pid")
    for pid in pids:
        require(isinstance(pid, int) and not isinstance(pid, bool) and pid >= 1,
                f"spawned_server_pids entries must be positive integers, saw {pid!r}")
    require(len(set(pids)) == len(pids), "spawned_server_pids must not repeat a pid")
    require(ledger["all_orderly_exited"] is True,
            "all_orderly_exited must be true; the supervised process must shut down cleanly")


def validate_bound_to_launch_spec(receipt: dict[str, Any], spec: dict[str, Any]) -> None:
    """Bind the receipt to its declared precondition.

    Recomputes the canonical launch-spec digest (must equal the recorded
    ``launch_spec_digest``) and rejects every drift between the declared
    subject and the observed subject: source SHA, IDE identity, plugin
    identity, and the exact binary reference + digest.
    """
    launch_spec_contract.validate(spec)
    expected_digest = launch_spec_contract.canonical_spec_digest(spec)
    require(receipt.get("launch_spec_digest") == expected_digest,
            "launch_spec_digest does not match the canonical digest of the supplied launch spec; "
            "the receipt is not bound to this declared precondition")

    spec_binary = spec["server_binary"]
    receipt_binary = receipt["server_binary"]
    drifts: list[str] = []
    if str(receipt.get("source_sha")) != str(spec.get("source_sha")):
        drifts.append(f"source_sha: {spec.get('source_sha')} declared vs {receipt.get('source_sha')} recorded")
    spec_ide, host = spec["declared_ide"], receipt["host"]
    for field in ("product", "build_number", "platform", "arch"):
        if str(spec_ide[field]) != str(host[field]):
            drifts.append(f"ide.{field}: {spec_ide[field]!r} declared vs {host[field]!r} recorded")
    spec_plugin, receipt_plugin = spec["lsp4ij_plugin"], receipt["lsp4ij_plugin"]
    for field in ("id", "version"):
        if str(spec_plugin[field]) != str(receipt_plugin[field]):
            drifts.append(f"plugin.{field}: {spec_plugin[field]!r} declared vs {receipt_plugin[field]!r} recorded")
    if spec_plugin.get("pinned_commit") is not None:
        if str(receipt_plugin.get("pinned_commit")) != str(spec_plugin["pinned_commit"]):
            drifts.append("plugin.pinned_commit drifted between declaration and observation")
    if not launch_spec_contract.same_file_reference(spec_binary["path"], receipt_binary["path"]):
        drifts.append(f"binary path: {spec_binary['path']!r} declared vs {receipt_binary['path']!r} recorded")
    if str(spec_binary["sha256"]) != str(receipt_binary["sha256"]):
        drifts.append(
            f"binary sha256: {spec_binary['sha256']} declared vs {receipt_binary['sha256']} recorded"
        )
    if not launch_spec_contract.same_file_reference(spec_binary["command"][0], receipt_binary["command"][0]):
        drifts.append(
            f"command target: {spec_binary['command'][0]!r} declared vs "
            f"{receipt_binary['command'][0]!r} recorded"
        )
    if drifts:
        raise ValueError("receipt/launch-spec subject drift: " + "; ".join(drifts))


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("usage: validate_lsp4ij_host_receipt.py RECEIPT.json LAUNCH_SPEC.json", file=sys.stderr)
        return 2
    receipt_path, spec_path = Path(argv[1]), Path(argv[2])
    try:
        payload = json.loads(receipt_path.read_text(encoding="utf-8"))
        require(isinstance(payload, dict), "receipt root must be an object")
        validate(payload)
        spec_payload = json.loads(spec_path.read_text(encoding="utf-8"))
        validate_bound_to_launch_spec(payload, spec_payload)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"{receipt_path}: {error}", file=sys.stderr)
        return 1
    print(f"validated {receipt_path} against {spec_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
