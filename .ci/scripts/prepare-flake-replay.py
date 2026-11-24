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
from collections import Counter
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


def observation_identities(lines, receipt, allow_quarantine):
    """Derive legacy-name eligibility from the full, unfiltered libtest run."""
    targets = {}
    names = []
    block_names = []
    summaries = []
    block = None
    pending = set()
    complete = allow_quarantine and not any(
        receipt["stream"][key] for key in ("malformed_json_lines", "events_before_envelope")
    )
    for line in lines:
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict):
            continue
        kind, outcome = event.get("type"), event.get("event")
        if kind == "run":
            package, target = event["package"], event["target"]
            target_name = event.get("target_name")
            named_target = isinstance(target_name, str) and bool(target_name)
            complete &= target == "lib" and package not in targets and named_target
            if named_target:
                targets[package] = target_name
            block = (package, target, target_name)
            block_names.append([])
            pending = set()
        elif block is not None:
            if kind == "test" and outcome in ("started", "ok", "failed", "ignored"):
                name = event.get("name")
                if not isinstance(name, str) or not name:
                    complete = False
                elif outcome == "started":
                    complete &= name not in pending
                    pending.add(name)
                else:
                    complete &= outcome == "ignored" or name in pending
                    pending.discard(name)
                    block_names[-1].append(name)
                    names.append((*block, name))
            elif kind == "suite" and outcome in ("ok", "failed"):
                filtered = event.get("filtered_out")
                summaries.append(type(filtered) is int and filtered == 0)
            elif kind == "run_end":
                complete &= not pending
                block = None
    counts = Counter(identity[-1] for identity in names)
    complete &= len(summaries) == len(receipt["blocks"]) and all(summaries)
    complete &= all(
        len(observed) == len(set(observed)) == block["counts"]["discovered"]
        for observed, block in zip(block_names, receipt["blocks"], strict=True)
    )
    return {
        "library_targets": targets,
        "unique_tests": [identity for identity in names if counts[identity[-1]] == 1]
        if complete else [],
    }


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
    identities = Path("flake-observation-identities.json")
    identities.unlink(missing_ok=True)
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
        identities.write_text(json.dumps(
            observation_identities(lines, receipt, trusted and args.filter_count == 0)
        ), encoding="utf-8")
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(f"verdict={receipt['verdict']}\n")
            output.write(f"flake_count={receipt['counts']['failed']}\n")
            output.write(f"replay_count={len(eligible)}\n")
            output.write(f"aggregate_trusted={str(trusted).lower()}\n")
            output.write(f"observation_exit_code={final_exit}\n")
    except (OSError, ValueError, KeyError, TypeError) as error:
        Path("failed-tests.tsv").unlink(missing_ok=True)
        identities.unlink(missing_ok=True)
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
