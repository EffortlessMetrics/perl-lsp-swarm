#!/usr/bin/env python3
"""Validate the pinned Debug Adapter Protocol authority and project boundary."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Sequence

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from dap_authority_common import (  # noqa: E402
    DEBUG_ADAPTER_ROOT,
    DISPATCH_PATH,
    DOC_PATHS,
    MANIFEST_SCHEMA,
    MAX_SCHEMA_BYTES,
    RECEIPT_SCHEMA,
    REQUIRED_DEFINITIONS,
    REQUIRED_FIELDS,
    AuthorityError,
    git_blob_sha1,
    read_json,
    validate_manifest,
    write_json,
)
from dap_authority_docs import validate_docs  # noqa: E402
from dap_authority_production import (  # noqa: E402
    validate_production_boundary,
    verify_inventory_binding,
)
from dap_authority_receipt import build_receipt  # noqa: E402
from dap_authority_schema import fetch_schema, validate_schema_bytes  # noqa: E402

# Compatibility aliases for the existing focused tests and downstream imports.
_read_json = read_json
_write_json = write_json


def _load_schema_file(path: Path) -> bytes:
    try:
        return path.read_bytes()
    except FileNotFoundError as exc:
        raise AuthorityError(f"missing schema file: {path}") from exc
    except OSError as exc:
        raise AuthorityError(f"cannot read schema file {path}: {exc}") from exc


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    for command in ("observe", "check"):
        sub = subparsers.add_parser(command)
        sub.add_argument("--root", default=".")
        sub.add_argument("--manifest", default=".ci/dap/protocol-authority.json")
        sub.add_argument("--schema")
        sub.add_argument("--receipt", required=True)

    docs = subparsers.add_parser("check-docs")
    docs.add_argument("--root", default=".")
    docs.add_argument("--manifest", default=".ci/dap/protocol-authority.json")

    verify = subparsers.add_parser("verify-receipt")
    verify.add_argument("--root", default=".")
    verify.add_argument("--receipt", required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    root = Path(args.root).resolve()
    # `verify-receipt` compares a receipt against the tree; it takes no manifest.
    manifest_path = Path(getattr(args, "manifest", ".ci/dap/protocol-authority.json"))
    if not manifest_path.is_absolute():
        manifest_path = root / manifest_path

    try:
        if args.command == "verify-receipt":
            binding = verify_inventory_binding(root, read_json(Path(args.receipt)))
            print(f"DAP authority extractor: {binding['extractor_digest']}")
            print(
                f"DAP production source graph: {binding['source_graph_digest']} "
                f"({binding['source_graph_file_count']} files)"
            )
            print(f"DAP authority receipt is current for: {root}")
            return 0

        require_sha256 = args.command == "check"
        manifest = validate_manifest(read_json(manifest_path), require_sha256=require_sha256)
        validate_docs(root, manifest)
        if args.command == "check-docs":
            print("DAP protocol authority docs: valid")
            return 0

        data = _load_schema_file(Path(args.schema)) if args.schema else fetch_schema(manifest)
        observed = validate_schema_bytes(data, manifest, require_sha256=require_sha256)
        production = validate_production_boundary(root, manifest, observed)
        receipt = build_receipt(manifest, observed, production)
        write_json(Path(args.receipt), receipt)
        print(f"DAP upstream commit: {manifest['upstream']['commit']}")
        print(f"DAP upstream Git blob: {observed['git_blob_sha1']}")
        print(f"DAP upstream SHA-256: {observed['sha256']}")
        print(f"DAP project extensions: {production['project_extensions']}")
        print(f"DAP authority receipt: {args.receipt}")
    except AuthorityError as exc:
        print(f"DAP protocol authority error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
