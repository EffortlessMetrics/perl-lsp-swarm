"""Launch exact Zed for one prepared exact-source receipt subject."""

from __future__ import annotations

import argparse
from pathlib import Path

from zed_host.common import HostReceiptError, load_json
from zed_host.process import launch


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--timeout-seconds", type=int, default=3600)
    args = parser.parse_args()
    try:
        run_dir = args.run_dir.expanduser().resolve(strict=True)
        return launch(load_json(run_dir / "manifest.json"), run_dir, args.timeout_seconds)
    except HostReceiptError as error:
        parser.error(str(error))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
