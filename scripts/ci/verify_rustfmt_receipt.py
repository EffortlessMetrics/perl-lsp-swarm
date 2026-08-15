#!/usr/bin/env python3
"""Independently verify a candidate-bound rustfmt_check.v1 receipt."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import stat
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any, Sequence

SHA_RE = re.compile(r"^[0-9a-f]{40}$")
PINNED_RUST_RELEASE = "1.95.0"


class VerificationError(RuntimeError):
    pass


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def digest_file(root: Path, value: str, label: str) -> str:
    candidate = root / value if not Path(value).is_absolute() else Path(value)
    candidate_metadata = candidate.lstat()
    if stat.S_ISLNK(candidate_metadata.st_mode):
        raise VerificationError(f"{label} must be a regular non-symlink file")
    path = candidate.resolve()
    try:
        path.relative_to(root)
    except ValueError as error:
        raise VerificationError(f"{label} escapes repository") from error
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode):
        raise VerificationError(f"{label} must be a regular non-symlink file")
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def run(command: Sequence[str], root: Path) -> str:
    try:
        result = subprocess.run(command, cwd=root, text=True, capture_output=True, timeout=15, check=False)
    except (OSError, subprocess.SubprocessError) as error:
        raise VerificationError(f"tool call failed: {command[0]}: {error}") from error
    if result.returncode != 0:
        raise VerificationError(f"tool call exited {result.returncode}: {command[0]}")
    return result.stdout.strip()


def require_sha(value: object, label: str) -> str:
    if not isinstance(value, str) or not SHA_RE.fullmatch(value):
        raise VerificationError(f"{label} must be a full lowercase SHA")
    return value


def require_nonempty_string(row: dict[str, object], key: str, label: str) -> str:
    value = row.get(key)
    if not isinstance(value, str) or not value:
        raise VerificationError(f"{label} must be a nonempty string")
    return value


def metadata_path(root: Path, value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise VerificationError(f"{label} must be a nonempty string")
    candidate = Path(value)
    if not candidate.is_absolute():
        raise VerificationError(f"{label} must be absolute")
    try:
        candidate_metadata = candidate.lstat()
        resolved = candidate.resolve(strict=True)
        relative = resolved.relative_to(root)
    except (OSError, ValueError) as error:
        raise VerificationError(f"{label} is outside or unavailable in the repository") from error
    if stat.S_ISLNK(candidate_metadata.st_mode) or not stat.S_ISREG(resolved.lstat().st_mode):
        raise VerificationError(f"{label} must be a regular non-symlink file")
    return relative.as_posix()


def derive_cargo_inventory(cargo: Path, root: Path) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    output = run(
        [str(cargo), "metadata", "--no-deps", "--locked", "--format-version", "1"],
        root,
    )
    try:
        metadata: object = json.loads(output)
    except json.JSONDecodeError as error:
        raise VerificationError(f"cargo metadata returned malformed JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise VerificationError("cargo metadata root must be an object")
    packages = metadata.get("packages")
    members = metadata.get("workspace_members")
    workspace_root = metadata.get("workspace_root")
    if not isinstance(packages, list) or not isinstance(members, list):
        raise VerificationError("cargo metadata omitted packages or workspace_members")
    if not isinstance(workspace_root, str):
        raise VerificationError("cargo metadata omitted workspace_root")
    try:
        if Path(workspace_root).resolve(strict=True) != root:
            raise VerificationError("cargo metadata workspace_root does not match repository")
    except OSError as error:
        raise VerificationError("cargo metadata workspace_root is unavailable") from error
    if not members or any(not isinstance(member, str) or not member for member in members):
        raise VerificationError("cargo metadata workspace_members must contain nonempty strings")
    member_ids = set(members)
    if len(member_ids) != len(members):
        raise VerificationError("cargo metadata workspace_members must be unique")

    manifests: list[dict[str, object]] = []
    targets: list[dict[str, object]] = []
    observed_members: set[str] = set()
    for package in packages:
        if not isinstance(package, dict):
            raise VerificationError("cargo metadata packages must contain objects")
        package_id = package.get("id")
        if not isinstance(package_id, str) or not package_id:
            raise VerificationError("cargo metadata package id must be a nonempty string")
        if package_id not in member_ids:
            continue
        if package_id in observed_members:
            raise VerificationError("cargo metadata duplicates a workspace package")
        observed_members.add(package_id)
        package_name = require_nonempty_string(package, "name", "cargo metadata package name")
        manifest = metadata_path(
            root,
            package.get("manifest_path"),
            "cargo metadata manifest path",
        )
        manifests.append({"manifest": manifest, "package": package_name})
        package_targets = package.get("targets")
        if not isinstance(package_targets, list):
            raise VerificationError("cargo metadata workspace package omitted targets")
        for target in package_targets:
            if not isinstance(target, dict):
                raise VerificationError("cargo metadata targets must contain objects")
            target_name = require_nonempty_string(target, "name", "cargo metadata target name")
            kinds = target.get("kind")
            if not isinstance(kinds, list) or not kinds or any(
                not isinstance(kind, str) or not kind for kind in kinds
            ):
                raise VerificationError("cargo metadata target kind must contain nonempty strings")
            targets.append(
                {
                    "package": package_name,
                    "name": target_name,
                    "kind": sorted(kinds),
                    "source": metadata_path(
                        root,
                        target.get("src_path"),
                        "cargo metadata target source",
                    ),
                    "manifest": manifest,
                }
            )
    missing_members = member_ids - observed_members
    if missing_members:
        raise VerificationError("cargo metadata omitted workspace package records")
    manifests.sort(key=lambda row: (str(row["manifest"]), str(row["package"])))
    targets.sort(
        key=lambda row: (
            str(row["manifest"]),
            str(row["source"]),
            str(row["name"]),
        )
    )
    if not manifests or not targets:
        raise VerificationError("cargo metadata selected no workspace manifests or targets")
    return manifests, targets


def verify(args: argparse.Namespace) -> None:
    root = args.root.resolve(strict=True)
    if args.receipt.is_symlink():
        raise VerificationError("receipt must be a regular non-symlink file")
    receipt_path = args.receipt.resolve(strict=True)
    if not receipt_path.is_file():
        raise VerificationError("receipt must be a regular non-symlink file")
    if receipt_path.stat().st_size > 16 * 1024 * 1024:
        raise VerificationError("receipt exceeds 16 MiB verification bound")
    try:
        receipt: object = json.loads(receipt_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"receipt parse failed: {error}") from error
    if not isinstance(receipt, dict):
        raise VerificationError("receipt must be a JSON object")
    if receipt.get("schema_version") != "rustfmt_check.v1" or receipt.get("receipt_kind") != "rustfmt_check":
        raise VerificationError("receipt schema or kind mismatch")
    if receipt.get("result") != "pass":
        raise VerificationError("receipt result is not pass")

    expected_sha = require_sha(args.candidate_sha, "candidate SHA")
    expected_tree = require_sha(args.candidate_tree_sha, "candidate tree SHA")
    subject = receipt.get("subject", {})
    if not isinstance(subject, dict):
        raise VerificationError("receipt subject must be a JSON object")
    if subject != {"repository_sha": expected_sha, "repository_tree_sha": expected_tree}:
        raise VerificationError("receipt subject does not match expected candidate")
    if run(["git", "rev-parse", "HEAD^{commit}"], root) != expected_sha:
        raise VerificationError("live HEAD does not match candidate")
    if run(["git", "rev-parse", "HEAD^{tree}"], root) != expected_tree:
        raise VerificationError("live tree does not match candidate")
    if run(["git", "status", "--porcelain=v1", "--untracked-files=all"], root):
        raise VerificationError("live candidate worktree is not clean")

    claimed_digest = receipt.get("evidence_sha256")
    unsigned = {key: value for key, value in receipt.items() if key != "evidence_sha256"}
    actual_digest = "sha256:" + hashlib.sha256(canonical_json(unsigned)).hexdigest()
    if claimed_digest != actual_digest:
        raise VerificationError("canonical evidence digest mismatch")

    inputs = receipt.get("inputs", {})
    if not isinstance(inputs, dict):
        raise VerificationError("receipt inputs must be a JSON object")
    required_inputs = {
        "cargo_toml_sha256": "Cargo.toml",
        "rust_toolchain_sha256": "rust-toolchain.toml",
        "rustfmt_toml_sha256": "rustfmt.toml",
        "producer_sha256": str(args.producer),
    }
    for key, path in required_inputs.items():
        if inputs.get(key) != digest_file(root, path, key):
            raise VerificationError(f"input digest mismatch: {key}")
    lock = root / "Cargo.lock"
    expected_lock = digest_file(root, "Cargo.lock", "cargo_lock_sha256") if lock.exists() else None
    if inputs.get("cargo_lock_sha256") != expected_lock:
        raise VerificationError("input digest mismatch: cargo_lock_sha256")

    cargo_version = run([str(args.cargo), "--version"], root)
    rustfmt_version = run([str(args.rustfmt), "--version"], root)
    rustc_version = run([str(args.rustc), "-Vv"], root)
    if inputs.get("cargo_version") != cargo_version or inputs.get("rustfmt_version") != rustfmt_version:
        raise VerificationError("selected tool version mismatch")
    if inputs.get("rustc_version_verbose") != rustc_version:
        raise VerificationError("selected rustc version mismatch")
    release = next((line.removeprefix("release: ") for line in rustc_version.splitlines() if line.startswith("release: ")), None)
    try:
        pinned_channel = tomllib.loads((root / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
    except (OSError, UnicodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        raise VerificationError(f"repository toolchain pin is invalid: {error}") from error
    if pinned_channel != PINNED_RUST_RELEASE or release != pinned_channel:
        raise VerificationError("selected Rust toolchain is not pinned release 1.95.0")

    workspace = receipt.get("workspace", {})
    if not isinstance(workspace, dict):
        raise VerificationError("receipt workspace must be a JSON object")
    manifests = workspace.get("manifests")
    targets = workspace.get("targets")
    runs = receipt.get("runs")
    if not all(isinstance(value, list) and value for value in (manifests, targets, runs)):
        raise VerificationError("workspace manifests, targets, and runs must be nonempty")
    if not all(isinstance(row, dict) for rows in (manifests, targets, runs) for row in rows):
        raise VerificationError("workspace manifests, targets, and runs must contain objects")
    if workspace.get("manifest_count") != len(manifests) or workspace.get("target_count") != len(targets):
        raise VerificationError("workspace counts are incoherent")
    manifest_names = [
        require_nonempty_string(row, "manifest", "workspace manifest identity")
        for row in manifests
    ]
    manifest_packages = [
        require_nonempty_string(row, "package", "workspace manifest package")
        for row in manifests
    ]
    run_names = [
        require_nonempty_string(row, "manifest", "formatter run manifest identity")
        for row in runs
    ]
    run_packages = [
        require_nonempty_string(row, "package", "formatter run package")
        for row in runs
    ]
    manifest_identities = set(zip(manifest_packages, manifest_names, strict=True))
    if len(set(manifest_names)) != len(manifests) or len(set(manifest_packages)) != len(manifests):
        raise VerificationError("workspace manifests must be unique and coherent")
    target_sources = [
        require_nonempty_string(row, "source", "workspace target source")
        for row in targets
    ]
    target_identities = []
    for row in targets:
        target_package = require_nonempty_string(row, "package", "workspace target package")
        require_nonempty_string(row, "name", "workspace target name")
        target_manifest = require_nonempty_string(row, "manifest", "workspace target manifest")
        target_identities.append((target_package, target_manifest))
        kinds = row.get("kind")
        if not isinstance(kinds, list) or not kinds or any(
            not isinstance(kind, str) or not kind for kind in kinds
        ):
            raise VerificationError("workspace target kind must contain nonempty strings")
    if len(set(target_sources)) != len(targets):
        raise VerificationError("workspace targets must be unique and coherent")
    if any(identity not in manifest_identities for identity in target_identities):
        raise VerificationError("workspace target package and manifest must match a workspace manifest")
    authoritative_manifests, authoritative_targets = derive_cargo_inventory(args.cargo, root)
    if manifests != authoritative_manifests:
        raise VerificationError("receipt workspace manifests do not match current cargo metadata")
    if targets != authoritative_targets:
        raise VerificationError("receipt workspace targets do not match current cargo metadata")
    if (
        run_names != manifest_names
        or run_packages != manifest_packages
        or any(row.get("status") != "pass" for row in runs)
    ):
        raise VerificationError("each manifest must have exactly one successful run")
    if receipt.get("findings") != [] or receipt.get("instrument_failures") != []:
        raise VerificationError("passing receipt contains findings or instrument failures")
    if receipt.get("findings_truncated") is not False:
        raise VerificationError("passing receipt reports truncated findings")


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--candidate-tree-sha", required=True)
    parser.add_argument("--producer", type=Path, required=True)
    parser.add_argument("--rustfmt", type=Path, required=True)
    parser.add_argument("--rustc", type=Path, required=True)
    parser.add_argument("--cargo", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    try:
        verify(parse_args(argv))
    except (VerificationError, OSError, TypeError, ValueError) as error:
        print(f"rustfmt receipt verification failed: {error}", file=sys.stderr)
        return 2
    print("rustfmt receipt verification passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
