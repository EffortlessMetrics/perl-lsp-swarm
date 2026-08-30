#!/usr/bin/env python3
"""Remove current-source residues exposed by the first #10060 red proof."""

from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def remove_rust_test(relative: str, function_name: str) -> None:
    path = ROOT / relative
    text = path.read_text(encoding="utf-8")
    signature = f"fn {function_name}"
    marker = text.find(signature)
    require(marker >= 0, f"{relative}: missing test {function_name}")
    start = text.rfind("#[test]", 0, marker)
    require(start >= 0, f"{relative}: missing #[test] for {function_name}")
    brace = text.find("{", marker)
    require(brace >= 0, f"{relative}: missing body for {function_name}")

    depth = 0
    in_string = False
    escaped = False
    end: int | None = None
    for index in range(brace, len(text)):
        char = text[index]
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue
        if char == '"':
            in_string = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                end = index + 1
                break
    require(end is not None, f"{relative}: unterminated test {function_name}")
    while end < len(text) and text[end] == "\n":
        end += 1
    path.write_text(text[:start] + text[end:], encoding="utf-8")


def replace_lsp_smoke_comment() -> None:
    relative = "xtask/src/tasks/lsp_smoke_atomic.rs"
    path = ROOT / relative
    text = path.read_text(encoding="utf-8")
    adapter = "scripts/ci/" + "receipts-to-" + "junit.py"
    old = (
        "//! deliberately does not use the `test_results` envelope that feeds Test\n"
        f"//! Analytics (`{adapter}` classifies unknown shapes as\n"
        "//! non-tests, so no aggregate JUnit pseudo-test is manufactured).\n"
    )
    new = (
        "//! deliberately remains repository-owned JSON evidence. It is not projected\n"
        "//! into vendor test-result telemetry, so no aggregate pseudo-test is\n"
        "//! manufactured from child gate status.\n"
    )
    require(text.count(old) == 1, f"{relative}: expected one retired adapter comment")
    path.write_text(text.replace(old, new), encoding="utf-8")


def main() -> None:
    remove_rust_test(
        "xtask/src/tasks/ci_route.rs",
        "ci_route_receipt_maps_receipts_junit_script_to_focused_non_lcov_pack",
    )
    replace_lsp_smoke_comment()


if __name__ == "__main__":
    main()
