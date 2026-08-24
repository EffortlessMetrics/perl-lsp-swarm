"""Exact perl-dap version, DAP lifecycle, stdout-purity, and orphan proof.

The smoke proves only the public adapter process boundary: the binary
identifies itself as `perl-dap` with the expected release version, completes
one DAP initialize -> initialized -> disconnect -> terminated exchange over
stdio with protocol-only stdout, and leaves no surviving process. Breakpoint,
stack, and variables behavior belongs to #9486/#9487 and the generic
perl-dap product receipts, not here.
"""

from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Any

from .common import ReceiptError, sha256_bytes
from .framing import lsp_frame, parse_lsp_frames
from .process import matching_processes

MAX_STDERR_BYTES = 8192
PROCESS_TIMEOUT_SECONDS = 30


def _version(binary: Path, expected_version: str) -> str:
    try:
        version = subprocess.run(
            [str(binary), "--version"],
            check=False,
            capture_output=True,
            text=True,
            timeout=PROCESS_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise ReceiptError("perl-dap --version timed out") from error
    output = (version.stdout or version.stderr).strip()
    if version.returncode != 0:
        raise ReceiptError(f"perl-dap --version failed with exit {version.returncode}")
    if "perl-dap" not in output.lower():
        raise ReceiptError(f"version output does not identify perl-dap: {output!r}")
    if expected_version not in output:
        raise ReceiptError(
            f"version output {output!r} does not report the expected release "
            f"version {expected_version!r}"
        )
    return output


def run_dap_stdio_smoke(
    binary: Path,
    work_dir: Path,
    expected_version: str,
) -> dict[str, Any]:
    version_output = _version(binary, expected_version)

    before = matching_processes(binary)
    initialize = {
        "seq": 1,
        "type": "request",
        "command": "initialize",
        "arguments": {
            "clientID": "zed-perl-dap-asset-receipt",
            "clientName": "perl-dap public asset receipt",
            "adapterID": "perl-dap",
            "columnsStartAt1": True,
            "linesStartAt1": True,
            "pathFormat": "path",
        },
    }
    disconnect = {
        "seq": 2,
        "type": "request",
        "command": "disconnect",
        "arguments": {"terminateDebuggee": True},
    }
    payload = lsp_frame(initialize) + lsp_frame(disconnect)
    process = subprocess.Popen(
        [str(binary), "--stdio"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=work_dir,
    )
    try:
        after_launch = matching_processes(binary)
        if process.pid not in after_launch:
            raise ReceiptError(
                "post-launch inventory did not observe the launched perl-dap process "
                f"{process.pid}; process cleanup cannot be proven on this host"
            )
        stdout, stderr = process.communicate(payload, timeout=PROCESS_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        process.kill()
        process.wait(timeout=10)
        raise ReceiptError("perl-dap DAP lifecycle timed out") from error
    except BaseException:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=10)
        raise

    frames = parse_lsp_frames(stdout)
    responses = {
        frame.get("request_seq"): frame
        for frame in frames
        if frame.get("type") == "response"
    }
    events = [
        frame.get("event") for frame in frames if frame.get("type") == "event"
    ]

    initialize_response = responses.get(1)
    if not isinstance(initialize_response, dict) or initialize_response.get("command") != "initialize":
        raise ReceiptError("DAP lifecycle lacks the initialize response")
    if initialize_response.get("success") is not True:
        raise ReceiptError("DAP initialize response did not succeed")
    if not isinstance(initialize_response.get("body"), dict):
        raise ReceiptError("DAP initialize response lacks a capabilities body")
    if "initialized" not in events:
        raise ReceiptError("DAP lifecycle lacks the initialized event")

    disconnect_response = responses.get(2)
    if not isinstance(disconnect_response, dict) or disconnect_response.get("command") != "disconnect":
        raise ReceiptError("DAP lifecycle lacks the disconnect response")
    if disconnect_response.get("success") is not True:
        raise ReceiptError("DAP disconnect response did not succeed")
    if "terminated" not in events:
        raise ReceiptError("DAP lifecycle lacks the terminated event")
    if process.returncode != 0:
        raise ReceiptError(f"perl-dap DAP process exited with {process.returncode}")

    after = matching_processes(binary)
    leaked = sorted(after - before)
    if leaked:
        raise ReceiptError(
            "perl-dap process inventory grew after disconnect: " + ",".join(map(str, leaked))
        )
    if process.pid in after:
        raise ReceiptError(f"launched perl-dap process {process.pid} survived disconnect")

    bounded_stderr = stderr[-MAX_STDERR_BYTES:].decode("utf-8", errors="replace")
    bounded_stderr = bounded_stderr.replace(str(work_dir), "<work-dir>")
    return {
        "result": "pass",
        "version_output": version_output,
        "process_exit": process.returncode,
        "frames": len(frames),
        "initialize_response": True,
        "initialized_event": True,
        "disconnect_response": True,
        "terminated_event": True,
        "configuration_boundary": "not_crossed_no_launch",
        "stdout_pure": True,
        "launched_pid": process.pid,
        "process_inventory_before": sorted(before),
        "process_inventory_after_launch": sorted(after_launch),
        "process_inventory_after": sorted(after),
        "orphan_result": "no_orphans",
        "stderr_sha256": sha256_bytes(stderr),
        "stderr_tail": bounded_stderr,
    }
