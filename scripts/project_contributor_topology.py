#!/usr/bin/env python3
"""Project the canonical development/publication topology for contributors.

Static facts come from existing repository authorities. Optional observations
are captured read-only inputs; absent evidence remains NOT_PROVEN.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from contributor_topology_model import (  # noqa: E402,F401
    PRODUCT_IDENTITY_PATH,
    SYNC_PROTOCOL_PATH,
    ContributorTopologyError,
    build_projection,
    render_human,
    validate_projection,
)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--observation", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    root = SCRIPT_DIR.parent
    try:
        if args.check:
            if args.output is None:
                raise ContributorTopologyError("--check requires --output")
            projection = json.loads(args.output.read_text(encoding="utf-8"))
            validate_projection(projection, root)
        else:
            projection = build_projection(root, args.observation)
            validate_projection(projection, root)
            if args.output is not None:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(
                    json.dumps(projection, indent=2, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
    except (OSError, json.JSONDecodeError, ContributorTopologyError) as error:
        print(f"contributor-topology: NOT_PROVEN: {error}", file=sys.stderr)
        return 2

    print(json.dumps(projection, indent=2, sort_keys=True) if args.json else render_human(projection))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
