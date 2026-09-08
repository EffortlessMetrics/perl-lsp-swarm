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
from .dap_public import load_registry_manifest, validate_dap_public_receipt
from .dap_support import (
    ADAPTER_SCHEMA_RELATIVE_PATH,
    DOCS_OUTPUT_RELATIVE_PATH,
    EXTENSION_MANIFEST_RELATIVE_PATH,
    POLICY_OUTPUT_RELATIVE_PATH,
    PUBLIC_RECEIPT_RELATIVE_PATH,
    check_or_write_projection,
)
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


def command_validate_dap_public_receipt(args: argparse.Namespace) -> int:
    manifest_path = args.registry_manifest
    validate_dap_public_receipt(
        load_json(args.receipt),
        args.contract,
        load_json(args.contract),
        args.asset_receipt,
        load_json(args.asset_receipt),
        manifest_path,
        load_registry_manifest(manifest_path),
        receipts_dir=args.receipts_dir,
    )
    print("Zed perl-dap public registry receipt checks passed.")
    return 0


def command_project_dap_support(args: argparse.Namespace) -> int:
    check_or_write_projection(
        args.receipt,
        args.contract,
        args.asset_receipt,
        args.registry_manifest,
        args.receipts_dir,
        args.extension_manifest,
        args.adapter_schema,
        args.policy_output,
        args.docs_output,
        args.check,
    )
    if args.check:
        print("Zed perl-dap support projection is current and drift-free.")
    else:
        print(
            "Zed perl-dap support projection written to "
            f"{args.policy_output.as_posix()} and {args.docs_output.as_posix()}."
        )
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

    public_parser = subparsers.add_parser("validate-dap-public-receipt")
    public_parser.add_argument("--receipt", type=Path, required=True)
    public_parser.add_argument(
        "--contract",
        type=Path,
        required=True,
        help="checked #9516 managed-download contract the receipt is bound to",
    )
    public_parser.add_argument(
        "--asset-receipt",
        type=Path,
        required=True,
        help="committed #9516 aggregate asset receipt the receipt is bound to",
    )
    public_parser.add_argument(
        "--registry-manifest",
        type=Path,
        required=True,
        help="DU01 registry acceptance manifest owning the official-registry subject",
    )
    public_parser.add_argument(
        "--receipts-dir",
        type=Path,
        default=Path(".ci/fixtures/zed-perl-upstream/receipts"),
        help="committed receipts directory used for exact-source gate accounting",
    )
    public_parser.set_defaults(func=command_validate_dap_public_receipt)

    support_parser = subparsers.add_parser(
        "project-dap-support",
        help="project the #9489 Zed DAP support surface from the landed D05 authority",
    )
    support_parser.add_argument(
        "--receipt",
        type=Path,
        default=Path(PUBLIC_RECEIPT_RELATIVE_PATH),
        help="committed #9487 official-registry journey receipt consumed as landed",
    )
    support_parser.add_argument(
        "--contract",
        type=Path,
        default=Path(".ci/fixtures/zed-perl-upstream/perl-dap-managed-downloads.v1.json"),
        help="checked #9516 managed-download contract the receipt is bound to",
    )
    support_parser.add_argument(
        "--asset-receipt",
        type=Path,
        default=Path(".ci/fixtures/zed-perl-upstream/receipts/dap-asset-windows-x86_64.v1.json"),
        help="committed #9516 aggregate asset receipt the receipt is bound to",
    )
    support_parser.add_argument(
        "--registry-manifest",
        type=Path,
        default=Path(".ci/fixtures/zed-perl-upstream/registry/manifest.toml"),
        help="DU01 registry acceptance manifest owning the official-registry subject",
    )
    support_parser.add_argument(
        "--receipts-dir",
        type=Path,
        default=Path(".ci/fixtures/zed-perl-upstream/receipts"),
        help="committed receipts directory used for exact-source stage accounting",
    )
    support_parser.add_argument(
        "--extension-manifest",
        type=Path,
        default=Path(EXTENSION_MANIFEST_RELATIVE_PATH),
        help="staged Zed extension manifest owning the static adapter authority",
    )
    support_parser.add_argument(
        "--adapter-schema",
        type=Path,
        default=Path(ADAPTER_SCHEMA_RELATIVE_PATH),
        help="staged perl-dap debug-adapter configuration schema",
    )
    support_parser.add_argument(
        "--policy-output",
        type=Path,
        default=Path(POLICY_OUTPUT_RELATIVE_PATH),
        help="generated machine-readable support registry to check or write",
    )
    support_parser.add_argument(
        "--docs-output",
        type=Path,
        default=Path(DOCS_OUTPUT_RELATIVE_PATH),
        help="generated Zed debugger support documentation to check or write",
    )
    support_parser.add_argument(
        "--check",
        action="store_true",
        help="fail closed when the committed projection drifted from the current receipts",
    )
    support_parser.set_defaults(func=command_project_dap_support)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        return int(args.func(args))
    except ReceiptError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
