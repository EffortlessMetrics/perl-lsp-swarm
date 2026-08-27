"""Command-line surface for Lite XL public-artifact replay receipt validation."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .ledger import LedgerError, load_ledger_inventory
from .public_replay import ReceiptError, load_json, load_manifest, validate_public_replay_receipt


def command_validate_public_replay_receipt(args: argparse.Namespace) -> int:
    inventory = load_ledger_inventory(args.ledger)
    manifest = load_manifest(args.acceptance_manifest)
    receipt = load_json(args.receipt)
    validate_public_replay_receipt(
        receipt,
        args.ledger,
        inventory,
        args.acceptance_manifest,
        manifest,
        args.receipts_dir,
    )
    print(
        f"Lite XL public replay receipt {args.receipt.as_posix()} is a valid "
        f"{receipt.get('result')!r} observation bound to the landed ledger."
    )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    public_parser = subparsers.add_parser("validate-public-replay-receipt")
    public_parser.add_argument("--receipt", type=Path, required=True)
    public_parser.add_argument(
        "--ledger",
        type=Path,
        required=True,
        help="landed #11178 acceptance ledger consumed strictly as data",
    )
    public_parser.add_argument(
        "--acceptance-manifest",
        type=Path,
        required=True,
        help="committed upstream-acceptance manifest owning released-subject truth",
    )
    public_parser.add_argument(
        "--receipts-dir",
        type=Path,
        default=Path(".ci/fixtures/lite-xl-perl-upstream/receipts"),
        help="committed receipts directory used for exact-source gate accounting",
    )
    public_parser.set_defaults(func=command_validate_public_replay_receipt)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except (ReceiptError, LedgerError) as error:
        print(error, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
