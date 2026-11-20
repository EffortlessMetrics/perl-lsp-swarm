#!/usr/bin/env python3
"""Carry one CI run's xtask executable to its compatible shard jobs.

This is an executable handoff, not a Cargo cache or qualification receipt.
Cargo target trees and gate results stay private to each job. Nested Cargo
commands still select and build their own targets normally.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
import sys


def output(*args: str) -> str:
    return subprocess.check_output(args, text=True).strip()


def identity() -> dict[str, str | int]:
    sha = os.environ["GITHUB_SHA"]
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("invalid event SHA")
    if output("git", "rev-parse", "HEAD^{commit}") != sha:
        raise ValueError("checkout differs from the integration event SHA")
    workspace = Path(output("git", "rev-parse", "--show-toplevel")).resolve()
    if Path.cwd().resolve() != workspace:
        raise ValueError("xtask handoff must run at the checkout root")
    subprocess.run(["git", "diff", "--quiet", "HEAD", "--"], check=True)
    run_id = os.environ["GITHUB_RUN_ID"]
    attempt = os.environ["GITHUB_RUN_ATTEMPT"]
    if not all(re.fullmatch(r"[1-9][0-9]*", value) for value in (run_id, attempt)):
        raise ValueError("invalid workflow run or attempt")
    repository = os.environ["GITHUB_REPOSITORY"]
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid repository identity")
    return {
        "schema_version": 1,
        "repository": repository,
        "source_sha": sha,
        "tree_sha": output("git", "rev-parse", "HEAD^{tree}"),
        # xtask embeds CARGO_MANIFEST_DIR. Hosted jobs use the same default
        # checkout path; a relocated checkout cannot safely reuse this binary.
        "workspace": str(workspace),
        "run_id": run_id,
        "run_attempt": attempt,
        "rustc": output("rustc", "-vV"),
        "system": platform.system(),
        "machine": platform.machine(),
    }


def regular(path: Path) -> bytes:
    # Refuse links in both the file and its ancestors. The download must be
    # an ordinary two-file artifact, never a path into another build/cache.
    for part in (path, *path.parents):
        if part.is_symlink():
            raise ValueError(f"linked artifact path: {path}")
    if not stat.S_ISREG(path.stat().st_mode):
        raise ValueError(f"artifact member is not a regular file: {path}")
    return path.read_bytes()


def pack(artifact_dir: Path) -> None:
    binding = identity()
    binary = regular(Path("target/debug/xtask"))
    if not binary:
        raise ValueError("xtask executable is empty")
    binding["binary_sha256"] = hashlib.sha256(binary).hexdigest()
    binding["binary_bytes"] = len(binary)
    artifact_dir.mkdir(parents=True, exist_ok=False)
    (artifact_dir / "xtask").write_bytes(binary)
    (artifact_dir / "manifest.json").write_text(
        json.dumps(binding, sort_keys=True) + "\n", encoding="utf-8"
    )


def install(artifact_dir: Path, destination: Path) -> Path:
    current = identity()
    if "\n" in str(destination) or "\r" in str(destination):
        raise ValueError("invalid executable destination")
    if {path.name for path in artifact_dir.iterdir()} != {"xtask", "manifest.json"}:
        raise ValueError("xtask artifact must contain exactly the executable and manifest")
    manifest = json.loads(regular(artifact_dir / "manifest.json"))
    if not isinstance(manifest, dict) or set(manifest) != {
        *current, "binary_sha256", "binary_bytes"
    }:
        raise ValueError("invalid xtask artifact manifest")
    for name, expected in current.items():
        if name == "run_attempt":
            # Re-running failed shards keeps the successful producer job's
            # exact artifact ID. Its executable is reusable build material;
            # gate verdicts still belong to the consumer's current attempt.
            producer_attempt = manifest[name]
            if (
                not isinstance(producer_attempt, str)
                or not re.fullmatch(r"[1-9][0-9]*", producer_attempt)
                or int(producer_attempt) > int(expected)
            ):
                raise ValueError("invalid or future xtask artifact run_attempt")
            continue
        if type(manifest[name]) is not type(expected) or manifest[name] != expected:
            raise ValueError(f"xtask artifact identity mismatch: {name}")
    binary = regular(artifact_dir / "xtask")
    if (
        not binary
        or type(manifest["binary_bytes"]) is not int
        or manifest["binary_bytes"] != len(binary)
        or manifest["binary_sha256"] != hashlib.sha256(binary).hexdigest()
    ):
        raise ValueError("xtask artifact executable digest or size mismatch")
    # Copy out of both the downloaded artifact and Cargo's target tree. A
    # nested Cargo invocation must never replace the active shard runner.
    for part in (destination, *destination.parents):
        if part.is_symlink():
            raise ValueError("linked executable destination")
    destination.mkdir(parents=True, exist_ok=False)
    executable = destination / "xtask"
    with executable.open("xb") as stream:
        stream.write(binary)
    executable.chmod(0o500)
    print(
        f"Shared xtask producer attempt {manifest['run_attempt']}; "
        f"consumer attempt {current['run_attempt']}"
    )
    return executable.resolve()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    for command in ("pack", "install"):
        subparser = subcommands.add_parser(command)
        subparser.add_argument("--artifact-dir", type=Path, required=True)
        if command == "install":
            subparser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "pack":
            pack(args.artifact_dir)
        else:
            executable = install(args.artifact_dir, args.destination)
            with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as stream:
                stream.write(f"CI_XTASK={executable}\n")
            print(f"Verified same-run xtask: {executable}")
        return 0
    except (KeyError, OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"xtask artifact refused: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
