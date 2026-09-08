#!/usr/bin/env python3
"""Validate the perl-dap editor-transport inventory and retirement ruling."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Sequence

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from dap_editor_transport_schema import (  # noqa: E402
    RECEIPT_SCHEMA,
    TransportInventoryError,
    inventory_digest,
    read_json,
    validate_schema,
    write_json,
)
from dap_editor_transport_scan import (  # noqa: E402
    evaluate_ruling,
    scan_bind_sites,
    scan_cli_flags,
    scan_clients,
    scan_first_mile,
    scan_relays,
    scan_retired_native_editor_listener,
)


def check_inventory(root: Path, inventory: dict) -> list[str]:
    errors = validate_schema(inventory)
    if errors:
        # Schema/digest failures still run scans so a broken tree reports every seam.
        pass
    scan_errors: list[str] = []
    scan_errors.extend(scan_bind_sites(root, inventory))
    scan_errors.extend(scan_retired_native_editor_listener(root, inventory))
    scan_errors.extend(scan_cli_flags(root, inventory))
    scan_errors.extend(scan_first_mile(root, inventory))
    scan_errors.extend(scan_clients(root, inventory))
    scan_errors.extend(scan_relays(root, inventory))
    scan_errors.extend(evaluate_ruling(inventory, scan_errors))
    return errors + scan_errors


def build_receipt(inventory: dict, errors: list[str]) -> dict:
    return {
        "schema_version": RECEIPT_SCHEMA,
        "digest": inventory_digest(inventory),
        "ruling_status": inventory.get("ruling_status"),
        "transport_count": len(inventory.get("transports") or []),
        "client_count": len(inventory.get("clients") or []),
        "ok": not errors,
        "errors": errors,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    check = sub.add_parser("check")
    check.add_argument("--root", default=".")
    check.add_argument("--manifest", default=".ci/dap/editor-transport-inventory.v1.json")
    check.add_argument("--receipt", required=True)

    digest = sub.add_parser("digest")
    digest.add_argument("--root", default=".")
    digest.add_argument("--manifest", default=".ci/dap/editor-transport-inventory.v1.json")
    digest.add_argument("--write", action="store_true")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    root = Path(args.root).resolve()
    manifest_path = Path(args.manifest)
    if not manifest_path.is_absolute():
        manifest_path = root / manifest_path

    try:
        inventory = read_json(manifest_path)
        if args.command == "digest":
            digest = inventory_digest(inventory)
            if args.write:
                inventory["digest"] = digest
                manifest_path.write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
            print(digest)
            return 0

        errors = check_inventory(root, inventory)
        receipt = build_receipt(inventory, errors)
        write_json(Path(args.receipt), receipt)
        if errors:
            print("DAP editor-transport inventory errors:", file=sys.stderr)
            for item in errors:
                print(f"  {item}", file=sys.stderr)
            return 1
        print(f"DAP editor-transport inventory: valid ({receipt['digest']})")
        print(f"DAP editor-transport receipt: {args.receipt}")
    except TransportInventoryError as exc:
        print(f"DAP editor-transport inventory error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
