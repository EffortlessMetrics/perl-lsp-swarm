#!/usr/bin/env python3
"""Compatibility shim for the Rust non-Rust policy schema validator.

The schema validation logic lives in `cargo xtask non-rust validate-policy` so
file-policy checks share the same Rust implementation. This historical entrypoint
uses the existing Cargo admission owner; it does not select build storage itself.
"""

from __future__ import annotations

import argparse
import os
import pathlib
import subprocess
import sys


def caller_absolute(path: pathlib.Path) -> pathlib.Path:
    # Python 3.11 Path.absolute() does not anchor a different drive's relative
    # path. Resolve only the drive's cwd, never the symlink-sensitive tail.
    if path.drive and not path.root:
        return pathlib.Path(os.path.abspath(path.drive)).joinpath(*path.parts[1:])
    return path.absolute()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--allowlist",
        type=pathlib.Path,
        default=pathlib.Path("policy/non-rust-allowlist.toml"),
    )
    parser.add_argument(
        "--debt",
        type=pathlib.Path,
        default=pathlib.Path("policy/non-rust-debt.toml"),
    )
    args = parser.parse_args()

    repo_root = pathlib.Path(__file__).resolve().parents[2]
    # Freeze caller-relative and Windows drive-relative referents before changing
    # cwd. Anchoring preserves symlink/.. components without normalization;
    # resolve()/abspath() would rewrite their filesystem meaning.
    command = [
        sys.executable,
        "-B",
        str(repo_root / "scripts" / "cargo_admitted.py"),
        "run",
        "-p", "xtask",
        "--bin", "xtask",
        "--locked",
        "--",
        "non-rust",
        "validate-policy",
        "--allowlist",
        str(caller_absolute(args.allowlist)),
        "--debt",
        str(caller_absolute(args.debt)),
    ]
    return subprocess.run(command, check=False, cwd=repo_root).returncode


if __name__ == "__main__":
    sys.exit(main())
