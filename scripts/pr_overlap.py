#!/usr/bin/env python3
"""
pr_overlap.py — PR overlap detector for the perl-lsp swarm CI trust lane.

REPORT ONLY: this script NEVER closes or auto-mutates PRs.
It detects duplicate/complementary PRs from changed-file lists and semantic
surfaces (touched test files + public symbols). It explicitly does NOT consult
shared base commits or diffstat line counts — those signals are banned.

## Algorithm

Operates on a set of PRs, each with:
  - ``files``: list of changed file paths (required)
  - ``tests``: list of test file paths among changed files (optional)
  - ``symbols``: list of added/modified public symbol names (optional)
  - ``source``: typed changed-file source state (optional; see below)

For each pair (A, B):

  Step 0 — source gate (#15346):
    Only PRs whose changed-file source is ``known`` or ``known_empty`` may
    participate in a definitive overlap calculation. Any pair involving an
    ``unavailable`` source is class ``not_proven`` — never ``isolated``.
    A failed gh invocation must never look like a PR that changes no files.

  Step 1 — file-level Jaccard:
    jaccard_files = |files_A ∩ files_B| / |files_A ∪ files_B|
    If jaccard_files == 0.0 → class ``isolated`` immediately.
    Base commits and diffstat line counts are NOT consulted.

  Step 2 — surface Jaccard (only when jaccard_files > 0):
    jaccard_tests = Jaccard over test-file path sets.
    jaccard_syms  = Jaccard over public symbol name sets.
    Missing optional fields are treated as empty sets → jaccard 0.0.

  Step 3 — classification thresholds:
    isolated:          jaccard_files == 0.0
    likely-duplicate:  jaccard_files >= 0.8 AND jaccard_tests >= 0.5 AND jaccard_syms >= 0.5
    pick-one:          jaccard_files > 0.5 AND (jaccard_tests >= 0.3 OR jaccard_syms >= 0.3)
    sequence-both:     jaccard_files > 0 AND jaccard_tests < 0.3 AND jaccard_syms < 0.3
    (Note: likely-duplicate is checked before pick-one; pick-one before sequence-both.)

## Input format

JSON file (or stdin with ``-``) with the following schema:
  {
    "prs": [
      {
        "id": "<string or number>",     -- PR identifier (e.g. "123" or 123)
        "files": ["path/a.rs", ...],    -- changed files (required)
        "tests": ["tests/foo.rs", ...], -- test files changed (optional)
        "symbols": ["MyStruct::new"]    -- public symbols added/modified (optional)
      },
      ...
    ]
  }

The ``tests`` field defaults to auto-detection from ``files`` when omitted: any
path matching ``tests/**``, ``*_test.rs``, or ``test_*.py`` is treated as a
test file (case-sensitive).

In file/stdin mode the caller supplies ``files`` directly, so each PR is
treated as a ``known`` source (origin ``input``).

## Output

For each pair, one line:
  PR <A> vs PR <B>: <class>  files=<jf:.3f> tests=<jt:.3f> syms=<js:.3f>  — <rationale>

Pairs with an unavailable source print class ``not_proven`` with a structured
refusal (machine-readable via ``--json``: pair ``refusal.reason`` and
``refusal.sources``). Consumers must never infer isolation from a zero-length
file list or a zero exit status alone (#15346).

Exit codes:
  0  complete report (every pair classified from known/known_empty sources,
     and a genuinely empty changed-file set is a valid ``isolated`` result);
  3  --cluster mode only: at least one PR's changed-file source was
     unavailable. The full report (with ``not_proven`` pairs and structured
     refusals) is still printed; the posture line on stderr is
     ``{"posture": "NOT_PROVEN", ...}`` so callers can refuse without
     parsing prose.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from itertools import combinations
from typing import Any

# ---------------------------------------------------------------------------
# Constants — thresholds documented here so they match the module docstring.
# ---------------------------------------------------------------------------

# Minimum file-overlap to consider any surface overlap meaningful.
_THRESHOLD_FILES_PICK = 0.5   # jaccard_files > 0.5 needed for pick-one / likely-dup
_THRESHOLD_FILES_SEQ  = 0.0   # jaccard_files > 0.0 needed for sequence-both

# Surface thresholds
_THRESHOLD_TESTS_SURFACE = 0.3   # jaccard_tests >= 0.3 counts as "same test surface"
_THRESHOLD_SYMS_SURFACE  = 0.3   # jaccard_syms  >= 0.3 counts as "same symbol surface"

# likely-duplicate needs all three high
_THRESHOLD_FILES_DUP  = 0.8
_THRESHOLD_TESTS_DUP  = 0.5
_THRESHOLD_SYMS_DUP   = 0.5

# Source statuses that may participate in a definitive overlap calculation.
_DEFINITIVE_SOURCE_STATUSES = frozenset({"known", "known_empty"})

# Regex patterns for auto-detecting test files.
_TEST_FILE_PATTERNS = [
    re.compile(r"^tests/"),          # tests/** prefix
    re.compile(r"_tests?\.rs$"),     # Rust test suffix (_test.rs or _tests.rs)
    re.compile(r"/tests/"),          # nested tests/ directory
    re.compile(r"^test_.*\.py$"),    # Python test prefix
    re.compile(r"/test_.*\.py$"),    # nested Python test prefix
]

EXIT_OK = 0
EXIT_SOURCE_UNAVAILABLE = 3


# ---------------------------------------------------------------------------
# Jaccard helpers
# ---------------------------------------------------------------------------

def _jaccard(a: set[str], b: set[str]) -> float:
    """Return Jaccard similarity between two sets; 0.0 if both empty."""
    if not a and not b:
        return 0.0
    union = a | b
    if not union:
        return 0.0
    return len(a & b) / len(union)


def _is_test_file(path: str) -> bool:
    """Return True if the path looks like a test file."""
    return any(p.search(path) for p in _TEST_FILE_PATTERNS)


# ---------------------------------------------------------------------------
# PR normalisation
# ---------------------------------------------------------------------------

def _normalise_pr(raw: dict[str, Any]) -> dict[str, Any]:
    """
    Normalise a raw PR dict from the input JSON.

    - ``files`` is required; absence raises ValueError.
    - ``tests`` defaults to auto-detection from files when absent.
    - ``symbols`` defaults to empty list when absent.
    - ``id`` defaults to ``"?"`` when absent.
    - ``source`` defaults to a ``known`` state with origin ``input`` when
      absent (file/stdin mode supplies files authoritatively). Cluster mode
      attaches the typed state produced by ``_discover_pr_files`` (#15346).
    """
    if "files" not in raw:
        raise ValueError(f"PR entry missing required 'files' field: {raw!r}")

    pr_id = str(raw.get("id", "?"))
    files: list[str] = [str(f) for f in raw["files"]]

    if "tests" in raw:
        tests: list[str] = [str(t) for t in raw["tests"]]
    else:
        tests = [f for f in files if _is_test_file(f)]

    symbols: list[str] = [str(s) for s in raw.get("symbols", [])]

    source: dict[str, Any] = raw.get(
        "source", {"status": "known", "origin": "input"}
    )

    return {
        "id": pr_id,
        "files": files,
        "tests": tests,
        "symbols": symbols,
        "source": source,
    }


# ---------------------------------------------------------------------------
# Core classification
# ---------------------------------------------------------------------------

def classify_pair(
    pr_a: dict[str, Any],
    pr_b: dict[str, Any],
) -> dict[str, Any]:
    """
    Classify the overlap between two normalised PR dicts.

    Returns a dict with keys:
      class, jaccard_files, jaccard_tests, jaccard_syms, rationale
    Plus, for ``not_proven`` pairs, a structured ``refusal`` with
    ``reason`` and per-PR ``sources`` — never error prose to be parsed
    downstream (#15346).
    """
    source_a = pr_a.get("source", {"status": "known"})
    source_b = pr_b.get("source", {"status": "known"})
    status_a = str(source_a.get("status", "unknown"))
    status_b = str(source_b.get("status", "unknown"))
    if (
        status_a not in _DEFINITIVE_SOURCE_STATUSES
        or status_b not in _DEFINITIVE_SOURCE_STATUSES
    ):
        code_a = str(source_a.get("code", status_a))
        code_b = str(source_b.get("code", status_b))
        return {
            "class": "not_proven",
            "jaccard_files": None,
            "jaccard_tests": None,
            "jaccard_syms":  None,
            "rationale": (
                f"Changed-file source unavailable (PR {pr_a['id']}: {code_a}; "
                f"PR {pr_b['id']}: {code_b}); overlap/isolation cannot be "
                "proven from incomplete evidence."
            ),
            "refusal": {
                "reason": "pr_changed_file_source_unavailable",
                "sources": {"a": source_a, "b": source_b},
            },
        }

    files_a = set(pr_a["files"])
    files_b = set(pr_b["files"])
    tests_a = set(pr_a["tests"])
    tests_b = set(pr_b["tests"])
    syms_a  = set(pr_a["symbols"])
    syms_b  = set(pr_b["symbols"])

    # Step 1 — file-level Jaccard (base commits and diffstat NOT consulted)
    jf = _jaccard(files_a, files_b)

    if jf == 0.0:
        return {
            "class": "isolated",
            "jaccard_files": jf,
            "jaccard_tests": 0.0,
            "jaccard_syms":  0.0,
            "rationale": "No shared files; PRs touch entirely different parts of the codebase.",
        }

    # Step 2 — surface Jaccard
    jt = _jaccard(tests_a, tests_b)
    js = _jaccard(syms_a,  syms_b)

    # Step 3 — map to class (order: likely-duplicate > pick-one > sequence-both)
    if (
        jf >= _THRESHOLD_FILES_DUP
        and jt >= _THRESHOLD_TESTS_DUP
        and js >= _THRESHOLD_SYMS_DUP
    ):
        cls = "likely-duplicate"
        rationale = (
            f"High file overlap (jf={jf:.3f}), same test surface (jt={jt:.3f}), "
            f"same symbols (js={js:.3f}); likely redundant — review both before merging."
        )
    elif jf > _THRESHOLD_FILES_PICK and (
        jt >= _THRESHOLD_TESTS_SURFACE or js >= _THRESHOLD_SYMS_SURFACE
    ):
        cls = "pick-one"
        rationale = (
            f"Significant file overlap (jf={jf:.3f}) with overlapping surfaces "
            f"(jt={jt:.3f}, js={js:.3f}); only one should land without coordination."
        )
    else:
        cls = "sequence-both"
        rationale = (
            f"Shared files (jf={jf:.3f}) but different test/symbol surfaces "
            f"(jt={jt:.3f}, js={js:.3f}); both can land if sequenced carefully."
        )

    return {
        "class": cls,
        "jaccard_files": jf,
        "jaccard_tests": jt,
        "jaccard_syms":  js,
        "rationale": rationale,
    }


# ---------------------------------------------------------------------------
# Report generation
# ---------------------------------------------------------------------------

def generate_report(prs: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """
    Generate a list of pair-result dicts for all unique PR pairs.

    Each result dict contains: id_a, id_b, class, jaccard_files,
    jaccard_tests, jaccard_syms, rationale. Pairs involving an unavailable
    changed-file source carry class ``not_proven`` and a structured
    ``refusal`` instead of jaccard numbers (#15346).
    """
    normalised = [_normalise_pr(pr) for pr in prs]
    results = []
    for pr_a, pr_b in combinations(normalised, 2):
        pair_result = classify_pair(pr_a, pr_b)
        results.append({
            "id_a": pr_a["id"],
            "id_b": pr_b["id"],
            **pair_result,
        })
    return results


def format_result(r: dict[str, Any]) -> str:
    """Format a single pair result as a human-readable line."""
    if r.get("class") == "not_proven":
        sources = r["refusal"]["sources"]
        codes = "; ".join(
            f"PR {r[f'id_{side}']}: "
            f"{str(sources[side].get('code', sources[side].get('status', 'unknown')))}"
            for side in ("a", "b")
        )
        return (
            f"PR {r['id_a']} vs PR {r['id_b']}: not_proven  "
            f"— {r['rationale']}  refusal={r['refusal']['reason']} ({codes})"
        )
    return (
        f"PR {r['id_a']} vs PR {r['id_b']}: {r['class']}  "
        f"files={r['jaccard_files']:.3f} tests={r['jaccard_tests']:.3f} "
        f"syms={r['jaccard_syms']:.3f}  — {r['rationale']}"
    )


# ---------------------------------------------------------------------------
# GitHub --cluster mode (nice-to-have; requires gh CLI)
# ---------------------------------------------------------------------------

def _discover_pr_files(pr_number: str | int) -> dict[str, Any]:
    """
    Typed changed-file source for one PR (#15346).

    Returns exactly one of:

      {"status": "known", "files": [...], "count": N, "digest": "<sha256>"}
        a non-empty changed-file list from a successful ``gh pr view``
        (exit 0);
      {"status": "known_empty", "files": [], "count": 0, "digest": "<sha256>"}
        a genuinely empty changed-file list from a successful ``gh pr view``
        (exit 0) — the PR really changes no files;
      {"status": "unavailable", "code", "detail", "command"}
        an instrument failure: unspawnable/missing ``gh``, auth failure,
        rate limit, unknown PR, malformed JSON, non-array response, or any
        non-zero ``gh`` exit. Malformed payloads make ``gh --jq`` exit
        non-zero, so they surface here rather than as an empty list.

    A true empty changed-file set and an unavailable source are opposite
    facts. Discovery must never map a failed gh invocation to an empty file
    list: only ``known``/``known_empty`` sources may participate in a
    definitive overlap calculation (#15346).
    """
    command = [
        "gh", "pr", "view", str(pr_number),
        "--json", "files", "--jq", ".files[].path",
    ]
    try:
        proc = subprocess.run(command, capture_output=True, text=True)
    except OSError as exc:
        return {
            "status": "unavailable",
            "code": "gh-unspawnable",
            "detail": f"cannot run gh: {exc}",
            "command": command,
        }
    if proc.returncode != 0:
        stderr = (proc.stderr or "").strip()
        lowered = stderr.lower()
        if "rate limit" in lowered:
            code = "gh-rate-limit"
        elif "401" in lowered or "403" in lowered or "auth" in lowered:
            code = "gh-auth"
        elif "not found" in lowered:
            code = "gh-pr-not-found"
        else:
            code = f"gh-pr-view-exit-{proc.returncode}"
        return {
            "status": "unavailable",
            "code": code,
            "detail": stderr or f"gh pr view exited {proc.returncode} with no stderr",
            "command": command,
        }
    files = [line.strip() for line in proc.stdout.splitlines() if line.strip()]
    digest = hashlib.sha256("\n".join(sorted(files)).encode("utf-8")).hexdigest()
    if files:
        return {"status": "known", "files": files, "count": len(files), "digest": digest}
    return {"status": "known_empty", "files": [], "count": 0, "digest": digest}


def build_prs_from_cluster(pr_numbers: list[str]) -> list[dict[str, Any]]:
    """
    Build a PR list by fetching changed files from GitHub for each PR number.

    Each dict carries a typed ``source`` state from ``_discover_pr_files``.
    PRs whose source is ``unavailable`` keep an empty file list but are
    excluded from definitive overlap conclusions by ``classify_pair``
    (#15346). Symbols are not fetched (unavailable from ``gh pr view``);
    tests are auto-detected.
    """
    prs = []
    for num in pr_numbers:
        source = _discover_pr_files(num)
        prs.append({
            "id": num,
            "files": [str(f) for f in source.get("files", [])],
            "source": source,
        })
    return prs


# ---------------------------------------------------------------------------
# CLI entry point
# ---------------------------------------------------------------------------

def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="pr_overlap.py",
        description="Detect duplicate/complementary PRs. REPORT ONLY — never closes PRs.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument(
        "input",
        nargs="?",
        default=None,
        help="Path to JSON input file, or '-' to read from stdin.",
    )
    parser.add_argument(
        "--cluster",
        nargs="+",
        metavar="PR_NUMBER",
        help="Fetch changed files from GitHub for the given PR numbers (requires gh CLI).",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        dest="output_json",
        help="Output results as JSON array instead of human-readable lines.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)

    source_failure = False
    if args.cluster:
        prs = build_prs_from_cluster(args.cluster)
        source_failure = any(
            str(p.get("source", {}).get("status")) == "unavailable"
            for p in prs
        )
    elif args.input is None or args.input == "-":
        raw = json.load(sys.stdin)
        prs = raw.get("prs", [])
    else:
        with open(args.input, encoding="utf-8") as fh:
            raw = json.load(fh)
        prs = raw.get("prs", [])

    if len(prs) < 2:
        print("No pairs to compare (need at least 2 PRs).", file=sys.stderr)
        return EXIT_OK

    results = generate_report(prs)

    if args.output_json:
        print(json.dumps(results, indent=2))
    else:
        for r in results:
            print(format_result(r))

    if source_failure:
        unavailable = sorted(
            str(p["id"]) for p in prs
            if str(p.get("source", {}).get("status")) == "unavailable"
        )
        print(
            json.dumps({
                "posture": "NOT_PROVEN",
                "reason": "pr_changed_file_source_unavailable",
                "unavailable_prs": unavailable,
            }),
            file=sys.stderr,
        )
        return EXIT_SOURCE_UNAVAILABLE

    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main())
