#!/usr/bin/env python3
"""Read-only PR Smoke artifact inventory (#17231).

No build, cleanup, admission, peak or savings decision is performed. Cargo
capture integration belongs to the existing PR Smoke owner. Input identities
are retained declarations, not independently verified Git/Cargo fingerprints.
"""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import math
import os
import signal
import stat
import sys
import time
from pathlib import Path
from typing import Callable

MAX_SECONDS = 30.0
MAX_ENTRIES = 50000
MAX_INPUT_BYTES = 4 * 1024 * 1024
MAX_LINE_BYTES = 65536
MAX_UNIT_BYTES = 256 * 1024
MAX_OUTPUT_BYTES = 1024 * 1024
OUTPUT_RESERVE = 1.0
CATEGORIES = ("test_executable", "executable", "dependency_output", "other", "mixed")


class BudgetExpired(TimeoutError):
    pass


class Budget:
    """A shared collection budget; spent includes preceding invocation costs."""

    def __init__(self, seconds=30.0, spent=0.0, clock: Callable[[], float] = time.monotonic):
        if not math.isfinite(seconds) or not 0 < seconds <= MAX_SECONDS:
            raise ValueError("collection budget must be finite and in (0, 30]")
        if not math.isfinite(spent) or not 0 <= spent <= seconds:
            raise ValueError("spent collection time must be within the total budget")
        self.seconds = seconds
        self.spent = spent
        self.clock = clock
        self.start = clock()

    def elapsed(self):
        return max(0.0, self.clock() - self.start)

    def remaining(self):
        return max(0.0, self.seconds - self.spent - self.elapsed())

    def check(self):
        if self.remaining() <= OUTPUT_RESERVE:
            raise BudgetExpired("collection deadline; report-write reserve retained")

    def charged(self):
        # Conservatively reserve one second for serialization/output. The CLI
        # enforces that write deadline too; callers can charge measured full
        # invocation time instead, but must never grant a second 30s budget.
        return min(self.seconds, self.spent + self.elapsed() + OUTPUT_RESERVE)


def allocated_bytes(info):
    """Linux/POSIX inode allocation; never substitute logical length."""
    blocks = getattr(info, "st_blocks", None)
    return blocks * 512 if type(blocks) is int and blocks >= 0 else None


def inode_key(info):
    dev, ino = getattr(info, "st_dev", None), getattr(info, "st_ino", None)
    if type(dev) is int and type(ino) is int and dev >= 0 and ino > 0:
        return dev, ino
    return None


def _absolute(path):
    return os.path.abspath(os.fspath(path))


def _within(path, root):
    try:
        return os.path.commonpath((path, root)) == root and path != root
    except ValueError:
        return False


def _issue(report, kind, path=""):
    report["uncertainty_counts"][kind] = report["uncertainty_counts"].get(kind, 0) + 1
    if len(report["uncertainty_samples"]) < 32:
        report["uncertainty_samples"].append({"kind": kind, "path": str(path)[:512]})


def _open_regular(path):
    """Nonblocking/no-follow open: a raced FIFO cannot stall the collector."""
    flags = os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0)
    if Path(path).is_symlink():
        raise OSError("symlinked input refused")
    fd = os.open(path, flags)
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise OSError("nonregular input refused")
        return os.fdopen(fd, "rb")
    except BaseException:
        os.close(fd)
        raise


def read_identity(path, budget):
    budget.check()
    with _open_regular(path) as stream:
        data = stream.read(65537)
    budget.check()
    if len(data) > 65536:
        raise ValueError("identity input exceeds 64KiB")
    value = json.loads(data)
    if not isinstance(value, dict):
        raise ValueError("identity must be an object")
    return value


def _artifacts(path, identity, root, budget, report):
    classes = {}
    if path is None:
        _issue(report, "artifact_stream_absent")
        return classes
    digest = hashlib.sha256()
    consumed = unit_bytes = 0
    bound = finished = False
    malformed = False
    try:
        with _open_regular(path) as stream:
            while True:
                budget.check()
                line = stream.readline(MAX_LINE_BYTES + 1)
                if not line:
                    break
                consumed += len(line)
                digest.update(line)
                if len(line) > MAX_LINE_BYTES or consumed > MAX_INPUT_BYTES:
                    _issue(report, "artifact_input_limit", path)
                    malformed = True
                    break
                if not line.lstrip().startswith(b"{"):
                    continue  # Retained test/program output is not Cargo JSON.
                try:
                    item = json.loads(line)
                except (ValueError, UnicodeError):
                    _issue(report, "artifact_json_invalid", path)
                    malformed = True
                    continue
                if not isinstance(item, dict):
                    continue
                reason = item.get("reason")
                if reason == "pr-smoke-artifact-binding":
                    if bound or item.get("identity") != identity:
                        _issue(report, "artifact_identity_mismatch", path)
                        malformed = True
                    else:
                        bound = True
                elif reason == "build-finished":
                    if finished or type(item.get("success")) is not bool:
                        _issue(report, "artifact_terminal_invalid", path)
                        malformed = True
                    finished = True
                    report["cargo_build_success"] = item.get("success")
                elif reason == "compiler-artifact":
                    keys = ("package_id", "target", "features", "profile", "filenames", "executable", "fresh")
                    if finished or not all(k in item for k in keys):
                        _issue(report, "artifact_record_invalid", path)
                        malformed = True
                        continue
                    if (not isinstance(item["target"], dict)
                            or not isinstance(item["profile"], dict)
                            or type(item["profile"].get("test")) is not bool
                            or not isinstance(item["filenames"], list)
                            or not all(isinstance(p, str) for p in item["filenames"])
                            or not isinstance(item["features"], list)
                            or not all(isinstance(f, str) for f in item["features"])
                            or not isinstance(item["package_id"], str)
                            or type(item["fresh"]) is not bool
                            or not (item["executable"] is None or isinstance(item["executable"], str))):
                        _issue(report, "artifact_record_invalid", path)
                        malformed = True
                        continue
                    retained = {k: item[k] for k in keys}
                    unit_bytes += len(json.dumps(retained, ensure_ascii=True).encode())
                    if unit_bytes > MAX_UNIT_BYTES:
                        _issue(report, "artifact_identity_output_limit", path)
                        malformed = True
                        break
                    report["compiler_artifacts"].append(retained)
                    executable = _absolute(item["executable"]) if item["executable"] else None
                    for filename in set(item["filenames"] + ([item["executable"]] if item["executable"] else [])):
                        absolute = _absolute(filename)
                        if not _within(absolute, root):
                            _issue(report, "artifact_path_outside_target", filename)
                            malformed = True
                            continue
                        category = ("test_executable" if item["profile"]["test"] else "executable") if absolute == executable else "dependency_output"
                        classes.setdefault(absolute, set()).add(category)
        report["artifact_input_sha256"] = digest.hexdigest()
        report["artifact_input_bytes_read"] = consumed
        report["artifact_digest_complete"] = not malformed
    except (OSError, ValueError, BudgetExpired) as error:
        _issue(report, type(error).__name__ + "_artifact_input", path)
        malformed = True
    if not bound:
        _issue(report, "artifact_binding_absent", path)
    if not finished:
        _issue(report, "artifact_terminal_absent", path)
    report["artifact_attribution_complete"] = bound and finished and not malformed
    # Unbound or incomplete capture cannot qualify dependency/executable attribution.
    return classes if report["artifact_attribution_complete"] else {}


def collect(target, identity, *, artifacts=None, budget=None, max_entries=MAX_ENTRIES):
    """Collect bounded endpoint observations. Never infer peak/capacity/savings."""
    budget = budget or Budget()
    if not 1 <= max_entries <= MAX_ENTRIES:
        raise ValueError("entry bound must be in [1, 50000]")
    root = _absolute(target)
    report = {
        "identity": identity,
        "identity_authority": "caller-declared; no independent executed-subject admission",
        "target_dir": root,
        "observation": "endpoint only; physical extents, peak and savings NOT_PROVEN",
        "accounting_scope": "regular-file inodes only; directory/symlink/nonregular allocation excluded",
        "budget_seconds": budget.seconds,
        "prior_charged_seconds": budget.spent,
        "status": "partial",
        "scan_complete": False,
        "artifact_attribution_complete": False,
        "artifact_digest_complete": False,
        "compiler_artifacts": [],
        "uncertainty_counts": {},
        "uncertainty_samples": [],
        "regular_paths": 0,
        "symlinks_skipped": 0,
        "nonregular_skipped": 0,
        "entries_visited": 0,
    }
    if not isinstance(identity, dict):
        _issue(report, "identity_absent")
    classes = _artifacts(artifacts, identity, root, budget, report)
    inodes = {}
    stack = []
    try:
        budget.check()
        # Refuse a symlinked target or ancestor rather than traverse a new root.
        for ancestor in reversed([Path(root), *Path(root).parents]):
            budget.check()
            if stat.S_ISLNK(os.lstat(ancestor).st_mode):
                raise OSError("symlinked target ancestry refused")
        if not stat.S_ISDIR(os.lstat(root).st_mode):
            raise OSError("target is not a directory")
        stack.append(os.scandir(root))
        while stack:
            budget.check()
            try:
                entry = next(stack[-1])
            except StopIteration:
                stack.pop().close()
                continue
            if report["entries_visited"] >= max_entries:
                _issue(report, "entry_limit")
                break
            report["entries_visited"] += 1
            try:
                info = os.lstat(entry.path)
                if stat.S_ISLNK(info.st_mode):
                    report["symlinks_skipped"] += 1
                    continue
                if stat.S_ISDIR(info.st_mode):
                    if len(stack) >= 128:
                        _issue(report, "depth_limit", entry.path)
                    else:
                        stack.append(os.scandir(entry.path))
                    continue
                if not stat.S_ISREG(info.st_mode):
                    report["nonregular_skipped"] += 1
                    continue
                report["regular_paths"] += 1
                key = inode_key(info)
                if key is None:
                    _issue(report, "inode_identity_unsupported", entry.path)
                    continue
                allocation = allocated_bytes(info)
                if allocation is None:
                    _issue(report, "allocated_bytes_unsupported", entry.path)
                category = classes.get(_absolute(entry.path), {"other"})
                signature = (info.st_size, allocation, getattr(info, "st_mtime_ns", None))
                if key in inodes:
                    previous = inodes[key]
                    previous["categories"].update(category)
                    if previous["signature"] != signature:
                        previous["racy"] = True
                        _issue(report, "inode_changed", entry.path)
                else:
                    inodes[key] = {"signature": signature, "categories": set(category), "racy": False}
            except OSError as error:
                _issue(report, type(error).__name__ + "_entry", entry.path)
        report["scan_complete"] = not stack
    except (OSError, BudgetExpired) as error:
        _issue(report, type(error).__name__ + "_scan", root)
    finally:
        for iterator in stack:
            iterator.close()
    totals = {c: {"inodes": 0, "known_logical_bytes": 0, "known_inode_allocated_bytes": 0} for c in CATEGORIES}
    for observation in inodes.values():
        categories = observation["categories"]
        # A known executable alias supplies evidence for otherwise-unclassified aliases.
        categories.discard("other") if len(categories) > 1 else None
        category = next(iter(categories)) if len(categories) == 1 else "mixed"
        size, allocation, _ = observation["signature"]
        row = totals[category]
        row["inodes"] += 1
        if observation["racy"]:
            continue
        row["known_logical_bytes"] += size
        if allocation is not None:
            row["known_inode_allocated_bytes"] += allocation
    report["categories"] = totals
    report["unique_regular_inodes"] = len(inodes)
    report["observed_known_logical_bytes"] = sum(r["known_logical_bytes"] for r in totals.values())
    report["observed_known_inode_allocated_bytes"] = sum(r["known_inode_allocated_bytes"] for r in totals.values())
    complete = report["scan_complete"] and not any(
        k != "artifact_stream_absent" and k.startswith(("allocated_", "inode_", "Budget", "entry_", "depth_", "File", "Permission", "OSError"))
        for k in report["uncertainty_counts"]
    )
    report["complete_inode_allocated_bytes"] = report["observed_known_inode_allocated_bytes"] if complete else None
    report["collection_elapsed_before_output_seconds"] = budget.elapsed()
    report["charged_collection_seconds"] = budget.charged()
    report["status"] = "complete" if not report["uncertainty_counts"] else "partial"
    return report


def encode_report(report, limit=MAX_OUTPUT_BYTES):
    if not 128 <= limit <= MAX_OUTPUT_BYTES:
        raise ValueError("output limit must be in [128, 1048576]")
    data = (json.dumps(report, sort_keys=True, ensure_ascii=True) + "\n").encode()
    if len(data) <= limit:
        return data
    # Never truncate JSON or pretend omitted identities were retained.
    return (json.dumps({"status": "partial", "uncertainty": "output_limit",
                        "build_exit_code": report.get("build_exit_code"),
                        "complete_inode_allocated_bytes": None}) + "\n").encode()


@contextlib.contextmanager
def _deadline(seconds):
    """POSIX hard deadline for this telemetry process only."""
    if not hasattr(signal, "setitimer") or not hasattr(signal, "SIGALRM"):
        raise OSError("hard collection deadline unsupported")
    if seconds <= 0:
        raise BudgetExpired("shared collection budget exhausted")
    old_handler = signal.getsignal(signal.SIGALRM)
    old_timer = signal.getitimer(signal.ITIMER_REAL)
    if old_timer[0] or old_timer[1]:
        raise OSError("existing alarm refused; cannot replace caller deadline")
    def expired(signum, frame):
        raise BudgetExpired("hard collection deadline")
    signal.signal(signal.SIGALRM, expired)
    try:
        signal.setitimer(signal.ITIMER_REAL, seconds)
        yield
    finally:
        try:
            signal.setitimer(signal.ITIMER_REAL, 0)
        finally:
            signal.signal(signal.SIGALRM, old_handler)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target-dir", required=True)
    parser.add_argument("--identity-json", required=True)
    parser.add_argument("--artifact-jsonl")
    parser.add_argument("--output", required=True)
    parser.add_argument("--build-exit-code", type=int, default=0)
    parser.add_argument("--budget-seconds", type=float, default=30.0)
    parser.add_argument("--spent-seconds", type=float, default=0.0)
    args = parser.parse_args(argv)
    if not 0 <= args.build_exit_code <= 255:
        parser.error("build exit code must be in [0, 255]")
    try:
        budget = Budget(args.budget_seconds, args.spent_seconds)
        with _deadline(budget.remaining()):
            identity = read_identity(args.identity_json, budget)
            report = collect(args.target_dir, identity, artifacts=args.artifact_jsonl, budget=budget)
            report["build_exit_code"] = args.build_exit_code
            data = encode_report(report)
            # Output is exclusive/current-run: do not overwrite a prior report
            # or follow a symlink. No parent creation or artifact cleanup.
            budget.check()
            with _deadline_write():
                fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                             getattr(os, "O_NOFOLLOW", 0), 0o600)
                with os.fdopen(fd, "wb") as stream:
                    stream.write(data)
    except Exception as error:
        print(f"NOT_PROVEN: artifact inventory unavailable: {type(error).__name__}: {error}",
              file=sys.stderr)
    # Telemetry success/failure cannot convert the preceding build result.
    return args.build_exit_code


@contextlib.contextmanager
def _deadline_write():
    # Reduce (never expand) the active deadline to the charged output reserve.
    before = signal.getitimer(signal.ITIMER_REAL)[0]
    signal.setitimer(signal.ITIMER_REAL, min(before, OUTPUT_RESERVE))
    try:
        yield
    finally:
        # The enclosing deadline is about to finish; do not regrant time.
        pass


if __name__ == "__main__":
    sys.exit(main())
