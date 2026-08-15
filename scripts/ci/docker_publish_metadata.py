#!/usr/bin/env python3
"""Derive Docker publish image-tag metadata from the dispatch version input.

The dispatch version is untrusted. This module is the single place that decides
whether a version is publishable and which image tags it may claim, so the
decision can be tested directly instead of being asserted about inline YAML.

Two boundaries are enforced here:

1. The version never becomes interpreter source. It is read from the
   environment as data and matched against an anchored SemVer grammar. Nothing
   is passed to a shell.

2. The version never becomes GitHub *workflow-command* syntax. A value
   containing a newline followed by ``::add-mask::`` (or any other command)
   would otherwise be parsed by the runner as a second command when echoed into
   a ``::error::`` diagnostic. Rejected values are reported through
   ``repr()`` on an ordinary log line, and any value that does reach workflow
   command data is percent-encoded per GitHub's escaping rules.

Stable-channel protection is also decided here: a prerelease version publishes
only its own exact tag. It never claims ``latest``, ``<major>`` or
``<major>.<minor>``, which would otherwise silently replace the stable channel.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

# Anchored SemVer-with-optional-prerelease. No build metadata: a '+' has never
# been valid in a Docker tag, so accepting it here would only defer the failure
# to the registry. Numeric cores reject leading zeros.
VERSION_PATTERN = re.compile(
    r"^(?P<major>0|[1-9][0-9]*)"
    r"\.(?P<minor>0|[1-9][0-9]*)"
    r"\.(?P<patch>0|[1-9][0-9]*)"
    r"(?:-(?P<prerelease>[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?\Z"
)
# \Z, not $. Python's '$' also matches immediately before a trailing newline, so
# '$' would accept "1.2.3\n" — which is exactly the trailing-newline shape a
# workflow-command injection payload starts with. \Z anchors at end of string.


class InvalidVersion(ValueError):
    """Raised when the dispatch version is not publishable."""


def escape_command_data(value: str) -> str:
    """Percent-encode a value for use inside GitHub workflow-command data.

    Implements the runner's documented escaping. ``%`` must be encoded first so
    the encodings introduced for CR/LF are not themselves re-encoded.
    """
    return value.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def derive(version: str) -> dict[str, str]:
    """Validate ``version`` and derive its image-tag metadata.

    Raises ``InvalidVersion`` for anything outside the release grammar. This is
    deliberately the only exit from this function that a caller may treat as a
    non-crash failure, so a malformed value cannot fall through to tag
    derivation.
    """
    if not isinstance(version, str):
        raise InvalidVersion("version must be a string")

    # Matched explicitly rather than relying on the pattern, so the failure
    # message can distinguish "contains a NUL" from "wrong shape". A NUL would
    # also truncate the value for most downstream consumers.
    if "\x00" in version:
        raise InvalidVersion("version contains a NUL byte")

    match = VERSION_PATTERN.match(version)
    if match is None:
        raise InvalidVersion("version does not match the release grammar")

    major = match.group("major")
    minor = match.group("minor")
    prerelease = match.group("prerelease")
    is_stable = prerelease is None

    return {
        "version": version,
        "major": major,
        "major_minor": f"{major}.{minor}",
        # Consumed as a workflow expression to gate the alias tags. A
        # prerelease reports 'false' and therefore claims no stable channel.
        "is_stable": "true" if is_stable else "false",
    }


def write_outputs(outputs: dict[str, str], output_path: str | None) -> None:
    """Append ``outputs`` to ``GITHUB_OUTPUT``.

    Every value written here has already passed ``derive``, so it is drawn from
    a matched SemVer grammar and cannot contain a newline. That is what stops a
    crafted version from injecting an additional ``key=value`` output line.
    """
    if not output_path:
        return

    lines = []
    for key, value in outputs.items():
        if "\n" in value or "\r" in value:  # unreachable after derive(); belt and braces
            raise InvalidVersion(f"refusing to write multi-line output for {key}")
        lines.append(f"{key}={value}\n")

    with Path(output_path).open("a", encoding="utf-8") as handle:
        handle.write("".join(lines))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--env-var",
        default="VERSION_INPUT",
        help="Environment variable holding the untrusted dispatch version.",
    )
    args = parser.parse_args(argv)

    raw = os.environ.get(args.env_var, "")

    try:
        outputs = derive(raw)
    except InvalidVersion as exc:
        # Note the two-part diagnostic. The ::error:: annotation carries only
        # our own fixed text, so a crafted version cannot open a second runner
        # command. The offending value is shown on an ordinary log line via
        # repr(), which renders newlines as escapes rather than line breaks.
        print(f"::error::Invalid Docker publish version: {escape_command_data(str(exc))}")
        print(f"rejected version input: {raw!r}", file=sys.stderr)
        return 1

    write_outputs(outputs, os.environ.get("GITHUB_OUTPUT"))

    if outputs["is_stable"] == "true":
        print(f"Publishing stable version {outputs['version']}")
        print(f"  alias tags: {outputs['major_minor']}, {outputs['major']}, latest")
    else:
        print(f"Publishing prerelease version {outputs['version']}")
        print("  alias tags: none (stable channels are not moved by a prerelease)")

    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
