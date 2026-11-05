#!/usr/bin/env python3
"""Consume an initial flake receipt without hiding valid sibling failures (#19084).

The canonical extractor owns verdicts. This consumer binds eligible failures to
their exact complete observation envelopes, and defers aggregate failure until
read-only replays finish. It never authorizes mutations on an invalid aggregate.
"""

import argparse
import importlib.util
import json
import os
import sys
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "extract_failed_tests", Path(__file__).with_name("extract-failed-tests.py")
)
EXTRACTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXTRACTOR)


def eligible_failures(lines, receipt):
    # blocks[] intentionally has no line boundaries in schema v2. The
    # canonical parser emits one block per JSON run envelope in source order;
    # recover those starts instead of joining on a non-unique package name.
    starts = []
    for number, line in enumerate(lines, 1):
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict) and event.get("type") == "run":
            starts.append(number)
    if len(starts) != len(receipt["blocks"]):
        raise ValueError("observation envelope/block count mismatch")
    blocks = dict(zip(starts, receipt["blocks"]))
    eligible = []
    seen = set()
    for failure in receipt["failures"]:
        block = blocks.get(failure["block_start_line"])
        if block is None:
            raise ValueError("failure has no observation envelope")
        if block["verdict"] != "COMPLETE_WITH_FAILURES":
            continue
        identity = (failure["package"], failure["target"], failure["name"])
        if identity[:2] != (block["package"], block["target"]) or identity[1] != "lib":
            raise ValueError("failure is not bound to its library-test envelope")
        if any(not isinstance(value, str) or not value or
               any(ord(char) < 32 or ord(char) == 127 for char in value)
               for value in identity):
            raise ValueError("failure identity cannot be represented safely in TSV")
        if identity not in seen:
            eligible.append((identity[0], identity[2]))
            seen.add(identity)
    return eligible


def annotation(message):
    escaped = str(message).replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
    print("::error::" + escaped)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("observations", type=Path)
    parser.add_argument("receipt", type=Path)
    parser.add_argument("--extractor-exit", required=True, type=int)
    parser.add_argument("--expected-head", required=True)
    parser.add_argument("--expected-toolchain", required=True)
    parser.add_argument("--filter-count", required=True, type=int)
    args = parser.parse_args()

    # No output can survive a failed/stale consumer invocation in a reused
    # working directory. Outputs are appended only after all validation.
    Path("failed-tests.tsv").unlink(missing_ok=True)
    try:
        lines = args.observations.read_text(encoding="utf-8", errors="replace").splitlines(keepends=True)
        receipt = json.loads(args.receipt.read_text(encoding="utf-8"))
        expected = EXTRACTOR.build_receipt(lines, args.expected_head, args.expected_toolchain)
        if receipt != expected:
            raise ValueError("receipt does not match the canonical observation, head, and toolchain")
        if args.extractor_exit != receipt["exit_code"]:
            raise ValueError("extractor exit does not match its typed receipt")
        eligible = eligible_failures(lines, receipt)
        trusted = receipt["success_verdict"] and (
            receipt["verdict"] != "NO_TESTS_DISCOVERED" or args.filter_count > 0
        )
        final_exit = 0 if trusted else (args.extractor_exit or 1)
        Path("failed-tests.tsv").write_text(
            "".join(f"{package}\t{name}\n" for package, name in eligible), encoding="utf-8"
        )
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(f"verdict={receipt['verdict']}\n")
            output.write(f"flake_count={receipt['counts']['failed']}\n")
            output.write(f"replay_count={len(eligible)}\n")
            output.write(f"aggregate_trusted={str(trusted).lower()}\n")
            output.write(f"observation_exit_code={final_exit}\n")
    except (OSError, ValueError, KeyError, TypeError) as error:
        Path("failed-tests.tsv").unlink(missing_ok=True)
        annotation(f"flake receipt consumer failed: {error}")
        return 1

    print(f"Observation verdict: {receipt['verdict']} "
          f"(observed failed tests: {receipt['counts']['failed']}; replay-eligible: {len(eligible)})")
    if not trusted:
        for reason in receipt["stream"]["reasons"][:20]:
            annotation(reason)
        if len(receipt["stream"]["reasons"]) > 20:
            print("Further instrument reasons are retained in flake-receipt.json.")
        annotation(f"initial flake observation is untrusted; exit {final_exit} deferred until after read-only replay")
    return 0


if __name__ == "__main__":
    sys.exit(main())
