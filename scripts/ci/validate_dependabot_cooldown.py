#!/usr/bin/env python3
"""Validate the 14-day Dependabot admission scalar and canonical guidance."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from validate_dependabot_contract import (
    CONFIG_PATH,
    EXPECTED_ROWS,
    MANAGEMENT_GUIDE,
    QUICK_REFERENCE,
    ParseError,
    parse_yaml_subset,
)

EXPECTED_DEFAULT_DAYS = 14
GUIDES = (MANAGEMENT_GUIDE, QUICK_REFERENCE)


def _read(root: Path, rel: Path) -> str:
    try:
        return (root / rel).read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        raise RuntimeError(f"unreadable source: {rel.as_posix()}") from exc


def validate(root: Path) -> list[str]:
    findings: list[str] = []
    try:
        doc = parse_yaml_subset(_read(root, CONFIG_PATH))
    except (ParseError, RuntimeError) as exc:
        return [f"config-unreadable: {exc}"]

    updates = doc.get("updates")
    if not isinstance(updates, list):
        return ["config-shape: updates must be a sequence"]

    rows: dict[tuple[str, str], dict[str, object]] = {}
    for item in updates:
        if not isinstance(item, dict):
            continue
        ecosystem = item.get("package-ecosystem")
        directory = item.get("directory")
        if isinstance(ecosystem, str) and isinstance(directory, str):
            rows.setdefault((ecosystem, directory), item)

    for ecosystem, directory in EXPECTED_ROWS:
        row = rows.get((ecosystem, directory))
        label = f"{ecosystem} {directory}"
        if row is None:
            findings.append(f"cooldown-row-missing: {label}")
            continue
        cooldown = row.get("cooldown")
        if not isinstance(cooldown, dict):
            findings.append(f"cooldown-missing: {label}")
            continue
        days = cooldown.get("default-days")
        if days != EXPECTED_DEFAULT_DAYS:
            findings.append(
                f"cooldown-drift: {label} default-days must be {EXPECTED_DEFAULT_DAYS}; got {days!r}"
            )

    for guide in GUIDES:
        try:
            text = _read(root, guide)
        except RuntimeError as exc:
            findings.append(f"guide-unreadable: {exc}")
            continue
        if "14-day" not in text or "default-days: 14" not in text:
            findings.append(
                f"cooldown-guide-drift: {guide.as_posix()} must state the 14-day cooldown and default-days: 14"
            )

    return sorted(findings)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    args = parser.parse_args(argv)
    findings = validate(args.repo_root.resolve())
    if findings:
        print("FAIL: Dependabot cooldown contract", file=sys.stderr)
        for finding in findings:
            print(f"  {finding}", file=sys.stderr)
        return 1
    print("OK: Dependabot cooldown contract")
    return 0


if __name__ == "__main__":
    sys.exit(main())
