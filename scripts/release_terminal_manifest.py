#!/usr/bin/env python3
"""Build or verify the fail-closed release-candidate terminal manifest.

The manifest is the handoff between credentialless candidate construction and
the publication jobs.  It proves archive checksums, a non-empty SPDX document,
and one exact build-identity/receipt pair per archive target.  It is not itself
permission to publish; workflow dependencies provide that admission boundary.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

SCHEMA = "perl_lsp.release_terminal_manifest.v1"
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


class ManifestError(ValueError):
    """The candidate is incomplete, contradictory, or stale."""


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def load_object(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ManifestError(f"{label} is not valid UTF-8 JSON: {path}") from error
    if not isinstance(value, dict):
        raise ManifestError(f"{label} must be a JSON object: {path}")
    return value


def checksum_entries(path: Path) -> dict[str, str]:
    entries: dict[str, str] = {}
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeDecodeError) as error:
        raise ManifestError(f"checksum file is unavailable: {path}") from error
    for line in lines:
        parts = line.split(maxsplit=1)
        if len(parts) != 2 or not HEX64.fullmatch(parts[0]):
            raise ManifestError(f"malformed checksum row: {line!r}")
        name = parts[1].lstrip("*./")
        if not name or "/" in name or "\\" in name:
            raise ManifestError(f"checksum subject must be a flat file name: {name!r}")
        if name in entries:
            raise ManifestError(f"duplicate checksum subject: {name}")
        entries[name] = parts[0]
    if not entries:
        raise ManifestError("checksum file has no subjects")
    return entries


def evidence_rows(
    evidence: Path, source_sha: str, version: str
) -> tuple[list[str], str, list[dict[str, str]]]:
    identities = sorted(evidence.rglob("release-build-identity.json"))
    receipts = sorted(evidence.rglob("release-build-receipt.json"))
    topologies = sorted(evidence.rglob("release-topology.json"))
    if not identities or len(identities) != len(receipts) or len(identities) != len(topologies):
        raise ManifestError("build evidence requires one identity, receipt, and topology per target")

    targets: list[str] = []
    topology_digests: set[str] = set()
    for path in identities:
        row = load_object(path, "release build identity")
        if row.get("source_revision") != source_sha:
            raise ManifestError(f"build identity names another source: {path}")
        if row.get("release_version") != version:
            raise ManifestError(f"build identity names another version: {path}")
        target = row.get("target")
        if not isinstance(target, str) or target in targets:
            raise ManifestError(f"build identity target is missing or duplicated: {path}")
        topology_digest = row.get("release_topology_digest")
        if not isinstance(topology_digest, str) or not HEX64.fullmatch(topology_digest):
            raise ManifestError(f"build identity has invalid topology digest: {path}")
        targets.append(target)
        topology_digests.add(topology_digest)
        receipt_path = path.with_name("release-build-receipt.json")
        topology_path = path.with_name("release-topology.json")
        receipt = load_object(receipt_path, "release build receipt")
        if receipt.get("input") != row:
            raise ManifestError(f"build receipt does not bind its exact identity: {receipt_path}")
        if digest(topology_path) != topology_digest:
            raise ManifestError(f"build identity does not bind its exact topology: {path}")

    observed_topologies = {digest(path) for path in topologies}
    if len(topology_digests) != 1 or observed_topologies != topology_digests:
        raise ManifestError("build targets do not share one exact release topology")
    subjects = [
        {"path": path.relative_to(evidence.parent).as_posix(), "sha256": digest(path)}
        for path in sorted([*identities, *receipts, *topologies])
    ]
    return sorted(targets), next(iter(topology_digests)), subjects


def build_manifest(candidate: Path, source_sha: str, tag: str) -> dict[str, Any]:
    if not HEX40.fullmatch(source_sha):
        raise ManifestError("source SHA must be a lowercase 40-hex commit")
    if not tag.startswith("v") or len(tag) < 2:
        raise ManifestError("release tag must start with v")
    version = tag[1:]
    dist = candidate / "dist"
    evidence = candidate / "evidence"
    checksum_path = dist / "SHA256SUMS"
    sbom_path = dist / "sbom-spdx.json"
    entries = checksum_entries(checksum_path)

    archives: list[dict[str, str]] = []
    targets: list[str] = []
    for name, expected in sorted(entries.items()):
        prefix = f"perllsp-{version}-"
        if not name.startswith(prefix):
            raise ManifestError(f"unexpected archive subject: {name}")
        if name.endswith(".tar.gz"):
            target = name[len(prefix) : -len(".tar.gz")]
        elif name.endswith(".zip"):
            target = name[len(prefix) : -len(".zip")]
        else:
            raise ManifestError(f"unexpected archive subject: {name}")
        if not target:
            raise ManifestError(f"archive target is empty: {name}")
        path = dist / name
        if not path.is_file() or digest(path) != expected:
            raise ManifestError(f"archive checksum mismatch: {name}")
        if target in targets:
            raise ManifestError(f"duplicate archive target: {target}")
        targets.append(target)
        archives.append({"path": f"dist/{name}", "sha256": expected, "target": target})

    extras = sorted(
        path.name
        for path in dist.iterdir()
        if path.is_file()
        and (path.name.endswith(".zip") or path.name.endswith(".tar.gz"))
        and path.name not in entries
    )
    if extras:
        raise ManifestError(f"archives missing from SHA256SUMS: {extras}")

    sbom = load_object(sbom_path, "SPDX SBOM")
    packages = sbom.get("packages")
    if sbom.get("spdxVersion") != "SPDX-2.3" or not isinstance(packages, list) or not packages:
        raise ManifestError("SBOM must be SPDX-2.3 with a non-empty packages array")

    evidence_targets, topology_digest, evidence_subjects = evidence_rows(
        evidence, source_sha, version
    )
    if sorted(targets) != evidence_targets:
        raise ManifestError("archive targets differ from exact build-evidence targets")

    subjects = [row["path"] for row in archives]
    subjects.extend(row["path"] for row in evidence_subjects)
    subjects.extend(["dist/SHA256SUMS", "dist/sbom-spdx.json", "dist/release-terminal-manifest.json"])
    return {
        "schema_version": SCHEMA,
        "status": "eligible",
        "source_sha": source_sha,
        "tag": tag,
        "version": version,
        "release_topology_sha256": topology_digest,
        "archives": archives,
        "checksums": {"path": "dist/SHA256SUMS", "sha256": digest(checksum_path), "entries": len(entries)},
        "sbom": {"path": "dist/sbom-spdx.json", "sha256": digest(sbom_path), "packages": len(packages)},
        "build_evidence": {
            "targets": evidence_targets,
            "target_count": len(evidence_targets),
            "subjects": evidence_subjects,
        },
        "attestation_subject_paths": subjects,
        "claim_boundary": "Candidate inputs are complete and internally consistent; publication still requires successful provenance attestation and downstream workflow admission.",
    }


def canonical(value: dict[str, Any]) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        value = build_manifest(args.candidate, args.source_sha, args.tag)
        output = args.output or args.candidate / "dist" / "release-terminal-manifest.json"
        rendered = canonical(value)
        if args.check:
            if output.read_bytes() != rendered:
                raise ManifestError("terminal manifest is missing, stale, or non-canonical")
        else:
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_bytes(rendered)
    except (ManifestError, OSError) as error:
        print(f"release terminal manifest: NOT_PROVEN: {error}", file=sys.stderr)
        return 1
    print(f"release terminal manifest: eligible: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
