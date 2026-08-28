#!/usr/bin/env python3
"""Place the route-plan input import at shared test-only module scope."""

from __future__ import annotations

from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / "xtask/src/tasks/gates/planning_types.rs"
IMPORT = "#[cfg(test)]\nuse xtask::ci_route_plan::CompileRoutePlanInput;"


def main() -> None:
    text = TARGET.read_text(encoding="utf-8")
    if IMPORT in text:
        raise RuntimeError("shared test-only CompileRoutePlanInput import already exists")

    start = text.find("use xtask::ci_route_plan::{")
    if start < 0:
        raise RuntimeError("top-level ci_route_plan import group is missing")
    end = text.find("\n};", start)
    if end < 0:
        raise RuntimeError("top-level ci_route_plan import group is unterminated")
    end += len("\n};")

    prefix = text[:end]
    if "CompileRoutePlanInput" in prefix:
        raise RuntimeError("CompileRoutePlanInput still leaks into the production import group")

    TARGET.write_text(text[:end] + "\n\n" + IMPORT + text[end:], encoding="utf-8")
    Path(__file__).unlink()


if __name__ == "__main__":
    main()
