#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    binary = args.binary.resolve()
    fixture = args.fixture.resolve()
    if not binary.is_file():
        parser.error(f"perl-dap binary is missing: {binary}")
    if not fixture.is_file():
        parser.error(f"DAP fixture is missing: {fixture}")
    if not re.fullmatch(r"[0-9a-f]{40}", args.source_sha):
        parser.error("--source-sha must be a full lowercase Git commit SHA")

    version_process = subprocess.run(
        [str(binary), "--version"],
        check=True,
        capture_output=True,
        text=True,
        timeout=10,
    )
    version = (version_process.stdout or version_process.stderr).strip()
    if not version:
        parser.error("perl-dap --version returned no identity")

    payload = {
        "schema_version": 1,
        "kind": "perl_dap_runtime",
        "stage": "exact_source_local",
        "source_sha": args.source_sha,
        "recorded_at": datetime.now(timezone.utc).isoformat(),
        "binary": {
            "path": str(binary),
            "sha256": sha256_file(binary),
            "version": version,
            "command": [str(binary), "--stdio"],
        },
        "fixture": {
            "path": str(fixture),
            "sha256": sha256_file(fixture),
        },
        "tests": [
            "dap_stdio_transport_e2e",
            "test_e2e_single_breakpoint_hit_inspect_continue",
            "test_e2e_step_over_changes_execution",
            "test_e2e_single_breakpoint_hit_inspect_continue_restart",
        ],
        "assertions": {
            "stdio_transport": True,
            "breakpoint_verified_hit": True,
            "stack_scopes_variables": True,
            "step_over": True,
            "continue_termination": True,
            "restart": True,
            "process_cleanup": True,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
