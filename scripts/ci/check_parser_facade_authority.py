#!/usr/bin/env python3
"""Validate and render the staged perl-parser facade authority ledger."""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Sequence

from parser_facade_authority import check, render_markdown

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / ".ci/parser-facade"


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--ledger", type=Path, default=LEDGER)
    parser.add_argument("--check-doc", action="store_true")
    parser.add_argument("--write-doc", action="store_true")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        root = args.root.resolve()
        ledger_path = args.ledger if args.ledger.is_absolute() else root / args.ledger
        ledger, summary = check(root, ledger_path)
        document = render_markdown(ledger, summary)
        doc_path = root / ledger["sources"]["generated_doc"]
        if args.write_doc:
            doc_path.parent.mkdir(parents=True, exist_ok=True)
            doc_path.write_text(document, encoding="utf-8")
        if args.check_doc:
            if doc_path.read_text(encoding="utf-8") != document:
                raise ValueError(
                    "generated facade authority document is stale; run "
                    "python3 scripts/ci/check_parser_facade_authority.py --write-doc"
                )
        print(json.dumps(summary, indent=2, sort_keys=True))
        return 0
    except (OSError, KeyError, TypeError, ValueError) as error:
        print(f"parser facade authority check failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
