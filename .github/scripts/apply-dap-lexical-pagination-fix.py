#!/usr/bin/env python3
"""Run the previous bounded DAP repair with its generated Rust strings kept raw."""

from __future__ import annotations

import subprocess

SCRIPT_PATH = ".github/scripts/apply-dap-lexical-pagination-fix.py"


def main() -> None:
    previous = subprocess.run(
        ["git", "show", f"HEAD^:{SCRIPT_PATH}"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout

    old = '    new_collection_lines = """'
    new = '    new_collection_lines = r"""'
    count = previous.count(old)
    if count != 1:
        raise SystemExit(
            f"raw collection replacement: expected one match, found {count}"
        )

    repaired = previous.replace(old, new, 1)
    namespace = {"__name__": "__main__", "__file__": SCRIPT_PATH}
    exec(compile(repaired, SCRIPT_PATH, "exec"), namespace)


if __name__ == "__main__":
    main()
