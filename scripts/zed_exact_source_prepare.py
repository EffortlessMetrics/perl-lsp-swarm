"""Prepare one isolated exact-source Zed receipt subject."""

from __future__ import annotations

import argparse
from pathlib import Path

from zed_host.common import HostReceiptError
from zed_host.prepare import prepare


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--zed-cli", type=Path, required=True)
    parser.add_argument("--zed-app", type=Path, required=True)
    parser.add_argument("--zed-version", required=True)
    parser.add_argument("--zed-channel", required=True)
    parser.add_argument("--zed-build", required=True)
    parser.add_argument("--extension-dir", type=Path, required=True)
    parser.add_argument("--extension-base", required=True)
    parser.add_argument("--extension-candidate", required=True)
    parser.add_argument("--extension-version", required=True)
    parser.add_argument("--wasm", type=Path, required=True)
    parser.add_argument("--perllsp", type=Path, required=True)
    parser.add_argument("--perllsp-version", required=True)
    parser.add_argument("--perllsp-build", required=True)
    parser.add_argument(
        "--resolution-route",
        choices=["binary_override", "worktree_path"],
        required=True,
    )
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--fixture-id", required=True)
    parser.add_argument("--root-identity")
    parser.add_argument("--perl-settings", type=Path)
    args = parser.parse_args()
    try:
        return prepare(args, Path(__file__).resolve().parents[1])
    except HostReceiptError as error:
        parser.error(str(error))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
