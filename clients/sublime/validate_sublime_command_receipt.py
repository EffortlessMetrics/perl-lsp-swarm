#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

REQUIRED_ASSERTIONS = {
    "active_view_session_selection",
    "advertised_command_gate",
    "workspace_execute_command",
    # The panel renders the same caption for a served report and for a JSON-RPC
    # or application failure, so the journey must record that it saw neither
    # failure rendering before this receipt can validate.
    "command_reported_success",
    "bounded_structured_result",
    "no_destructive_binding",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def validate(payload: dict[str, Any]) -> None:
    require(payload.get("schema_version") == 1, "schema_version must be 1")
    require(payload.get("stage") == "exact_source_local", "stage must remain exact_source_local")
    require(
        bool(re.fullmatch(r"[0-9a-f]{40}", str(payload.get("source_sha", "")))),
        "source_sha must be a full lowercase Git commit SHA",
    )

    host = payload.get("host")
    require(isinstance(host, dict), "host must be an object")
    require(host.get("name") == "Sublime Text", "host.name must be Sublime Text")
    require(str(host.get("version", "")).isdigit(), "host.version must be a Sublime build number")
    require(host.get("platform") in {"linux", "osx", "windows"}, "unsupported host.platform")
    require(host.get("arch") in {"x64", "arm64"}, "unsupported host.arch")

    lsp = payload.get("lsp_package")
    require(isinstance(lsp, dict), "lsp_package must be an object")
    require(lsp.get("repository") == "sublimelsp/LSP", "unexpected LSP repository")
    require(
        lsp.get("ref") == "cc9f5201d9f053d9ab67aa0ea575b494fd133803",
        "receipt must identify the exact LSP 2.13.0 source commit",
    )

    command = payload.get("command")
    require(isinstance(command, dict), "command must be an object")
    require(command.get("action") == "workspace_trust_report", "unexpected command action")
    require(command.get("id") == "perl.workspaceTrustReport", "unexpected command id")
    require(command.get("session") == "LSP-perllsp", "command used the wrong session")
    require(command.get("result_surface") == "output.perllsp", "unexpected result surface")

    assertions = payload.get("assertions")
    require(isinstance(assertions, dict), "assertions must be an object")
    missing = REQUIRED_ASSERTIONS.difference(assertions)
    require(not missing, f"receipt is missing assertions: {sorted(missing)}")
    failed = sorted(name for name in REQUIRED_ASSERTIONS if assertions.get(name) is not True)
    require(not failed, f"receipt contains failed assertions: {failed}")


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: validate_sublime_command_receipt.py RECEIPT.json", file=sys.stderr)
        return 2
    path = Path(argv[1])
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
        require(isinstance(payload, dict), "receipt root must be an object")
        validate(payload)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"{path}: {error}", file=sys.stderr)
        return 1
    print(f"validated {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
