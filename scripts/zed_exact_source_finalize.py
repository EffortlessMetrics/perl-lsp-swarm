"""Finalize one prepared exact-source Zed receipt subject."""

from __future__ import annotations

import argparse
from pathlib import Path

from zed_host.common import HostReceiptError
from zed_host.finalize import finalize


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--observations", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        return finalize(args, Path(__file__).resolve().parents[1])
    except HostReceiptError as error:
        parser.error(str(error))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
