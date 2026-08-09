#!/usr/bin/env python3
"""Run one repository Cargo check from a composite action without a shell."""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Sequence

_ARGUMENT_ENV = {
    "clippy": "CLIPPY_ARGS",
    "clippy-prod": "CLIPPY_PROD_ARGS",
    "test": "TEST_ARGS",
}


def parse_argument_string(raw: str) -> list[str]:
    """Parse the action's documented POSIX shell-like argument string."""
    if "\x00" in raw:
        raise ValueError("argument string contains a NUL byte")
    try:
        return shlex.split(raw, posix=True)
    except ValueError as error:
        raise ValueError(f"invalid argument string: {error}") from error


def build_command(kind: str, environment: dict[str, str] | None = None) -> list[str]:
    """Build the exact argv for one supported check kind."""
    env = os.environ if environment is None else environment
    if kind == "fmt":
        return ["cargo", "xtask", "fmt", "--check"]
    argument_env = _ARGUMENT_ENV.get(kind)
    if argument_env is None:
        raise ValueError(f"unsupported check kind: {kind}")
    subcommand = "test" if kind == "test" else "clippy"
    return ["cargo", subcommand, *parse_argument_string(env.get(argument_env, ""))]


def run_command(
    kind: str,
    *,
    environment: dict[str, str] | None = None,
    executable: str = "cargo",
) -> int:
    """Execute the check as argv and return a normalized process status."""
    child_environment = os.environ.copy()
    if environment is not None:
        child_environment.update(environment)
    command = build_command(kind, child_environment)
    command[0] = executable
    try:
        return subprocess.run(command, check=False, env=child_environment).returncode
    except FileNotFoundError:
        print(f"unable to execute {executable!r}: executable not found", file=sys.stderr)
        return 127
    except OSError as error:
        print(f"unable to execute {executable!r}: {error}", file=sys.stderr)
        return 126


def write_status(output_path: str | None, status: int) -> None:
    """Publish the status even when the command failed or input was malformed."""
    if not output_path:
        return
    with Path(output_path).open("a", encoding="utf-8", newline="\n") as output:
        output.write(f"status={status}\n")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--kind", choices=("fmt", "clippy", "clippy-prod", "test"), required=True
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        status = run_command(args.kind)
    except ValueError as error:
        print(error, file=sys.stderr)
        status = 2
    write_status(os.environ.get("GITHUB_OUTPUT"), status)
    return status


if __name__ == "__main__":
    raise SystemExit(main())
