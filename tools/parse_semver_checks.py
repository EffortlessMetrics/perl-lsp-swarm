#!/usr/bin/env python3
"""Run cargo semver-checks per-crate against v0.17.0 baseline and aggregate
per-crate waived-break data into JSON and Markdown for the v0.18 release ledger.

Usage:
        python tools/parse_semver_checks.py [--out-dir DIR]

Outputs:
        <out-dir>/semver-v0.17.0-vs-main.json        — machine-readable inventory
        <out-dir>/semver-v0.17.0-vs-main.md          — human-readable summary
        target/semver-reports/per-crate/<crate>.txt  — raw human output (gitignored,
                                                        matches `just semver-report` convention)

Source of truth for crate list: .ci/public-api-baselines/ratchet-crates.txt
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
RATCHET_LIST = REPO / ".ci" / "public-api-baselines" / "ratchet-crates.txt"
DEFAULT_OUT = REPO / "docs" / "releases"
DEFAULT_RAW_DIR = REPO / "target" / "semver-reports" / "per-crate"
BASELINE = "v0.17.0"


def ratchet_crates() -> list[str]:
    """Read the single authority for ratcheted crates."""
    crates: list[str] = []
    for raw in RATCHET_LIST.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        crates.append(line)
    if not crates:
        raise SystemExit(f"no crates parsed from {RATCHET_LIST}")
    return crates


@dataclass
class Failure:
    rule: str            # e.g. "enum_marked_non_exhaustive"
    description: str     # first line of human-readable rule description
    locations: list[str] = field(default_factory=list)


@dataclass
class CrateResult:
    name: str
    exit_code: int
    summary_line: str           # the "Checked [...]" line
    pass_count: int
    fail_count: int
    warn_count: int
    skip_count: int
    failures: list[Failure] = field(default_factory=list)
    raw_output: str = ""

    @property
    def status(self) -> str:
        if self.fail_count > 0:
            return "fail"
        if self.exit_code == 0:
            return "pass"
        return "error"

    def major_break_count(self) -> int:
        # cargo-semver-checks' `fail` counter reports one finding per rule
        # violation it emits; the issue body (#15269) cites this same
        # counter in its per-crate summary, so it is the authoritative
        # major-break count for the waived-break inventory.
        return self.fail_count


SUMMARY_RE = re.compile(
    r"Checked\s*\[.*?\]\s*"
    r"(?P<pass>\d+)\s*checks?:\s*"
    r"(?P<pass_n>\d+)\s*pass,"
    r"(?:\s*(?P<fail>\d+)\s*fail,\s*(?P<warn>\d+)\s*warn,)?\s*"
    r"(?P<skip>\d+)\s*skip"
)
FAILURE_HEADER_RE = re.compile(r"^---\s*failure\s+(?P<rule>[^:]+):\s*(?P<desc>.*?)\s*---\s*$")
LOCATION_RE = re.compile(r"^\s+(?P<loc>.+?)\s+in\s+(?P<path>[^:]+):(?P<line>\d+)\s*$")


def parse_human_output(text: str) -> tuple[int, int, int, int, list[Failure]]:
    """Parse cargo semver-checks human output into counts and Failure list."""
    summary = SUMMARY_RE.search(text)
    if not summary:
        return 0, 0, 0, 0, []
    pass_count = int(summary.group("pass_n"))
    # When `fail` and `warn` are absent in the "no semver update required"
    # output, the optional groups are None — coerce to 0.
    fail_count = int(summary.group("fail") or 0)
    warn_count = int(summary.group("warn") or 0)
    skip_count = int(summary.group("skip"))

    failures: list[Failure] = []
    current: Failure | None = None
    in_failed_in = False
    for raw_line in text.splitlines():
        line = raw_line.rstrip()
        m = FAILURE_HEADER_RE.match(line)
        if m:
            if current is not None:
                failures.append(current)
            current = Failure(rule=m.group("rule").strip(),
                              description=m.group("desc").strip())
            in_failed_in = False
            continue
        if current is None:
            continue
        if line.strip().startswith("Failed in:"):
            in_failed_in = True
            continue
        if in_failed_in:
            lm = LOCATION_RE.match(line)
            if lm:
                location = f"{lm.group('loc').strip()} @ {lm.group('path')}:{lm.group('line')}"
                current.locations.append(location)
            elif line.strip() == "":
                in_failed_in = False
            # else: still inside Failed-in block but unparseable — skip
    if current is not None:
        failures.append(current)
    return pass_count, fail_count, warn_count, skip_count, failures


def run_crate(crate: str, raw_dir: Path) -> CrateResult:
    raw_path = raw_dir / f"{crate}.txt"
    raw_path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "cargo", "semver-checks", "check-release",
        "-p", crate,
        "--baseline-rev", BASELINE,
        "--color", "never",
    ]
    proc = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True)
    raw = proc.stdout + ("\n" + proc.stderr if proc.stderr else "")
    raw_path.write_text(raw, encoding="utf-8")
    pass_count, fail_count, warn_count, skip_count, failures = parse_human_output(raw)
    summary_line = ""
    m = SUMMARY_RE.search(raw)
    if m:
        summary_line = m.group(0)
    return CrateResult(
        name=crate,
        exit_code=proc.returncode,
        summary_line=summary_line,
        pass_count=pass_count,
        fail_count=fail_count,
        warn_count=warn_count,
        skip_count=skip_count,
        failures=failures,
        raw_output=raw,
    )


def render_markdown(results: list[CrateResult]) -> str:
    failing = [r for r in results if r.fail_count > 0]
    passing = [r for r in results if r.fail_count == 0 and r.exit_code == 0]
    erring = [r for r in results if r.exit_code != 0 and r.fail_count == 0]

    lines: list[str] = []
    lines.append("# v0.18 SemVer waived-break inventory (v0.17.0 → current main)")
    lines.append("")
    lines.append("Generated by `cargo semver-checks check-release` against the "
                 f"`{BASELINE}` git tag for every crate listed in "
                 "`.ci/public-api-baselines/ratchet-crates.txt`.")
    lines.append("")
    lines.append("This inventory captures the major-level SemVer breaks that the "
                 "0.18 release is *absorbing* (not blocking). After 0.18 ships, "
                 "any new major break reported against the `v0.18.x` baseline "
                 "must either be reverted, versioned into a 0.19 minor bump, "
                 "or added to a successor waived inventory — the regression "
                 "gate that consumes this file (see #15269 follow-up) will fail "
                 "the build otherwise.")
    lines.append("")
    lines.append("## Summary")
    lines.append("")
    lines.append("| Crate | Status | Checks (pass / fail / warn / skip) | Major-level breaks |")
    lines.append("|---|---|---|---|")
    for r in results:
        if r.fail_count > 0:
            status = f"❌ {r.fail_count} major break(s)"
        elif r.exit_code == 0:
            status = "✅ pass"
        else:
            status = f"⚠️  tool error (exit {r.exit_code})"
        lines.append(
            f"| `{r.name}` | {status} | "
            f"{r.pass_count} / {r.fail_count} / {r.warn_count} / {r.skip_count} | "
            f"{r.major_break_count()} |"
        )
    lines.append("")
    lines.append("Totals: "
                 f"{len(failing)} crate(s) with major breaks, "
                 f"{len(passing)} passing, "
                 f"{len(erring)} with tool errors.")
    total_breaks = sum(r.major_break_count() for r in failing)
    lines.append(f"Total major-level breaks absorbed by 0.18: **{total_breaks}**.")
    lines.append("")

    if failing:
        lines.append("## Per-crate waived breaks")
        lines.append("")
        for r in failing:
            lines.append(f"### `{r.name}` — {r.fail_count} waived break(s)")
            lines.append("")
            lines.append(f"Raw tool output: `target/semver-reports/per-crate/{r.name}.txt`")
            lines.append("")
            for i, f in enumerate(r.failures, start=1):
                lines.append(f"{i}. **{f.rule}** — {f.description}")
                for loc in f.locations:
                    lines.append(f"   - `{loc}`")
            lines.append("")
    if erring:
        lines.append("## Tool errors")
        lines.append("")
        for r in erring:
            lines.append(f"- `{r.name}`: cargo semver-checks exit {r.exit_code}; "
                         f"see `target/semver-reports/per-crate/{r.name}.txt` for raw output.")
        lines.append("")

    lines.append("## Reproduction")
    lines.append("")
    lines.append("```powershell")
    lines.append("# Per-crate (mirrors `just semver-check-package`):")
    lines.append("cargo semver-checks check-release -p <crate> --baseline-rev v0.17.0")
    lines.append("")
    lines.append("# Full inventory regeneration (writes this file + per-crate raw output):")
    lines.append("python tools/parse_semver_checks.py --out-dir docs/releases")
    lines.append("```")
    lines.append("")
    lines.append("## Provenance")
    lines.append("")
    lines.append("- Baseline tag: `v0.17.0` (materialized by #15263).")
    lines.append("- Current main SHA at regeneration: captured in the `head_sha` field of the "
                 "companion JSON, or `git rev-parse HEAD` of the regenerating checkout.")
    lines.append("- Tool: `cargo-semver-checks` v0.46.0 (0.47.0 when installed via "
                 "`just _semver-check-install`).")
    lines.append("- Ratchet crate list: `.ci/public-api-baselines/ratchet-crates.txt` (#14607).")
    lines.append("")
    lines.append("## Follow-ups")
    lines.append("")
    lines.append("- Wire this inventory into the release gate so post-0.18 drift is "
                 "enforced (separate issue — does not belong in this PR per #15269).")
    lines.append("- Reconcile `.cargo-semver-checks.toml [[packages]]` with "
                 "`.ci/public-api-baselines/ratchet-crates.txt` (#14961).")
    lines.append("")
    return "\n".join(lines)


def render_json(results: list[CrateResult], head_sha: str) -> dict:
    failing = [r for r in results if r.fail_count > 0]
    out = {
        "schema": 1,
        "release": "0.18",
        "baseline_tag": BASELINE,
        "head_sha": head_sha,
        "tool": "cargo-semver-checks",
        "ratchet_list": str(RATCHET_LIST.relative_to(REPO)),
        "totals": {
            "crates": len(results),
            "crates_with_breaks": len(failing),
            "crates_passing": sum(1 for r in results
                                 if r.fail_count == 0 and r.exit_code == 0),
            "crates_with_tool_errors": sum(1 for r in results
                                          if r.exit_code != 0 and r.fail_count == 0),
            "major_level_breaks_absorbed": sum(r.major_break_count() for r in failing),
        },
        "crates": [
            {
                "name": r.name,
                "exit_code": r.exit_code,
                "status": r.status,
                "checks": {
                    "pass": r.pass_count,
                    "fail": r.fail_count,
                    "warn": r.warn_count,
                    "skip": r.skip_count,
                },
                "major_level_breaks": [
                    {
                        "rule": f.rule,
                        "description": f.description,
                        "locations": f.locations,
                    }
                    for f in r.failures
                ],
            }
            for r in results
        ],
    }
    return out


def head_sha() -> str:
    return subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=REPO, text=True
    ).strip()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out-dir", default=str(DEFAULT_OUT),
                        help=f"output directory (default {DEFAULT_OUT})")
    parser.add_argument("--raw-dir", default=str(DEFAULT_RAW_DIR),
                        help=f"per-crate raw output directory (default {DEFAULT_RAW_DIR}, gitignored)")
    parser.add_argument("--crate", action="append", default=[],
                        help="limit to one or more crates (default: ratchet list)")
    parser.add_argument("--dry-run", action="store_true",
                        help="list crates that would be checked, do not run")
    args = parser.parse_args(argv)

    crates = args.crate or ratchet_crates()
    out_dir = Path(args.out_dir).resolve()
    raw_dir = Path(args.raw_dir).resolve()
    raw_dir.mkdir(parents=True, exist_ok=True)

    if args.dry_run:
        for c in crates:
            print(c)
        return 0

    results: list[CrateResult] = []
    for i, crate in enumerate(crates, start=1):
        print(f"[{i}/{len(crates)}] {crate} ...", flush=True)
        result = run_crate(crate, raw_dir)
        results.append(result)
        print(f"    exit={result.exit_code} fail={result.fail_count} "
              f"warn={result.warn_count} pass={result.pass_count} skip={result.skip_count}",
              flush=True)

    out_dir.mkdir(parents=True, exist_ok=True)
    head = head_sha()

    json_path = out_dir / "semver-v0.17.0-vs-main.json"
    json_path.write_text(json.dumps(render_json(results, head), indent=2) + "\n",
                         encoding="utf-8")
    md_path = out_dir / "semver-v0.17.0-vs-main.md"
    md_path.write_text(render_markdown(results), encoding="utf-8")

    failing = sum(1 for r in results if r.fail_count > 0)
    print(f"\nWrote {json_path.relative_to(REPO)} "
          f"and {md_path.relative_to(REPO)}")
    print(f"Per-crate raw output: {raw_dir.relative_to(REPO)}/")
    print(f"Head SHA at capture: {head}")
    print(f"{failing} crate(s) have major-level breaks against {BASELINE}.")
    return 0 if failing else 1  # exit 0 if we recorded any waived breaks (inventory is the deliverable),
                                # exit 1 if none (regression of issue #15269)


if __name__ == "__main__":
    sys.exit(main())