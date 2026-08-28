#!/usr/bin/env python3
"""Generate the repository-scoped Shields endpoint consumed by README badges."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import sys
import threading
import time
from typing import BinaryIO

RIPR_TIMEOUT_SECONDS = 15 * 60
TERMINATION_GRACE_SECONDS = 5
STDERR_DIAGNOSTIC_LIMIT = 2_048
PRODUCER_STDOUT_LIMIT = 64 * 1_024
PRODUCER_STDERR_LIMIT = 64 * 1_024
STREAM_READ_CHUNK_SIZE = 8 * 1_024
EXPECTED_COUNT_FIELDS = (
    "unsuppressed_exposure_gaps",
    "unsuppressed_test_efficiency_findings",
)


class RiprOutputLimitExceeded(RuntimeError):
    """RIPR exceeded a producer-stream memory bound."""

    def __init__(self, stream_name: str, limit: int) -> None:
        super().__init__(
            f"ripr {stream_name} exceeded {limit} bytes; its process tree was terminated"
        )
        self.stream_name = stream_name
        self.limit = limit
        self.retained_stdout_bytes = 0
        self.retained_stderr_bytes = 0
        self.cleanup_failure: str | None = None

    def record_cleanup_failure(self, detail: str) -> None:
        self.cleanup_failure = detail
        self.args = (f"{self.args[0]}; cleanup incomplete: {detail}",)


def bounded_stderr(stderr: str) -> str:
    normalized = stderr.strip()
    if len(normalized) <= STDERR_DIAGNOSTIC_LIMIT:
        return normalized
    return normalized[:STDERR_DIAGNOSTIC_LIMIT] + "... [truncated]"


def terminate_process_tree(
    process: subprocess.Popen[bytes], *, windows: bool | None = None
) -> list[str]:
    """Best-effort tree termination that never masks the triggering failure."""
    failures: list[str] = []
    is_windows = os.name == "nt" if windows is None else windows
    if is_windows:
        try:
            result = subprocess.run(
                ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=TERMINATION_GRACE_SECONDS,
                check=False,
            )
        except subprocess.TimeoutExpired:
            failures.append("taskkill timed out")
        except OSError as error:
            failures.append(f"taskkill failed: {error}")
        else:
            if result.returncode:
                failures.append(f"taskkill exited {result.returncode}")
        if failures and process.poll() is None:
            try:
                process.kill()
            except OSError as error:
                failures.append(f"direct process kill failed: {error}")
    else:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        except OSError as error:
            failures.append(f"process-group SIGTERM failed: {error}")
        try:
            process.wait(timeout=TERMINATION_GRACE_SECONDS)
        except subprocess.TimeoutExpired:
            pass
        # The direct process can exit before a descendant that ignores SIGTERM.
        # Kill any surviving member of the original process group before return.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except OSError as error:
            failures.append(f"process-group SIGKILL failed: {error}")
    try:
        process.wait(timeout=TERMINATION_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        failures.append("direct process wait timed out")
        try:
            process.kill()
            process.wait(timeout=TERMINATION_GRACE_SECONDS)
        except (OSError, subprocess.TimeoutExpired) as error:
            failures.append(f"direct process fallback did not terminate: {error}")
    except OSError as error:
        failures.append(f"direct process wait failed: {error}")
    return failures


def finish_readers(
    readers: list[threading.Thread], streams: list[BinaryIO]
) -> str | None:
    """Close pipe streams only after every bounded reader is terminal."""
    for reader in readers:
        reader.join(timeout=TERMINATION_GRACE_SECONDS)
    still_running = [reader.name for reader in readers if reader.is_alive()]
    if still_running:
        return "output readers did not stop: " + ", ".join(still_running)
    for stream in streams:
        stream.close()
    return None


def read_bounded_stream(
    stream: BinaryIO,
    destination: bytearray,
    limit: int,
    stream_name: str,
    overflow: queue.Queue[tuple[str, int]],
) -> None:
    """Read at most ``limit`` bytes and signal before retaining any excess."""
    while True:
        read1 = getattr(stream, "read1", stream.read)
        chunk = read1(STREAM_READ_CHUNK_SIZE)
        if not chunk:
            return
        remaining = limit - len(destination)
        destination.extend(chunk[:remaining])
        if len(chunk) > remaining:
            overflow.put((stream_name, limit))
            return


def run_ripr(root: Path, timeout_seconds: float = RIPR_TIMEOUT_SECONDS) -> str:
    ripr = os.environ.get("RIPR_BIN", "ripr")
    command = [ripr, "check", "--root", str(root), "--format", "repo-badge-json"]
    platform_options: dict[str, object]
    if os.name == "nt":
        platform_options = {
            "creationflags": getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0),
        }
    else:
        platform_options = {"start_new_session": True}
    process = subprocess.Popen(
        command,
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        **platform_options,
    )
    if process.stdout is None or process.stderr is None:
        cleanup = terminate_process_tree(process)
        suffix = f"; cleanup incomplete: {'; '.join(cleanup)}" if cleanup else ""
        raise RuntimeError(f"ripr output pipes were not created{suffix}")

    stdout_bytes = bytearray()
    stderr_bytes = bytearray()
    overflow: queue.Queue[tuple[str, int]] = queue.Queue()
    readers = [
        threading.Thread(
            target=read_bounded_stream,
            args=(process.stdout, stdout_bytes, PRODUCER_STDOUT_LIMIT, "stdout", overflow),
            name="ripr-stdout-reader",
            daemon=True,
        ),
        threading.Thread(
            target=read_bounded_stream,
            args=(process.stderr, stderr_bytes, PRODUCER_STDERR_LIMIT, "stderr", overflow),
            name="ripr-stderr-reader",
            daemon=True,
        ),
    ]
    for reader in readers:
        reader.start()

    deadline = time.monotonic() + timeout_seconds
    failure: RuntimeError | None = None
    while True:
        try:
            stream_name, limit = overflow.get_nowait()
        except queue.Empty:
            pass
        else:
            failure = RiprOutputLimitExceeded(stream_name, limit)
            break
        if process.poll() is not None and all(not reader.is_alive() for reader in readers):
            break
        if time.monotonic() >= deadline:
            failure = RuntimeError(
                f"ripr check timed out after {timeout_seconds:g}s and its process tree was terminated"
            )
            break
        time.sleep(0.01)

    cleanup_failures = terminate_process_tree(process) if failure is not None else []
    reader_failure = finish_readers(readers, [process.stdout, process.stderr])
    if reader_failure is not None:
        cleanup_failures.append(reader_failure)

    stderr = stderr_bytes.decode("utf-8", errors="replace")
    if failure is not None:
        if isinstance(failure, RiprOutputLimitExceeded):
            failure.retained_stdout_bytes = len(stdout_bytes)
            failure.retained_stderr_bytes = len(stderr_bytes)
            if cleanup_failures:
                failure.record_cleanup_failure("; ".join(cleanup_failures))
            raise failure
        diagnostic = bounded_stderr(stderr)
        suffix = f"; stderr: {diagnostic}" if diagnostic else ""
        cleanup_suffix = (
            f"; cleanup incomplete: {'; '.join(cleanup_failures)}"
            if cleanup_failures
            else ""
        )
        raise RuntimeError(f"{failure}{suffix}{cleanup_suffix}")
    if process.returncode:
        diagnostic = bounded_stderr(stderr)
        suffix = f": {diagnostic}" if diagnostic else ""
        raise RuntimeError(f"ripr check failed for ripr+ badge (exit {process.returncode}){suffix}")
    try:
        stdout = stdout_bytes.decode("utf-8")
    except UnicodeDecodeError as error:
        raise RuntimeError("ripr stdout was not valid UTF-8") from error
    return stdout


def badge_from_ripr(payload: object) -> dict[str, object]:
    if not isinstance(payload, dict):
        raise ValueError("ripr emitted a non-object repo-badge-json payload")
    counts = payload.get("counts")
    if not isinstance(counts, dict):
        raise ValueError("ripr repo-badge-json payload must contain an object `counts` field")

    def count(name: str) -> int:
        if name not in counts:
            raise ValueError(f"ripr counts must contain {name!r}")
        value = counts[name]
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise ValueError(f"ripr count {name!r} must be a non-negative integer")
        return value

    unresolved = sum(count(name) for name in EXPECTED_COUNT_FIELDS)
    return {"schemaVersion": 1, "label": "ripr+", "message": str(unresolved), "color": "brightgreen" if unresolved == 0 else "yellow"}


def generate(root: Path, check: bool, ripr_timeout_seconds: float = RIPR_TIMEOUT_SECONDS) -> None:
    started = time.monotonic()
    print(f"badges: starting RIPR analysis ({time.monotonic() - started:.1f}s)", flush=True)
    stdout = run_ripr(root, ripr_timeout_seconds)
    print(f"badges: RIPR analysis finished ({time.monotonic() - started:.1f}s)", flush=True)
    try:
        badge = badge_from_ripr(json.loads(stdout))
    except (json.JSONDecodeError, ValueError) as error:
        raise RuntimeError(f"invalid repo-badge-json from RIPR: {error}") from error

    target = root / "target" / "xtask" / "badges" / "ripr-plus.json"
    committed = root / "badges" / "ripr-plus.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(badge, indent=2) + "\n"
    target.write_text(encoded, encoding="utf-8")
    if check:
        if not committed.is_file() or committed.read_text(encoding="utf-8") != encoded:
            raise RuntimeError(
                f"badge endpoint drift detected for {committed}; "
                "run `python3 scripts/generate-badges.py`"
            )
        print("badges: committed endpoints are current")
        return
    committed.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(target, committed)
    print(f"badges: refreshed public endpoint JSON ({time.monotonic() - started:.1f}s)")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    try:
        generate(Path(__file__).resolve().parents[1], parser.parse_args().check)
    except (OSError, RuntimeError) as error:
        print(f"badges: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
