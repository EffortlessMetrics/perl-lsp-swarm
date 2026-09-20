#!/usr/bin/env python3
"""Run cargo semver-checks per-crate against the v0.17.0 baseline and aggregate
per-crate waived-break data into JSON and Markdown for the v0.18 release ledger.

Usage:
        python tools/parse_semver_checks.py [--out-dir DIR] [--raw-dir DIR]
                                            [--crate NAME]... [--dry-run]

Outputs:
        <out-dir>/semver-v0.17.0-vs-main.json        — machine-readable inventory
        <out-dir>/semver-v0.17.0-vs-main.md          — human-readable summary
        target/semver-reports/per-crate/<crate>.txt  — raw human output (gitignored,
                                                        matches `just semver-report` convention)

Filtered runs (--crate) produce a partial denominator and MUST target a
non-canonical --out-dir. The canonical docs/releases ledger is only published
by a full ratchet-list run on a clean tracked tree, where every ratcheted
crate reaches a terminal verdict (pass, fail/waived, or not_applicable backed
by baseline absence). Any unresolved instrument error fails publication; the
publication verdict is computed before any canonical write, so a run without
a publishable verdict leaves the canonical ledger untouched.

Source of truth for crate list: .ci/public-api-baselines/ratchet-crates.txt
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
RATCHET_LIST = REPO / ".ci" / "public-api-baselines" / "ratchet-crates.txt"
DEFAULT_OUT = REPO / "docs" / "releases"
DEFAULT_RAW_DIR = REPO / "target" / "semver-reports" / "per-crate"
BASELINE_TAG = "v0.17.0"

# A crate reaches a terminal verdict in one of these states. Anything else
# (unparsable output, non-zero exit without a parsed break) is an unresolved
# instrument error and fails publication of the canonical ledger.
TERMINAL_STATUSES = {"pass", "fail", "not_applicable"}


def parse_ratchet_list(text: str) -> list[str]:
    """Parse the ratchet crate list text into crate names.

    The authority file promises that everything after a `#` is a comment, so
    inline comments are stripped; duplicate crate names fail closed before any
    instrument runs, so a mis-edited list can neither double-check nor
    double-disposition a crate.
    """
    crates: list[str] = []
    seen: set[str] = set()
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        if line in seen:
            raise SystemExit(
                f"duplicate crate {line!r} in ratchet list; each ratcheted "
                "crate must be listed exactly once")
        seen.add(line)
        crates.append(line)
    if not crates:
        raise SystemExit("no crates parsed from ratchet list")
    return crates


def ratchet_crates() -> list[str]:
    """Read the single authority for ratcheted crates."""
    return parse_ratchet_list(RATCHET_LIST.read_text(encoding="utf-8"))


class SemverParseError(Exception):
    """cargo semver-checks human output did not match the expected format."""


@dataclass
class Failure:
    rule: str            # e.g. "enum_marked_non_exhaustive"
    description: str     # first line of human-readable rule description
    locations: list[str] = field(default_factory=list)


@dataclass
class CrateResult:
    name: str
    exit_code: int
    summary_line: str = ""      # the "Checked [...]" line
    pass_count: int = 0
    fail_count: int = 0
    warn_count: int = 0
    skip_count: int = 0
    failures: list[Failure] = field(default_factory=list)
    raw_output: str = ""
    parse_error: str = ""
    raw_report_sha256: str = ""
    not_applicable: bool = False
    disposition_basis: str = ""
    disposition_evidence: str = ""

    @property
    def status(self) -> str:
        # Exit-code contract for cargo-semver-checks 0.47.0: `fail` only for
        # exit 1 with reported breaks, `pass` only for exit 0 with none, and
        # `error` for every other combination — including the invalid
        # exit 0 + fail>0, which must never enter the ledger as a verdict.
        if self.not_applicable:
            return "not_applicable"
        if self.parse_error:
            return "error"
        if self.fail_count > 0:
            return "fail" if self.exit_code == 1 else "error"
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
# The path is matched lazily and anchored on the trailing `:<line>` so both
# POSIX-relative paths and Windows absolute paths (`C:\...\lib.rs:10`, whose
# drive-letter colon would break a `[^:]+` path group) parse.
LOCATION_RE = re.compile(r"^\s+(?P<loc>.+?)\s+in\s+(?P<path>.+?):(?P<line>\d+)\s*$")


def parse_human_output(text: str) -> tuple[int, int, int, int, list[Failure]]:
    """Parse cargo semver-checks human output into counts and Failure list.

    Raises SemverParseError when the summary line is missing or duplicated:
    a successful exit with unrecognized output must never be mistaken for a
    clean 0/0/0/0 result.
    """
    summaries = list(SUMMARY_RE.finditer(text))
    if not summaries:
        raise SemverParseError("no 'Checked [...]' summary line found in output")
    if len(summaries) > 1:
        raise SemverParseError(
            f"{len(summaries)} 'Checked [...]' summary lines found; expected exactly one")
    summary = summaries[0]
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


def result_from_raw(name: str, exit_code: int, raw: str) -> CrateResult:
    """Build a CrateResult from raw tool output, failing closed on parse errors."""
    try:
        pass_count, fail_count, warn_count, skip_count, failures = parse_human_output(raw)
        parse_error = ""
    except SemverParseError as exc:
        pass_count = fail_count = warn_count = skip_count = 0
        failures = []
        parse_error = str(exc)
    summary_line = ""
    m = SUMMARY_RE.search(raw)
    if m:
        summary_line = m.group(0)
    return CrateResult(
        name=name,
        exit_code=exit_code,
        summary_line=summary_line,
        pass_count=pass_count,
        fail_count=fail_count,
        warn_count=warn_count,
        skip_count=skip_count,
        failures=failures,
        raw_output=raw,
        parse_error=parse_error,
        raw_report_sha256=hashlib.sha256(raw.encode("utf-8")).hexdigest(),
    )


def run_crate(crate: str, raw_dir: Path, baseline_commit: str) -> CrateResult:
    raw_path = raw_dir / f"{crate}.txt"
    raw_path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "cargo", "semver-checks", "check-release",
        "-p", crate,
        "--baseline-rev", baseline_commit,
        "--color", "never",
    ]
    proc = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True)
    raw = proc.stdout + ("\n" + proc.stderr if proc.stderr else "")
    raw_path.write_text(raw, encoding="utf-8")
    return result_from_raw(crate, proc.returncode, raw)


def crate_not_in_baseline(crate: str, rel_manifest: str, baseline_commit: str) -> CrateResult:
    """Terminal not_applicable verdict for a crate absent from the baseline."""
    return CrateResult(
        name=crate,
        exit_code=0,
        not_applicable=True,
        disposition_basis="baseline_absent",
        disposition_evidence=(
            f"{rel_manifest} does not exist at baseline commit {baseline_commit} "
            f"({BASELINE_TAG}); the crate first publishes after the baseline, so it "
            f"carries no absorbed-break obligation for this release"),
    )


class BaselineLookup(Enum):
    """Typed outcome of a git-history existence probe.

    ERROR means the git read itself failed; it must never be folded into
    ABSENT, or a broken repository could silently waive a crate.
    """

    PRESENT = "present"
    ABSENT = "absent"
    ERROR = "error"


def manifest_exists_at(rel_manifest: str, commit: str, runner=None) -> BaselineLookup:
    """Classify whether <commit>:<rel_manifest> exists in git history.

    Exit 1 from `git cat-file -e` is the tool's "object does not exist"
    outcome (baseline absence); any other non-zero exit is a git read
    failure, which is not a disposition.
    """
    run = runner or subprocess.run
    proc = run(
        ["git", "cat-file", "-e", f"{commit}:{rel_manifest}"],
        cwd=REPO, capture_output=True,
    )
    if proc.returncode == 0:
        return BaselineLookup.PRESENT
    if proc.returncode == 1:
        return BaselineLookup.ABSENT
    return BaselineLookup.ERROR


def workspace_manifest_map() -> dict[str, str]:
    """Map workspace package name -> repo-relative manifest path."""
    proc = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=REPO, capture_output=True, text=True,
    )
    if proc.returncode != 0:
        raise SystemExit(f"cargo metadata failed: {proc.stderr.strip()}")
    meta = json.loads(proc.stdout)
    mapping: dict[str, str] = {}
    for pkg in meta["packages"]:
        manifest = Path(pkg["manifest_path"]).resolve()
        mapping[pkg["name"]] = manifest.relative_to(REPO).as_posix()
    return mapping


def render_markdown(results: list[CrateResult]) -> str:
    failing = [r for r in results if r.status == "fail"]
    passing = [r for r in results if r.status == "pass"]
    not_applicable = [r for r in results if r.status == "not_applicable"]
    erring = [r for r in results if r.status == "error"]

    lines: list[str] = []
    lines.append("# v0.18 SemVer waived-break inventory (v0.17.0 → current main)")
    lines.append("")
    lines.append("Generated by `cargo semver-checks check-release` against the "
                 f"`{BASELINE_TAG}` baseline commit for every crate listed in "
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
    lines.append("A crate absent from the baseline records `not_applicable` "
                 "(basis `baseline_absent`) and carries no absorbed-break "
                 "obligation; every other ratcheted crate must reach a terminal "
                 "pass or waived-fail verdict, and unresolved tool errors fail "
                 "publication.")
    lines.append("")
    lines.append("## Summary")
    lines.append("")
    lines.append("| Crate | Status | Checks (pass / fail / warn / skip) | Major-level breaks |")
    lines.append("|---|---|---|---|")
    for r in results:
        if r.status == "fail":
            status = f"❌ {r.fail_count} major break(s)"
        elif r.status == "pass":
            status = "✅ pass"
        elif r.status == "not_applicable":
            status = "➖ not applicable (baseline_absent)"
        else:
            detail = r.parse_error or f"exit {r.exit_code}"
            status = f"⚠️  tool error ({detail})"
        lines.append(
            f"| `{r.name}` | {status} | "
            f"{r.pass_count} / {r.fail_count} / {r.warn_count} / {r.skip_count} | "
            f"{r.major_break_count()} |"
        )
    lines.append("")
    lines.append("Totals: "
                 f"{len(failing)} crate(s) with major breaks, "
                 f"{len(passing)} passing, "
                 f"{len(not_applicable)} not applicable (absent from baseline), "
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
    if not_applicable:
        lines.append("## Not applicable (absent from baseline)")
        lines.append("")
        for r in not_applicable:
            lines.append(f"- `{r.name}`: {r.disposition_evidence}")
        lines.append("")
    if erring:
        lines.append("## Tool errors")
        lines.append("")
        for r in erring:
            detail = r.parse_error or f"cargo semver-checks exit {r.exit_code}"
            lines.append(f"- `{r.name}`: {detail}; "
                         f"see `target/semver-reports/per-crate/{r.name}.txt` for raw output.")
        lines.append("")

    lines.append("## Reproduction")
    lines.append("")
    lines.append("```powershell")
    lines.append("# Per-crate (mirrors `just semver-check-package`):")
    lines.append("cargo semver-checks check-release -p <crate> --baseline-rev <baseline-commit>")
    lines.append("")
    lines.append("# Full inventory regeneration (writes this file + per-crate raw output;")
    lines.append("# requires a clean tracked tree and resolves --crate drafts to a")
    lines.append("# non-canonical --out-dir):")
    lines.append("python tools/parse_semver_checks.py --out-dir docs/releases")
    lines.append("```")
    lines.append("")
    lines.append("## Provenance")
    lines.append("")
    lines.append(f"- Baseline tag: `{BASELINE_TAG}` (materialized by #15263), resolved to "
                 "the commit recorded in the `baseline_commit` field of the companion JSON.")
    lines.append("- Analyzed source: the clean tracked working tree; `head_sha` and "
                 "`head_tree` in the companion JSON identify the exact commit and "
                 "content tree, and publication refuses a dirty tracked tree.")
    lines.append("- Tool: exact `cargo semver-checks --version` captured in the JSON "
                 "`tool_version` field at regeneration time.")
    lines.append("- Each per-crate raw report is digested into the JSON "
                 "`raw_report_sha256` field of its crate entry.")
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


def render_json(results: list[CrateResult], head_sha: str, baseline_commit: str,
                tool_version: str, head_tree: str) -> dict:
    failing = [r for r in results if r.status == "fail"]
    out = {
        "schema": 2,
        "release": "0.18",
        "baseline_tag": BASELINE_TAG,
        "baseline_commit": baseline_commit,
        "head_sha": head_sha,
        "head_tree": head_tree,
        "tool": "cargo-semver-checks",
        "tool_version": tool_version,
        "ratchet_list": RATCHET_LIST.relative_to(REPO).as_posix(),
        "totals": {
            "crates": len(results),
            "crates_with_breaks": len(failing),
            "crates_passing": sum(1 for r in results if r.status == "pass"),
            "crates_not_applicable": sum(1 for r in results
                                         if r.status == "not_applicable"),
            "crates_with_tool_errors": sum(1 for r in results if r.status == "error"),
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
                "raw_report_sha256": r.raw_report_sha256,
                **({"disposition": {
                    "basis": r.disposition_basis,
                    "evidence": r.disposition_evidence,
                }} if r.status == "not_applicable" else {}),
            }
            for r in results
        ],
    }
    return out


def git_rev_resolve(rev: str) -> str:
    return subprocess.check_output(
        ["git", "rev-parse", rev], cwd=REPO, text=True
    ).strip()


def head_sha() -> str:
    return git_rev_resolve("HEAD")


def head_tree() -> str:
    return git_rev_resolve("HEAD^{tree}")


def tracked_dirty() -> bool:
    proc = subprocess.run(
        ["git", "status", "--porcelain"], cwd=REPO, capture_output=True, text=True
    )
    if proc.returncode != 0:
        raise SystemExit(f"git status failed: {proc.stderr.strip()}")
    return bool(proc.stdout.strip())


def tool_version() -> str:
    proc = subprocess.run(
        ["cargo", "semver-checks", "--version"], cwd=REPO, capture_output=True, text=True
    )
    if proc.returncode != 0:
        raise SystemExit("cargo semver-checks is not runnable; install it via "
                         "`just _semver-check-install` (provenance requires the "
                         "exact tool version)")
    return proc.stdout.strip().splitlines()[0].strip()


def check_filtered_out_dir(filtered: bool, out_dir: Path) -> None:
    """Refuse canonical publication of a filtered (partial) inventory."""
    if filtered and out_dir == DEFAULT_OUT.resolve():
        raise SystemExit(
            "--crate filtered runs produce a partial denominator and must set "
            f"--out-dir away from the canonical ledger directory ({DEFAULT_OUT}); "
            "the canonical docs/releases inventory requires a full ratchet-list run")


def publication_exit_code(results: list[CrateResult]) -> int:
    """Exit 0 only when the inventory is complete and non-empty.

    Any unresolved instrument error (status `error`) fails closed even when
    other crates recorded waived breaks; a fully clean workspace with zero
    waived breaks is a regression of the inventory record itself (#15269).
    """
    if any(r.status == "error" for r in results):
        return 1
    if any(r.major_break_count() > 0 for r in results):
        return 0
    return 1


def publication_gate(results: list[CrateResult], out_dir: Path) -> int:
    """Compute the publication verdict before any canonical write.

    Canonical ledger writes happen only for a publishable verdict (0): an
    error-containing run — or a fully clean one, which regresses the
    inventory record — must never overwrite the canonical ledger with a
    non-publishable set. Draft --out-dir targets always proceed so failed
    runs remain debuggable.
    """
    publication = publication_exit_code(results)
    if publication != 0 and out_dir == DEFAULT_OUT.resolve():
        raise SystemExit(
            "refusing canonical publication: the run has no publishable verdict "
            "(unresolved instrument error, or zero waived breaks against the "
            "inventory record); inspect it via a non-canonical --out-dir draft")
    return publication


def display_path(path: Path) -> str:
    """Render a path for diagnostics: repo-relative when inside the repo,
    otherwise absolute — an --out-dir/--raw-dir outside the repository must
    not crash the summary prints."""
    try:
        return path.resolve().relative_to(REPO).as_posix()
    except ValueError:
        return str(path)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out-dir", default=str(DEFAULT_OUT),
                        help=f"output directory (default {DEFAULT_OUT})")
    parser.add_argument("--raw-dir", default=str(DEFAULT_RAW_DIR),
                        help=f"per-crate raw output directory (default {DEFAULT_RAW_DIR}, gitignored)")
    parser.add_argument("--crate", action="append", default=[],
                        help="limit to one or more crates (default: ratchet list); "
                             "requires a non-canonical --out-dir")
    parser.add_argument("--dry-run", action="store_true",
                        help="list crates that would be checked, do not run")
    args = parser.parse_args(argv)

    crates = args.crate or ratchet_crates()
    out_dir = Path(args.out_dir).resolve()
    raw_dir = Path(args.raw_dir).resolve()
    raw_dir.mkdir(parents=True, exist_ok=True)
    check_filtered_out_dir(bool(args.crate), out_dir)

    if args.dry_run:
        for c in crates:
            print(c)
        return 0

    baseline_commit = git_rev_resolve(f"{BASELINE_TAG}^{{commit}}")
    version = tool_version()
    manifest_map = workspace_manifest_map()

    results: list[CrateResult] = []
    for i, crate in enumerate(crates, start=1):
        rel_manifest = manifest_map.get(crate)
        if rel_manifest is None:
            raise SystemExit(f"crate {crate!r} is not a workspace package; "
                             "the ratchet list is stale")
        lookup = manifest_exists_at(rel_manifest, baseline_commit)
        if lookup is BaselineLookup.ERROR:
            raise SystemExit(
                f"git lookup for {rel_manifest!r} at {baseline_commit} failed; "
                "a read failure must not be recorded as baseline absence")
        if lookup is BaselineLookup.ABSENT:
            print(f"[{i}/{len(crates)}] {crate} ... not_applicable (baseline_absent)",
                  flush=True)
            results.append(crate_not_in_baseline(crate, rel_manifest, baseline_commit))
            continue
        print(f"[{i}/{len(crates)}] {crate} ...", flush=True)
        result = run_crate(crate, raw_dir, baseline_commit)
        results.append(result)
        status = result.status
        detail = f" parse_error={result.parse_error!r}" if result.parse_error else ""
        print(f"    exit={result.exit_code} status={status} fail={result.fail_count} "
              f"warn={result.warn_count} pass={result.pass_count} skip={result.skip_count}"
              f"{detail}",
              flush=True)

    if out_dir == DEFAULT_OUT.resolve() and tracked_dirty():
        raise SystemExit(
            "refusing canonical publication: the tracked working tree is dirty, so "
            "HEAD would not identify the analyzed source bytes; commit or stash-free "
            "restore first, or write a draft with --out-dir")

    publication = publication_gate(results, out_dir)

    out_dir.mkdir(parents=True, exist_ok=True)
    head = head_sha()
    tree = head_tree()

    json_path = out_dir / "semver-v0.17.0-vs-main.json"
    json_path.write_text(
        json.dumps(render_json(results, head, baseline_commit, version, tree),
                   indent=2) + "\n",
        encoding="utf-8")
    md_path = out_dir / "semver-v0.17.0-vs-main.md"
    md_path.write_text(render_markdown(results), encoding="utf-8")

    erring = [r for r in results if r.status == "error"]
    failing = sum(1 for r in results if r.status == "fail")
    print(f"\nWrote {display_path(json_path)} "
          f"and {display_path(md_path)}")
    print(f"Per-crate raw output: {display_path(raw_dir)}/")
    print(f"Head SHA at capture: {head} (tree {tree})")
    print(f"Baseline: {BASELINE_TAG} = {baseline_commit}; tool: {version}")
    print(f"{failing} crate(s) have major-level breaks against {BASELINE_TAG}.")
    if erring:
        print(f"FAIL: {len(erring)} ratcheted crate(s) lack a terminal verdict: "
              + ", ".join(r.name for r in erring))
    return publication


if __name__ == "__main__":
    sys.exit(main())
