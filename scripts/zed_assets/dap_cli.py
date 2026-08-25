"""Command line interface for Zed perl-dap public asset receipts."""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path

from .common import ReceiptError, load_json
from .dap_cache import run_recovery_scenarios
from .dap_contract import validate_dap_contract
from .dap_producer import execute_dap
from .dap_validation import validate_dap_receipt


def command_validate_dap_contract(args: argparse.Namespace) -> int:
    repo_root = Path.cwd().resolve() if args.bind_repo_root else None
    validate_dap_contract(load_json(args.contract), repo_root)
    print("Zed perl-dap managed asset contract checks passed.")
    return 0


def command_validate_dap_receipt(args: argparse.Namespace) -> int:
    validate_dap_receipt(load_json(args.receipt), args.contract, load_json(args.contract))
    print("Zed perl-dap managed asset receipt checks passed.")
    return 0


def command_execute_dap(args: argparse.Namespace) -> int:
    contract = load_json(args.contract)
    token = os.environ.get(args.token_env) if args.token_env else None
    repo_root = Path.cwd().resolve()
    if args.work_dir is not None:
        return execute_dap(
            args.contract, contract, args.output, args.work_dir, token, repo_root
        )
    with tempfile.TemporaryDirectory(prefix="zed-perl-dap-assets-") as temporary_work_dir:
        return execute_dap(
            args.contract, contract, args.output, Path(temporary_work_dir), token, repo_root
        )


def command_dap_cache_recovery(args: argparse.Namespace) -> int:
    scenarios = run_recovery_scenarios(args.work_dir)
    encoded = json.dumps(scenarios, indent=2, sort_keys=True)
    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    else:
        print(encoded)
    if scenarios["result"] != "pass":
        print(
            "error: managed-DAP cache recovery scenarios failed",
            file=sys.stderr,
        )
        return 1
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    contract_parser = subparsers.add_parser("validate-dap-contract")
    contract_parser.add_argument("--contract", type=Path, required=True)
    contract_parser.add_argument(
        "--bind-repo-root",
        action="store_true",
        help="recompute the topology and projection binding digests against this tree",
    )
    contract_parser.set_defaults(func=command_validate_dap_contract)

    receipt_parser = subparsers.add_parser("validate-dap-receipt")
    receipt_parser.add_argument("--receipt", type=Path, required=True)
    receipt_parser.add_argument(
        "--contract",
        type=Path,
        required=True,
        help="checked contract the receipt must have been produced against",
    )
    receipt_parser.set_defaults(func=command_validate_dap_receipt)

    execute_parser = subparsers.add_parser("execute-dap")
    execute_parser.add_argument("--contract", type=Path, required=True)
    execute_parser.add_argument("--output", type=Path, required=True)
    execute_parser.add_argument("--work-dir", type=Path)
    execute_parser.add_argument("--token-env", default="GITHUB_TOKEN")
    execute_parser.set_defaults(func=command_execute_dap)

    recovery_parser = subparsers.add_parser("dap-cache-recovery")
    recovery_parser.add_argument("--work-dir", type=Path, required=True)
    recovery_parser.add_argument("--output", type=Path)
    recovery_parser.set_defaults(func=command_dap_cache_recovery)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        return int(args.func(args))
    except ReceiptError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
