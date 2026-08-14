#!/usr/bin/env python3
"""Fail-honest static checks for the staged Mason perllsp package.

This intentionally does not replace Mason's own schema/package test. It guards
our local source/release/target/binary/lspconfig contract so the external packet
cannot drift silently while waiting for its admission gate.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
PACKAGE = ROOT / "integrations/neovim/mason-registry/packages/perllsp/package.yaml"


def fail(message: str) -> None:
    raise SystemExit(f"mason perllsp candidate invalid: {message}")


def main() -> int:
    text = PACKAGE.read_text(encoding="utf-8")

    required_literals = [
        "name: perllsp",
        "homepage: https://github.com/EffortlessMetrics/perl-lsp",
        "id: pkg:github/EffortlessMetrics/perl-lsp@v0.17.0",
        "  - MIT",
        "  - Apache-2.0",
        "  - Perl",
        "  - LSP",
        'perllsp: "{{source.asset.bin}}"',
        "  lspconfig: perllsp",
    ]
    for literal in required_literals:
        if literal not in text:
            fail(f"missing required literal {literal!r}")

    if text.count("name: perllsp") != 1:
        fail("package name must occur exactly once")
    if text.count("lspconfig: perllsp") != 1:
        fail("nvim-lspconfig identity must occur exactly once")

    version_expr = '{{ version | strip_prefix "v" }}'
    expected = {
        "darwin_arm64": (
            f"perllsp-{version_expr}-aarch64-apple-darwin.tar.gz",
            f"perllsp-{version_expr}-aarch64-apple-darwin/perllsp",
        ),
        "darwin_x64": (
            f"perllsp-{version_expr}-x86_64-apple-darwin.tar.gz",
            f"perllsp-{version_expr}-x86_64-apple-darwin/perllsp",
        ),
        "linux_arm64_gnu": (
            f"perllsp-{version_expr}-aarch64-unknown-linux-gnu.tar.gz",
            f"perllsp-{version_expr}-aarch64-unknown-linux-gnu/perllsp",
        ),
        "linux_x64_gnu": (
            f"perllsp-{version_expr}-x86_64-unknown-linux-gnu.tar.gz",
            f"perllsp-{version_expr}-x86_64-unknown-linux-gnu/perllsp",
        ),
        "linux_arm64_musl": (
            f"perllsp-{version_expr}-aarch64-unknown-linux-musl.tar.gz",
            f"perllsp-{version_expr}-aarch64-unknown-linux-musl/perllsp",
        ),
        "linux_x64_musl": (
            f"perllsp-{version_expr}-x86_64-unknown-linux-musl.tar.gz",
            f"perllsp-{version_expr}-x86_64-unknown-linux-musl/perllsp",
        ),
        "win_x64": (
            f"perllsp-{version_expr}-x86_64-pc-windows-msvc.zip",
            f"perllsp-{version_expr}-x86_64-pc-windows-msvc/perllsp.exe",
        ),
    }

    target_re = re.compile(
        r"^\s*- target: (?P<target>[^\n]+)\n"
        r"\s+file: '(?P<file>[^']+)'\n"
        r"\s+bin: '(?P<bin>[^']+)'$",
        re.MULTILINE,
    )
    found: dict[str, tuple[str, str]] = {}
    for match in target_re.finditer(text):
        target = match.group("target").strip()
        if target in found:
            fail(f"duplicate target row {target}")
        found[target] = (match.group("file"), match.group("bin"))

    if set(found) != set(expected):
        fail(
            "target denominator drifted; "
            f"expected={sorted(expected)}, found={sorted(found)}"
        )

    for target, expected_pair in expected.items():
        if found[target] != expected_pair:
            fail(
                f"{target} asset/bin mismatch: expected={expected_pair!r}, "
                f"found={found[target]!r}"
            )

    forbidden = [
        "win_arm64",
        "aarch64-pc-windows-msvc",
        "perl-lsp --stdio",
        "perlnavigator",
        "perl-ls",
    ]
    # Token match: avoid false positives such as darwin_arm64 containing win_arm64.
    for value in forbidden:
        pattern = rf"(?<![A-Za-z0-9_]){re.escape(value)}(?![A-Za-z0-9_])"
        if re.search(pattern, text):
            fail(f"unsupported or wrong-product value present: {value}")

    print(
        "mason perllsp candidate: PASS "
        f"version=v0.17.0 targets={','.join(sorted(found))}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
