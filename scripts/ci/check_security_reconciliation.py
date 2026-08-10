#!/usr/bin/env python3
"""Validate or regenerate the May 2026 security reconciliation ledger."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from security_reconciliation_io import check_or_write
from security_reconciliation_model import LedgerError

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_LEDGER = ROOT / ".ci/security/may-2026-findings.json"
DEFAULT_MARKDOWN = ROOT / "docs/security/may-2026-findings.md"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, default=DEFAULT_LEDGER)
    parser.add_argument("--markdown", type=Path, default=DEFAULT_MARKDOWN)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--write", action="store_true", help="regenerate the Markdown projection")
    mode.add_argument("--check", action="store_true", help="validate source and generated output (default)")
    args = parser.parse_args(argv)
    try:
        check_or_write(args.ledger, args.markdown, args.write)
    except LedgerError as exc:
        print(f"security reconciliation check failed: {exc}", file=sys.stderr)
        return 1
    action = "wrote" if args.write else "validated"
    print(f"{action} {args.ledger} and {args.markdown}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
