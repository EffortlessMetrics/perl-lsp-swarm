#!/usr/bin/env python3
"""Generate the repository-scoped Shields endpoint consumed by README badges."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


def badge_from_ripr(payload: object) -> dict[str, object]:
    if not isinstance(payload, dict):
        raise ValueError("ripr emitted a non-object repo-badge-json payload")
    counts = payload.get("counts")
    if not isinstance(counts, dict):
        counts = {}

    def count(name: str) -> int:
        value = counts.get(name, 0)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise ValueError(f"ripr count {name!r} must be a non-negative integer")
        return value

    unresolved = count("unsuppressed_exposure_gaps") + count("unsuppressed_test_efficiency_findings")
    return {"schemaVersion": 1, "label": "ripr+", "message": str(unresolved), "color": "brightgreen" if unresolved == 0 else "yellow"}


def generate(root: Path, check: bool) -> None:
    started = time.monotonic()
    ripr = os.environ.get("RIPR_BIN", "ripr")
    print(f"badges: starting RIPR analysis ({time.monotonic() - started:.1f}s)", flush=True)
    result = subprocess.run([ripr, "check", "--root", str(root), "--format", "repo-badge-json"], cwd=root, capture_output=True, text=True)
    print(f"badges: RIPR analysis finished ({time.monotonic() - started:.1f}s)", flush=True)
    if result.returncode:
        raise RuntimeError(f"ripr check failed for ripr+ badge: {result.stderr.strip()}")
    try:
        badge = badge_from_ripr(json.loads(result.stdout))
    except (json.JSONDecodeError, ValueError) as error:
        raise RuntimeError(f"invalid repo-badge-json from {ripr}: {error}") from error

    target = root / "target" / "xtask" / "badges" / "ripr-plus.json"
    committed = root / "badges" / "ripr-plus.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(badge, indent=2) + "\n"
    target.write_text(encoded, encoding="utf-8")
    if check:
        if not committed.is_file() or committed.read_text(encoding="utf-8") != encoded:
            raise RuntimeError(f"badge endpoint drift detected for {committed}; run `cargo xtask badges`")
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
