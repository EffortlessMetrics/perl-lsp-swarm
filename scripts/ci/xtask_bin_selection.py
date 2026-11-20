"""Run python3 scripts/ci/xtask_bin_selection.py > selection.json.

Audits direct literal cargo test invocations in required-merge-surface.sh.
Listing cannot establish shell reachability or hosted execution.
Historical paths come from #19470, run 43690066899. Listing tests measures
selection only. The JSON contains each route, their runnable union, and historical
paths selected, uncovered (including ignored), or absent. Absent paths return
nonzero. The expansion control measures one temporary filter addition.
"""

import hashlib
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
TARGET = ["-p", "xtask", "--bin", "xtask"]


def routes(source):
    result = []
    for line in source.replace("\\\n", " ").splitlines():
        if line.lstrip().startswith("#") or not re.search(r"\bcargo\s+test\b", line):
            continue
        command = re.sub(r' \| tee "\$RUNNER_TEMP/[\w-]+\.log"\Z', "", line)
        words = shlex.split(command)
        # Skip only a literal, scoped invocation of other named targets.
        other = re.fullmatch(
            r"cargo test -p ([\w-]+) ((?:(?:--test|--bin) [\w-]+ )+)--locked -- [\w:= -]+",
            command,
        )
        if other and (other[1] != "xtask" or "--bin xtask " not in other[2]):
            continue
        if words[:6] != ["cargo", "test", *TARGET] or any(
            not re.fullmatch(r"[\w:-]+", word) for word in words[6:]
        ):
            raise ValueError(f"unsupported xtask producer command: {line}")
        args = words[6:]
        if "--" not in args:
            raise ValueError(f"missing harness separator: {line}")
        split = args.index("--")
        if (
            "--locked" not in args[:split]
            or any(w.startswith("-") and w != "--locked" for w in args[:split])
            or any(
                w.startswith("-") and w not in {"--exact", "--nocapture"}
                for w in args[split + 1 :]
            )
        ):
            raise ValueError(f"unsupported selection arguments: {line}")
        result.append(args)
    if not result:
        raise ValueError("no xtask bin commands found")
    return result


def listing(args):
    output = subprocess.check_output(
        [
            "bash",
            "scripts/cargo-admitted",
            "test",
            *TARGET,
            "--profile",
            "agent",
            *args,
            "--list",
        ],
        cwd=ROOT,
        text=True,
    )
    names = re.findall(r"^([^\s]+): test$", output, re.M)
    total = re.search(r"^(\d+) tests?, (\d+) benchmarks?\s*\Z", output, re.M)
    if not total or int(total[1]) != len(set(names)) or len(names) != len(set(names)):
        raise ValueError("incomplete libtest listing")
    return set(names)


def report(commands, known, list_tests=listing):
    inventory = list_tests(["--locked", "--"])
    ignored = list_tests(["--locked", "--", "--ignored"])
    if not inventory or not ignored <= inventory:
        raise ValueError("inconsistent libtest inventory")
    selected = set()
    rows = []
    for args in commands:
        matches = list_tests(args)
        if not matches or not matches <= inventory:
            raise ValueError(f"empty or inconsistent selection: {args}")
        runnable = matches - ignored
        selected.update(runnable)
        rows.append(
            {
                "args": args,
                "selected": sorted(runnable),
                "ignored": sorted(matches & ignored),
            }
        )
    # Widen an exact invocation in memory and require the harness to name
    # precisely one additional path. The installed producer stays untouched.
    uncovered = sorted((known & inventory) - selected - ignored)
    control = None
    if uncovered:
        exact = next((args for args in commands if "--exact" in args), None)
        if exact is None:
            raise ValueError("no exact route available for expansion control")
        baseline = list_tests(exact)
        expanded = list_tests([*exact, uncovered[0]])
        if expanded != baseline | {uncovered[0]}:
            raise ValueError(f"filter expansion did not add exactly {uncovered[0]}")
        control = uncovered[0]
    return {
        "expansion_control_added": control,
        "inventory_count": len(inventory),
        "routes": rows,
        "selected": sorted(selected),
        "historical_selected": sorted(known & selected),
        "historical_uncovered": sorted((known & inventory) - selected),
        "historical_absent": sorted(known - inventory),
    }


def historical_paths(source):
    paths = source.splitlines()
    # Pin the exact reported population, independently of row order.
    digest = hashlib.sha256("\n".join(sorted(paths)).encode()).hexdigest()
    if digest != "77db423b7415488cfab5cfc993b5828d6fff9023bfe5b5d0e9b4ac598d1db8cf":
        raise ValueError("historical paths differ from issue #19470 run 43690066899")
    return set(paths)


if __name__ == "__main__":
    known = historical_paths((HERE / "xtask-bin-historical-failures.txt").read_text())
    data = report(routes((HERE / "required-merge-surface.sh").read_text()), known)
    print(json.dumps(data, indent=2))
    sys.exit(bool(data["historical_absent"]))
