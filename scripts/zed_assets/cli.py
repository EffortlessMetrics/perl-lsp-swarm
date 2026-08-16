"""Command line interface for Zed managed asset receipts."""

from __future__ import annotations

import argparse
import os
import sys
import tempfile
from pathlib import Path

from .common import ReceiptError, load_json
from .contract import validate_contract
from .producer import execute
from .validation import validate_receipt


def command_validate_contract(args: argparse.Namespace) -> int:
    validate_contract(load_json(args.contract))
    print("Zed managed asset contract checks passed.")
    return 0


def command_validate_receipt(args: argparse.Namespace) -> int:
    validate_receipt(load_json(args.receipt), args.contract, load_json(args.contract))
    print("Zed managed asset receipt checks passed.")
    return 0


def command_execute(args: argparse.Namespace) -> int:
    contract = load_json(args.contract)
    token = os.environ.get(args.token_env) if args.token_env else None
    if args.work_dir is not None:
        return execute(args.contract, contract, args.output, args.work_dir, token)
    with tempfile.TemporaryDirectory(prefix="zed-perllsp-assets-") as temporary_work_dir:
        return execute(args.contract, contract, args.output, Path(temporary_work_dir), token)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    contract_parser = subparsers.add_parser("validate-contract")
    contract_parser.add_argument("--contract", type=Path, required=True)
    contract_parser.set_defaults(func=command_validate_contract)

    receipt_parser = subparsers.add_parser("validate-receipt")
    receipt_parser.add_argument("--receipt", type=Path, required=True)
    receipt_parser.add_argument(
        "--contract",
        type=Path,
        required=True,
        help="checked contract the receipt must have been produced against",
    )
    receipt_parser.set_defaults(func=command_validate_receipt)

    execute_parser = subparsers.add_parser("execute")
    execute_parser.add_argument("--contract", type=Path, required=True)
    execute_parser.add_argument("--output", type=Path, required=True)
    execute_parser.add_argument("--work-dir", type=Path)
    execute_parser.add_argument("--token-env", default="GITHUB_TOKEN")
    execute_parser.set_defaults(func=command_execute)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        return int(args.func(args))
    except ReceiptError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
