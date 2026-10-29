"""Explicit offline RC identity projection; never allocates an extension version."""

import re
from typing import Any

NUMERIC_VERSION = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
RC_VERSION = NUMERIC_VERSION + r"-rc\.[1-9][0-9]*"


def mapped_vsix_identity(mapping: Any, package: dict, release: str, prepared: str) -> dict:
    """Validate supplied identity against the actual prepared package and subject."""
    if not isinstance(mapping, dict) or set(mapping) != {"extension", "candidate", "preRelease"}:
        raise ValueError("VSIX mapping requires exactly extension, candidate, preRelease")
    extension, candidate = mapping["extension"], mapping["candidate"]
    if not isinstance(extension, dict) or set(extension) != {"id", "version", "sourceSha"}:
        raise ValueError("VSIX mapping extension identity has invalid fields")
    if not isinstance(candidate, dict) or set(candidate) != {"id", "release", "sourceSha"}:
        raise ValueError("VSIX mapping candidate identity has invalid fields")
    if not isinstance(release, str) or re.fullmatch(RC_VERSION, release) is None:
        raise ValueError("mapped release must be a complete canonical X.Y.Z-rc.N identity")
    if not isinstance(prepared, str) or re.fullmatch(r"[0-9a-f]{40}", prepared) is None:
        raise ValueError("mapped VSIX requires a non-null prepared source SHA")
    if mapping["preRelease"] is not True:
        raise ValueError("mapped RC VSIX requires preRelease=true")
    for key in ("publisher", "name"):
        if not isinstance(package.get(key), str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", package[key]):
            raise ValueError(f"extension {key} must be a safe manifest identity")
    version = package.get("version")
    if not isinstance(version, str) or re.fullmatch(NUMERIC_VERSION, version) is None:
        raise ValueError("extension version must be canonical numeric X.Y.Z")
    if extension != {"id": f"{package['publisher']}.{package['name']}", "version": version, "sourceSha": prepared}:
        raise ValueError("VSIX mapping differs from prepared extension identity")
    if not isinstance(candidate["id"], str) or not candidate["id"].strip():
        raise ValueError("VSIX candidate id must be non-empty")
    if candidate["release"] != release or candidate["sourceSha"] != prepared:
        raise ValueError("VSIX mapping differs from exact RC/prepared subject")
    return {
        "candidate_id": candidate["id"], "publisher": package["publisher"],
        "name": package["name"], "version": version, "pre_release": True,
        "asset_name": f"{package['name']}-{version}-{release}.vsix",
    }


def mapping_from_topology(topology: dict) -> dict:
    """Project existing authority fields; no new mapping is inferred."""
    if type(topology.get("schema")) not in (int, float) or topology["schema"] != 4:
        raise ValueError("mapped VSIX requires explicit topology v4")
    vsix = topology.get("vsix", {})
    return {
        "extension": {"id": f"{vsix.get('publisher')}.{vsix.get('name')}", "version": vsix.get("version"), "sourceSha": topology.get("prepared_swarm_sha")},
        "candidate": {"id": vsix.get("candidate_id"), "release": topology.get("release"), "sourceSha": topology.get("prepared_swarm_sha")},
        "preRelease": vsix.get("pre_release"),
    }
