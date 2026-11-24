#!/usr/bin/env python3
"""Record exact replay results and project only proven names into the legacy ledger input."""

import argparse
import json
import os
import re
from pathlib import Path


def record(name, cause):
    with Path("retry-obs.jsonl").open(encoding="utf-8") as observations:
        replay = json.loads(observations.readline())
    identities = json.loads(Path("flake-observation-identities.json").read_text(encoding="utf-8"))
    package = replay["package"]
    target = replay["target"]
    command = replay["command"]
    if command[-3:] != ["--exact", name, "--test-threads=1"]:
        raise ValueError("result does not match the exact replay command")
    print(json.dumps({
        "package": package,
        "target": target,
        "target_name": identities["library_targets"].get(package),
        "name": name,
        "command": command,
        "cause": cause or None,
    }, ensure_ascii=False))


def code(value):
    # JSON quoting retains edge whitespace and control characters in reports.
    literal = json.dumps(value, ensure_ascii=False)
    fence = "`" * (1 + max((len(run) for run in re.findall(r"`+", literal)), default=0))
    return f"{fence}{literal}{fence}"


def report():
    identities = json.loads(Path("flake-observation-identities.json").read_text(encoding="utf-8"))
    unique = {tuple(identity) for identity in identities["unique_tests"]}
    with (
        Path("quarantine-test-names.txt").open("w", encoding="utf-8") as names,
        Path("quarantine-test-causes.txt").open("w", encoding="utf-8") as causes,
        open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as outputs,
    ):
        for prefix, count_key in (("flaky-tests", "flake_count"),
                                  ("stable-failures", "stable_failure_count")):
            with (
                Path(f"{prefix}.jsonl").open(encoding="utf-8") as results,
                Path(f"{prefix}.md").open("w", encoding="utf-8") as markdown,
            ):
                count = 0
                for line in results:
                    result = json.loads(line)
                    package, target, target_name, name = identity = tuple(
                        result[key] for key in ("package", "target", "target_name", "name")
                    )
                    subject = f"{code(package)} / {code(target)}"
                    if target_name is not None:
                        subject += f" {code(target_name)}"
                    entry = f"- {subject} / {code(name)}"
                    # The artifact retains the full command. The report need only
                    # show the subject options after the fixed cargo test prefix.
                    command = result["command"]
                    options = command[6:command.index("--")]
                    if options:
                        entry += f" · Cargo options {code(options)}"
                    cause = result["cause"]
                    if cause is not None:
                        if identity in unique and "|" not in name:
                            names.write(f"{name}\n")
                            causes.write(f"{name}|{cause}\n")
                            decision = "eligible for ledger"
                        else:
                            decision = "skipped for ledger"
                        entry += f" · {code(cause)} · {decision}"
                    markdown.write(entry + "\n")
                    count += 1
            outputs.write(f"{count_key}={count}\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    record_args = commands.add_parser("record")
    record_args.add_argument("name")
    record_args.add_argument("cause")
    commands.add_parser("report")
    args = parser.parse_args()
    if args.action == "record":
        record(args.name, args.cause)
    else:
        report()
