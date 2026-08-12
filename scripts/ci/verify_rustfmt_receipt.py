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
from pathlib import Path
from typing import Any, Sequence

SHA_RE = re.compile(r"^[0-9a-f]{40}$")
PINNED_RUSTFMT = "1.95.0"


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
        receipt: dict[str, Any] = json.loads(receipt_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"receipt parse failed: {error}") from error
    if receipt.get("schema_version") != "rustfmt_check.v1" or receipt.get("receipt_kind") != "rustfmt_check":
        raise VerificationError("receipt schema or kind mismatch")
    if receipt.get("result") != "pass":
        raise VerificationError("receipt result is not pass")

    expected_sha = require_sha(args.candidate_sha, "candidate SHA")
    expected_tree = require_sha(args.candidate_tree_sha, "candidate tree SHA")
    subject = receipt.get("subject", {})
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
    if inputs.get("cargo_version") != cargo_version or inputs.get("rustfmt_version") != rustfmt_version:
        raise VerificationError("selected tool version mismatch")
    if not re.match(rf"^rustfmt {re.escape(PINNED_RUSTFMT)}(?:[ -]|$)", rustfmt_version):
        raise VerificationError("rustfmt is not pinned 1.95.0")

    workspace = receipt.get("workspace", {})
    manifests = workspace.get("manifests")
    targets = workspace.get("targets")
    runs = receipt.get("runs")
    if not all(isinstance(value, list) and value for value in (manifests, targets, runs)):
        raise VerificationError("workspace manifests, targets, and runs must be nonempty")
    if workspace.get("manifest_count") != len(manifests) or workspace.get("target_count") != len(targets):
        raise VerificationError("workspace counts are incoherent")
    manifest_names = [row.get("manifest") for row in manifests if isinstance(row, dict)]
    run_names = [row.get("manifest") for row in runs if isinstance(row, dict)]
    if len(manifest_names) != len(manifests) or len(set(manifest_names)) != len(manifests):
        raise VerificationError("workspace manifests must be unique and coherent")
    target_sources = [row.get("source") for row in targets if isinstance(row, dict)]
    if len(target_sources) != len(targets) or len(set(target_sources)) != len(targets):
        raise VerificationError("workspace targets must be unique and coherent")
    if run_names != manifest_names or any(row.get("status") != "pass" for row in runs):
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
