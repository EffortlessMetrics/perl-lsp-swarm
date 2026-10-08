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
SECURE_TRAVERSAL_SUPPORTED = (
    hasattr(os, "O_NOFOLLOW") and hasattr(os, "O_DIRECTORY")
    and os.open in os.supports_dir_fd and os.stat in os.supports_dir_fd
    and os.stat in os.supports_follow_symlinks and os.scandir in os.supports_fd
)


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


def _identity_complete(value):
    """Require named declarations; retain incomplete/unsupported values as given."""
    if not isinstance(value, dict):
        return False
    def sha(name):
        text = value.get(name)
        return isinstance(text, str) and len(text) == 40 and all(c in "0123456789abcdef" for c in text)
    cache = value.get("cache")
    command = value.get("command")
    return (sha("source_sha") and sha("tree_sha") and type(value.get("git_dirty")) is bool
            and isinstance(value.get("run_id"), str) and bool(value["run_id"])
            and type(value.get("run_attempt")) is int and value["run_attempt"] >= 1
            and isinstance(cache, dict) and isinstance(cache.get("class"), str) and bool(cache["class"])
            and isinstance(cache.get("resolved_key"), str) and bool(cache["resolved_key"])
            and isinstance(value.get("profile"), dict) and bool(value["profile"])
            and isinstance(value.get("toolchain"), str) and bool(value["toolchain"])
            and isinstance(value.get("target_triple"), str) and bool(value["target_triple"])
            and isinstance(command, list) and bool(command)
            and all(isinstance(part, str) and bool(part) for part in command))


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
                    if not bound:
                        _issue(report, "artifact_binding_order", path)
                        malformed = True
                        continue
                    if finished or type(item.get("success")) is not bool:
                        _issue(report, "artifact_terminal_invalid", path)
                        malformed = True
                    finished = True
                    report["cargo_build_success"] = item.get("success")
                elif reason == "compiler-artifact":
                    if not bound:
                        _issue(report, "artifact_binding_order", path)
                        malformed = True
                        continue
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
                        if not os.path.isabs(filename) or not _within(absolute, root):
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
    report["artifact_attribution_complete"] = _identity_complete(identity) and bound and finished and not malformed
    # Unbound or incomplete capture cannot qualify dependency/executable attribution.
    return classes if report["artifact_attribution_complete"] else {}


def _entry_info(directory_fd, name, display_path):
    # display_path is only a report/classification identity, never a lookup.
    return os.stat(name, dir_fd=directory_fd, follow_symlinks=False)


def _open_target(root, budget):
    """Anchor every ancestry component without following a replaced symlink."""
    if not SECURE_TRAVERSAL_SUPPORTED:
        raise OSError("descriptor-relative no-follow traversal unsupported")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    parts = Path(root).parts
    fd = os.open(parts[0], flags)
    try:
        for name in parts[1:]:
            budget.check()
            next_fd = os.open(name, flags, dir_fd=fd)
            os.close(fd)
            fd = next_fd
        return fd
    except BaseException:
        os.close(fd)
        raise


def _scan_frame(fd, display_path):
    try:
        return os.scandir(fd), fd, display_path
    except BaseException:
        os.close(fd)
        raise


def _close_frame(frame):
    try:
        frame[0].close()
    finally:
        os.close(frame[1])


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
        "consistency": "bounded walk; concurrent mutations outside observed checks NOT_PROVEN",
        "accounting_scope": "regular-file inodes only; directory/symlink/nonregular allocation excluded",
        "budget_seconds": budget.seconds,
        "prior_charged_seconds": budget.spent,
        "charge_scope": "internal collection only; caller must supervise and measure full invocations",
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
    if not _identity_complete(identity):
        _issue(report, "identity_incomplete")
    classes = _artifacts(artifacts, identity, root, budget, report)
    matched_artifact_paths = set()
    inodes = {}
    stack = []
    accounting_complete = True
    try:
        budget.check()
        stack.append(_scan_frame(_open_target(root, budget), root))
        while stack:
            budget.check()
            iterator, directory_fd, directory_path = stack[-1]
            try:
                entry = next(iterator)
            except StopIteration:
                _close_frame(stack.pop())
                continue
            if report["entries_visited"] >= max_entries:
                accounting_complete = False
                _issue(report, "entry_limit")
                break
            report["entries_visited"] += 1
            display_path = os.path.join(directory_path, entry.name)
            try:
                info = _entry_info(directory_fd, entry.name, display_path)
                if stat.S_ISLNK(info.st_mode):
                    report["symlinks_skipped"] += 1
                    continue
                if stat.S_ISDIR(info.st_mode):
                    if len(stack) >= 128:
                        accounting_complete = False
                        _issue(report, "depth_limit", display_path)
                    else:
                        fd = os.open(entry.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                                     dir_fd=directory_fd)
                        try:
                            opened = os.fstat(fd)
                            if (inode_key(info) is None or inode_key(opened) is None
                                    or inode_key(opened) != inode_key(info)):
                                accounting_complete = False
                                _issue(report, "directory_changed", display_path)
                                continue
                            frame_fd, fd = fd, None  # Frame owns/closes it even if scandir fails.
                            frame = _scan_frame(frame_fd, display_path)
                            stack.append(frame)
                        finally:
                            if fd is not None:
                                os.close(fd)
                    continue
                if not stat.S_ISREG(info.st_mode):
                    report["nonregular_skipped"] += 1
                    continue
                report["regular_paths"] += 1
                if display_path in classes:
                    matched_artifact_paths.add(display_path)
                key = inode_key(info)
                if key is None:
                    accounting_complete = False
                    _issue(report, "inode_identity_unsupported", display_path)
                    continue
                allocation = allocated_bytes(info)
                if allocation is None:
                    accounting_complete = False
                    _issue(report, "allocated_bytes_unsupported", display_path)
                category = classes.get(display_path, {"other"})
                signature = (info.st_size, allocation, getattr(info, "st_mtime_ns", None))
                if key in inodes:
                    previous = inodes[key]
                    previous["categories"].update(category)
                    if previous["signature"] != signature:
                        accounting_complete = False
                        previous["racy"] = True
                        _issue(report, "inode_changed", display_path)
                else:
                    inodes[key] = {"signature": signature, "categories": set(category), "racy": False}
            except OSError as error:
                accounting_complete = False
                _issue(report, type(error).__name__ + "_entry", display_path)
        report["scan_complete"] = not stack
    except (OSError, BudgetExpired) as error:
        accounting_complete = False
        _issue(report, type(error).__name__ + "_scan", root)
    finally:
        for frame in stack:
            _close_frame(frame)
    # A syntactically complete capture does not prove its declared paths were
    # observed as regular files. Keep known endpoint allocation separate.
    unobserved = set(classes) - matched_artifact_paths
    if unobserved:
        report["artifact_attribution_complete"] = False
        for path in unobserved:
            _issue(report, "artifact_path_unobserved", path)
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
    complete = report["scan_complete"] and accounting_complete
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
            # Output is exclusive/current-run: do not overwrite a prior report
            # or follow a symlink. No parent creation or artifact cleanup.
            with _deadline_write(budget):
                report["charged_collection_seconds"] = budget.charged()
                data = encode_report(report)
                fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                             getattr(os, "O_NOFOLLOW", 0), 0o600)
                with os.fdopen(fd, "wb") as stream:
                    stream.write(data)
    except Exception as error:
        try:
            print(f"NOT_PROVEN: artifact inventory unavailable: {type(error).__name__}: {error}",
                  file=sys.stderr)
        except (OSError, UnicodeError, ValueError):
            pass  # A failed telemetry error stream cannot replace the build result.
    # Telemetry success/failure cannot convert the preceding build result.
    return args.build_exit_code


@contextlib.contextmanager
def _deadline_write(budget):
    # Reduce (never expand) the active deadline to the charged output reserve.
    before = signal.getitimer(signal.ITIMER_REAL)[0]
    remaining = budget.remaining()
    if before <= 0 or remaining <= 0:
        raise BudgetExpired("shared collection budget exhausted before output")
    signal.setitimer(signal.ITIMER_REAL, min(before, remaining, OUTPUT_RESERVE))
    try:
        yield
    finally:
        # The enclosing deadline is about to finish; do not regrant time.
        pass


if __name__ == "__main__":
    sys.exit(main())
