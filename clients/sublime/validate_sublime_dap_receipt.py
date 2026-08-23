#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

RUNTIME_ASSERTIONS = {
    "stdio_transport",
    "breakpoint_verified_hit",
    "stack_scopes_variables",
    "step_over",
    "continue_termination",
    "restart",
    "process_cleanup",
}
HOST_ASSERTIONS = {
    "debugger_loaded",
    "adapter_registered",
    "trusted_binary_authority",
    "direct_stdio_transport",
    "exact_binary_launched",
    "adapter_process_cleanup",
    "launch_configuration",
    "runtime_breakpoint_verified_hit",
    "runtime_stack_scopes_variables",
    "runtime_step_over",
    "runtime_continue_termination",
    "runtime_restart",
    "runtime_process_cleanup",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def _validate_common(payload: dict[str, Any]) -> None:
    require(payload.get("schema_version") == 1, "schema_version must be 1")
    require(payload.get("stage") == "exact_source_local", "stage must remain exact_source_local")
    require(
        bool(re.fullmatch(r"[0-9a-f]{40}", str(payload.get("source_sha", "")))),
        "source_sha must be a full lowercase Git commit SHA",
    )


def _validate_binary(binary: Any) -> None:
    require(isinstance(binary, dict), "binary must be an object")
    require(
        bool(re.fullmatch(r"[0-9a-f]{64}", str(binary.get("sha256", "")))),
        "binary.sha256 must be a lowercase SHA-256 digest",
    )
    command = binary.get("command")
    require(isinstance(command, list) and len(command) == 2, "binary.command must have two entries")
    require(command[1] == "--stdio", "binary.command must use --stdio")
    require(
        Path(str(command[0])).name in {"perl-dap", "perl-dap.exe"},
        "binary.command must launch perl-dap",
    )
    require(str(binary.get("version", "")).startswith("perl-dap "), "binary.version must identify perl-dap")


def _validate_assertions(assertions: Any, required: set[str]) -> None:
    require(isinstance(assertions, dict), "assertions must be an object")
    missing = required.difference(assertions)
    require(not missing, f"receipt is missing assertions: {sorted(missing)}")
    failed = sorted(name for name in required if assertions.get(name) is not True)
    require(not failed, f"receipt contains failed assertions: {failed}")


def validate_runtime(payload: dict[str, Any]) -> None:
    _validate_common(payload)
    require(payload.get("kind") == "perl_dap_runtime", "unexpected runtime receipt kind")
    _validate_binary(payload.get("binary"))
    fixture = payload.get("fixture")
    require(isinstance(fixture, dict), "fixture must be an object")
    require(
        bool(re.fullmatch(r"[0-9a-f]{64}", str(fixture.get("sha256", "")))),
        "fixture.sha256 must be a lowercase SHA-256 digest",
    )
    tests = payload.get("tests")
    require(isinstance(tests, list) and len(tests) >= 4, "runtime receipt must bind the lifecycle tests")
    _validate_assertions(payload.get("assertions"), RUNTIME_ASSERTIONS)


def validate_host(payload: dict[str, Any]) -> None:
    _validate_common(payload)
    require(payload.get("kind") == "sublime_debugger_host", "unexpected host receipt kind")

    host = payload.get("host")
    require(isinstance(host, dict), "host must be an object")
    require(host.get("name") == "Sublime Text", "host.name must be Sublime Text")
    require(str(host.get("version", "")).isdigit(), "host.version must be a Sublime build number")
    require(host.get("platform") in {"linux", "osx", "windows"}, "unsupported host.platform")
    require(host.get("arch") in {"x64", "arm64"}, "unsupported host.arch")

    debugger = payload.get("debugger")
    require(isinstance(debugger, dict), "debugger must be an object")
    require(debugger.get("repository") == "daveleroy/SublimeDebugger", "unexpected Debugger repository")
    require(debugger.get("version") == "0.11.6", "unexpected Debugger version")
    require(
        debugger.get("ref") == "58ed02acb8c06759445be62b63aef071462e0349",
        "receipt must bind the exact Debugger 0.11.6 commit",
    )

    adapter = payload.get("adapter")
    require(isinstance(adapter, dict), "adapter must be an object")
    require(adapter.get("type") == "perl", "adapter.type must be perl")
    require(adapter.get("transport") == "stdio", "adapter.transport must be stdio")
    require(str(adapter.get("module", "")).endswith("debugger_adapter"), "unexpected adapter module")

    _validate_binary(payload.get("binary"))
    runtime = payload.get("runtime_receipt")
    require(isinstance(runtime, dict), "runtime_receipt must be an object")
    require(
        bool(re.fullmatch(r"[0-9a-f]{64}", str(runtime.get("sha256", "")))),
        "runtime_receipt.sha256 must be a lowercase SHA-256 digest",
    )
    require(runtime.get("kind") == "perl_dap_runtime", "host receipt must bind runtime evidence")
    require(runtime.get("source_sha") == payload.get("source_sha"), "runtime source identity drifted")
    require(runtime.get("binary_sha256") == payload["binary"]["sha256"], "runtime binary identity drifted")

    _validate_assertions(payload.get("assertions"), HOST_ASSERTIONS)


def validate(payload: dict[str, Any]) -> None:
    kind = payload.get("kind")
    if kind == "perl_dap_runtime":
        validate_runtime(payload)
    elif kind == "sublime_debugger_host":
        validate_host(payload)
    else:
        raise ValueError(f"unsupported DAP receipt kind: {kind}")


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: validate_sublime_dap_receipt.py RECEIPT.json [...]", file=sys.stderr)
        return 2
    failed = False
    for value in argv[1:]:
        path = Path(value)
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
            require(isinstance(payload, dict), "receipt root must be an object")
            validate(payload)
        except (OSError, json.JSONDecodeError, ValueError) as error:
            print(f"{path}: {error}", file=sys.stderr)
            failed = True
        else:
            print(f"validated {path}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
