#!/usr/bin/env python3
"""Typed flake-observation extractor for the flake-detection workflow (#15349).

Consumes an *observation stream*: JSONL that interleaves identity envelopes
with raw cargo/libtest JSON test events:

    {"type": "run", "package": "pkg", "target": "lib", "command": ["cargo", ...]}
    {"type": "suite", "event": "started", "test_count": 3}
    {"type": "test", "event": "failed", "name": "mod::case", "stdout": "..."}
    {"type": "suite", "event": "failed", "passed": 2, "failed": 1, ...}
    {"type": "run_end", "package": "pkg", "exit_status": 101}

and emits a single JSON receipt with a typed verdict. The verdict is the
fail-closed contract from #15349: only COMPLETE_PASS,
COMPLETE_WITH_FAILURES, and NO_TESTS_DISCOVERED are success exits; every
harness/toolchain/stream/instrument contradiction is a typed non-success
that must fail the scheduled job instead of reporting zero flakes.

Verdicts and process exit codes:

    COMPLETE_PASS           0   every suite ran and passed
    COMPLETE_WITH_FAILURES  0   suites ran to completion; failures observed
    NO_TESTS_DISCOVERED     0   suites ran; zero tests discovered
    COMMAND_FAILED          2   cargo/toolchain failed before any suite ran
    STREAM_INVALID          3   malformed/unattributable/empty stream
    STREAM_INCOMPLETE       4   stream truncated (missing suite end/trailer)
    EXTRACTOR_FAILED        5   unreadable input / extractor usage error
    NOT_PROVEN              6   event/summary contradiction; cannot classify

Usage:
    extract-failed-tests.py <observations.jsonl> [--head SHA] [--toolchain V]
"""

import argparse
import hashlib
import json
import sys
from pathlib import Path

SCHEMA_VERSION = 2

# Verdicts that represent a usable (complete) test observation.
SUCCESS_VERDICTS = ("COMPLETE_PASS", "COMPLETE_WITH_FAILURES", "NO_TESTS_DISCOVERED")

EXIT_CODES = {
    "COMPLETE_PASS": 0,
    "COMPLETE_WITH_FAILURES": 0,
    "NO_TESTS_DISCOVERED": 0,
    "COMMAND_FAILED": 2,
    "STREAM_INVALID": 3,
    "STREAM_INCOMPLETE": 4,
    "EXTRACTOR_FAILED": 5,
    "NOT_PROVEN": 6,
}

# Aggregate precedence, worst first. Any non-success verdict fails the job,
# so the ordering only decides which typed label a mixed stream reports.
_VERDICT_PRECEDENCE = (
    "STREAM_INVALID",
    "COMMAND_FAILED",
    "STREAM_INCOMPLETE",
    "NOT_PROVEN",
    "NO_TESTS_DISCOVERED",
    "COMPLETE_WITH_FAILURES",
    "COMPLETE_PASS",
)


def _blank_block_counts():
    return {
        "discovered": 0,
        "started": 0,
        "passed": 0,
        "failed": 0,
        "ignored": 0,
        "noise_lines": 0,
        "blocks": 0,
    }


def _new_block(envelope, line_no):
    return {
        "package": envelope.get("package"),
        "target": envelope.get("target", "lib"),
        "command": envelope.get("command", []),
        "start_line": line_no,
        "end_line": None,
        "exit_status": None,
        "saw_suite_start": False,
        "test_count": None,
        "suite_finished": False,
        "suite_event": None,
        "suite_reported": {},
        "counts": _blank_block_counts(),
        "failures": [],
        "verdict": None,
        "reasons": [],
    }


def _close_block(block, truncated_no_trailer):
    """Assign a per-block verdict once the block is closed (or cut off)."""
    counts = block["counts"]
    exit_status = block["exit_status"]
    if block["verdict"] == "STREAM_INVALID":
        # Corruption was observed mid-block; keep the fail-closed verdict.
        return
    if truncated_no_trailer:
        block["verdict"] = "STREAM_INCOMPLETE"
        block["reasons"].append(
            f"no run_end trailer for block starting at line {block['start_line']}"
        )
        return
    if not block["saw_suite_start"]:
        if exit_status not in (None, 0):
            block["verdict"] = "COMMAND_FAILED"
            block["reasons"].append(
                f"cargo exited {exit_status} before emitting any suite events"
            )
        else:
            block["verdict"] = "STREAM_INVALID"
            block["reasons"].append(
                "run completed without any usable libtest suite events"
            )
        return
    if not block["suite_finished"]:
        block["verdict"] = "STREAM_INCOMPLETE"
        block["reasons"].append("suite started but never reported a final suite event")
        return
    reported = block["suite_reported"]
    reconciles = (
        reported.get("passed") == counts["passed"]
        and reported.get("failed") == counts["failed"]
        and reported.get("ignored") == counts["ignored"]
    )
    if not reconciles:
        block["verdict"] = "NOT_PROVEN"
        block["reasons"].append(
            "suite summary counts do not reconcile with observed test events"
        )
        return
    if block["suite_event"] == "ok" and counts["failed"] > 0:
        block["verdict"] = "NOT_PROVEN"
        block["reasons"].append("suite reported ok but failed test events were observed")
        return
    if block["suite_event"] == "failed" and exit_status == 0:
        block["verdict"] = "NOT_PROVEN"
        block["reasons"].append("suite reported failures but cargo exited 0")
        return
    if counts["discovered"] == 0:
        block["verdict"] = "NO_TESTS_DISCOVERED"
        return
    if counts["failed"] > 0:
        block["verdict"] = "COMPLETE_WITH_FAILURES"
        return
    block["verdict"] = "COMPLETE_PASS"


def parse_observation_stream(lines):
    """Parse an observation stream into a receipt dict (without metadata)."""
    blocks = []
    current = None
    failures = []
    total_counts = _blank_block_counts()
    malformed_json_lines = 0
    events_before_envelope = 0
    line_no = 0
    had_any_line = False

    for raw_line in lines:
        line_no += 1
        line = raw_line.rstrip("\r\n")
        if not line.strip():
            continue
        had_any_line = True
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            # Plain-text diagnostics (cargo/rustc stderr) can legitimately be
            # interleaved into a redirected stream. They are recorded as
            # noise and only invalidate a block that produces no usable
            # events; brace-prefixed junk means JSON format drift.
            if line.lstrip().startswith("{"):
                malformed_json_lines += 1
                if current is not None:
                    current["counts"]["noise_lines"] += 1
                    current["reasons"].append(
                        f"line {line_no}: brace-prefixed line is not valid JSON"
                    )
                    current["verdict"] = "STREAM_INVALID"
                else:
                    events_before_envelope += 1
            elif current is not None:
                current["counts"]["noise_lines"] += 1
            else:
                events_before_envelope += 1
            continue

        if not isinstance(event, dict) or "type" not in event:
            if current is not None:
                current["counts"]["noise_lines"] += 1
            else:
                events_before_envelope += 1
            continue

        etype = event.get("type")

        if etype == "run":
            if current is not None:
                _close_block(current, truncated_no_trailer=True)
                blocks.append(current)
            current = _new_block(event, line_no)
            continue

        if etype == "run_end":
            if current is None:
                events_before_envelope += 1
                continue
            current["exit_status"] = event.get("exit_status")
            current["end_line"] = line_no
            _close_block(current, truncated_no_trailer=False)
            blocks.append(current)
            current = None
            continue

        if current is None:
            events_before_envelope += 1
            continue

        if etype == "suite":
            suite_event = event.get("event")
            if suite_event == "started":
                current["saw_suite_start"] = True
                current["test_count"] = event.get("test_count")
                current["counts"]["discovered"] = event.get("test_count") or 0
            elif suite_event in ("ok", "failed"):
                current["suite_finished"] = True
                current["suite_event"] = suite_event
                current["suite_reported"] = {
                    "passed": event.get("passed"),
                    "failed": event.get("failed"),
                    "ignored": event.get("ignored"),
                }
            # Unknown suite events (bench, ...) are tolerated, not evidence.
            continue

        if etype == "test":
            tevent = event.get("event")
            name = event.get("name")
            if tevent == "started":
                current["counts"]["started"] += 1
            elif tevent in ("ok", "failed", "ignored"):
                if tevent == "ok":
                    current["counts"]["passed"] += 1
                elif tevent == "failed":
                    current["counts"]["failed"] += 1
                else:
                    current["counts"]["ignored"] += 1
                if tevent == "failed" and name:
                    failure = {
                        "package": current["package"],
                        "target": current["target"],
                        "name": name,
                        "block_start_line": current["start_line"],
                    }
                    current["failures"].append(failure)
                    failures.append(failure)
            # Unknown test events are noise, never evidence of failure.
            continue

        # Any other structured event (bench, ...) is not evidence of failure.
        current["counts"]["noise_lines"] += 1

    if current is not None:
        _close_block(current, truncated_no_trailer=True)
        blocks.append(current)

    block_verdicts = [b["verdict"] for b in blocks]
    if not had_any_line:
        verdict = "STREAM_INVALID"
        reasons = ["observation stream is empty; no test evidence was produced"]
    elif not blocks:
        verdict = "STREAM_INVALID"
        reasons = [
            "stream contained no run envelopes; events cannot be attributed "
            "to a package/target"
        ]
    else:
        verdict = "COMPLETE_PASS"
        for candidate in _VERDICT_PRECEDENCE:
            if candidate in block_verdicts:
                verdict = candidate
                break
        reasons = []
        for b in blocks:
            reasons.extend(f"[{b['package']}] {r}" for r in b["reasons"])

    if malformed_json_lines:
        reasons.append(f"{malformed_json_lines} brace-prefixed malformed JSON line(s)")
    if events_before_envelope:
        reasons.append(
            f"{events_before_envelope} event(s) appeared outside any run envelope"
        )

    total_counts["discovered"] = sum(b["counts"]["discovered"] for b in blocks)
    total_counts["started"] = sum(b["counts"]["started"] for b in blocks)
    total_counts["passed"] = sum(b["counts"]["passed"] for b in blocks)
    total_counts["failed"] = sum(b["counts"]["failed"] for b in blocks)
    total_counts["ignored"] = sum(b["counts"]["ignored"] for b in blocks)
    total_counts["noise_lines"] = sum(b["counts"]["noise_lines"] for b in blocks)
    total_counts["blocks"] = len(blocks)

    return {
        "schema_version": SCHEMA_VERSION,
        "verdict": verdict,
        "exit_code": EXIT_CODES[verdict],
        "success_verdict": verdict in SUCCESS_VERDICTS,
        "counts": total_counts,
        "failures": failures,
        "blocks": [
            {
                "package": b["package"],
                "target": b["target"],
                "exit_status": b["exit_status"],
                "verdict": b["verdict"],
                "counts": dict(b["counts"]),
                "reasons": b["reasons"],
            }
            for b in blocks
        ],
        "stream": {
            "malformed_json_lines": malformed_json_lines,
            "events_before_envelope": events_before_envelope,
            "reasons": reasons,
        },
    }


def build_receipt(lines, head, toolchain):
    receipt = parse_observation_stream(lines)
    receipt["metadata"] = {
        "head": head,
        "toolchain": toolchain,
        "observed_target_families": sorted({b["target"] for b in receipt["blocks"]}),
    }
    receipt["stream"]["digest"] = hashlib.sha256(
        "".join(lines).encode("utf-8", errors="replace")
    ).hexdigest()
    return receipt


def _failure_receipt(reason, args):
    return {
        "schema_version": SCHEMA_VERSION,
        "verdict": "EXTRACTOR_FAILED",
        "exit_code": EXIT_CODES["EXTRACTOR_FAILED"],
        "success_verdict": False,
        "counts": _blank_block_counts(),
        "failures": [],
        "blocks": [],
        "stream": {"reasons": [reason]},
        "metadata": {"head": args.head, "toolchain": args.toolchain},
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("observation_file", nargs="?")
    parser.add_argument("--head", default=None, help="repository head SHA")
    parser.add_argument("--toolchain", default=None, help="toolchain identity string")
    args = parser.parse_args(argv)

    if args.observation_file:
        path = Path(args.observation_file)
        try:
            lines = path.read_text(encoding="utf-8", errors="replace").splitlines(
                keepends=True
            )
        except OSError as exc:
            receipt = _failure_receipt(f"unreadable input: {exc}", args)
            print(json.dumps(receipt))
            return receipt["exit_code"]
    else:
        lines = sys.stdin.readlines()

    receipt = build_receipt(lines, args.head, args.toolchain)
    print(json.dumps(receipt, indent=2))
    return receipt["exit_code"]


if __name__ == "__main__":
    sys.exit(main())
