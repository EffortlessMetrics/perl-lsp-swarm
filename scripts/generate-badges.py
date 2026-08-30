#!/usr/bin/env python3
"""Generate the repository-scoped Shields endpoint consumed by README badges."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

RIPR_TIMEOUT_SECONDS = 15 * 60
TERMINATION_GRACE_SECONDS = 5
STDERR_DIAGNOSTIC_LIMIT = 2_048
EXPECTED_COUNT_FIELDS = (
    "unsuppressed_exposure_gaps",
    "unsuppressed_test_efficiency_findings",
)
EXPECTED_RIPR_VERSION = "0.9.0"


def bounded_stderr(stderr: str) -> str:
    normalized = stderr.strip()
    if len(normalized) <= STDERR_DIAGNOSTIC_LIMIT:
        return normalized
    return normalized[:STDERR_DIAGNOSTIC_LIMIT] + "... [truncated]"


def terminate_process_tree(process: subprocess.Popen[str]) -> None:
    """Terminate the RIPR process group on both supported runner families."""
    if os.name == "nt":
        result = subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=TERMINATION_GRACE_SECONDS,
            check=False,
        )
        if result.returncode and process.poll() is None:
            process.kill()
    else:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
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
    process.wait(timeout=TERMINATION_GRACE_SECONDS)


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
        text=True,
        **platform_options,
    )
    try:
        stdout, stderr = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired as error:
        terminate_process_tree(process)
        _, stderr = process.communicate()
        diagnostic = bounded_stderr(stderr)
        suffix = f"; stderr: {diagnostic}" if diagnostic else ""
        raise RuntimeError(
            f"ripr check timed out after {timeout_seconds:g}s and its process tree was terminated{suffix}"
        ) from error
    if process.returncode:
        diagnostic = bounded_stderr(stderr)
        suffix = f": {diagnostic}" if diagnostic else ""
        raise RuntimeError(f"ripr check failed for ripr+ badge (exit {process.returncode}){suffix}")
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
    if not str(receipt.get("source_format", "")).startswith("ripr check --format repo-badge-json"):
        raise ValueError("RIPR receipt is not based on repo-badge-json")
    if producer.get("schema_version") != 1 or producer.get("kind") != "ripr_badge_producer":
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
        print(f"badges: consuming exact RIPR receipt ({time.monotonic() - started:.1f}s)", flush=True)
        try:
            badge = badge_from_receipt(receipt_path, producer_path, source_sha)
        except (OSError, json.JSONDecodeError, ValueError) as error:
            raise RuntimeError(f"invalid exact RIPR receipt: {error}") from error
    else:
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
