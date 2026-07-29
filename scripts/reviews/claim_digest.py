#!/usr/bin/env python3
"""Stable digest for the material PR claim/review subject."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Final

MATERIAL_SECTIONS: Final[tuple[str, ...]] = (
    "Claim",
    "What this establishes",
    "What this does not establish",
    "Risk and rollback",
    "Review index",
)

# The first alias is the current-generation canonical heading. Later aliases
# preserve meaningful review identity for PRs created from the existing
# repository template while the template migration lands separately.
SECTION_ALIASES: Final[dict[str, tuple[str, ...]]] = {
    "Claim": ("Claim", "Claim Boundary"),
    "What this establishes": ("What this establishes", "Behavior", "Changes"),
    "What this does not establish": (
        "What this does not establish",
        "Non-goals",
        "Remaining Work",
    ),
    "Risk and rollback": ("Risk and rollback", "Risk", "Rollback", "Risks"),
    "Review index": ("Review index",),
}

_HEADING = re.compile(r"^##\s+(.+?)\s*$")


def _normalize_text(text: str) -> str:
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    normalized = [line.rstrip() for line in lines]
    while normalized and normalized[-1] == "":
        normalized.pop()
    return "\n".join(normalized).strip()


def canonical_material_claim(body: str) -> tuple[str, str]:
    """Return canonical material text and extraction mode."""

    normalized = body.replace("\r\n", "\n").replace("\r", "\n")
    sections: dict[str, list[str]] = {}
    current: str | None = None

    for line in normalized.split("\n"):
        match = _HEADING.match(line)
        if match:
            current = match.group(1).strip().casefold()
            sections.setdefault(current, [])
            continue
        if current is not None:
            sections[current].append(line)

    recognized_keys = {
        alias.casefold()
        for aliases in SECTION_ALIASES.values()
        for alias in aliases
    }
    if not recognized_keys.intersection(sections):
        return _normalize_text(body), "full_body_fallback"

    parts: list[str] = []
    for canonical_name in MATERIAL_SECTIONS:
        values: list[str] = []
        for alias in SECTION_ALIASES[canonical_name]:
            key = alias.casefold()
            if key not in sections:
                continue
            value = _normalize_text("\n".join(sections[key]))
            values.append(f"### {alias}\n{value}")

        material = "\n\n".join(values) if values else "<missing>"
        parts.append(f"## {canonical_name}\n{material}")

    return "\n\n".join(parts), "material_sections"


def claim_digest(body: str) -> dict[str, object]:
    canonical, mode = canonical_material_claim(body)
    digest = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    return {
        "algorithm": "sha256",
        "digest": digest,
        "mode": mode,
        "sections": list(MATERIAL_SECTIONS),
        "canonical_bytes": len(canonical.encode("utf-8")),
    }


def _read_live_pr_body(pr: str, repo: str | None) -> str:
    command = [
        "gh",
        "pr",
        "view",
        pr,
        "--json",
        "body",
        "--jq",
        '.body // ""',
    ]
    if repo:
        command.extend(["--repo", repo])
    completed = subprocess.run(command, check=False, capture_output=True, text=True)
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or "gh pr view failed")
    return completed.stdout


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--pr", help="GitHub pull-request number or URL")
    source.add_argument("--body-file", type=Path, help="Read PR body from a fixture/file")
    source.add_argument("--stdin", action="store_true", help="Read PR body from stdin")
    parser.add_argument("--repo", help="owner/repo for --pr")
    parser.add_argument("--json", action="store_true", help="Emit the full digest record")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.pr:
            body = _read_live_pr_body(args.pr, args.repo)
        elif args.body_file:
            body = args.body_file.read_text(encoding="utf-8")
        else:
            body = sys.stdin.read()
    except (OSError, RuntimeError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2

    record = claim_digest(body)
    if args.json:
        print(json.dumps(record, sort_keys=True))
    else:
        print(record["digest"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
