#!/usr/bin/env python3
"""Capture explicit-date upstream cadence without changing native policy decisions."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import time

REPORT_NAME = "cargo-allow-cadence.json"
RECEIPT_NAME = "execution-receipt.json"
STDERR_NAME = "cargo-allow-cadence.stderr"
MAX_STDOUT_BYTES = 1024 * 1024
MAX_STDERR_BYTES = 64 * 1024
TIMEOUT_SECONDS = 30


class CaptureError(Exception):
    """A refused capture; existing output artifacts remain unchanged."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(64 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def absolute_path(value: str, label: str) -> Path:
    path = Path(value)
    if not path.is_absolute():
        raise CaptureError(f"{label} must be an absolute path")
    return path.resolve()


def explicit_date(value: str) -> str:
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", value):
        raise CaptureError("--as-of must be an explicit YYYY-MM-DD date")
    try:
        datetime.date.fromisoformat(value)
    except ValueError as error:
        raise CaptureError("--as-of is not a calendar date") from error
    return value


def kill_group(process: subprocess.Popen[bytes]) -> None:
    # The leader may have exited while a descendant still holds a pipe open.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def run_bounded(argv: list[str], root: Path) -> tuple[bytes, bytes]:
    """Drain both pipes concurrently, with byte/time bounds and group cleanup."""
    if os.name != "posix":
        raise CaptureError("capture requires a POSIX host (including WSL)")
    process = subprocess.Popen(
        argv, cwd=root, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, start_new_session=True,
    )
    buffers = {"stdout": bytearray(), "stderr": bytearray()}
    caps = {"stdout": MAX_STDOUT_BYTES, "stderr": MAX_STDERR_BYTES}
    deadline = time.monotonic() + TIMEOUT_SECONDS
    try:
        with selectors.DefaultSelector() as selector:
            for name, stream in (("stdout", process.stdout), ("stderr", process.stderr)):
                if stream is None:
                    raise CaptureError(f"missing child {name} pipe")
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, name)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise CaptureError("cargo-allow cadence exceeded the 30-second time bound")
                for key, _ in selector.select(min(remaining, 0.1)):
                    chunk = os.read(key.fd, 8192)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    name = key.data
                    if len(buffers[name]) + len(chunk) > caps[name]:
                        raise CaptureError(f"cargo-allow cadence exceeded the {name} byte bound")
                    buffers[name].extend(chunk)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise CaptureError("cargo-allow cadence exceeded the 30-second time bound")
            try:
                code = process.wait(timeout=remaining)
            except subprocess.TimeoutExpired as error:
                raise CaptureError("cargo-allow cadence exceeded the 30-second time bound") from error
        if code != 0:
            diagnostic = bytes(buffers["stderr"]).decode("utf-8", errors="replace").strip()
            raise CaptureError(f"cargo-allow cadence exited {code}: {diagnostic[:2000]}")
        return bytes(buffers["stdout"]), bytes(buffers["stderr"])
    finally:
        kill_group(process)
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired as error:
            raise CaptureError("child could not be reaped after termination") from error
        finally:
            for stream in (process.stdout, process.stderr):
                if stream is not None:
                    stream.close()


def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    value: dict[str, object] = {}
    for key, item in pairs:
        if key in value:
            raise CaptureError(f"duplicate upstream JSON key: {key}")
        value[key] = item
    return value


def validate_report(raw: bytes, root: Path, policy: Path, as_of: str) -> None:
    try:
        report = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise CaptureError("upstream stdout is not one complete UTF-8 JSON report") from error
    if not isinstance(report, dict):
        raise CaptureError("upstream cadence report must be a JSON object")
    required = {
        "schema_id": "cargo-allow.cadence.v1", "schema_version": 1,
        "tool": "cargo-allow", "command": "cadence", "as_of": as_of,
        "as_of_source": "explicit",
        "horizons": {"review_due_soon_days": 14, "expiring_soon_days": 14},
    }
    for key, expected in required.items():
        if report.get(key) != expected or (key == "schema_version" and type(report.get(key)) is not int):
            raise CaptureError(f"upstream cadence {key} does not match the requested contract")
    if report.get("policy_path") != str(policy):
        raise CaptureError("upstream cadence policy_path does not match the captured policy")
    inventory = report.get("inventory")
    if not isinstance(inventory, dict) or inventory.get("root") != str(root):
        raise CaptureError("upstream cadence inventory.root does not match --root")
    if (inventory.get("scope"), inventory.get("scanner"), inventory.get("source")) != (
        "source_tree", "source_syntax", "git_tracked",
    ):
        raise CaptureError("upstream cadence inventory must report git-tracked source syntax")
    for key, expected_type in (("rows", list), ("summary", dict), ("claim_boundary", list), ("scanner_limitations", list)):
        if not isinstance(report.get(key), expected_type):
            raise CaptureError(f"upstream cadence {key} has an invalid shape")
    if not all(isinstance(row, dict) for row in report["rows"]):
        raise CaptureError("upstream cadence rows must be objects")
    counts = {key: 0 for key in ("current", "review_due_soon", "review_overdue", "expiring", "expired", "invalid")}
    identifiers: set[str] = set()
    for row in report["rows"]:
        for key in ("allow_id", "owner", "classification", "selector_summary", "required_disposition"):
            if not isinstance(row.get(key), str) or not row[key]:
                raise CaptureError(f"upstream cadence row {key} must be a nonempty string")
        if row["allow_id"] in identifiers:
            raise CaptureError("upstream cadence allow_id must be unique")
        identifiers.add(row["allow_id"])
        if not isinstance(row.get("class"), str) or row["class"] not in counts:
            raise CaptureError("upstream cadence row class is invalid")
        counts[row["class"]] += 1
        for key in ("source_path", "source_glob", "review_after", "expires", "driving_date"):
            if key not in row or (row[key] is not None and not isinstance(row[key], str)):
                raise CaptureError(f"upstream cadence row {key} must be a string or null")
        if "days_remaining" not in row or (row["days_remaining"] is not None and type(row["days_remaining"]) is not int):
            raise CaptureError("upstream cadence row days_remaining must be an integer or null")
        if not isinstance(row.get("evidence_refs"), list) or not all(isinstance(item, str) for item in row["evidence_refs"]):
            raise CaptureError("upstream cadence row evidence_refs must be strings")
    expected_summary = {"total_entries": len(report["rows"]), **counts}
    for key, expected in expected_summary.items():
        if type(report["summary"].get(key)) is not int or report["summary"][key] != expected:
            raise CaptureError("upstream cadence summary does not match its rows")
    for key in ("claim_boundary", "scanner_limitations"):
        if not all(isinstance(item, str) for item in report[key]):
            raise CaptureError(f"upstream cadence {key} must contain strings")


def check_output(output: Path, inputs: tuple[Path, Path]) -> None:
    for name in (REPORT_NAME, RECEIPT_NAME, STDERR_NAME):
        artifact = output / name
        if artifact.is_symlink():
            raise CaptureError("output artifacts must not be symlinks")
        for source in inputs:
            if artifact == source or (artifact.exists() and os.path.samefile(artifact, source)):
                raise CaptureError("output artifacts must not alias the executable or policy")
    if output.exists():
        if not output.is_dir() or {item.name for item in output.iterdir()} != {REPORT_NAME, RECEIPT_NAME, STDERR_NAME}:
            raise CaptureError("--output-dir must be fresh or contain only an identical prior capture")
        for name, cap in ((REPORT_NAME, MAX_STDOUT_BYTES), (RECEIPT_NAME, 16 * 1024), (STDERR_NAME, MAX_STDERR_BYTES)):
            path = output / name
            if not path.is_file() or path.stat().st_size > cap:
                raise CaptureError("existing capture artifacts have an invalid type or size")
    if not output.parent.is_dir():
        raise CaptureError("--output-dir parent must already exist")


def publish(output: Path, raw: bytes, stderr: bytes, receipt: bytes, inputs: tuple[Path, Path]) -> str:
    check_output(output, inputs)
    if output.exists():
        if (output / REPORT_NAME).read_bytes() != raw or (output / STDERR_NAME).read_bytes() != stderr or (output / RECEIPT_NAME).read_bytes() != receipt:
            raise CaptureError("existing capture differs; choose a fresh --output-dir")
        return "unchanged"
    staging = Path(tempfile.mkdtemp(prefix=".cargo-allow-cadence-", dir=output.parent))
    try:
        (staging / REPORT_NAME).write_bytes(raw)
        (staging / STDERR_NAME).write_bytes(stderr)
        (staging / RECEIPT_NAME).write_bytes(receipt)
        # All artifacts become visible together. Existing captures are never rewritten.
        if output.exists():
            raise CaptureError("--output-dir appeared during capture; choose a fresh directory")
        staging.rename(output)
    finally:
        if staging.exists():
            shutil.rmtree(staging)
    return "captured"


def capture(args: argparse.Namespace) -> str:
    executable = absolute_path(args.executable, "--executable")
    root = absolute_path(args.root, "--root")
    output = absolute_path(args.output_dir, "--output-dir")
    as_of = explicit_date(args.as_of)
    if not re.fullmatch(r"[0-9a-fA-F]{64}", args.expected_sha256):
        raise CaptureError("--expected-sha256 must contain exactly 64 hexadecimal digits")
    if not executable.is_file() or not root.is_dir():
        raise CaptureError("--executable must be a file and --root must be a directory")
    policy = (root / "policy" / "allow.toml").resolve()
    if not policy.is_file():
        raise CaptureError("--root must contain policy/allow.toml")
    executable_before = sha256_file(executable)
    if executable_before != args.expected_sha256.lower():
        raise CaptureError("executable SHA256 differs from --expected-sha256; child was not started")
    policy_before = sha256_file(policy)
    check_output(output, (executable, policy))
    argv = [str(executable), "cadence", "--root", str(root), "--config", str(policy), "--as-of", as_of, "--format", "json"]
    raw, stderr = run_bounded(argv, root)
    executable_after = sha256_file(executable)
    policy_after = sha256_file(policy)
    if executable_before != executable_after or policy_before != policy_after:
        raise CaptureError("executable or policy changed during capture; no artifacts published")
    validate_report(raw, root, policy, as_of)
    receipt = {
        "schema_id": "perl-lsp.cargo-allow-cadence-execution.v1", "schema_version": 1,
        "advisory_only": True, "argv": argv, "root": str(root), "policy_path": str(policy),
        "as_of": as_of, "exit_code": 0,
        "executable_sha256": {"expected": args.expected_sha256.lower(), "before": executable_before, "after": executable_after},
        "policy_sha256": {"before": policy_before, "after": policy_after},
        "report_sha256": hashlib.sha256(raw).hexdigest(),
        "stderr_sha256": hashlib.sha256(stderr).hexdigest(), "stderr_bytes": len(stderr),
        "bounds": {"timeout_seconds": TIMEOUT_SECONDS, "stdout_bytes": MAX_STDOUT_BYTES, "stderr_bytes": MAX_STDERR_BYTES},
        "claim_boundary": ["upstream_stdout_preserved", "explicit_date", "observed_executable_and_policy_pre_post_hashes", "inventory_context_only", "no_whole_worktree_snapshot", "no_immutability_or_toctou_guarantee", "no_policy_promotion"],
    }
    receipt_bytes = (json.dumps(receipt, indent=2, sort_keys=True) + "\n").encode("utf-8")
    return publish(output, raw, stderr, receipt_bytes, (executable, policy))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", required=True, help="absolute installed cargo-allow executable")
    parser.add_argument("--expected-sha256", required=True, help="caller-qualified executable SHA256")
    parser.add_argument("--as-of", required=True, help="explicit YYYY-MM-DD date")
    parser.add_argument("--root", required=True, help="absolute consumer root containing policy/allow.toml")
    parser.add_argument("--output-dir", required=True, help="absolute fresh capture directory, or identical prior capture")
    args = parser.parse_args(argv)
    try:
        result = capture(args)
    except (CaptureError, OSError) as error:
        print(f"cargo-allow cadence capture refused: {error}", file=sys.stderr)
        return 1
    print(f"cargo-allow cadence {result}: {args.output_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
