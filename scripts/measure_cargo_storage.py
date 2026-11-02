#!/usr/bin/env python3
"""Observe an already admitted command; never grants admission or cleanup authority."""
import argparse
import ctypes
import json
import math
import os
from pathlib import Path
import shutil
import signal
import stat
import subprocess
import threading
import time
import traceback
from ctypes import wintypes


class MeasurementError(RuntimeError):
    pass


def _windows_allocated_bytes(path):
    """Return NTFS allocated size for a regular file, or None when unavailable."""
    if os.name != "nt":
        return None
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    get_size = kernel32.GetCompressedFileSizeW
    get_size.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(wintypes.DWORD)]
    get_size.restype = wintypes.DWORD
    high = wintypes.DWORD()
    low = get_size(str(path), ctypes.byref(high))
    if low == 0xFFFFFFFF and ctypes.get_last_error() != 0:
        return None
    return (int(high.value) << 32) + int(low)


def entry_allocated_bytes(path, metadata):
    """Known inode/cluster allocation, or None when the entry is not measured."""
    blocks = getattr(metadata, "st_blocks", None)
    if type(blocks) is int and blocks >= 0:
        return blocks * 512
    if stat.S_ISDIR(metadata.st_mode):
        return 0 if os.name == "nt" else None
    if stat.S_ISREG(metadata.st_mode) and os.name == "nt":
        return _windows_allocated_bytes(path)
    return None


def filesystem_snapshot(ancestor):
    if os.name == "nt":
        total, _used, free = shutil.disk_usage(ancestor)
        return {"observed_at": str(ancestor), "free_bytes": free, "total_bytes": total}
    vfs = os.statvfs(ancestor)
    return {
        "observed_at": str(ancestor),
        "free_bytes": vfs.f_bavail * vfs.f_frsize,
        "total_bytes": vfs.f_blocks * vfs.f_frsize,
    }


def allocation(roots, stop=None, diagnostics=None):
    """Walk observed roots once; vanishing paths are sampling limits, not fatal errors."""
    seen = set()
    total = 0
    measured = True
    races = 0
    inaccessible = []
    attributed = [0] * len(roots)
    pending = list(reversed(list(enumerate(roots))))
    while pending:
        if stop is not None and stop.is_set():
            raise MeasurementError("allocation sample interrupted")
        index, path = pending.pop()
        try:
            metadata = path.lstat()
        except FileNotFoundError:
            races += 1
            continue
        except PermissionError as error:
            inaccessible.append({"kind": "permission_denied", "path_kind": "entry"})
            if error.args and isinstance(error.args[0], str):
                inaccessible[-1]["reason"] = error.args[0][:256]
            measured = False
            continue
        except OSError as error:
            if isinstance(error, PermissionError) or getattr(error, "errno", None) in (
                getattr(os, "EACCES", None),
                getattr(os, "EPERM", None),
            ):
                inaccessible.append({"kind": "permission_denied", "path_kind": "entry"})
                if error.args and isinstance(error.args[0], str):
                    inaccessible[-1]["reason"] = error.args[0][:256]
                measured = False
                continue
            raise
        identity = (metadata.st_dev, metadata.st_ino)
        if identity in seen:
            continue
        seen.add(identity)
        allocated = entry_allocated_bytes(path, metadata)
        if allocated is None:
            measured = False
            continue
        total += allocated
        attributed[index] += allocated
        if stat.S_ISDIR(metadata.st_mode):
            try:
                with os.scandir(path) as entries:
                    pending.extend((index, Path(entry.path)) for entry in entries)
            except FileNotFoundError:
                races += 1
            except PermissionError as error:
                inaccessible.append({"kind": "permission_denied", "path_kind": "directory"})
                if error.args and isinstance(error.args[0], str):
                    inaccessible[-1]["reason"] = error.args[0][:256]
                measured = False
            except OSError as error:
                if isinstance(error, PermissionError) or getattr(error, "errno", None) in (
                    getattr(os, "EACCES", None),
                    getattr(os, "EPERM", None),
                ):
                    inaccessible.append({"kind": "permission_denied", "path_kind": "directory"})
                    if error.args and isinstance(error.args[0], str):
                        inaccessible[-1]["reason"] = error.args[0][:256]
                    measured = False
                else:
                    raise
    if diagnostics is not None:
        diagnostics["vanished_entries"] = races
        diagnostics["inaccessible_entries"] = inaccessible
        diagnostics["inaccessible_entry_count"] = len(inaccessible)
        diagnostics["coverage_complete"] = races == 0 and not inaccessible and measured
        diagnostics["allocated_by_root"] = [
            {"root": str(root), "allocated_bytes": count} for root, count in zip(roots, attributed)
        ]
    return total


def observe(roots, stop=None):
    started = time.monotonic()
    filesystems = {}
    for root in roots:
        ancestor = root
        while not ancestor.exists():
            if ancestor.parent == ancestor:
                raise MeasurementError(f"no existing ancestor: {root}")
            ancestor = ancestor.parent
        identity = ancestor.stat().st_dev
        filesystems.setdefault(str(identity), filesystem_snapshot(ancestor))
    diagnostics = {}
    allocated = allocation(roots, stop, diagnostics)
    row = {
        "allocated_bytes": allocated,
        "filesystems": filesystems,
        "scan_seconds": time.monotonic() - started,
        **diagnostics,
    }
    return row


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", action="append", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--interval", type=float, default=1.0)
    parser.add_argument(
        "--min-free-bytes",
        type=int,
        default=0,
        help="Cancel the admitted owner if a sample falls below this reserve",
    )
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or not math.isfinite(args.interval) or args.interval <= 0 or args.min_free_bytes < 0:
        parser.error("a command and positive finite interval are required")
    roots = [root.absolute() for root in args.root]
    evidence = args.evidence.absolute()
    if any(evidence == root or root in evidence.parents for root in roots):
        parser.error("evidence must be outside observed roots")
    stop = threading.Event()
    scan_stop = threading.Event()
    samples = []
    cancelled = []
    worker_state = {"finished": False, "failed": False}
    worker_failure = {
        "kind": "worker-failure",
        "valid": False,
        "error": "periodic measurement worker failed",
    }
    sample_phase = "observation"
    started = time.monotonic()
    with evidence.open("x", encoding="utf-8") as stream:
        def write(row):
            stream.write(json.dumps(row, sort_keys=True) + "\n")
            stream.flush()
            os.fsync(stream.fileno())

        def sample(kind, event=None):
            nonlocal sample_phase
            sample_phase = "observation"
            row = {"kind": kind, "elapsed_seconds": time.monotonic() - started}
            try:
                row.update(observe(roots, event))
                row["valid"] = True
            except (OSError, MeasurementError) as error:
                row.update(valid=False, error=str(error))
            samples.append(row)
            sample_phase = "persistence"
            write(row)

        write(
            {
                "schema": "cargo-storage-measurement.v1",
                "kind": "header",
                "roots": [str(root) for root in roots],
                "interval_seconds": args.interval,
                "min_free_bytes": args.min_free_bytes,
                "limitations": (
                    "Sampled allocation is a lower bound on transient peak; scans are non-atomic "
                    "and vanishing entries are counted. Inaccessible entries are diagnostic only. "
                    "Partial coverage withholds complete growth claims even when sampling continues. "
                    "Per-root hardlink attribution follows root order. Free-space observations include "
                    "unrelated writers. Missing roots count as zero only at observation time. "
                    "No admission, quota, settlement or cleanup authority."
                ),
            }
        )
        sample("initial")

        def reserve_lost(row):
            return row["valid"] and any(
                fs["free_bytes"] < args.min_free_bytes for fs in row["filesystems"].values()
            )

        if not samples[-1]["valid"] or reserve_lost(samples[-1]):
            return 75
        command_started = time.monotonic()
        child = subprocess.Popen(command)

        def signal_child(signum):
            if child.poll() is None:
                try:
                    child.send_signal(signum)
                except ProcessLookupError:
                    pass

        def interrupt(signum, _frame):
            cancelled.append(signum)
            scan_stop.set()
            signal_child(signum)

        old = {sig: signal.signal(sig, interrupt) for sig in (signal.SIGINT, signal.SIGTERM)}

        def monitor():
            try:
                while not stop.wait(args.interval):
                    sample("periodic", scan_stop)
                    if not samples[-1]["valid"] or reserve_lost(samples[-1]):
                        if not cancelled:
                            signal_child(signal.SIGINT)
                        return
            except BaseException as error:
                worker_state["failed"] = True
                if not cancelled:
                    signal_child(signal.SIGINT)
                worker_failure["phase"] = sample_phase
                worker_failure["error_type"] = type(error).__name__
                if error.args and isinstance(error.args[0], str):
                    worker_failure["error"] = error.args[0][:1024]
                try:
                    traceback.print_exception(type(error), error, error.__traceback__)
                except BaseException:
                    pass
                try:
                    samples.append(worker_failure)
                    write(worker_failure)
                except BaseException:
                    pass
            finally:
                worker_state["finished"] = True

        worker = threading.Thread(target=monitor)
        worker.start()
        try:
            status = child.wait()
            command_elapsed = time.monotonic() - command_started
        finally:
            stop.set()
            worker.join()
            for sig, handler in old.items():
                signal.signal(sig, handler)
        sample("retained")
        valid = [row for row in samples if row.get("valid")]
        partial_coverage = any(not row.get("coverage_complete", True) for row in valid)
        complete = (
            worker_state["finished"]
            and not worker_state["failed"]
            and all(row.get("valid") for row in samples)
            and not partial_coverage
        )
        reserve_preserved = all(not reserve_lost(row) for row in samples)
        peak_rows = [row for row in valid if row.get("coverage_complete", True)]
        if peak_rows:
            sampled_peak = max(row["allocated_bytes"] for row in peak_rows)
        elif valid:
            sampled_peak = max(row["allocated_bytes"] for row in valid)
        else:
            sampled_peak = None
        write(
            {
                "kind": "summary",
                "command_status": status,
                "cancelled_signals": cancelled,
                "command_elapsed_seconds": command_elapsed,
                "elapsed_seconds": time.monotonic() - started,
                "sample_count": len(samples),
                "measurement_complete": complete,
                "partial_coverage": partial_coverage,
                "growth_claim_complete": complete,
                "worker_failure": worker_failure if worker_state["failed"] else None,
                "observed_reserve_preserved": reserve_preserved,
                "sampled_peak_allocated_bytes": sampled_peak,
                "retained_allocated_bytes": samples[-1].get("allocated_bytes"),
                "peak_is_bound": False,
            }
        )
    if cancelled:
        return 130
    if not complete or not reserve_preserved:
        return 75
    return status if status >= 0 else 128 - status


if __name__ == "__main__":
    raise SystemExit(main())
