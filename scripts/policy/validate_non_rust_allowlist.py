#!/usr/bin/env python3
"""Compatibility shim for the Rust non-Rust policy schema validator.

The schema validation logic lives in `cargo xtask non-rust validate-policy` so
file-policy checks share the same Rust implementation. This historical entrypoint
uses the existing Cargo admission owner; it does not select build storage itself.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys


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
    caller = pathlib.Path.cwd()
    # Anchor the producer to this checkout, but retain the caller's policy-file
    # referents. Joining an absolute cwd preserves symlink/.. filesystem
    # semantics; resolve()/abspath() would unnecessarily rewrite those paths.
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
        str(caller / args.allowlist),
        "--debt",
        str(caller / args.debt),
    ]
    return subprocess.run(command, check=False, cwd=repo_root).returncode


if __name__ == "__main__":
    sys.exit(main())
