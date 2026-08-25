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


def canonical_version_line(output: str) -> str:
    """The first non-empty line of the version output."""
    for line in output.splitlines():
        line = line.strip()
        if line:
            return line
    return ""


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
    canonical = f"perl-dap {expected_version}"
    first_line = canonical_version_line(output)
    if first_line != canonical:
        raise ReceiptError(
            f"version output {first_line!r} is not the exact canonical line "
            f"{canonical!r}; a prefix or suffix version cannot satisfy the row"
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

    # Cleanup is proven before the protocol assertions: a candidate whose
    # transcript later fails validation must not escape the orphan inventory
    # just because parsing raised first.
    after = matching_processes(binary)
    leaked = sorted(after - before)
    if leaked:
        raise ReceiptError(
            "perl-dap process inventory grew after disconnect: " + ",".join(map(str, leaked))
        )
    if process.pid in after:
        raise ReceiptError(f"launched perl-dap process {process.pid} survived disconnect")

    frames = parse_lsp_frames(stdout)

    # The lifecycle claim is an ordered protocol sequence, so the proof keeps
    # the observed order: folding frames into maps would let an out-of-order
    # transcript pass membership checks. The adapter does not guarantee where
    # the `initialized` event lands relative to the disconnect response (the
    # event may flush after it), so the enforced invariant is the protocol's
    # partial order: exactly these four frames, initialize response before
    # the disconnect response, initialized before terminated, terminated last.
    sequence: list[tuple[str, str]] = []
    responses: dict[Any, dict[str, Any]] = {}
    for frame in frames:
        frame_type = frame.get("type")
        if frame_type == "response":
            sequence.append(("response", str(frame.get("command"))))
            responses[frame.get("request_seq")] = frame
        elif frame_type == "event":
            sequence.append(("event", str(frame.get("event"))))
        else:
            raise ReceiptError(
                f"DAP stdout carries an unexpected frame type {frame_type!r}; "
                "protocol-only stdout admits responses and events only"
            )
    expected_frames = {
        ("response", "initialize"),
        ("event", "initialized"),
        ("response", "disconnect"),
        ("event", "terminated"),
    }
    if set(sequence) != expected_frames or len(sequence) != len(expected_frames):
        raise ReceiptError(
            f"DAP frame transcript {sequence} is not exactly the initialize/"
            "initialized/disconnect/terminated exchange"
        )

    def first_index(entry: tuple[str, str]) -> int:
        return sequence.index(entry)

    if first_index(("response", "initialize")) >= first_index(("response", "disconnect")):
        raise ReceiptError(
            "DAP transcript answers disconnect before initialize; the required "
            "initialize-before-disconnect partial order is violated"
        )
    if first_index(("event", "initialized")) >= first_index(("event", "terminated")):
        raise ReceiptError(
            "DAP transcript reports termination before initialization; the "
            "initialized-before-terminated partial order is violated"
        )
    if sequence[-1] != ("event", "terminated"):
        raise ReceiptError("DAP transcript does not end with the terminated event")

    initialize_response = responses.get(1)
    if not isinstance(initialize_response, dict):
        raise ReceiptError("DAP initialize response did not echo request_seq 1")
    if initialize_response.get("success") is not True:
        raise ReceiptError("DAP initialize response did not succeed")
    if not isinstance(initialize_response.get("body"), dict):
        raise ReceiptError("DAP initialize response lacks a capabilities body")
    disconnect_response = responses.get(2)
    if not isinstance(disconnect_response, dict):
        raise ReceiptError("DAP disconnect response did not echo request_seq 2")
    if disconnect_response.get("success") is not True:
        raise ReceiptError("DAP disconnect response did not succeed")
    if process.returncode != 0:
        raise ReceiptError(f"perl-dap DAP process exited with {process.returncode}")

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
