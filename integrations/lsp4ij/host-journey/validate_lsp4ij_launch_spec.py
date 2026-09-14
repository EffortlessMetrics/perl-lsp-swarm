#!/usr/bin/env python3
"""Validate one LSP4IJ declared-host launch spec (the run precondition).

The launch spec is configuration, never evidence. It declares exactly one
hermetic IntelliJ-platform IDE subject plus the pinned LSP4IJ plugin, the
exact current-source ``perllsp --stdio`` binary, sandboxed IDE state roots,
and the repro fixture project before anything is launched. The matching
post-run evidence contract lives in ``validate_lsp4ij_host_receipt.py``.

Exit codes: 0 valid, 1 invalid spec, 2 usage error.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import sys
from pathlib import Path
from typing import Any

# docs/EDITORS/INTELLIJ_IDEA_SETUP.md declares 0.20.0 and newer as the
# maintained LSP4IJ line; a declared subject below it is not reviewable here.
MIN_LSP4IJ_VERSION = (0, 20, 0)

VALID_PLATFORMS = {"linux", "macos", "windows"}
VALID_ARCHES = {"x64", "arm64"}
VALID_PLUGIN_SOURCES = {"released_marketplace", "pinned_release_archive"}

SANDBOX_ROOTS = ("config_root", "system_root", "plugins_root", "log_root")

KNOWN_KEYS = {
    "schema_version",
    "stage",
    "source_sha",
    "declared_ide",
    "lsp4ij_plugin",
    "server_binary",
    "sandbox",
    "fixture_project",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def canonical_spec_bytes(payload: dict[str, Any]) -> bytes:
    """The one canonical byte string a receipt's launch_spec_digest is bound to."""
    return json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def canonical_spec_digest(payload: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_spec_bytes(payload)).hexdigest()


def same_file_reference(left: Any, right: Any) -> bool:
    """Placeholder-tolerant executable binding: after OS-normalizing separators
    and case, the declared binary path and the launched command target must be
    the identical reference. Placeholders are allowed only when both fields
    spell the exact same reference."""
    if not isinstance(left, str) or not isinstance(right, str):
        return False
    return os.path.normcase(os.path.normpath(left)) == os.path.normcase(os.path.normpath(right))


def _is_sha256(value: Any) -> bool:
    return isinstance(value, str) and bool(re.fullmatch(r"[0-9a-f]{64}", value))


def _is_source_sha(value: Any) -> bool:
    return isinstance(value, str) and bool(re.fullmatch(r"[0-9a-f]{40}", value))


def _is_build_number(value: Any) -> bool:
    return isinstance(value, str) and bool(re.fullmatch(r"[A-Z]{2}-[0-9]{3,4}(\.[0-9]+)+", value))


def _is_plugin_version(value: Any) -> bool:
    return isinstance(value, str) and bool(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", value))


def validate(payload: dict[str, Any]) -> None:
    unexpected = sorted(set(payload) - KNOWN_KEYS)
    require(not unexpected, f"launch spec contains keys outside the v1 contract: {unexpected}")
    require(payload.get("schema_version") == 1, "schema_version must be 1")
    require(payload.get("stage") == "declared_host", "stage must remain declared_host")
    require(_is_source_sha(payload.get("source_sha")), "source_sha must be a full lowercase Git commit SHA")

    ide = payload.get("declared_ide")
    require(isinstance(ide, dict), "declared_ide must be an object")
    require(set(ide) == {"product", "build_number", "platform", "arch", "distribution_root"},
            "declared_ide keys drifted from the v1 contract")
    require(isinstance(ide["product"], str) and ide["product"].strip() != "", "declared_ide.product must be non-empty")
    require(_is_build_number(ide["build_number"]), 'declared_ide.build_number must look like "IC-241.18034.62"')
    require(ide["platform"] in VALID_PLATFORMS, f"declared_ide.platform must be in {sorted(VALID_PLATFORMS)}")
    require(ide["arch"] in VALID_ARCHES, f"declared_ide.arch must be in {sorted(VALID_ARCHES)}")
    require(isinstance(ide["distribution_root"], str) and len(ide["distribution_root"]) >= 2,
            "declared_ide.distribution_root must point at the exact declared IDE installation")

    plugin = payload.get("lsp4ij_plugin")
    require(isinstance(plugin, dict), "lsp4ij_plugin must be an object")
    require(plugin.get("id") == "com.redhat.devtools.lsp4ij", "unexpected LSP4IJ plugin id")
    require(plugin.get("upstream_repository") == "redhat-developer/lsp4ij", "unexpected LSP4IJ upstream repository")
    require(_is_plugin_version(plugin.get("version")), "lsp4ij_plugin.version must be a three-part semver string")
    version = tuple(int(part) for part in str(plugin["version"]).split("."))
    require(version >= MIN_LSP4IJ_VERSION,
            f"lsp4ij_plugin.version {plugin['version']} is below the maintained line "
            f"{'.'.join(str(p) for p in MIN_LSP4IJ_VERSION)} (docs/EDITORS/INTELLIJ_IDEA_SETUP.md)")
    require(plugin.get("source") in VALID_PLUGIN_SOURCES, "lsp4ij_plugin.source must name an admitted provenance")
    if plugin.get("pinned_commit") is not None:
        require(_is_source_sha(plugin["pinned_commit"]), "lsp4ij_plugin.pinned_commit must be a full Git commit SHA")
    if plugin.get("source") == "pinned_release_archive":
        require(_is_source_sha(plugin.get("pinned_commit")),
                "a pinned_release_archive declaration must also pin its exact upstream commit")

    binary = payload.get("server_binary")
    require(isinstance(binary, dict), "server_binary must be an object")
    require(set(binary) == {"path", "command", "sha256"}, "server_binary keys drifted from the v1 contract")
    require(isinstance(binary["path"], str) and binary["path"] != "", "server_binary.path must be non-empty")
    require(_is_sha256(binary["sha256"]),
            "server_binary.sha256 is required and must be a lowercase SHA-256 digest of the declared binary")
    command = binary["command"]
    require(isinstance(command, list) and len(command) == 2, "server_binary.command must have two entries")
    require(command[1] == "--stdio", "server_binary.command must use stdio")
    require(Path(str(command[0])).name in {"perllsp", "perllsp.exe"}, "server_binary.command must launch perllsp")
    require(same_file_reference(binary["path"], command[0]),
            "server_binary.command[0] must target exactly the declared server_binary.path; "
            "a declared path with a different launched executable invalidates the subject")

    sandbox = payload.get("sandbox")
    require(isinstance(sandbox, dict), "sandbox must be an object")
    require(set(sandbox) == set(SANDBOX_ROOTS), "sandbox roots drifted from the v1 contract")
    for root_name in SANDBOX_ROOTS:
        require(isinstance(sandbox[root_name], str) and sandbox[root_name] != "",
                f"sandbox.{root_name} must be a non-empty path")
    roots = [str(sandbox[name]) for name in SANDBOX_ROOTS]
    require(len(set(roots)) == len(roots), "sandbox roots must be pairwise distinct run-owned directories")

    fixture = payload.get("fixture_project")
    require(isinstance(fixture, dict), "fixture_project must be an object")
    require(isinstance(fixture.get("root"), str) and bool(re.search(r"host-fixture$", fixture["root"])),
            "fixture_project.root must end in host-fixture")
    if fixture.get("sha256") is not None:
        require(_is_sha256(fixture["sha256"]), "fixture_project.sha256 must be a lowercase SHA-256 digest")


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: validate_lsp4ij_launch_spec.py LAUNCH_SPEC.json", file=sys.stderr)
        return 2
    path = Path(argv[1])
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
        require(isinstance(payload, dict), "launch spec root must be an object")
        validate(payload)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"{path}: {error}", file=sys.stderr)
        return 1
    print(f"validated {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
