#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

REQUIRED_ASSERTIONS = {
    "file_family_activation",
    "exact_binary_launch",
    "pull_diagnostics_open",
    "pull_diagnostics_after_edit",
    "completion",
    "definition",
    "hover",
    "project_configuration",
    "workspace_edit_applied",
    "semantic_tokens",
    "custom_semantic_mapping",
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

    helper = payload.get("helper_package")
    require(isinstance(helper, dict), "helper_package must be an object")
    require(helper.get("name") == "LSP-perllsp", "unexpected helper package name")
    require(
        helper.get("source") == "clients/sublime/LSP-perllsp",
        "helper package source identity drifted",
    )

    binary = payload.get("binary")
    require(isinstance(binary, dict), "binary must be an object")
    require(
        bool(re.fullmatch(r"[0-9a-f]{64}", str(binary.get("sha256", "")))),
        "binary.sha256 must be a lowercase SHA-256 digest",
    )
    command = binary.get("command")
    require(isinstance(command, list) and len(command) == 2, "binary.command must have two entries")
    require(command[1] == "--stdio", "binary.command must use stdio")
    require(
        Path(str(command[0])).name in {"perllsp", "perllsp.exe"},
        "binary.command must launch perllsp",
    )

    fixtures = payload.get("fixtures")
    require(isinstance(fixtures, dict), "fixtures must be an object")
    require(set(fixtures) == {"pl", "pm", "t"}, "receipt must cover .pl, .pm, and .t fixtures")
    require(str(fixtures["pl"]).endswith(".pl"), "pl fixture identity is invalid")
    require(str(fixtures["pm"]).endswith(".pm"), "pm fixture identity is invalid")
    require(str(fixtures["t"]).endswith(".t"), "t fixture identity is invalid")

    assertions = payload.get("assertions")
    require(isinstance(assertions, dict), "assertions must be an object")
    missing = REQUIRED_ASSERTIONS.difference(assertions)
    require(not missing, f"receipt is missing assertions: {sorted(missing)}")
    failed = sorted(name for name in REQUIRED_ASSERTIONS if assertions.get(name) is not True)
    require(not failed, f"receipt contains failed assertions: {failed}")


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: validate_sublime_host_receipt.py RECEIPT.json", file=sys.stderr)
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
