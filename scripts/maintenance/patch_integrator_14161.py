#!/usr/bin/env python3
from pathlib import Path

path = Path("scripts/maintenance/integrate_non_rust_policy_14161.py")
lines = path.read_text(encoding="utf-8").splitlines()
out: list[str] = []
repaired_lanes = False
repaired_whitelist = False
repaired_file_policy_path = False
repaired_main_path = False
repaired_doc_paths = False
repaired_economics = False
index = 0

while index < len(lines):
    line = lines[index]
    stripped = line.strip()
    indent = line[: len(line) - len(line.lstrip())]

    if stripped.startswith("replace_once(ci_lanes,"):
        out.append(
            '    write(ci_lanes, read(ci_lanes).rstrip("\\n") + "\\n\\n" + lane_block)'
        )
        repaired_lanes = True
        index += 1
        continue

    if (
        stripped == "replace_once("
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

    if stripped == 'require_absent(file_policy, "docs/policy/NON_RUST_INVENTORY.md")':
        out.extend(
            [
                'legacy_inventory_path = "docs/policy/NON_RUST_INVENTORY.md"',
                'if legacy_inventory_path in read(file_policy):',
                '    write(',
                '        file_policy,',
                '        read(file_policy).replace(',
                '            legacy_inventory_path, "target/policy/non-rust-inventory.md"',
                '        ),',
                '    )',
                line,
            ]
        )
        repaired_file_policy_path = True
        index += 1
        continue

    if stripped == 'require_absent(main_rs, "docs/policy/NON_RUST_INVENTORY.md")':
        out.extend(
            [
                'if legacy_inventory_path in read(main_rs):',
                '    write(',
                '        main_rs,',
                '        read(main_rs).replace(',
                '            legacy_inventory_path, "target/policy/non-rust-inventory.md"',
                '        ),',
                '    )',
                line,
            ]
        )
        repaired_main_path = True
        index += 1
        continue

    if stripped == 'require_absent(doc_path, "docs/policy/NON_RUST_INVENTORY.md")':
        out.extend(
            [
                f'{indent}if legacy_inventory_path in read(doc_path):',
                f'{indent}    write(',
                f'{indent}        doc_path,',
                f'{indent}        read(doc_path).replace(',
                f'{indent}            legacy_inventory_path, "target/policy/non-rust-inventory.md"',
                f'{indent}        ),',
                f'{indent}    )',
                line,
            ]
        )
        repaired_doc_paths = True
        index += 1
        continue

    if stripped == "write(economics_path, economics)":
        out.extend(
            [
                'economics = economics.replace(',
                '    "14 mapped + 10 unmapped = 24 lanes",',
                '    "15 mapped + 10 unmapped = 25 lanes",',
                ')',
                line,
            ]
        )
        repaired_economics = True
        index += 1
        continue

    out.append(line)
    index += 1

repairs = {
    "lanes": repaired_lanes,
    "whitelist": repaired_whitelist,
    "file_policy_path": repaired_file_policy_path,
    "main_path": repaired_main_path,
    "doc_paths": repaired_doc_paths,
    "economics": repaired_economics,
}
missing = [name for name, repaired in repairs.items() if not repaired]
if missing:
    raise RuntimeError(f"expected current-tree integrator repairs were not applied: {missing}")

path.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
