#!/usr/bin/env python3
"""Reject caller-supplied receipt paths that would widen the uploaded artifact.

`actions/upload-artifact` parses its `path:` input as a newline-separated path
list. Each of this action's path inputs is interpolated as one entry in that
list, so a CR or LF inside a single value silently appends *additional* entries.
A value such as ``target/receipt.json\\n/home/runner/.config`` would therefore
widen the artifact to unrelated runner files, in jobs that may hold secrets.

Moving the values into `env:` stops them being read as shell source, but the
path-list DSL is a separate interpreter. Treat these inputs as data and reject
anything that is not a single printable line before it reaches that DSL.

Note: paths are deliberately *not* required to live under `GITHUB_WORKSPACE`.
The in-repo caller (`.github/workflows/droid-review.yml`) legitimately uploads
from `${{ runner.temp }}`, which is outside the workspace.
"""

from __future__ import annotations

import os
import sys
from typing import Iterable, Sequence

# (action input name, environment variable carrying its value)
PATH_INPUTS: tuple[tuple[str, str], ...] = (
    ("receipt-path", "RECEIPT_PATH"),
    ("logs-path", "LOGS_PATH"),
    ("artifacts-path", "ARTIFACTS_PATH"),
)


def control_characters(value: str) -> list[str]:
    """Return sorted ``U+XXXX`` labels for every control character in `value`."""
    found = {
        f"U+{ord(char):04X}" for char in value if ord(char) < 0x20 or ord(char) == 0x7F
    }
    return sorted(found)


def validate(input_name: str, value: str) -> str | None:
    """Return an error message when `value` is unusable as a single path entry."""
    if not value.strip():
        return f"upload-receipt: '{input_name}' must not be empty"
    bad = control_characters(value)
    if bad:
        return (
            f"upload-receipt: '{input_name}' must be a single line without control "
            f"characters (found {', '.join(bad)})"
        )
    return None


def collect_errors(
    environment: dict[str, str], inputs: Iterable[tuple[str, str]] = PATH_INPUTS
) -> list[str]:
    """Return every validation error for the given environment mapping."""
    errors = []
    for input_name, env_name in inputs:
        error = validate(input_name, environment.get(env_name, ""))
        if error is not None:
            errors.append(error)
    return errors


def main(argv: Sequence[str] | None = None) -> int:
    errors = collect_errors(dict(os.environ))
    for error in errors:
        print(error, file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
