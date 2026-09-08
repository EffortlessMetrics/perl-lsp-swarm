#!/usr/bin/env python3
"""Bind validated build outputs to the exact post-strip archive members."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tarfile
import zipfile
from pathlib import Path
from typing import Any

SCHEMA = "perl_lsp.release_package_evidence.v1"
HEX40 = __import__("re").compile(r"^[0-9a-f]{40}$")


class PackageEvidenceError(ValueError):
    """Package lineage could not be proven."""


def digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def digest(path: Path) -> str:
    # Stream in bounded chunks: release archives and build outputs can be far
    # larger than a whole-file read wants to hold in memory.
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def canonical(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def load(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PackageEvidenceError(f"{label} is not valid JSON: {path}") from error
    if not isinstance(value, dict):
        raise PackageEvidenceError(f"{label} must be an object")
    return value


def archive_member(archive: Path, name: str) -> bytes:
    # Duplicate member names are refused: both zipfile and tarfile silently
    # resolve a duplicated name to the last entry, so evidence could be taken
    # from a member that installers and extractors may not select the same way.
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as bundle:
            matches = [info for info in bundle.infolist() if info.filename == name]
            if not matches:
                raise PackageEvidenceError(f"archive member is missing: {name}")
            if len(matches) > 1:
                raise PackageEvidenceError(f"archive has duplicate members: {name}")
            return bundle.read(matches[0])
    with tarfile.open(archive, "r:gz") as bundle:
        matches = [member for member in bundle.getmembers() if member.name == name]
        if not matches:
            raise PackageEvidenceError(f"archive member is missing: {name}")
        if len(matches) > 1:
            raise PackageEvidenceError(f"archive has duplicate members: {name}")
        member = matches[0]
        if member.issym() or member.islnk():
            raise PackageEvidenceError(f"archive member is a link, not a file: {name}")
        handle = bundle.extractfile(member)
        if handle is None:
            raise PackageEvidenceError(f"archive member is not a file: {name}")
        return handle.read()


def build(
    receipt_path: Path,
    package_dir: Path,
    archive: Path,
    source_sha: str,
    version: str,
    target: str,
) -> dict[str, Any]:
    if not HEX40.fullmatch(source_sha):
        raise PackageEvidenceError("source SHA must be lowercase 40-hex")
    receipt = load(receipt_path, "build receipt")
    identity = receipt.get("input")
    if receipt.get("status") != "pass" or not isinstance(identity, dict):
        raise PackageEvidenceError("build receipt is not passing")
    if identity.get("source_revision") != source_sha or identity.get("release_version") != version or identity.get("target") != target:
        raise PackageEvidenceError("build receipt names another package subject")
    rows = receipt.get("binaries")
    if not isinstance(rows, list) or len(rows) != 2:
        raise PackageEvidenceError("build receipt requires two binary records")

    workspace = Path.cwd().resolve(strict=True)
    package_dir = package_dir.resolve(strict=True)
    archive = archive.resolve(strict=True)
    for path, label in ((package_dir, "package directory"), (archive, "archive")):
        try:
            path.relative_to(workspace)
        except ValueError as error:
            raise PackageEvidenceError(f"{label} escapes workspace") from error
    expected_extension = ".zip" if "windows" in target else ".tar.gz"
    expected_archive = f"perllsp-{version}-{target}{expected_extension}"
    if archive.name != expected_archive:
        raise PackageEvidenceError(
            f"archive name is not canonical: expected {expected_archive}, got {archive.name}"
        )
    evidence: list[dict[str, str]] = []
    observed: set[str] = set()
    for row in rows:
        if not isinstance(row, dict):
            raise PackageEvidenceError("build receipt binary record is malformed")
        executable = row.get("executable")
        pre_strip = row.get("file_sha256")
        path_role = row.get("path_role")
        if not isinstance(executable, str) or executable in observed:
            raise PackageEvidenceError("build receipt binary identity is invalid")
        if not isinstance(pre_strip, str) or not isinstance(path_role, str):
            raise PackageEvidenceError("build receipt binary digest/path is missing")
        observed.add(executable)
        file_name = executable + (".exe" if "windows" in target else "")
        expected_path_role = f"target/{target}/release/{file_name}"
        if path_role != expected_path_role:
            raise PackageEvidenceError(f"build receipt path is not canonical: {path_role}")
        build_output = workspace / path_role
        if digest(build_output) != pre_strip:
            raise PackageEvidenceError(f"pre-strip build output digest mismatch: {executable}")
        packaged = package_dir / file_name
        post_strip = digest(packaged)
        # 7-Zip receives the expanded Windows file list and stores flat names;
        # tar receives the package directory and retains its top-level prefix.
        member_path = file_name if "windows" in target else f"{package_dir.name}/{file_name}"
        if digest_bytes(archive_member(archive, member_path)) != post_strip:
            raise PackageEvidenceError(f"archive member differs from packaged bytes: {member_path}")
        evidence.append(
            {
                "executable": executable,
                "member_path": member_path,
                "pre_strip_sha256": pre_strip,
                "post_strip_sha256": post_strip,
            }
        )
    if observed != {"perllsp", "perl-dap"}:
        raise PackageEvidenceError("package evidence requires perllsp and perl-dap")
    return {
        "schema_version": SCHEMA,
        "status": "pass",
        "source_revision": source_sha,
        "release_version": version,
        "target": target,
        "archive": {"name": archive.name, "sha256": digest(archive)},
        "binaries": evidence,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--package-dir", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        value = build(args.receipt, args.package_dir, args.archive, args.source_sha, args.version, args.target)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(canonical(value))
    except (OSError, PackageEvidenceError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"release package evidence: NOT_PROVEN: {error}", file=sys.stderr)
        return 1
    print(f"release package evidence: PASS: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
