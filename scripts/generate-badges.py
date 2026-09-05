#!/usr/bin/env python3
"""Generate the repository-scoped Shields endpoint consumed by README badges."""

from __future__ import annotations

import argparse
import ctypes
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
WINDOWS_JOB_LAUNCHER_FLAG = "--windows-job-launcher"
EXPECTED_COUNT_FIELDS = (
    "unsuppressed_exposure_gaps",
    "unsuppressed_test_efficiency_findings",
)
EXPECTED_RIPR_VERSION = "0.9.0"


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


class WindowsJob:
    """Own a Windows process tree independently of its direct leader."""

    _KILL_ON_JOB_CLOSE = 0x00002000
    _EXTENDED_LIMIT_INFORMATION = 9

    def __init__(self) -> None:
        from ctypes import wintypes

        class BasicLimitInformation(ctypes.Structure):
            _fields_ = [
                ("PerProcessUserTimeLimit", ctypes.c_longlong),
                ("PerJobUserTimeLimit", ctypes.c_longlong),
                ("LimitFlags", wintypes.DWORD),
                ("MinimumWorkingSetSize", ctypes.c_size_t),
                ("MaximumWorkingSetSize", ctypes.c_size_t),
                ("ActiveProcessLimit", wintypes.DWORD),
                ("Affinity", ctypes.c_size_t),
                ("PriorityClass", wintypes.DWORD),
                ("SchedulingClass", wintypes.DWORD),
            ]

        class IoCounters(ctypes.Structure):
            _fields_ = [
                ("ReadOperationCount", ctypes.c_ulonglong),
                ("WriteOperationCount", ctypes.c_ulonglong),
                ("OtherOperationCount", ctypes.c_ulonglong),
                ("ReadTransferCount", ctypes.c_ulonglong),
                ("WriteTransferCount", ctypes.c_ulonglong),
                ("OtherTransferCount", ctypes.c_ulonglong),
            ]

        class ExtendedLimitInformation(ctypes.Structure):
            _fields_ = [
                ("BasicLimitInformation", BasicLimitInformation),
                ("IoInfo", IoCounters),
                ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t),
                ("PeakProcessMemoryUsed", ctypes.c_size_t),
                ("PeakJobMemoryUsed", ctypes.c_size_t),
            ]

        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        kernel32.CreateJobObjectW.restype = wintypes.HANDLE
        kernel32.SetInformationJobObject.argtypes = [
            wintypes.HANDLE,
            ctypes.c_int,
            ctypes.c_void_p,
            wintypes.DWORD,
        ]
        kernel32.SetInformationJobObject.restype = wintypes.BOOL
        kernel32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        kernel32.AssignProcessToJobObject.restype = wintypes.BOOL
        kernel32.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        kernel32.TerminateJobObject.restype = wintypes.BOOL
        kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
        kernel32.CloseHandle.restype = wintypes.BOOL

        handle = kernel32.CreateJobObjectW(None, None)
        if not handle:
            self._raise_last_error("CreateJobObjectW")
        self._kernel32 = kernel32
        self._handle = handle
        limits = ExtendedLimitInformation()
        limits.BasicLimitInformation.LimitFlags = self._KILL_ON_JOB_CLOSE
        if not kernel32.SetInformationJobObject(
            handle,
            self._EXTENDED_LIMIT_INFORMATION,
            ctypes.byref(limits),
            ctypes.sizeof(limits),
        ):
            error = ctypes.get_last_error()
            kernel32.CloseHandle(handle)
            self._handle = None
            raise OSError(error, f"SetInformationJobObject: {ctypes.FormatError(error)}")

    @staticmethod
    def _raise_last_error(operation: str) -> None:
        error = ctypes.get_last_error()
        raise OSError(error, f"{operation}: {ctypes.FormatError(error)}")

    def assign(self, process: subprocess.Popen[bytes]) -> None:
        from ctypes import wintypes

        if self._handle is None:
            raise OSError("Windows job handle is closed")
        if not self._kernel32.AssignProcessToJobObject(
            self._handle, wintypes.HANDLE(process._handle)
        ):
            self._raise_last_error("AssignProcessToJobObject")

    def terminate(self) -> list[str]:
        failures: list[str] = []
        if self._handle is None:
            return failures
        if not self._kernel32.TerminateJobObject(self._handle, 1):
            error = ctypes.get_last_error()
            failures.append(f"TerminateJobObject failed: {ctypes.FormatError(error)}")
        failures.extend(self.close())
        return failures

    def close(self) -> list[str]:
        if self._handle is None:
            return []
        handle = self._handle
        self._handle = None
        if not self._kernel32.CloseHandle(handle):
            error = ctypes.get_last_error()
            return [f"CloseHandle(job) failed: {ctypes.FormatError(error)}"]
        return []


def bounded_stderr(stderr: str) -> str:
    normalized = stderr.strip()
    if len(normalized) <= STDERR_DIAGNOSTIC_LIMIT:
        return normalized
    return normalized[:STDERR_DIAGNOSTIC_LIMIT] + "... [truncated]"


def terminate_process_tree(
    process: subprocess.Popen[bytes],
    *,
    windows: bool | None = None,
    windows_job: WindowsJob | None = None,
) -> list[str]:
    """Best-effort tree termination that never masks the triggering failure."""
    failures: list[str] = []
    is_windows = os.name == "nt" if windows is None else windows
    if is_windows:
        needs_taskkill = windows_job is None
        if windows_job is not None:
            job_failures = windows_job.terminate()
            failures.extend(job_failures)
            needs_taskkill = bool(job_failures)
        if needs_taskkill:
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
    readers: list[threading.Thread], streams: list[tuple[str, BinaryIO]]
) -> str | None:
    """Close pipe streams only after every bounded reader is terminal."""
    for reader in readers:
        reader.join(timeout=TERMINATION_GRACE_SECONDS)
    still_running = [reader.name for reader in readers if reader.is_alive()]
    if still_running:
        return "output readers did not stop: " + ", ".join(still_running)
    failures: list[str] = []
    for stream_name, stream in streams:
        try:
            stream.close()
        except OSError as error:
            failures.append(f"{stream_name} close failed: {error}")
    return "; ".join(failures) if failures else None


def take_overflow(
    overflow: queue.Queue[tuple[str, int]],
) -> RiprOutputLimitExceeded | None:
    """Consume one persisted overflow signal, if present."""
    try:
        stream_name, limit = overflow.get_nowait()
    except queue.Empty:
        return None
    return RiprOutputLimitExceeded(stream_name, limit)


def take_reader_failures(
    failures: queue.Queue[tuple[str, OSError]],
) -> str | None:
    """Drain persisted reader failures before captured bytes can be accepted."""
    details: list[str] = []
    while True:
        try:
            stream_name, error = failures.get_nowait()
        except queue.Empty:
            break
        details.append(f"{stream_name} read failed: {error}")
    return "; ".join(details) if details else None


def read_bounded_stream(
    stream: BinaryIO,
    destination: bytearray,
    limit: int,
    stream_name: str,
    overflow: queue.Queue[tuple[str, int]],
    failures: queue.Queue[tuple[str, BaseException]],
) -> None:
    """Read at most ``limit`` bytes and signal before retaining any excess."""
    while True:
        read1 = getattr(stream, "read1", stream.read)
        try:
            chunk = read1(STREAM_READ_CHUNK_SIZE)
        except BaseException as error:
            # Fail closed on any reader failure: a dead reader thread must
            # never be mistaken for a completed stream.
            failures.put((stream_name, error))
            return
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
    windows_job: WindowsJob | None = None
    platform_options: dict[str, object]
    if os.name == "nt":
        try:
            windows_job = WindowsJob()
        except OSError as error:
            raise RuntimeError(f"could not create Windows process-tree owner: {error}") from error
        platform_options = {
            "creationflags": getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0),
            "stdin": subprocess.PIPE,
        }
        launched_command = [
            sys.executable,
            str(Path(__file__).resolve()),
            WINDOWS_JOB_LAUNCHER_FLAG,
            *command,
        ]
    else:
        platform_options = {"start_new_session": True}
        launched_command = command
    try:
        process = subprocess.Popen(
            launched_command,
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            **platform_options,
        )
    except BaseException as error:
        if windows_job is None:
            raise
        cleanup = windows_job.close()
        if isinstance(error, Exception):
            suffix = f"; cleanup incomplete: {'; '.join(cleanup)}" if cleanup else ""
            raise RuntimeError(
                f"could not launch ripr badge producer: {error}{suffix}"
            ) from error
        if cleanup:
            try:
                print(
                    "ripr launch cleanup incomplete after interrupt: "
                    + "; ".join(cleanup),
                    file=sys.stderr,
                )
            except BaseException:
                pass  # never let a failed diagnostic replace the interrupt
        raise
    if windows_job is not None:
        try:
            windows_job.assign(process)
            if process.stdin is None:
                raise OSError("Windows launcher input pipe was not created")
            process.stdin.write(b"\0")
            process.stdin.close()
        except OSError as error:
            try:
                process.kill()
                process.wait(timeout=TERMINATION_GRACE_SECONDS)
            except (OSError, subprocess.TimeoutExpired):
                pass
            cleanup = windows_job.terminate()
            suffix = f"; cleanup incomplete: {'; '.join(cleanup)}" if cleanup else ""
            raise RuntimeError(
                f"could not establish Windows process-tree ownership: {error}{suffix}"
            ) from error
    if process.stdout is None or process.stderr is None:
        cleanup = terminate_process_tree(process, windows_job=windows_job)
        suffix = f"; cleanup incomplete: {'; '.join(cleanup)}" if cleanup else ""
        raise RuntimeError(f"ripr output pipes were not created{suffix}")

    stdout_bytes = bytearray()
    stderr_bytes = bytearray()
    overflow: queue.Queue[tuple[str, int]] = queue.Queue()
    reader_failures: queue.Queue[tuple[str, BaseException]] = queue.Queue()
    readers = [
        threading.Thread(
            target=read_bounded_stream,
            args=(
                process.stdout,
                stdout_bytes,
                PRODUCER_STDOUT_LIMIT,
                "stdout",
                overflow,
                reader_failures,
            ),
            name="ripr-stdout-reader",
            daemon=True,
        ),
        threading.Thread(
            target=read_bounded_stream,
            args=(
                process.stderr,
                stderr_bytes,
                PRODUCER_STDERR_LIMIT,
                "stderr",
                overflow,
                reader_failures,
            ),
            name="ripr-stderr-reader",
            daemon=True,
        ),
    ]
    started_readers: list[threading.Thread] = []
    lifecycle_released = False
    try:
        for reader in readers:
            reader.start()
            started_readers.append(reader)

        deadline = time.monotonic() + timeout_seconds
        failure: RuntimeError | None = None
        while True:
            failure = take_overflow(overflow)
            if failure is not None:
                break
            reader_failure = take_reader_failures(reader_failures)
            if reader_failure is not None:
                failure = RuntimeError(
                    f"ripr output reader failed: {reader_failure}; "
                    "its process tree was terminated"
                )
                break
            if process.poll() is not None and all(
                not reader.is_alive() for reader in readers
            ):
                break
            if time.monotonic() >= deadline:
                failure = RuntimeError(
                    f"ripr check timed out after {timeout_seconds:g}s and its process tree was terminated"
                )
                break
            time.sleep(0.01)

        cleanup_failures = (
            terminate_process_tree(process, windows_job=windows_job)
            if failure is not None
            else []
        )
        reader_failure = finish_readers(
            readers, [("stdout", process.stdout), ("stderr", process.stderr)]
        )
        if reader_failure is not None:
            cleanup_failures.append(reader_failure)
        if failure is None:
            failure = take_overflow(overflow)
            if failure is not None:
                cleanup_failures.extend(
                    terminate_process_tree(process, windows_job=windows_job)
                )
        late_reader_failure = take_reader_failures(reader_failures)
        if late_reader_failure is not None:
            if failure is None:
                failure = RuntimeError(
                    f"ripr output reader failed: {late_reader_failure}; "
                    "its process tree was terminated"
                )
                cleanup_failures.extend(
                    terminate_process_tree(process, windows_job=windows_job)
                )
            else:
                cleanup_failures.append(late_reader_failure)
        if failure is None and windows_job is not None:
            cleanup_failures.extend(windows_job.close())
        lifecycle_released = True

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
            cleanup_suffix = (
                f"; cleanup incomplete: {'; '.join(cleanup_failures)}"
                if cleanup_failures
                else ""
            )
            raise RuntimeError(
                f"ripr check failed for ripr+ badge (exit {process.returncode})"
                f"{suffix}{cleanup_suffix}"
            )
        if cleanup_failures:
            raise RuntimeError(f"ripr cleanup incomplete: {'; '.join(cleanup_failures)}")
        try:
            stdout = stdout_bytes.decode("utf-8")
        except UnicodeDecodeError as error:
            raise RuntimeError("ripr stdout was not valid UTF-8") from error
        return stdout
    except BaseException as error:
        if not lifecycle_released:
            emergency_failures: list[str] = []
            try:
                emergency_failures.extend(
                    terminate_process_tree(process, windows_job=windows_job)
                )
            except BaseException as cleanup_error:
                emergency_failures.append(f"process-tree cleanup raised: {cleanup_error}")
            try:
                reader_failure = finish_readers(
                    started_readers,
                    [("stdout", process.stdout), ("stderr", process.stderr)],
                )
                if reader_failure is not None:
                    emergency_failures.append(reader_failure)
            except BaseException as cleanup_error:
                emergency_failures.append(f"reader cleanup raised: {cleanup_error}")
            if windows_job is not None:
                try:
                    emergency_failures.extend(windows_job.close())
                except BaseException as cleanup_error:
                    emergency_failures.append(f"job close raised: {cleanup_error}")
            if emergency_failures:
                detail = "; ".join(emergency_failures)
                if isinstance(error, RiprOutputLimitExceeded):
                    error.record_cleanup_failure(detail)
                else:
                    # BaseException.add_note requires Python 3.11; keep the
                    # cleanup diagnostic visible on older interpreters.
                    add_note = getattr(error, "add_note", None)
                    if add_note is not None:
                        add_note(f"ripr emergency cleanup: {detail}")
                    else:
                        print(f"ripr emergency cleanup: {detail}", file=sys.stderr)
        raise


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
    return {
        "schemaVersion": 1,
        "label": "ripr+",
        "message": str(unresolved),
        "color": "brightgreen" if unresolved == 0 else "yellow",
    }


def badge_from_receipt(
    receipt_path: Path, producer_path: Path, expected_source_sha: str
) -> dict[str, object]:
    """Map only an exact, successful RIPR producer artifact to the badge."""
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    producer = json.loads(producer_path.read_text(encoding="utf-8"))
    if receipt.get("schema_version") != 2 or receipt.get("kind") != "ripr_plus_baseline":
        raise ValueError("RIPR receipt has an unsupported schema or kind")
    if receipt.get("head") != expected_source_sha:
        raise ValueError("RIPR receipt head does not match the requested source SHA")
    if receipt.get("root") != ".":
        raise ValueError("RIPR receipt root is not the repository root")
    if not str(receipt.get("source_format", "")).startswith(
        "ripr check --format repo-badge-json"
    ):
        raise ValueError("RIPR receipt is not based on repo-badge-json")
    if (
        producer.get("schema_version") != 1
        or producer.get("kind") != "ripr_badge_producer"
    ):
        raise ValueError("RIPR producer receipt has an unsupported schema or kind")
    if producer.get("head") != expected_source_sha or producer.get("root") != ".":
        raise ValueError("RIPR producer receipt is not bound to the requested source SHA")
    if producer.get("ripr_version") != EXPECTED_RIPR_VERSION:
        raise ValueError("RIPR producer receipt version is not reviewed")
    counts = receipt.get("counts")
    if not isinstance(counts, dict):
        raise ValueError("RIPR receipt counts are missing")
    return badge_from_ripr({"counts": counts})


def generate(
    root: Path,
    check: bool,
    ripr_timeout_seconds: float = RIPR_TIMEOUT_SECONDS,
    receipt_path: Path | None = None,
    producer_path: Path | None = None,
    source_sha: str | None = None,
) -> None:
    started = time.monotonic()
    if receipt_path is not None or producer_path is not None:
        if receipt_path is None or producer_path is None or source_sha is None:
            raise RuntimeError("RIPR receipt mode requires receipt, producer, and source SHA")
        print(
            f"badges: consuming exact RIPR receipt ({time.monotonic() - started:.1f}s)",
            flush=True,
        )
        try:
            badge = badge_from_receipt(receipt_path, producer_path, source_sha)
        except (OSError, json.JSONDecodeError, ValueError) as error:
            raise RuntimeError(f"invalid exact RIPR receipt: {error}") from error
    else:
        print(
            f"badges: starting RIPR analysis ({time.monotonic() - started:.1f}s)",
            flush=True,
        )
        stdout = run_ripr(root, ripr_timeout_seconds)
        print(
            f"badges: RIPR analysis finished ({time.monotonic() - started:.1f}s)",
            flush=True,
        )
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
    if len(sys.argv) > 1 and sys.argv[1] == WINDOWS_JOB_LAUNCHER_FLAG:
        if os.name != "nt" or len(sys.argv) < 3:
            print("badges: invalid Windows job launcher invocation", file=sys.stderr)
            return 125
        if sys.stdin.buffer.read(1) != b"\0":
            print("badges: Windows job launcher was not released", file=sys.stderr)
            return 125
        try:
            return subprocess.run(sys.argv[2:], check=False).returncode
        except OSError as error:
            print(f"badges: Windows job launcher failed: {error}", file=sys.stderr)
            return 126

    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--ripr-receipt", type=Path)
    parser.add_argument("--producer-receipt", type=Path)
    parser.add_argument("--source-sha")
    try:
        args = parser.parse_args()
        generate(
            Path(__file__).resolve().parents[1],
            args.check,
            receipt_path=args.ripr_receipt,
            producer_path=args.producer_receipt,
            source_sha=args.source_sha,
        )
    except (OSError, RuntimeError) as error:
        print(f"badges: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
