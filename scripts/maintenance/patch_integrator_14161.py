#!/usr/bin/env python3
from pathlib import Path

path = Path("scripts/maintenance/integrate_non_rust_policy_14161.py")
lines = path.read_text(encoding="utf-8").splitlines()
out: list[str] = []
repaired_lanes = False
repaired_whitelist = False
index = 0

while index < len(lines):
    line = lines[index]
    if line.strip().startswith("replace_once(ci_lanes,"):
        out.append(
            '    write(ci_lanes, read(ci_lanes).rstrip("\\n") + "\\n\\n" + lane_block)'
        )
        repaired_lanes = True
        index += 1
        continue

    if (
        line.strip() == "replace_once("
        and index + 1 < len(lines)
        and lines[index + 1].strip() == "lane_whitelist,"
    ):
        out.append(
            '    write(lane_whitelist, read(lane_whitelist).rstrip("\\n") + "\\n\\n" + whitelist_block)'
        )
        repaired_whitelist = True
        index += 2
        while index < len(lines) and lines[index].strip() != ")":
            index += 1
        if index >= len(lines):
            raise RuntimeError("unterminated lane-whitelist replacement block")
        index += 1
        continue

    out.append(line)
    index += 1

if not repaired_lanes or not repaired_whitelist:
    raise RuntimeError(
        f"expected both current-registry repairs, got lanes={repaired_lanes} whitelist={repaired_whitelist}"
    )

path.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
