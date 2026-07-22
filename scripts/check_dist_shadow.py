#!/usr/bin/env python3
"""Validate that dist-workspace.toml is a non-publishing shadow of release.yml."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys
import tomllib
from typing import Any


TARGET_RE = re.compile(r"^\s*- target:\s*([A-Za-z0-9_-]+)\s*$", re.MULTILINE)
BUILD_RE = re.compile(
    r"^\s*\$BUILD_CMD build\s+.*?\s-p\s+([A-Za-z0-9_-]+)\s+--bin\s+([A-Za-z0-9_-]+)\s*$",
    re.MULTILINE,
)
STALE_TOKENS = {"perl-lsp", "perl-parse", "aarch64-pc-windows-msvc"}


class DistShadowError(RuntimeError):
    """Raised when a shadow input cannot be read or parsed."""


def load_toml(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as handle:
            data = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise DistShadowError(f"cannot read {path}: {exc}") from exc
    if not isinstance(data, dict):
        raise DistShadowError(f"{path} did not contain a TOML table")
    return data


def load_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise DistShadowError(f"cannot read {path}: {exc}") from exc


def _string_set(value: Any, label: str, errors: list[str]) -> set[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        errors.append(f"{label} must be an array of strings")
        return set()
    if len(value) != len(set(value)):
        errors.append(f"{label} must not contain duplicates")
    return set(value)


def validate_contract(dist_data: dict[str, Any], release_text: str) -> list[str]:
    errors: list[str] = []
    dist = dist_data.get("dist")
    if not isinstance(dist, dict):
        return ["dist-workspace.toml must contain a [dist] table"]

    release_targets = set(TARGET_RE.findall(release_text))
    build_pairs = set(BUILD_RE.findall(release_text))
    release_bins = {binary for _, binary in build_pairs}
    release_packages = {package for package, _ in build_pairs}

    if not release_targets:
        errors.append("release.yml target matrix could not be resolved")
    if not build_pairs:
        errors.append("release.yml package/bin build commands could not be resolved")

    includes = _string_set(dist.get("include"), "dist.include", errors)
    targets = _string_set(dist.get("targets"), "dist.targets", errors)
    installers = _string_set(dist.get("installers"), "dist.installers", errors)

    if includes != release_bins:
        errors.append(
            f"dist.include must equal live release binaries {sorted(release_bins)}, "
            f"found {sorted(includes)}"
        )
    if release_packages != release_bins:
        errors.append(
            "release.yml package/bin names diverge; shadow contract requires one "
            f"name per app, found packages={sorted(release_packages)} bins={sorted(release_bins)}"
        )
    if targets != release_targets:
        errors.append(
            f"dist.targets must equal live release targets {sorted(release_targets)}, "
            f"found {sorted(targets)}"
        )
    if installers:
        errors.append(
            "dist.installers must be empty while the active release workflow ships archives only; "
            f"found {sorted(installers)}"
        )

    if dist.get("cargo-dist-version") != "0.29.0":
        errors.append("dist.cargo-dist-version must remain pinned to 0.29.0 for the pilot")
    if dist.get("pr-run-mode") != "skip":
        errors.append("dist.pr-run-mode must be 'skip' in shadow mode")
    if dist.get("ci") != "github":
        errors.append("dist.ci must be 'github' so generated plans use the audited backend")
    if dist.get("checksum") != "sha256":
        errors.append("dist.checksum must be 'sha256'")

    github_releases = dist.get("github-releases")
    if not isinstance(github_releases, dict) or github_releases.get("create") is not False:
        errors.append("[dist.github-releases].create must be false in shadow mode")

    serialized = repr(dist_data)
    for token in sorted(STALE_TOKENS):
        if token in serialized:
            errors.append(f"stale dist contract token remains: {token}")

    return errors


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist", type=Path, default=Path("dist-workspace.toml"))
    parser.add_argument(
        "--release-workflow", type=Path, default=Path(".github/workflows/release.yml")
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        dist_data = load_toml(args.dist)
        release_text = load_text(args.release_workflow)
    except DistShadowError as exc:
        print(f"dist shadow: NOT PROVEN: {exc}", file=sys.stderr)
        return 2

    errors = validate_contract(dist_data, release_text)
    if errors:
        print("dist shadow contract failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    targets = sorted(TARGET_RE.findall(release_text))
    bins = sorted({binary for _, binary in BUILD_RE.findall(release_text)})
    print(f"dist shadow: OK — binaries={bins}, targets={targets}, publishing=disabled")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
