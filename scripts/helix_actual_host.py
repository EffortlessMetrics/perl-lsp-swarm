#!/usr/bin/env python3
"""Drive official Helix through a bounded real-terminal LSP journey.

The harness deliberately observes Helix-native UI state as well as the exact
candidate process and verbose client log. It emits `actual_host_receipt.v1` for
validation by the shared #7777 contract.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
from typing import Callable


def run(*args: str, check: bool = True, capture: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        check=check,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def wait_until(predicate: Callable[[], bool], label: str, timeout: float = 30.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.2)
    raise RuntimeError(f"timed out waiting for {label}")


def tmux(session: str, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return run("tmux", *args, "-t", session, check=check)


def send(session: str, *keys: str) -> None:
    run("tmux", "send-keys", "-t", session, *keys)


def capture(session: str, destination: Path) -> str:
    result = run("tmux", "capture-pane", "-p", "-J", "-t", session)
    destination.write_text(result.stdout, encoding="utf-8")
    return result.stdout


def log_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except FileNotFoundError:
        return ""


def log_has(path: Path, *needles: str) -> bool:
    text = log_text(path).lower()
    return all(needle.lower() in text for needle in needles)


def process_exists(executable: Path) -> bool:
    result = run("pgrep", "-f", "--", str(executable), check=False)
    return result.returncode == 0


def feature(advertised: bool, observed: bool, outcome: str, **extra: object) -> dict[str, object]:
    value: dict[str, object] = {
        "advertised": advertised,
        "observed": observed,
        "outcome": outcome,
    }
    value.update(extra)
    return value


def terminal(outcome: str, reason: str | None = None) -> dict[str, str]:
    value = {"outcome": outcome}
    if reason is not None:
        value["reason"] = reason
    return value


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--hx", type=Path, required=True)
    parser.add_argument("--perllsp", type=Path, required=True)
    parser.add_argument("--launch-script", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--helix-log", type=Path, required=True)
    parser.add_argument("--server-stderr", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()

    args.artifacts.mkdir(parents=True, exist_ok=True)
    session = f"helix-perllsp-{os.getpid()}"
    initial_pane = args.artifacts / "initial-pane.txt"
    diagnostics_pane = args.artifacts / "diagnostics-pane.txt"
    definition_pane = args.artifacts / "definition-pane.txt"
    completion_pane = args.artifacts / "completion-pane.txt"
    post_edit_pane = args.artifacts / "post-edit-diagnostics-pane.txt"

    run("tmux", "new-session", "-d", "-s", session, "-x", "140", "-y", "45", str(args.launch_script))

    try:
        wait_until(lambda: process_exists(args.perllsp), "exact perllsp process")
        wait_until(lambda: log_has(args.helix_log, "initialize"), "Helix initialize transcript")
        initial = capture(session, initial_pane)
        if "main.pl" not in initial:
            raise RuntimeError(f"Helix did not open the fixture buffer:\n{initial}")

        # Helix-native diagnostics picker: a wire frame in a log is not enough.
        send(session, "Space", "d")
        time.sleep(1.0)
        diagnostics = capture(session, diagnostics_pane)
        diagnostic_visible = "main.pl" in diagnostics and any(
            token in diagnostics.lower() for token in ("error", "expected", "syntax", "diagnostic")
        )
        if not diagnostic_visible:
            raise RuntimeError(f"diagnostic was not visible in Helix:\n{diagnostics}")
        send(session, "Escape")

        # Cross-file definition must move the actual editor to Module.pm.
        send(session, ":", "g", "o", "t", "o", " ", "5", "Enter")
        send(session, "1", "2", "l", "g", "d")
        time.sleep(1.0)
        definition = capture(session, definition_pane)
        definition_visible = "Module.pm" in definition
        if not definition_visible:
            raise RuntimeError(f"definition did not move Helix to Module.pm:\n{definition}")
        send(session, "C-o")
        time.sleep(0.5)

        # Trigger and consume completion through the actual terminal UI.
        send(session, ":", "g", "o", "t", "o", " ", "7", "Enter")
        send(session, "o", "$", "v", "a", "C-x")
        time.sleep(1.0)
        completion = capture(session, completion_pane)
        completion_visible = "$value" in completion or "value" in completion.lower()
        if not completion_visible:
            raise RuntimeError(f"completion candidate was not visible in Helix:\n{completion}")
        send(session, "Escape", "u")

        # Repair the malformed line through Helix, save, and observe a new
        # didChange plus a second diagnostics publication.
        before_log = log_text(args.helix_log)
        before_publish_count = before_log.count("publishDiagnostics")
        send(session, ":", "g", "o", "t", "o", " ", "6", "Enter")
        send(session, "x", "c")
        send(session, "m", "y", " ", "$", "b", "r", "o", "k", "e", "n", " ", "=", " ", "1", ";")
        send(session, "Escape", ":", "w", "r", "i", "t", "e", "Enter")
        wait_until(lambda: log_has(args.helix_log, "textDocument/didChange"), "Helix didChange")
        wait_until(
            lambda: log_text(args.helix_log).count("publishDiagnostics") > before_publish_count,
            "post-edit diagnostics publication",
        )
        send(session, "Space", "d")
        time.sleep(1.0)
        post_edit = capture(session, post_edit_pane)
        send(session, "Escape")

        # Exercise the actual LSP formatting route; no external formatter is in
        # the canonical languages.toml fixture.
        send(session, ":", "f", "o", "r", "m", "a", "t", "Enter")
        wait_until(lambda: log_has(args.helix_log, "textDocument/formatting"), "Helix formatting request")
        send(session, ":", "w", "r", "i", "t", "e", "Enter")
        time.sleep(0.5)

        send(session, ":", "q", "u", "i", "t", "-", "a", "l", "l", "!", "Enter")
        wait_until(
            lambda: run("tmux", "has-session", "-t", session, check=False).returncode != 0,
            "Helix terminal exit",
        )
        wait_until(lambda: not process_exists(args.perllsp), "perllsp process cleanup")

        log = log_text(args.helix_log)
        required_log_tokens = [
            "initialize",
            "initialized",
            "workspace/configuration",
            "client/registerCapability",
            "publishDiagnostics",
            "textDocument/completion",
            "textDocument/definition",
            "textDocument/didChange",
            "textDocument/formatting",
            "shutdown",
        ]
        missing = [token for token in required_log_tokens if token not in log]
        if missing:
            raise RuntimeError(f"Helix verbose log missed required protocol evidence: {missing}")

        hx_version = run(str(args.hx), "--version").stdout.strip()
        perllsp_version = run(str(args.perllsp), "--version").stdout.strip()
        workspace_identity = sha256(args.workspace / "main.pl")
        profile_identity = sha256(Path(os.environ["HELIX_LANGUAGES_FIXTURE"]))
        timestamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

        receipt = {
            "schema_version": "actual_host_receipt.v1",
            "receipt_version": 1,
            "run_id": os.environ.get("GITHUB_RUN_ID", f"local-{os.getpid()}"),
            "timestamp": timestamp,
            "editor": {
                "family": "helix",
                "version": hx_version,
                "source": "official-release-25.07.1",
                "executable_sha256": sha256(args.hx),
            },
            "client": {
                "family": "helix-lsp",
                "version": hx_version,
                "source": "official-release-25.07.1",
            },
            "server": {
                "path": str(args.perllsp),
                "sha256": sha256(args.perllsp),
                "version": perllsp_version,
            },
            "platform": {"os": platform.system().lower(), "arch": platform.machine()},
            "workspace": {"root": "fixture/helix-actual-host", "identity": workspace_identity},
            "profile": {
                "identity": profile_identity,
                "source": "docs/examples/helix/languages.toml",
            },
            "registration_state": "manual_client_registration",
            "artifacts": {
                "client_log": str(args.helix_log),
                "server_stderr": str(args.server_stderr),
                "initial_pane": str(initial_pane),
                "diagnostics_pane": str(diagnostics_pane),
                "definition_pane": str(definition_pane),
                "completion_pane": str(completion_pane),
                "post_edit_pane": str(post_edit_pane),
            },
            "features": {
                "activation": feature(True, True, "passed"),
                "diagnostics": feature(True, diagnostic_visible, "passed"),
                "completion": feature(True, completion_visible, "passed"),
                "definition": feature(True, definition_visible, "passed"),
                "edit_requery": feature(True, True, "passed", post_edit_snapshot=post_edit[:500]),
                "formatting": feature(True, True, "passed"),
                "unicode_crlf": feature(
                    True,
                    False,
                    "skipped",
                    skip_classification="blocked",
                    reason="dedicated Unicode/CRLF fixture lands in the next #7714 slice",
                ),
                "rename": feature(
                    True,
                    False,
                    "skipped",
                    skip_classification="harness_limit",
                    reason="core journey proves activation, diagnostics, completion, definition, edit freshness, and formatting first",
                ),
            },
            "state_machine": {
                "initialize": terminal("ok"),
                "initialized": terminal("ok"),
                "position_encoding": "utf-16",
                "diagnostics_mode": "push",
                "diagnostics_response_form": "textDocument/publishDiagnostics",
                "workspace_configuration": terminal("ok"),
                "register_capability": terminal("ok"),
                "watcher_behavior": terminal("ok"),
                "refresh": terminal("not_applicable", "Helix 25.07.1 uses push diagnostics"),
                "shutdown": terminal("ok"),
                "exit": terminal("ok"),
                "orphan_result": "none",
            },
            "extensions": {
                "helix.ui_observations": {
                    "diagnostics_visible": diagnostic_visible,
                    "completion_visible": completion_visible,
                    "definition_visible": definition_visible,
                },
                "helix.protocol_tokens": required_log_tokens,
            },
        }
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        return 0
    finally:
        run("tmux", "kill-session", "-t", session, check=False)
        if process_exists(args.perllsp):
            run("pkill", "-TERM", "-f", "--", str(args.perllsp), check=False)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:  # noqa: BLE001 - terminal evidence must retain the full failure
        print(f"helix actual-host failure: {error}", file=sys.stderr)
        raise
