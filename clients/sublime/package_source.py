#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import os
import shutil
import tempfile
from pathlib import Path, PurePosixPath
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parent
PACKAGE_ROOT = ROOT / "LSP-perllsp"
MANIFEST_PATH = ROOT / "package-source.v1.json"
TEXT_SUFFIXES = {
    "",
    ".json",
    ".md",
    ".py",
    ".settings",
    ".txt",
    ".yaml",
    ".yml",
}
EPHEMERAL_PATTERNS = ("__pycache__/**", "*.pyc", "*.pyo", ".DS_Store")


class AuthorityError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AuthorityError(message)


def _relative_path(raw: str) -> str:
    require(isinstance(raw, str) and raw != "", "manifest paths must be non-empty strings")
    require("\\" not in raw, f"manifest paths must use forward slashes: {raw}")
    path = PurePosixPath(raw)
    require(not path.is_absolute(), f"manifest path must be relative: {raw}")
    require(raw == path.as_posix(), f"manifest path is not normalized: {raw}")
    require(all(part not in {"", ".", ".."} for part in path.parts), f"unsafe manifest path: {raw}")
    return path.as_posix()


def _path_list(value: Any, field: str) -> tuple[str, ...]:
    require(isinstance(value, list) and value, f"{field} must be a non-empty array")
    paths = tuple(_relative_path(item) for item in value)
    require(len(paths) == len(set(paths)), f"{field} contains duplicate paths")
    require(list(paths) == sorted(paths), f"{field} must be sorted")
    for path in paths:
        require(not _is_ephemeral(path), f"{field} must not include ephemeral file: {path}")
    return paths


def load_manifest(path: Path = MANIFEST_PATH) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise AuthorityError(f"could not load source authority manifest: {error}") from error
    require(isinstance(payload, dict), "source authority manifest must be an object")
    require(payload.get("schema_version") == 1, "source authority schema_version must be 1")
    require(payload.get("package") == "LSP-perllsp", "unexpected package identity")
    require(
        payload.get("development_repository") == "EffortlessMetrics/perl-lsp-swarm",
        "unexpected development repository",
    )
    require(payload.get("development_root") == "clients/sublime/LSP-perllsp", "unexpected development root")
    require(
        payload.get("public_repository") == "EffortlessMetrics/LSP-perllsp",
        "unexpected public repository",
    )
    phase = payload.get("authority_phase")
    require(phase in {"pre_public_release", "public_repository_authoritative"}, "invalid authority phase")
    editable = payload.get("editable_authority")
    expected = "development_repository" if phase == "pre_public_release" else "public_repository"
    require(editable == expected, f"editable_authority must be {expected} during {phase}")
    source_files = _path_list(payload.get("source_files"), "source_files")
    package_files = _path_list(payload.get("package_files"), "package_files")
    require(set(package_files).issubset(source_files), "package_files must be a subset of source_files")
    payload["source_files"] = source_files
    payload["package_files"] = package_files
    return payload


def _is_ephemeral(relative: str) -> bool:
    return any(fnmatch.fnmatch(relative, pattern) for pattern in EPHEMERAL_PATTERNS)


def discover_files(root: Path) -> tuple[str, ...]:
    require(root.is_dir(), f"package source root does not exist: {root}")
    discovered: list[str] = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if _is_ephemeral(relative):
            continue
        if path.is_symlink():
            raise AuthorityError(f"package source must not contain symlinks: {relative}")
        if path.is_dir():
            continue
        require(path.is_file(), f"package source entry is not a regular file: {relative}")
        discovered.append(relative)
    return tuple(discovered)


def validate_source_tree(
    manifest: dict[str, Any] | None = None,
    package_root: Path = PACKAGE_ROOT,
) -> tuple[str, ...]:
    manifest = manifest or load_manifest()
    expected = tuple(manifest["source_files"])
    actual = discover_files(package_root)
    missing = sorted(set(expected) - set(actual))
    undeclared = sorted(set(actual) - set(expected))
    require(not missing, f"source manifest names missing files: {missing}")
    require(not undeclared, f"package source contains undeclared files: {undeclared}")
    for relative in expected:
        path = package_root / relative
        require(path.is_file() and not path.is_symlink(), f"invalid declared source file: {relative}")
    return expected


def _normalized_bytes(path: Path) -> bytes:
    data = path.read_bytes()
    if path.suffix.lower() in TEXT_SUFFIXES or path.name in {"LICENSE-APACHE", "LICENSE-MIT"}:
        data = data.replace(b"\r\n", b"\n").replace(b"\r", b"\n")
    return data


def tree_digest(root: Path, files: Iterable[str]) -> str:
    digest = hashlib.sha256()
    for relative in sorted(files):
        path = root / relative
        require(path.is_file() and not path.is_symlink(), f"cannot digest invalid source file: {relative}")
        data = _normalized_bytes(path)
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(len(data)).encode("ascii"))
        digest.update(b"\0")
        digest.update(data)
        digest.update(b"\0")
    return digest.hexdigest()


def _prepare_destination(destination: Path) -> None:
    if destination.exists():
        require(destination.is_dir() and not destination.is_symlink(), "destination must be a real directory")
        existing = [
            path
            for path in destination.rglob("*")
            if not _is_ephemeral(path.relative_to(destination).as_posix())
        ]
        require(not existing, f"destination must be empty: {destination}")
    else:
        destination.mkdir(parents=True)


def export_source_tree(
    destination: Path,
    *,
    source_commit: str,
    receipt_path: Path | None = None,
    manifest: dict[str, Any] | None = None,
    package_root: Path = PACKAGE_ROOT,
) -> dict[str, Any]:
    manifest = manifest or load_manifest()
    files = validate_source_tree(manifest, package_root)
    require(
        len(source_commit) == 40 and all(char in "0123456789abcdef" for char in source_commit),
        "source_commit must be a full lowercase Git commit SHA",
    )
    _prepare_destination(destination)
    for relative in files:
        source = package_root / relative
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(_normalized_bytes(source))
        target.chmod(0o644)

    source_sha = tree_digest(package_root, files)
    destination_sha = tree_digest(destination, files)
    require(source_sha == destination_sha, "exported source tree digest does not match source")
    receipt = {
        "schema_version": 1,
        "package": manifest["package"],
        "source_repository": manifest["development_repository"],
        "source_path": manifest["development_root"],
        "source_commit": source_commit,
        "source_tree_sha256": source_sha,
        "destination_repository": manifest["public_repository"],
        "destination_tree_sha256": destination_sha,
        "file_count": len(files),
        "authority_phase": manifest["authority_phase"],
    }
    if receipt_path is not None:
        receipt_path.parent.mkdir(parents=True, exist_ok=True)
        receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return receipt


def check_export(
    destination: Path,
    *,
    manifest: dict[str, Any] | None = None,
    package_root: Path = PACKAGE_ROOT,
) -> str:
    manifest = manifest or load_manifest()
    source_files = validate_source_tree(manifest, package_root)
    destination_files = discover_files(destination)
    missing = sorted(set(source_files) - set(destination_files))
    undeclared = sorted(set(destination_files) - set(source_files))
    require(not missing, f"exported source tree is missing files: {missing}")
    require(not undeclared, f"exported source tree contains undeclared files: {undeclared}")
    source_sha = tree_digest(package_root, source_files)
    destination_sha = tree_digest(destination, source_files)
    require(source_sha == destination_sha, "exported source tree content differs from authority")
    return destination_sha


def _replace_directory_atomically(staged: Path, destination: Path) -> None:
    if destination.exists():
        shutil.rmtree(destination)
    os.replace(staged, destination)


def export_atomically(
    destination: Path,
    *,
    source_commit: str,
    receipt_path: Path | None = None,
) -> dict[str, Any]:
    destination_parent = destination.parent
    destination_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="lsp-perllsp-source-", dir=str(destination_parent)) as temporary:
        staged = Path(temporary) / destination.name
        receipt = export_source_tree(staged, source_commit=source_commit, receipt_path=None)
        _replace_directory_atomically(staged, destination)
    if receipt_path is not None:
        receipt_path.parent.mkdir(parents=True, exist_ok=True)
        receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate and export the canonical LSP-perllsp source tree")
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("check")
    digest_parser = subparsers.add_parser("digest")
    digest_parser.add_argument("--package-root", type=Path, default=PACKAGE_ROOT)
    export_parser = subparsers.add_parser("export")
    export_parser.add_argument("--destination", type=Path, required=True)
    export_parser.add_argument("--source-commit", required=True)
    export_parser.add_argument("--receipt", type=Path)
    check_parser = subparsers.add_parser("check-export")
    check_parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()

    if args.command == "check":
        files = validate_source_tree()
        print(f"validated {len(files)} source files")
    elif args.command == "digest":
        manifest = load_manifest()
        files = validate_source_tree(manifest, args.package_root)
        print(tree_digest(args.package_root, files))
    elif args.command == "export":
        receipt = export_atomically(args.destination, source_commit=args.source_commit, receipt_path=args.receipt)
        print(receipt["destination_tree_sha256"])
    elif args.command == "check-export":
        print(check_export(args.destination))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
