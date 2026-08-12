#!/usr/bin/env python3
"""Run and guard the bounded parser-integration proof set for issue #6107."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
TARGETS_PATH = ROOT / ".ci/parser-integration-targets.json"
MIN_TARGETS = 7


def load_targets(path: Path = TARGETS_PATH) -> list[tuple[str, str]]:
    try:
        payload: Any = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read parser integration target manifest: {error}") from error
    if (
        not isinstance(payload, dict)
        or payload.get("schema_version") != 1
        or not isinstance(payload.get("targets"), list)
    ):
        raise ValueError("unsupported parser integration target manifest")

    result: list[tuple[str, str]] = []
    target_names: set[str] = set()
    for item in payload["targets"]:
        if (
            not isinstance(item, dict)
            or not isinstance(item.get("package"), str)
            or not item.get("package")
            or not isinstance(item.get("target"), str)
            or not item.get("target")
        ):
            raise ValueError("parser integration target must contain non-empty string package and target")
        entry = (item["package"], item["target"])
        if entry in result:
            raise ValueError(f"duplicate parser integration target: {entry[0]}:{entry[1]}")
        if entry[1] in target_names:
            raise ValueError(f"duplicate parser integration target name: {entry[1]}")
        result.append(entry)
        target_names.add(entry[1])
    if len(result) < MIN_TARGETS:
        raise ValueError(f"parser integration target manifest shrank below {MIN_TARGETS} targets")
    return result


def available_targets() -> set[tuple[str, str]]:
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            "cargo metadata failed while validating parser integration targets:\n"
            + completed.stderr
        )
    try:
        payload: Any = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"cargo metadata returned invalid JSON: {error}") from error

    result: set[tuple[str, str]] = set()
    for package in payload.get("packages", []):
        package_name = package.get("name")
        for target in package.get("targets", []):
            if package_name and "test" in target.get("kind", []):
                result.add((package_name, target.get("name", "")))
    return result


def cargo_command(targets: list[tuple[str, str]]) -> list[str]:
    command = ["cargo", "test", "--locked", "--no-fail-fast"]
    for package in sorted({package for package, _ in targets}):
        command.extend(["--package", package])
    for _, target in targets:
        command.extend(["--test", target])
    command.extend(["--", "--test-threads=4"])
    return command


def main() -> int:
    try:
        targets = load_targets()
        missing = sorted(set(targets) - available_targets())
        if missing:
            details = ", ".join(f"{package}:{target}" for package, target in missing)
            raise ValueError(f"parser integration target manifest is stale: {details}")
        command = cargo_command(targets)
        print("parser integration targets:", ", ".join(f"{package}:{target}" for package, target in targets))
        print("running:", " ".join(command))
        result = subprocess.run(command, cwd=ROOT, check=False)
        if result.returncode != 0:
            return result.returncode

        # Feature-gated parser tests are not exercised by the default target
        # command. Keep these explicit proofs in the same bounded parser gate.
        incremental_command = [
            "cargo",
            "test",
            "--locked",
            "--package",
            "perl-parser",
            "--features",
            "incremental",
            "--test",
            "incremental_parser_accuracy",
            "--test",
            "incremental_parse_output",
            "--test",
            "incremental_parse_snapshot",
            "--test",
            "incremental_recovery_transitions",
            "--",
            "--test-threads=4",
        ]
        print("running:", " ".join(incremental_command))
        return subprocess.run(incremental_command, cwd=ROOT, check=False).returncode
    except (OSError, RuntimeError, ValueError) as error:
        print(f"parser integration guard failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())