#!/usr/bin/env python3
"""Canonical scope-crate-set hash for scope-aware cargo cache keys (#2908).

Scoped lanes build only the crates ci-scope selected for the current diff, but
a cache key derived from Cargo.lock alone forces one workspace-wide entry: a
2-crate build and a 200-crate build restore and churn the same cache lane.

This helper canonicalizes the selected package arguments into one stable hash
component so each distinct crate set owns its own cache lane:

- order-insensitive and duplicate-insensitive (`-p b -p a` == `-p a -p b`);
- scope-partitioned (adding an unrelated crate changes only that set's hash);
- stable across force-pushes that change neither Cargo.lock nor the set;
- fail-closed on malformed input instead of caching under a wrong identity.

Composition: the crate names are validated, deduplicated, sorted, joined with
newlines, encoded UTF-8, hashed with SHA-256, hex-encoded, and truncated to
``--length`` characters. The empty set is a valid deterministic input unless
the workflow boundary selects ``--require-non-empty``.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys

PACKAGE_FLAGS = ("-p", "--package")
CRATE_NAME_PATTERN = re.compile(r"^[A-Za-z0-9_-]+$")

DEFAULT_HASH_LENGTH = 16
MIN_HASH_LENGTH = 8
MAX_HASH_LENGTH = 64


def parse_package_args(package_args: str) -> list[str]:
    """Extract crate names from strict ``-p <name>`` / ``--package <name>`` pairs.

    Anything else (bare names, unknown flags, dangling flags, invalid crate
    characters) raises ``ValueError`` so callers fail closed rather than
    deriving a cache key from an unexpected shape.
    """
    tokens = package_args.split()
    names: list[str] = []
    index = 0
    while index < len(tokens):
        flag = tokens[index]
        if flag not in PACKAGE_FLAGS:
            raise ValueError(
                f"expected '-p' or '--package' flag, found {flag!r}"
            )
        index += 1
        if index >= len(tokens):
            raise ValueError(f"{flag} is missing its crate name")
        name = tokens[index]
        if not CRATE_NAME_PATTERN.fullmatch(name):
            raise ValueError(f"invalid crate name {name!r}")
        names.append(name)
        index += 1
    return names


def canonical_crate_set(names: list[str]) -> str:
    """Return the order-insensitive, duplicate-free canonical set text."""
    return "\n".join(sorted(set(names)))


def scope_cache_key(
    package_args: str,
    length: int = DEFAULT_HASH_LENGTH,
    *,
    require_non_empty: bool = False,
) -> str:
    """Hash the canonical crate set into the cache-key component."""
    if not MIN_HASH_LENGTH <= length <= MAX_HASH_LENGTH:
        raise ValueError(
            f"length must be between {MIN_HASH_LENGTH} and {MAX_HASH_LENGTH}, "
            f"got {length}"
        )
    names = parse_package_args(package_args)
    if require_non_empty and not names:
        raise ValueError("the selected crate scope must not be empty")
    canonical = canonical_crate_set(names)
    digest = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    return digest[:length]


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Derive the scope-crate-set hash used in scope-aware cargo "
            "cache keys (#2908)."
        )
    )
    parser.add_argument(
        "--package-args",
        required=True,
        help=(
            "the ci-scope package selection, e.g. \"-p perl-uri -p perl-workspace\""
        ),
    )
    parser.add_argument(
        "--require-non-empty",
        action="store_true",
        help="reject a missing, empty, or whitespace-only selected crate scope",
    )
    parser.add_argument(
        "--length",
        type=int,
        default=DEFAULT_HASH_LENGTH,
        help=(
            "number of hex characters to keep (default: "
            f"{DEFAULT_HASH_LENGTH}, range {MIN_HASH_LENGTH}..{MAX_HASH_LENGTH})"
        ),
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        print(
            scope_cache_key(
                args.package_args,
                args.length,
                require_non_empty=args.require_non_empty,
            )
        )
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
