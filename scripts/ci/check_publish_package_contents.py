#!/usr/bin/env python3
"""Reject packaged crate files that are not present in the Git tree."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path, PurePosixPath


GENERATED_PACKAGE_FILES = frozenset({".cargo_vcs_info.json", "Cargo.toml.orig"})


def normalize_package_path(raw: str) -> str:
    """Normalize Cargo's platform-dependent listing and reject unsafe paths."""

    value = raw.strip().replace("\\", "/")
    path = PurePosixPath(value)
    if (
        not value
        or value.startswith("/")
        or ":" in path.parts[0]
        or ".." in path.parts
    ):
        raise ValueError(f"unsafe package path {raw!r}")
    return path.as_posix()


def unexpected_packaged_files(
    package_files: set[str],
    tracked_files: set[str],
    *,
    generated_files: frozenset[str] = GENERATED_PACKAGE_FILES,
) -> list[str]:
    """Return packaged paths that are neither tracked nor Cargo-generated."""

    return sorted(package_files - tracked_files - generated_files)


def run(command: list[str], *, cwd: Path) -> str:
    print("+ " + " ".join(command), flush=True)
    completed = subprocess.run(
        command,
        cwd=cwd,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if completed.returncode != 0:
        print(completed.stderr, file=sys.stderr, end="")
        raise RuntimeError(f"command failed with exit code {completed.returncode}: {command!r}")
    return completed.stdout


def git_root() -> Path:
    return Path(
        run(["git", "rev-parse", "--show-toplevel"], cwd=Path.cwd()).strip()
    ).resolve()


def tracked_package_files(root: Path, package_root: Path) -> set[str]:
    relative_root = package_root.relative_to(root).as_posix()
    output = run(["git", "ls-files", "--cached", "--", relative_root], cwd=root)
    files: set[str] = set()
    for line in output.splitlines():
        if not line.strip():
            continue
        path = (root / line.strip()).resolve()
        files.add(path.relative_to(package_root).as_posix())

    # Cargo includes the workspace lockfile in each package, but it lives above
    # the package root and therefore is not returned by the scoped ls-files call.
    lockfile = root / "Cargo.lock"
    if lockfile.is_file() and run(["git", "ls-files", "--error-unmatch", "Cargo.lock"], cwd=root).strip():
        files.add("Cargo.lock")
    return files


def check_package(crate: str, manifest: Path, root: Path) -> int:
    manifest = manifest.resolve()
    try:
        manifest.relative_to(root)
    except ValueError as error:
        raise ValueError(f"manifest is outside the repository: {manifest}") from error
    package_root = manifest.parent

    listing = run(
        [
            "cargo",
            "package",
            "--list",
            "--no-verify",
            "--allow-dirty",
            "--manifest-path",
            str(manifest),
        ],
        cwd=root,
    )
    package_files = {normalize_package_path(line) for line in listing.splitlines() if line.strip()}
    tracked_files = tracked_package_files(root, package_root)
    unexpected = unexpected_packaged_files(package_files, tracked_files)
    if unexpected:
        print(
            f"ERROR: {crate} package contains files outside the tracked tree:",
            file=sys.stderr,
        )
        for path in unexpected:
            print(f"- {path}", file=sys.stderr)
        return 1

    print(f"OK: {crate} package contains no untracked files ({len(package_files)} files)")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crate", required=True)
    parser.add_argument("--manifest", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        return check_package(args.crate, args.manifest, git_root())
    except (OSError, RuntimeError, ValueError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
