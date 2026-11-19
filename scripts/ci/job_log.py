#!/usr/bin/env python3
"""Read a CI job log without the silent truncation ``gh run view --log`` applies.

Why this exists
---------------

``gh run view --log`` and ``gh run view --job <id> --log-failed`` return a
prefix of the job log. The cut is not announced, and it does not always keep
the failing test: a job whose ``conclusion`` is ``failure`` can come back with
no failure text in it at all, so a triage read of the truncated log concludes
"no failure found" (#19559). The same read can also name a *different* step as
the failing one than GitHub marked ``failure``.

The Actions API exposes the untruncated log at
``GET /repos/{owner}/{repo}/actions/jobs/{job_id}/logs``. That endpoint is not
paginated and returns the whole log: a zip archive when the job produced more
than one log file, and plain text otherwise. This reader uses it.

It also reports how much it actually read, so a short read can never be
mistaken for a clean read, and it can list the per-step conclusions GitHub
recorded, which is the surface that identifies *which* step failed.

Usage
-----

::

    python3 scripts/ci/job_log.py --job <job_id> [--repo owner/name] [--grep PATTERN]
    python3 scripts/ci/job_log.py --job <job_id> --steps
    python3 scripts/ci/job_log.py --job <job_id> --json
    python3 scripts/ci/job_log.py --from-file <path> [--grep PATTERN] [--json]

Exit codes
----------

``0``  the log was read (and, with ``--grep``, the pattern was found).
``1``  the log was read but the ``--grep`` pattern was not found.
``2``  usage error, or the log could not be retrieved.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import re
import subprocess
import sys
import zipfile
from typing import Any

DEFAULT_REPO = "EffortlessMetrics/perl-lsp-swarm"

# GitHub's job-log archive stores one member per log file, named by ordinal.
_MEMBER_ORDER = re.compile(r"^(\d+)")


def _repo_default() -> str:
    """Resolve ``owner/name`` from ``GITHUB_REPOSITORY`` or the git remote."""
    env = os.environ.get("GITHUB_REPOSITORY")
    if env and "/" in env:
        return env
    try:
        url = subprocess.run(
            ["git", "config", "--get", "remote.origin.url"],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        ).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        url = ""
    if url:
        # ssh://git@host/owner/repo.git | git@host:owner/repo.git | https://host/owner/repo.git
        match = re.search(r"[:/](?P<owner>[^/:]+)/(?P<repo>[^/:]+?)(?:\.git)?$", url)
        if match:
            return "%s/%s" % (match.group("owner"), match.group("repo"))
    return DEFAULT_REPO


def decode_log_bytes(raw: bytes) -> str:
    """Return the log text for a job-log payload.

    The endpoint answers with a zip archive when the job wrote more than one
    log file and with plain text otherwise, so both shapes are accepted.
    Members are concatenated in ordinal order so the emitted text keeps the
    order the job wrote it in.
    """
    if raw[:2] == b"PK":
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            members = [n for n in archive.namelist() if not n.endswith("/")]
            members.sort(key=lambda n: (int(_MEMBER_ORDER.match(n).group(1)) if _MEMBER_ORDER.match(n) else 1 << 30, n))
            parts = []
            for name in members:
                parts.append(archive.read(name).decode("utf-8", errors="replace"))
            return "".join(parts)
    return raw.decode("utf-8", errors="replace")


def _gh_api_bytes(path: str) -> bytes:
    """Fetch a raw API payload through ``gh`` (the authenticated path here)."""
    proc = subprocess.run(
        ["gh", "api", path],
        capture_output=True,
        timeout=120,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            "gh api %s failed (exit %d): %s"
            % (path, proc.returncode, proc.stderr.decode("utf-8", errors="replace").strip())
        )
    return proc.stdout


def fetch_job_log(repo: str, job_id: int) -> str:
    """Return the untruncated log text for ``job_id``."""
    path = "repos/%s/actions/jobs/%d/logs" % (repo, job_id)
    return decode_log_bytes(_gh_api_bytes(path))


def parse_job_steps(payload: dict[str, Any]) -> list[dict[str, Any]]:
    """Return the per-step conclusions carried by a job payload."""
    steps = payload.get("steps") or []
    return [
        {
            "number": step.get("number"),
            "name": step.get("name"),
            "status": step.get("status"),
            "conclusion": step.get("conclusion"),
        }
        for step in steps
    ]


def fetch_job_steps(repo: str, job_id: int) -> list[dict[str, Any]]:
    """Return the per-step conclusions GitHub recorded for ``job_id``."""
    path = "repos/%s/actions/jobs/%d" % (repo, job_id)
    payload = json.loads(_gh_api_bytes(path).decode("utf-8"))
    return parse_job_steps(payload)


def read_log_file(path: str) -> str:
    """Return the log text stored at ``path`` (zip archive or plain text)."""
    with open(path, "rb") as handle:
        return decode_log_bytes(handle.read())


def grep_lines(text: str, pattern: str) -> list[tuple[int, str]]:
    """Return ``(line_number, line)`` for every line matching ``pattern``."""
    needle = re.compile(pattern)
    hits: list[tuple[int, str]] = []
    for number, line in enumerate(text.splitlines(), start=1):
        if needle.search(line):
            hits.append((number, line))
    return hits


def build_receipt(source: str, text: str, matched: int | None = None) -> dict[str, Any]:
    """Summarise what was read, so a short read is never mistaken for a clean one."""
    lines = text.splitlines()
    receipt: dict[str, Any] = {
        "source": source,
        "lines_read": len(lines),
        "bytes_read": len(text.encode("utf-8", errors="replace")),
    }
    if matched is not None:
        receipt["pattern_matches"] = matched
    return receipt


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="job_log.py",
        description="Read a CI job log from the raw per-job endpoint (no silent truncation).",
    )
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--job", type=int, help="Actions job id to read.")
    source.add_argument("--from-file", help="Read a saved job-log payload (zip or plain text).")
    parser.add_argument("--repo", help="owner/name of the repository (default: the current remote).")
    parser.add_argument("--grep", dest="pattern", help="Only print lines matching this regex.")
    parser.add_argument("--steps", action="store_true", help="Print the per-step conclusions GitHub recorded.")
    parser.add_argument("--summary", action="store_true", help="Print only the read receipt.")
    parser.add_argument("--json", action="store_true", help="Emit the receipt as JSON.")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    repo = args.repo or _repo_default()

    # A job log can carry any byte sequence. Emit it as UTF-8 rather than
    # letting the platform's console encoding raise on the first non-ASCII
    # character, which would turn a readable log into a traceback.
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    try:
        if args.from_file:
            source = args.from_file
            text = read_log_file(args.from_file)
        else:
            source = "jobs/%d" % args.job
            text = fetch_job_log(repo, args.job)
    except (OSError, RuntimeError, zipfile.BadZipFile) as exc:
        print("error: %s" % exc, file=sys.stderr)
        return 2

    hits = grep_lines(text, args.pattern) if args.pattern else None
    receipt = build_receipt(source, text, len(hits) if hits is not None else None)

    if args.json:
        payload = dict(receipt)
        if hits is not None:
            payload["matches"] = [{"line": number, "text": line} for number, line in hits]
        print(json.dumps(payload, indent=2, sort_keys=True))
    elif args.summary:
        print(json.dumps(receipt, indent=2, sort_keys=True))
    else:
        if hits is not None:
            for number, line in hits:
                print("%d:%s" % (number, line))
        else:
            sys.stdout.write(text if text.endswith("\n") else text + "\n")
        print(
            "[job_log] read %d lines from %s" % (receipt["lines_read"], source),
            file=sys.stderr,
        )

    if args.steps:
        if args.from_file:
            print(
                "error: --steps needs --job (step conclusions come from the job API)",
                file=sys.stderr,
            )
            return 2
        try:
            steps = fetch_job_steps(repo, args.job)
        except (RuntimeError, ValueError) as exc:
            print("error: %s" % exc, file=sys.stderr)
            return 2
        for step in steps:
            print(
                "%s | %s | %s/%s"
                % (step.get("number"), step.get("name"), step.get("status"), step.get("conclusion"))
            )

    if args.pattern and hits is not None and not hits:
        # The log was read in full; the pattern is genuinely absent. This is a
        # different state from "the read was incomplete", and the receipt above
        # is what tells the two apart.
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
