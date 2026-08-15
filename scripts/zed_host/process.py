"""Launch exact Zed with an isolated profile and retain exact perllsp process evidence."""

from __future__ import annotations

import json
import os
import shlex
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from .common import (
    HostReceiptError,
    artifact_reference,
    redactions,
    sha256_file,
    write_json,
)


def _linux_processes(executable: Path) -> list[dict[str, Any]]:
    expected = executable.resolve()
    processes: list[dict[str, Any]] = []
    proc = Path("/proc")
    if not proc.is_dir():
        raise HostReceiptError("Linux exact process inventory requires /proc")
    for entry in proc.iterdir():
        if not entry.name.isdigit():
            continue
        try:
            actual = (entry / "exe").resolve(strict=True)
            command = (
                (entry / "cmdline")
                .read_bytes()
                .replace(b"\0", b" ")
                .decode("utf-8", errors="replace")
            )
        except (FileNotFoundError, PermissionError, OSError):
            continue
        if actual == expected:
            processes.append(
                {"pid": int(entry.name), "executable": str(actual), "command": command}
            )
    return processes


def _macos_processes(executable: Path) -> list[dict[str, Any]]:
    completed = subprocess.run(
        ["ps", "-axo", "pid=,command="],
        check=True,
        capture_output=True,
        text=True,
        timeout=15,
    )
    expected = executable.resolve()
    processes: list[dict[str, Any]] = []
    for line in completed.stdout.splitlines():
        pid_text, separator, command = line.strip().partition(" ")
        if not separator or not pid_text.isdigit():
            continue
        try:
            executable_token = shlex.split(command)[0]
            actual = Path(executable_token).expanduser().resolve()
        except (IndexError, OSError, ValueError):
            continue
        if actual == expected:
            processes.append(
                {"pid": int(pid_text), "executable": str(expected), "command": command}
            )
    return processes


def _windows_powershell() -> str:
    for name in ("pwsh", "powershell.exe"):
        resolved = shutil.which(name)
        if resolved:
            return resolved
    raise HostReceiptError("Windows process inventory requires pwsh or powershell.exe")


def _windows_processes(executable: Path) -> list[dict[str, Any]]:
    query = (
        "$ErrorActionPreference='Stop';"
        "Get-CimInstance Win32_Process | "
        "Select-Object ProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress"
    )
    completed = subprocess.run(
        [_windows_powershell(), "-NoLogo", "-NoProfile", "-Command", query],
        check=True,
        capture_output=True,
        text=True,
        timeout=20,
    )
    payload = completed.stdout.strip()
    if not payload:
        return []
    value = json.loads(payload)
    rows = value if isinstance(value, list) else [value]
    expected = os.path.normcase(str(executable.resolve()))
    processes: list[dict[str, Any]] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        actual = row.get("ExecutablePath")
        pid = row.get("ProcessId")
        if isinstance(actual, str) and isinstance(pid, int):
            if os.path.normcase(actual) == expected:
                processes.append(
                    {
                        "pid": pid,
                        "executable": actual,
                        "command": row.get("CommandLine") or "",
                    }
                )
    return processes


def matching_processes(executable: Path) -> list[dict[str, Any]]:
    if os.name == "nt":
        return _windows_processes(executable)
    if sys.platform == "darwin":
        return _macos_processes(executable)
    return _linux_processes(executable)


def _process_ids(rows: list[dict[str, Any]]) -> set[int]:
    return {row["pid"] for row in rows if isinstance(row.get("pid"), int)}


def launch(manifest: dict[str, Any], run_dir: Path, timeout_seconds: int) -> int:
    prepared_manifest = run_dir / "manifest.json"
    if not prepared_manifest.is_file():
        raise HostReceiptError("prepared manifest is missing")
    prepared_manifest_sha256 = sha256_file(prepared_manifest)

    zed_cli = Path(manifest["zed"]["cli"])
    zed_app = Path(manifest["zed"]["app"])
    profile = Path(manifest["profile"]["directory"])
    workspace = Path(manifest["workspace"]["directory"])
    perllsp = Path(manifest["perllsp"]["command"])
    artifacts = run_dir / "artifacts"
    artifacts.mkdir(parents=True, exist_ok=True)
    stdout_path = artifacts / "zed-foreground.stdout.log"
    stderr_path = artifacts / "zed-foreground.stderr.log"
    process_path = artifacts / "process-inventory.json"

    before = matching_processes(perllsp)
    command = [
        str(zed_cli),
        "--zed",
        str(zed_app),
        "--foreground",
        "--wait",
        "--user-data-dir",
        str(profile),
        str(workspace),
    ]
    print("Zed will start in an isolated profile.")
    print("Inside Zed, invoke zed::InstallDevExtension and select:")
    print(manifest["extension"]["directory"])
    print(
        "Complete the exact observation checklist, then close the Zed window normally."
    )

    samples: list[dict[str, Any]] = []
    before_pids = _process_ids(before)
    started_at = time.monotonic()
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        process = subprocess.Popen(command, stdout=stdout, stderr=stderr)
        while process.poll() is None:
            rows = matching_processes(perllsp)
            new_rows = [row for row in rows if row.get("pid") not in before_pids]
            if new_rows:
                samples.append(
                    {
                        "offset_seconds": round(time.monotonic() - started_at, 3),
                        "rows": new_rows,
                    }
                )
            if time.monotonic() - started_at > timeout_seconds:
                process.terminate()
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=15)
                raise HostReceiptError(
                    "Zed exact-source host session exceeded the bounded timeout"
                )
            time.sleep(1.0)
        return_code = process.returncode

    after = matching_processes(perllsp)
    leaked = sorted(_process_ids(after) - _process_ids(before))
    replacements = redactions(manifest, run_dir)
    for sample in samples:
        for row in sample["rows"]:
            command_text = str(row.get("command", ""))
            for source, replacement in replacements:
                command_text = command_text.replace(source, replacement)
            row["command"] = command_text
            row["executable"] = "<perllsp>"
    inventory = {
        "schema_version": "zed_exact_source_process_inventory.v1",
        "prepared_manifest_sha256": prepared_manifest_sha256,
        "command": [
            "<zed-cli>",
            "--zed",
            "<zed-app>",
            "--foreground",
            "--wait",
            "--user-data-dir",
            "<profile>",
            "<workspace>",
        ],
        "zed_return_code": return_code,
        "perllsp_observed": bool(samples),
        "perllsp_samples": samples,
        "preexisting_perllsp_pids": sorted(_process_ids(before)),
        "post_session_perllsp_pids": sorted(_process_ids(after)),
        "new_surviving_perllsp_pids": leaked,
    }
    write_json(process_path, inventory)
    launch_result = {
        "schema_version": "zed_exact_source_launch.v1",
        "prepared_manifest_sha256": prepared_manifest_sha256,
        "result": "pass" if return_code == 0 and samples and not leaked else "fail",
        "zed_return_code": return_code,
        "perllsp_observed": bool(samples),
        "new_surviving_perllsp_pids": leaked,
        "stdout": artifact_reference(stdout_path, run_dir),
        "stderr": artifact_reference(stderr_path, run_dir),
        "process_inventory": artifact_reference(process_path, run_dir),
    }
    write_json(run_dir / "launch.json", launch_result)
    return 0 if launch_result["result"] == "pass" else 1
