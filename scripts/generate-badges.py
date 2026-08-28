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
EXPECTED_RIPR_BADGE_IDENTITY = {
    "schema_version": "0.6",
    "kind": "ripr",
    "scope": "repo",
    "basis": "canonical_actionable_gap",
}


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
    for name, expected in EXPECTED_RIPR_BADGE_IDENTITY.items():
        actual = payload.get(name)
        if actual != expected:
            raise ValueError(f"ripr {name} must be {expected!r}, got {actual!r}")
    preview_skipped = payload.get("preview_skipped")
    if not isinstance(preview_skipped, list):
        raise ValueError("ripr preview_skipped must be an array")
    if preview_skipped:
        raise ValueError("ripr repo badge cannot be clean when preview languages were skipped")
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
