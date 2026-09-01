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
import tarfile
import zipfile
from datetime import datetime
from pathlib import Path
from typing import Any

SCHEMA = "perl_lsp.release_terminal_manifest.v1"
IDENTITY_SCHEMA = "perl_lsp.release_build_identity.v1"
RECEIPT_SCHEMA = "perl_lsp.release_build_identity_receipt.v1"
PACKET_SCHEMA = "perl_lsp.binary_identity.v1"
PACKAGE_SCHEMA = "perl_lsp.release_package_evidence.v1"
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
SPDX_ID = re.compile(r"^SPDXRef-[A-Za-z0-9.-]+$")
SPDX_CREATED = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?Z$"
)


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


def require_exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise ManifestError(
            f"{label} fields differ: missing={sorted(expected - set(value))}, "
            f"unknown={sorted(set(value) - expected)}"
        )


def validate_spdx(value: dict[str, Any]) -> list[dict[str, Any]]:
    required_document = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
    }
    for key, expected in required_document.items():
        if value.get(key) != expected:
            raise ManifestError(f"SPDX document requires {key}={expected!r}")
    for key in ("name", "documentNamespace"):
        if not isinstance(value.get(key), str) or not value[key].strip():
            raise ManifestError(f"SPDX document requires nonempty {key}")
    if not value["documentNamespace"].startswith(("https://", "http://")):
        raise ManifestError("SPDX documentNamespace must be an absolute HTTP(S) URI")
    creation = value.get("creationInfo")
    if not isinstance(creation, dict):
        raise ManifestError("SPDX document requires creationInfo")
    creators = creation.get("creators")
    if not isinstance(creators, list) or not creators or not all(
        isinstance(creator, str) and creator.strip() for creator in creators
    ):
        raise ManifestError("SPDX creationInfo requires nonempty creators")
    created = creation.get("created")
    if not isinstance(created, str) or not SPDX_CREATED.fullmatch(created):
        raise ManifestError("SPDX creationInfo requires created timestamp")
    try:
        parsed = datetime.fromisoformat(created.replace("Z", "+00:00"))
        if parsed.utcoffset() is None or parsed.utcoffset().total_seconds() != 0:
            raise ValueError("SPDX timestamp is not UTC")
    except ValueError as error:
        raise ManifestError("SPDX creationInfo.created is not RFC3339-like") from error
    packages = value.get("packages")
    if not isinstance(packages, list) or not packages:
        raise ManifestError("SPDX document requires a nonempty packages array")
    package_ids: set[str] = set()
    for package in packages:
        if not isinstance(package, dict):
            raise ManifestError("SPDX package must be an object")
        for key in (
            "SPDXID",
            "name",
            "downloadLocation",
            "copyrightText",
            "licenseConcluded",
            "licenseDeclared",
        ):
            if not isinstance(package.get(key), str) or not package[key].strip():
                raise ManifestError(f"SPDX package requires nonempty {key}")
        if not SPDX_ID.fullmatch(package["SPDXID"]) or package["SPDXID"] in package_ids:
            raise ManifestError("SPDX package IDs must be unique SPDXRef values")
        package_ids.add(package["SPDXID"])
        if not isinstance(package.get("filesAnalyzed"), bool):
            raise ManifestError("SPDX package requires boolean filesAnalyzed")
    return packages


def validate_identity(value: dict[str, Any], source_sha: str, version: str) -> None:
    expected = {
        "schema_version", "repository", "release_version", "source_revision",
        "source_tree_digest", "target", "profile", "candidate_identity",
        "artifact_role", "product_identity_contract_digest", "release_topology_digest",
        "toolchain_digest",
    }
    require_exact_keys(value, expected, "release build identity")
    if value.get("schema_version") != IDENTITY_SCHEMA:
        raise ManifestError("release build identity schema is unsupported")
    if value.get("source_revision") != source_sha:
        raise ManifestError("build identity names another source")
    if value.get("release_version") != version:
        raise ManifestError("build identity names another version")
    if value.get("repository") not in {
        "EffortlessMetrics/perl-lsp",
        "EffortlessMetrics/perl-lsp-swarm",
    }:
        raise ManifestError("build identity repository is unsupported")
    if value.get("candidate_identity") != f"v{version}":
        raise ManifestError("build identity candidate is not the release tag")
    if not isinstance(value.get("target"), str) or "-" not in value["target"]:
        raise ManifestError("build identity target is malformed")
    if value.get("profile") != "release" or value.get("artifact_role") != "archive":
        raise ManifestError("build identity is not release/archive shaped")
    for key in (
        "source_tree_digest", "product_identity_contract_digest",
        "release_topology_digest", "toolchain_digest",
    ):
        if not isinstance(value.get(key), str) or not HEX64.fullmatch(value[key]):
            raise ManifestError(f"release build identity has invalid {key}")


def validate_binary_row(
    row: Any, identity: dict[str, Any], expected_executable: str, expected_role: str
) -> None:
    if not isinstance(row, dict):
        raise ManifestError("build receipt binary record must be an object")
    require_exact_keys(
        row,
        {"role", "executable", "path_role", "file_sha256", "packet_sha256", "packet"},
        "build receipt binary record",
    )
    if row["executable"] != expected_executable or row["role"] != expected_role:
        raise ManifestError("build receipt binary identity is incomplete or reordered")
    if not isinstance(row["path_role"], str) or not row["path_role"].endswith(expected_executable + (".exe" if "windows" in identity["target"] else "")):
        raise ManifestError("build receipt binary path is inconsistent")
    for key in ("file_sha256", "packet_sha256"):
        if not isinstance(row[key], str) or not HEX64.fullmatch(row[key]):
            raise ManifestError(f"build receipt binary has invalid {key}")
    packet = row["packet"]
    if not isinstance(packet, dict) or packet.get("schema_version") != PACKET_SCHEMA:
        raise ManifestError("build receipt binary packet schema is unsupported")
    require_exact_keys(
        packet,
        {"schema_version", "product", "binary", "build", "artifact", "compatibility", "limitations"},
        "build receipt binary packet",
    )
    if digest_bytes(canonical(packet)) != row["packet_sha256"]:
        raise ManifestError("build receipt binary packet digest mismatch")
    binary = packet.get("binary")
    build = packet.get("build")
    artifact = packet.get("artifact")
    if not isinstance(binary, dict) or not isinstance(build, dict) or not isinstance(artifact, dict):
        raise ManifestError("build receipt binary packet is incomplete")
    expected_package = "perllsp" if expected_executable == "perllsp" else "perl-dap"
    if binary != {
        "executable": expected_executable,
        "cargo_package": expected_package,
        "role": expected_role,
        "version": identity["release_version"],
    }:
        raise ManifestError("build receipt binary packet identity mismatch")
    if build != {
        "source_revision": identity["source_revision"],
        "source_tree_digest": identity["source_tree_digest"],
        "target": identity["target"],
        "profile": "release",
        "identity_state": "exact",
    }:
        raise ManifestError("build receipt binary packet source mismatch")
    if artifact != {"role": "archive", "digest": None, "candidate_identity": identity["candidate_identity"]}:
        raise ManifestError("build receipt binary packet artifact mismatch")
    if packet.get("product") != {
        "name": "perl-lsp",
        "public_repository": "EffortlessMetrics/perl-lsp",
        "development_repository": "EffortlessMetrics/perl-lsp-swarm",
    }:
        raise ManifestError("build receipt binary packet product mismatch")
    if packet.get("compatibility") != {
        "expected_product_identity_version": 1,
        "dap_posture": "preview",
    } or packet.get("limitations") != []:
        raise ManifestError("build receipt binary packet is not exact-compatible")


def validate_receipt(value: dict[str, Any], identity: dict[str, Any]) -> dict[str, dict[str, Any]]:
    expected = {
        "schema_version", "status", "input_sha256", "input", "runner",
        "build_execution", "build_commands", "binaries", "claim_boundary",
    }
    require_exact_keys(value, expected, "release build receipt")
    if value.get("schema_version") != RECEIPT_SCHEMA or value.get("status") != "pass":
        raise ManifestError("release build receipt is not a passing supported receipt")
    if value.get("input") != identity:
        raise ManifestError("release build receipt input differs from identity")
    if value.get("input_sha256") != digest_bytes(canonical(identity)):
        raise ManifestError("release build receipt input hash mismatch")
    if value.get("runner") not in {"cargo", "cross"} or value.get("build_execution") != "external_release_workflow":
        raise ManifestError("release build receipt execution authority is invalid")
    if not isinstance(value.get("claim_boundary"), str) or not value["claim_boundary"].strip():
        raise ManifestError("release build receipt claim boundary is missing")
    commands = value.get("build_commands")
    expected_commands = [
        [
            value["runner"], "build", "--locked", "--release", "--target",
            identity["target"], "-p", package, "--bin", binary,
        ]
        for package, binary in (("perllsp", "perllsp"), ("perl-dap", "perl-dap"))
    ]
    if commands != expected_commands:
        raise ManifestError("release build receipt commands differ from producer argv")
    binaries = value.get("binaries")
    if not isinstance(binaries, list) or len(binaries) != 2:
        raise ManifestError("release build receipt requires exactly two binaries")
    expected = (("perllsp", "server"), ("perl-dap", "dap"))
    result: dict[str, dict[str, Any]] = {}
    for row, (executable, role) in zip(binaries, expected, strict=True):
        validate_binary_row(row, identity, executable, role)
        result[executable] = row
    return result


def digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def archive_member_digest(archive: Path, member_path: str) -> str:
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as bundle:
            try:
                return digest_bytes(bundle.read(member_path))
            except KeyError as error:
                raise ManifestError(f"archive member is missing: {member_path}") from error
    with tarfile.open(archive, "r:gz") as bundle:
        try:
            member = bundle.getmember(member_path)
        except KeyError as error:
            raise ManifestError(f"archive member is missing: {member_path}") from error
        handle = bundle.extractfile(member)
        if handle is None:
            raise ManifestError(f"archive member is not a regular file: {member_path}")
        return digest_bytes(handle.read())


def validate_package_evidence(
    value: dict[str, Any], archive: Path, dist: Path, identity: dict[str, Any],
    receipt_binaries: dict[str, dict[str, Any]], checksum_digest: str,
) -> None:
    require_exact_keys(
        value,
        {"schema_version", "status", "source_revision", "release_version", "target", "archive", "binaries"},
        "release package evidence",
    )
    if value.get("schema_version") != PACKAGE_SCHEMA or value.get("status") != "pass":
        raise ManifestError("release package evidence is not a passing supported record")
    for key in ("source_revision", "release_version", "target"):
        identity_key = "source_revision" if key == "source_revision" else key
        if value.get(key) != identity[identity_key]:
            raise ManifestError(f"release package evidence {key} mismatch")
    archive_row = value.get("archive")
    if not isinstance(archive_row, dict) or set(archive_row) != {"name", "sha256"}:
        raise ManifestError("release package evidence archive is malformed")
    expected_extension = ".zip" if "windows" in identity["target"] else ".tar.gz"
    expected_name = (
        f"perllsp-{identity['release_version']}-{identity['target']}{expected_extension}"
    )
    archive_name = archive_row.get("name")
    if (
        archive_name != expected_name
        or not isinstance(archive_name, str)
        or Path(archive_name).name != archive_name
    ):
        raise ManifestError("release package evidence does not name its canonical archive")
    candidate_dist = dist.resolve(strict=True)
    archive_resolved = archive.resolve(strict=True)
    if archive.parent.resolve(strict=True) != candidate_dist or archive_resolved.parent != candidate_dist:
        raise ManifestError("release package evidence archive escapes candidate/dist")
    if archive_row != {"name": archive.name, "sha256": digest(archive)}:
        raise ManifestError("release package evidence archive digest mismatch")
    if archive_row["sha256"] != checksum_digest:
        raise ManifestError("release package evidence is not bound to its checksum row")
    binaries = value.get("binaries")
    if not isinstance(binaries, list) or len(binaries) != 2:
        raise ManifestError("release package evidence requires exactly two binaries")
    observed: set[str] = set()
    for row in binaries:
        if not isinstance(row, dict) or set(row) != {"executable", "member_path", "pre_strip_sha256", "post_strip_sha256"}:
            raise ManifestError("release package binary evidence is malformed")
        executable = row["executable"]
        if executable not in receipt_binaries or executable in observed:
            raise ManifestError("release package binary evidence identity is invalid")
        observed.add(executable)
        if row["pre_strip_sha256"] != receipt_binaries[executable]["file_sha256"]:
            raise ManifestError("release package evidence does not bind pre-strip bytes")
        if not isinstance(row["member_path"], str) or not isinstance(row["post_strip_sha256"], str) or not HEX64.fullmatch(row["post_strip_sha256"]):
            raise ManifestError("release package binary evidence digest is malformed")
        if archive_member_digest(archive, row["member_path"]) != row["post_strip_sha256"]:
            raise ManifestError(f"archive member digest differs from post-strip evidence: {row['member_path']}")


def evidence_rows(
    evidence: Path, dist: Path, archive_entries: dict[str, str], source_sha: str, version: str
) -> tuple[list[str], str, list[dict[str, str]]]:
    identities = sorted(evidence.rglob("release-build-identity.json"))
    receipts = sorted(evidence.rglob("release-build-receipt.json"))
    topologies = sorted(evidence.rglob("release-topology.json"))
    packages = sorted(evidence.rglob("release-package-evidence.json"))
    if not identities or not (len(identities) == len(receipts) == len(topologies) == len(packages)):
        raise ManifestError("build evidence requires one identity, receipt, topology, and package record per target")

    targets: list[str] = []
    topology_digests: set[str] = set()
    for path in identities:
        row = load_object(path, "release build identity")
        validate_identity(row, source_sha, version)
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
        receipt_binaries = validate_receipt(receipt, row)
        if digest(topology_path) != topology_digest:
            raise ManifestError(f"build identity does not bind its exact topology: {path}")
        package_path = path.with_name("release-package-evidence.json")
        package = load_object(package_path, "release package evidence")
        archive_name = package.get("archive", {}).get("name") if isinstance(package.get("archive"), dict) else None
        if not isinstance(archive_name, str):
            raise ManifestError("release package evidence omits archive name")
        expected_extension = ".zip" if "windows" in row["target"] else ".tar.gz"
        expected_archive = f"perllsp-{version}-{row['target']}{expected_extension}"
        if archive_name != expected_archive or Path(archive_name).name != archive_name:
            raise ManifestError("release package evidence does not name its canonical archive")
        if archive_name not in archive_entries:
            raise ManifestError("release package evidence has no one-to-one checksum row")
        validate_package_evidence(
            package,
            dist / archive_name,
            dist,
            row,
            receipt_binaries,
            archive_entries[archive_name],
        )

    observed_topologies = {digest(path) for path in topologies}
    if len(topology_digests) != 1 or observed_topologies != topology_digests:
        raise ManifestError("build targets do not share one exact release topology")
    subjects = [
        {"path": path.relative_to(evidence.parent).as_posix(), "sha256": digest(path)}
        for path in sorted([*identities, *receipts, *topologies, *packages])
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
    candidate_root = candidate.resolve(strict=True)
    if dist.resolve(strict=True).parent != candidate_root:
        raise ManifestError("candidate dist directory escapes candidate root")
    if evidence.resolve(strict=True).parent != candidate_root:
        raise ManifestError("candidate evidence directory escapes candidate root")
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
    packages = validate_spdx(sbom)

    evidence_targets, topology_digest, evidence_subjects = evidence_rows(
        evidence, dist, entries, source_sha, version
    )
    if sorted(targets) != evidence_targets:
        raise ManifestError("archive targets differ from exact build-evidence targets")

    subjects = [row["path"] for row in archives]
    subjects.extend(row["path"] for row in evidence_subjects)
    subjects.extend(["dist/SHA256SUMS", "dist/sbom-spdx.json", "dist/release-terminal-manifest.json"])
    if (candidate / "release_notes.md").is_file():
        subjects.append("release_notes.md")
    # Resolve once so symlinks inside the candidate cannot escape the prefix
    # stripped by relative_to, and so a relative candidate argument still
    # yields canonical POSIX member paths.
    candidate_root = candidate.resolve()
    actual_files = {
        path.relative_to(candidate_root).as_posix()
        for path in candidate_root.rglob("*")
        if path.is_file()
    }
    admitted_files = set(subjects) | {"attestation-subjects.sha256"}
    unexpected_files = sorted(actual_files - admitted_files)
    if unexpected_files:
        raise ManifestError(f"candidate contains unadmitted attestation drift: {unexpected_files}")
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
        "claim_boundary": (
            "Candidate inputs are complete and internally consistent; publication still "
            "requires provenance attestation and immutable exact-tag authority. Structural "
            "graph tests do not prove hosted zero mutation; #8576 remains NOT_PROVEN until "
            "its runtime rehearsal."
        ),
    }


def canonical(value: dict[str, Any]) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def expected_inventory(candidate: Path, manifest: dict[str, Any]) -> bytes:
    paths = manifest.get("attestation_subject_paths")
    if not isinstance(paths, list) or not paths or len(paths) != len(set(paths)):
        raise ManifestError("terminal manifest attestation subjects are not a closed unique list")
    lines: list[str] = []
    root = candidate.resolve(strict=True)
    for relative in sorted(paths):
        if not isinstance(relative, str):
            raise ManifestError("terminal manifest attestation subject path is malformed")
        path = (root / relative).resolve(strict=True)
        try:
            path.relative_to(root)
        except ValueError as error:
            raise ManifestError("attestation subject escapes candidate") from error
        lines.append(f"{digest(path)}  {relative}\n")
    return "".join(lines).encode("utf-8")


def write_outputs(candidate: Path, source_sha: str, tag: str) -> tuple[Path, Path]:
    manifest = build_manifest(candidate, source_sha, tag)
    output = candidate / "dist" / "release-terminal-manifest.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(canonical(manifest))
    inventory = candidate / "attestation-subjects.sha256"
    inventory.write_bytes(expected_inventory(candidate, manifest))
    return output, inventory


def check_outputs(candidate: Path, source_sha: str, tag: str) -> tuple[Path, Path]:
    manifest = build_manifest(candidate, source_sha, tag)
    output = candidate / "dist" / "release-terminal-manifest.json"
    inventory = candidate / "attestation-subjects.sha256"
    if output.read_bytes() != canonical(manifest):
        raise ManifestError("terminal manifest is missing, stale, or non-canonical")
    if inventory.read_bytes() != expected_inventory(candidate, manifest):
        raise ManifestError("attestation subject inventory is missing, stale, or contains drift")
    return output, inventory


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        if args.output:
            raise ManifestError("custom output is unsupported for the closed terminal inventory")
        if args.check:
            output, inventory = check_outputs(args.candidate, args.source_sha, args.tag)
        else:
            output, inventory = write_outputs(args.candidate, args.source_sha, args.tag)
    except (ManifestError, OSError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"release terminal manifest: NOT_PROVEN: {error}", file=sys.stderr)
        return 1
    print(f"release terminal manifest: eligible: {output}; subjects: {inventory}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
