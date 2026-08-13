"""Exact perllsp version, lifecycle, stdout, and orphan proof."""

from __future__ import annotations

import csv
import io
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

from .common import ReceiptError, sha256_bytes
from .framing import lsp_frame, parse_lsp_frames

MAX_STDERR_BYTES = 8192
PROCESS_TIMEOUT_SECONDS = 30


def _linux_processes(binary: Path) -> set[int]:
    matches: set[int] = set()
    proc = Path("/proc")
    if not proc.is_dir():
        raise ReceiptError("Linux process inventory requires /proc")
    expected = binary.resolve()
    for entry in proc.iterdir():
        if not entry.name.isdigit():
            continue
        try:
            executable = (entry / "exe").resolve(strict=True)
        except (FileNotFoundError, PermissionError, OSError):
            continue
        if executable == expected:
            matches.add(int(entry.name))
    return matches


def _macos_processes(binary: Path) -> set[int]:
    completed = subprocess.run(
        ["ps", "-axo", "pid=,command="],
        check=True,
        capture_output=True,
        text=True,
        timeout=15,
    )
    expected = str(binary.resolve())
    matches: set[int] = set()
    for line in completed.stdout.splitlines():
        pid_text, separator, command = line.strip().partition(" ")
        if separator and expected in command and pid_text.isdigit():
            matches.add(int(pid_text))
    return matches


def _windows_processes(binary: Path) -> set[int]:
    completed = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {binary.name}", "/FO", "CSV", "/NH"],
        check=True,
        capture_output=True,
        text=True,
        timeout=15,
    )
    matches: set[int] = set()
    for row in csv.reader(io.StringIO(completed.stdout)):
        if len(row) < 2 or row[0].lower() != binary.name.lower():
            continue
        pid = row[1].replace(",", "")
        if pid.isdigit():
            matches.add(int(pid))
    return matches


def matching_processes(binary: Path) -> set[int]:
    if os.name == "nt":
        return _windows_processes(binary)
    if sys.platform == "darwin":
        return _macos_processes(binary)
    return _linux_processes(binary)


def run_stdio_smoke(binary: Path, work_dir: Path) -> dict[str, Any]:
    version = subprocess.run(
        [str(binary), "--version"],
        check=False,
        capture_output=True,
        text=True,
        timeout=PROCESS_TIMEOUT_SECONDS,
    )
    version_output = (version.stdout or version.stderr).strip()
    if version.returncode != 0:
        raise ReceiptError(f"perllsp --version failed with exit {version.returncode}")
    if "perllsp" not in version_output.lower():
        raise ReceiptError(f"version output does not identify perllsp: {version_output!r}")

    before = matching_processes(binary)
    messages = [
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": None,
                "clientInfo": {"name": "zed-public-asset-receipt", "version": "1"},
                "rootUri": None,
                "capabilities": {},
            },
        },
        {"jsonrpc": "2.0", "method": "initialized", "params": {}},
        {"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": None},
        {"jsonrpc": "2.0", "method": "exit", "params": None},
    ]
    payload = b"".join(lsp_frame(message) for message in messages)
    process = subprocess.Popen(
        [str(binary), "--stdio"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=work_dir,
    )
    try:
        stdout, stderr = process.communicate(payload, timeout=PROCESS_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        process.kill()
        process.wait(timeout=10)
        raise ReceiptError("perllsp stdio lifecycle timed out") from error

    frames = parse_lsp_frames(stdout)
    responses = {frame.get("id"): frame for frame in frames if "id" in frame}
    initialize = responses.get(1)
    shutdown = responses.get(2)
    if not isinstance(initialize, dict) or "result" not in initialize:
        raise ReceiptError("stdio lifecycle lacks initialize response")
    if not isinstance(shutdown, dict) or shutdown.get("result") is not None:
        raise ReceiptError("stdio lifecycle lacks the expected null shutdown response")
    if process.returncode != 0:
        raise ReceiptError(f"perllsp stdio process exited with {process.returncode}")

    after = matching_processes(binary)
    leaked = sorted(after - before)
    if leaked:
        raise ReceiptError("perllsp process inventory grew after shutdown: " + ",".join(map(str, leaked)))

    bounded_stderr = stderr[-MAX_STDERR_BYTES:].decode("utf-8", errors="replace")
    bounded_stderr = bounded_stderr.replace(str(work_dir), "<work-dir>")
    return {
        "result": "pass",
        "version_output": version_output,
        "process_exit": process.returncode,
        "frames": len(frames),
        "initialize_response": True,
        "shutdown_response": True,
        "stdout_pure": True,
        "process_inventory_before": sorted(before),
        "process_inventory_after": sorted(after),
        "process_group_clean": True,
        "stderr_sha256": sha256_bytes(stderr),
        "stderr_tail": bounded_stderr,
    }
