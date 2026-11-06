#!/usr/bin/env python3
"""Validate and consume versioned publish_crates_topo JSON in CI workflows."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import sys
from pathlib import Path


def _publish_topo_module():
    scripts_dir = Path(__file__).resolve().parent.parent
    spec = importlib.util.spec_from_file_location(
        "publish_topo",
        scripts_dir / "publish-topo.py",
    )
    if spec is None or spec.loader is None:
        raise RuntimeError("failed to load scripts/publish-topo.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _fail(message: str) -> None:
    print(f"::error::{message}", file=sys.stderr)
    sys.exit(1)


def _load_crates_from_stdin(module) -> list[dict]:
    try:
        payload = json.load(sys.stdin)
    except json.JSONDecodeError as exc:
        _fail(f"CRATES_JSON is not valid JSON: {exc}")
    try:
        return module.load_publish_crates_payload(payload)
    except ValueError as exc:
        _fail(str(exc))


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Validate publish_crates_topo JSON read from stdin."
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--count",
        action="store_true",
        help="Print the number of crates in the envelope.",
    )
    group.add_argument(
        "--list",
        action="store_true",
        help="Print one '<name> <version>' line per crate.",
    )
    group.add_argument(
        "--validate-target-version",
        action="store_true",
        help="Require every crate version to match TARGET_VERSION.",
    )
    args = parser.parse_args()

    module = _publish_topo_module()
    crates = _load_crates_from_stdin(module)

    if args.count:
        print(len(crates))
        return

    if args.list:
        for crate in crates:
            print("{} {}".format(crate["name"], crate["version"]))
        return

    target = os.environ.get("TARGET_VERSION")
    if not target:
        _fail("TARGET_VERSION is not set")
    mismatches = [
        "{}@{}".format(crate["name"], crate["version"])
        for crate in crates
        if crate["version"] != target
    ]
    if mismatches:
        _fail(
            "Crate versions do not match release target {}: {}".format(
                target,
                ", ".join(mismatches),
            )
        )
    print("All publishable crates match target version {}".format(target))


if __name__ == "__main__":
    main()
