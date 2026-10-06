#!/usr/bin/env python3
"""Graded agentic task-completion battery runner.

Reads the machine-readable task specs in ``tasks/*.toml``, executes every
rubric step marked ``mode = "auto"``, and prints a per-task score table.

Steps marked ``mode = "manual"`` are NEVER auto-scored: they print as MANUAL
with their exact verification command. Tasks that need live GitHub/CI or
grader-supplied instances degrade to explicitly-marked MANUAL / NEEDS-PARAM
entries rather than faked automation.

Usage (from anywhere; repo root is auto-detected)::

    python3 tests/agentic-task-battery/run-battery.py --check
    python3 tests/agentic-task-battery/run-battery.py --task task-02-focused-test
    python3 tests/agentic-task-battery/run-battery.py --param change-type=rust-prod --results-out /tmp/results.toml

Exit codes: 0 = measurement complete (even when steps fail — this is a
measurement tool, failures are data); 1 = runner error; 2 = spec validation
failure. Never truncates a live child pipe: all child output is captured with
``communicate()`` and only then inspected.
"""

from __future__ import annotations

import argparse
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time

try:
    import tomllib
except ImportError:  # pragma: no cover - Python < 3.11
    print("run-battery.py requires Python 3.11+ (stdlib tomllib).", file=sys.stderr)
    sys.exit(1)

SCHEMA_VERSION = "agentic_task_battery.v1"
BATTERY_DIR = os.path.dirname(os.path.abspath(__file__))
TASKS_DIR = os.path.join(BATTERY_DIR, "tasks")
DEFAULT_REPO = os.path.normpath(os.path.join(BATTERY_DIR, "..", ".."))

# check key -> (task_id, human label)
KNOWN_CHECKS = {
    "toolchain", "clean_package", "admitted_attempt", "locked_build",
    "fresh_binary", "wall_time",
    "test_scope", "test_locked", "test_redirect", "test_totals",
    "test_exit_wall",
    "quick_bench", "baseline_named", "verdict_quoted",
    "bisect_clean", "bisect_run", "bisect_reset",
    "gate_select", "gate_order", "gate_exits", "proof_not_run",
    "no_workspace",
    "owning_crate", "owner_doc",
}


class StepResult:
    def __init__(self, step_id, description, points, mode):
        self.step_id = step_id
        self.description = description
        self.points = points
        self.mode = mode
        self.earned = 0
        self.status = "PENDING"  # PASS | FAIL | MANUAL | NEEDS-PARAM | SKIP
        self.evidence = ""
        self.verify = ""
        self.wall_s = 0.0


class TaskResult:
    def __init__(self, spec):
        self.spec = spec
        self.steps = []
        self.wall_s = 0.0
        self.status = "PENDING"  # PASS | BELOW_THRESHOLD | MANUAL | NOT_RUN


def load_specs():
    """Load and schema-validate every task spec. Returns (specs, errors)."""
    specs = []
    errors = []
    files = sorted(f for f in os.listdir(TASKS_DIR) if f.endswith(".toml"))
    if not files:
        errors.append(f"no task specs found in {TASKS_DIR}")
    for fname in files:
        path = os.path.join(TASKS_DIR, fname)
        try:
            with open(path, "rb") as fh:
                spec = tomllib.load(fh)
        except Exception as exc:  # noqa: BLE001 - report any parse failure
            errors.append(f"{fname}: TOML parse error: {exc}")
            continue
        spec["_source"] = fname
        for key in ("schema_version", "task_id", "title", "goal", "instruction",
                    "max_duration", "pass_threshold", "max_points", "automation"):
            if key not in spec:
                errors.append(f"{fname}: missing required key '{key}'")
        if spec.get("schema_version") != SCHEMA_VERSION:
            errors.append(f"{fname}: schema_version must be {SCHEMA_VERSION!r}")
        if spec.get("automation") not in ("full", "partial", "manual-only"):
            errors.append(f"{fname}: automation must be full|partial|manual-only")
        rubric = spec.get("rubric", [])
        if not rubric:
            errors.append(f"{fname}: empty rubric")
        total = 0
        seen = set()
        for i, step in enumerate(rubric):
            sid = step.get("step_id", f"#{i}")
            if sid in seen:
                errors.append(f"{fname}: duplicate step_id {sid}")
            seen.add(sid)
            for key in ("step_id", "description", "points", "mode"):
                if key not in step:
                    errors.append(f"{fname}/{sid}: missing required key '{key}'")
            pts = step.get("points", 0)
            if not isinstance(pts, int) or pts < 0:
                errors.append(f"{fname}/{sid}: points must be a non-negative int")
            else:
                total += pts
            mode = step.get("mode")
            if mode == "auto":
                if step.get("check") not in KNOWN_CHECKS:
                    errors.append(f"{fname}/{sid}: unknown check {step.get('check')!r}")
            elif mode == "manual":
                if not step.get("verify"):
                    errors.append(f"{fname}/{sid}: manual step needs exact 'verify' command")
                if not step.get("evidence"):
                    errors.append(f"{fname}/{sid}: manual step needs 'evidence' description")
            else:
                errors.append(f"{fname}/{sid}: mode must be auto|manual")
        if total != spec.get("max_points"):
            errors.append(f"{fname}: rubric sums to {total}, max_points is {spec.get('max_points')}")
        if not (0 < spec.get("pass_threshold", 0) <= spec.get("max_points", 0)):
            errors.append(f"{fname}: pass_threshold out of range")
        specs.append(spec)
    ids = [s["task_id"] for s in specs]
    if len(ids) != len(set(ids)):
        errors.append("duplicate task_id across specs")
    return specs, errors


def run_child(argv, cwd, timeout_s, log_path=None):
    """Run a child to completion, capturing ALL output (never a live-pipe cut).

    Returns dict(exit=int|None, out=str, wall_s=float, timed_out=bool).
    A log file is written only after the child completes.
    """
    start = time.time()
    try:
        proc = subprocess.run(
            argv, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            timeout=timeout_s, check=False, text=True, errors="replace",
        )
        out, exit_code, timed_out = proc.stdout, proc.returncode, False
    except subprocess.TimeoutExpired as exc:
        # On timeout the partial output may be bytes even in text mode;
        # decode it so a timed-out bench/bisect keeps its evidence.
        raw = exc.stdout or ""
        out = raw.decode("utf-8", "replace") if isinstance(raw, bytes) else raw
        exit_code, timed_out = None, True
    wall = time.time() - start
    if log_path:
        with open(log_path, "w", encoding="utf-8", errors="replace") as fh:
            fh.write(f"$ {' '.join(argv)}\n(exit={exit_code} wall={wall:.1f}s cwd={cwd})\n\n")
            fh.write(out)
    return {"exit": exit_code, "out": out, "wall_s": wall,
            "timed_out": timed_out, "log": log_path or ""}


def tail(text, n=8):
    lines = text.strip().splitlines()
    return "\n".join(lines[-n:]) if lines else "(no output)"


ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


def ansi_clean(text):
    return ANSI_RE.sub("", text)


def find_git_bash():
    """Locate Git Bash for `just` recipes on Windows (CONTRIBUTING rule)."""
    if os.name != "nt":
        return None
    for cand in (r"C:\Program Files\Git\bin\bash.exe",
                 r"C:\Program Files\Git\usr\bin\bash.exe"):
        if os.path.isfile(cand):
            return cand
    return shutil.which("bash")


def run_just(recipe_argv, repo, timeout_s, log_path):
    """Run a `just` recipe via Git Bash on Windows, directly elsewhere.

    Returns (result_dict, route_note).
    """
    bash = find_git_bash() if os.name == "nt" else None
    if bash:
        # Single -lc string; quote every part (the <pkg> slot comes from a
        # grader --param, so a naive join would be a command injection).
        cmd = "just " + " ".join(shlex.quote(p) for p in recipe_argv)
        res = run_child([bash, "-lc", cmd], cwd=repo, timeout_s=timeout_s,
                        log_path=log_path)
        return res, f"via Git Bash ({bash})"
    just = shutil.which("just")
    if not just:
        return {"exit": None, "out": "", "wall_s": 0.0, "timed_out": False,
                "log": ""}, "no `just` on PATH and no Git Bash found"
    res = run_child([just] + recipe_argv, cwd=repo, timeout_s=timeout_s,
                    log_path=log_path)
    return res, "direct `just`"


class Ctx:
    """Per-task-run shared context: repo, params, argv log, temp logs."""

    def __init__(self, repo, params, tmpdir):
        self.repo = repo
        self.params = params
        self.tmpdir = tmpdir
        self.argv_log = []  # every command argv executed, for scope/order checks
        self.notes = []

    def log_path(self, name):
        safe = re.sub(r"[^A-Za-z0-9_.-]", "_", name)
        return os.path.join(self.tmpdir, safe + ".log")

    def run(self, argv, timeout_s, name):
        self.argv_log.append(list(argv))
        return run_child(argv, self.repo, timeout_s, self.log_path(name))


# --------------------------------------------------------------------------
# Task 1: clean build
# --------------------------------------------------------------------------

def check_toolchain(ctx, res, state):
    r = ctx.run(["cargo", "--version"], 120, "t1-cargo-version")
    m = re.search(r"cargo (\d+\.\d+\.\d+)", r["out"])
    have = m.group(1) if m else "unparsed"
    pinned = "unparsed"
    try:
        with open(os.path.join(ctx.repo, "rust-toolchain.toml"), "rb") as fh:
            pinned = tomllib.load(fh).get("toolchain", {}).get("channel", "unparsed")
    except Exception as exc:  # noqa: BLE001 - evidence, not fatal
        res.evidence = f"rust-toolchain.toml unreadable: {exc}"
        res.status = "FAIL"
        return
    ok = (have == pinned)
    res.status = "PASS" if ok else "FAIL"
    res.earned = res.points if ok else 0
    res.evidence = f"cargo {have} vs rust-toolchain.toml channel {pinned}"


def check_clean_package(ctx, res, state):
    argv = ["cargo", "clean", "-p", "perllsp"]
    assert argv[1] == "clean" and "-p" in argv, "must clean only the target package"
    r = ctx.run(argv, 600, "t1-clean")
    ok = r["exit"] == 0
    removed = ""
    m = re.search(r"removed ([\d,]+ files?, [\d.]+ ?\w+)", r["out"])
    if m:
        removed = f"; {m.group(0)}"
    res.status = "PASS" if ok else "FAIL"
    res.earned = res.points if ok else 0
    res.evidence = f"`{' '.join(argv)}` exit={r['exit']}{removed}; wall={r['wall_s']:.1f}s"


def check_admitted_attempt(ctx, res, state):
    argv = [sys.executable, "scripts/cargo_admitted.py", "build",
           "-p", "perllsp", "--locked"]
    r = ctx.run(argv, 1200, "t1-admitted")
    denied = r["exit"] != 0 and "DENY" in r["out"]
    state["admitted_exit"] = r["exit"]
    state["admitted_denied"] = denied
    state["admitted_wall"] = r["wall_s"]
    state["admitted_out"] = r["out"]
    # The graded behavior is attempting the admitted route first; a denial is
    # environmental and honestly recorded, not a failure of the attempt.
    res.status = "PASS"
    res.earned = res.points
    if r["exit"] == 0:
        state["built_via"] = "admitted"
        res.evidence = f"admitted build exit=0 wall={r['wall_s']:.1f}s"
    elif denied:
        state["built_via"] = "direct-after-deny"
        deny_line = next((ln for ln in r["out"].splitlines() if "DENY" in ln),
                         "DENY (unparsed)")
        res.evidence = (f"admitted route DENIED ({deny_line.strip()}); "
                        f"falling back to direct cargo")
    else:
        state["built_via"] = "direct-after-error"
        res.evidence = (f"admitted route errored (exit={r['exit']}); falling back "
                        f"to direct cargo. Tail: {tail(r['out'], 3)}")


def check_locked_build(ctx, res, state):
    if state.get("built_via") == "admitted":
        res.status = "PASS"
        res.earned = res.points
        res.evidence = ("already built via admitted route with --locked "
                        f"(wall={state['admitted_wall']:.1f}s)")
        state["build_exit"] = 0
        return
    if state.get("built_via") != "direct-after-deny":
        # The task allows direct cargo only after a RECORDED DENIAL. An
        # admitted-route error (crash, timeout, missing script) is not a
        # denial: running direct cargo anyway would award locked-build and
        # freshness points for an unpermitted build.
        state["build_exit"] = state.get("admitted_exit", 1)
        res.status = "FAIL"
        res.evidence = ("no recorded admission denial "
                        f"(built_via={state.get('built_via')}); "
                        "direct cargo fallback is not allowed")
        return
    argv = ["cargo", "build", "--locked", "-p", "perllsp"]
    assert "--locked" in argv
    r = ctx.run(argv, 1200, "t1-build")
    state["build_exit"] = r["exit"]
    state["build_wall"] = r["wall_s"]
    state["build_start"] = time.time() - r["wall_s"]
    ok = r["exit"] == 0
    res.status = "PASS" if ok else "FAIL"
    res.earned = res.points if ok else 0
    res.evidence = (f"`{' '.join(argv)}` exit={r['exit']} "
                    f"wall={r['wall_s']:.1f}s")


def check_fresh_binary(ctx, res, state):
    if state.get("built_via") == "admitted":
        res.status = "PASS"
        res.earned = res.points
        res.evidence = ("admitted build exit=0; binary lives in the admission-routed "
                        "target dir, not target/ (no local timestamp claimed)")
        return
    if state.get("build_exit") != 0:
        res.status = "FAIL"
        res.evidence = "build did not exit 0; no freshness claim"
        return
    exe = "perllsp.exe" if os.name == "nt" else "perllsp"
    path = os.path.join(ctx.repo, "target", "debug", exe)
    if not os.path.isfile(path):
        res.status = "FAIL"
        res.evidence = f"{path} missing after exit-0 build"
        return
    age = time.time() - os.path.getmtime(path)
    fresh = age < (state.get("build_wall", 0) + 300)
    res.status = "PASS" if fresh else "FAIL"
    res.earned = res.points if fresh else 0
    res.evidence = (f"{path} mtime {age:.0f}s ago "
                    f"(build wall {state.get('build_wall', 0):.1f}s)")


def check_wall_time(ctx, res, state):
    total = state.get("task_wall", 0.0)
    res.status = "PASS"
    res.earned = res.points
    res.evidence = f"task wall time {total:.1f}s"


# --------------------------------------------------------------------------
# Task 2: focused test run
# --------------------------------------------------------------------------

TEST_RESULT_RE = re.compile(
    r"test result: (\w+)\.\s+(\d+) passed;\s+(\d+) failed", re.IGNORECASE)


def check_test_scope(ctx, res, state):
    argv = ["cargo", "test", "--locked", "-p", "perl-token"]
    state["test_argv"] = argv
    ok = "-p" in argv and "perl-token" in argv and "--workspace" not in argv
    res.status = "PASS" if ok else "FAIL"
    res.earned = res.points if ok else 0
    res.evidence = f"scope argv: {' '.join(argv)}"


def check_test_locked(ctx, res, state):
    ok = "--locked" in state["test_argv"]
    res.status = "PASS" if ok else "FAIL"
    res.earned = res.points if ok else 0
    res.evidence = "--locked present in test argv" if ok else "--locked MISSING"


def check_test_redirect(ctx, res, state):
    r = ctx.run(state["test_argv"], 1200, "t2-test")
    state["test_exit"] = r["exit"]
    state["test_wall"] = r["wall_s"]
    state["test_out"] = r["out"]
    state["test_log"] = r["log"]
    size = os.path.getsize(r["log"]) if r["log"] else 0
    res.status = "PASS"
    res.earned = res.points
    res.evidence = (f"full output captured via communicate() + temp log "
                    f"({size} bytes, no live-pipe truncation)")


def check_test_totals(ctx, res, state):
    passed = failed = bins = 0
    for m in TEST_RESULT_RE.finditer(state.get("test_out", "")):
        bins += 1
        passed += int(m.group(2))
        failed += int(m.group(3))
    state["test_passed"] = passed
    state["test_failed"] = failed
    if bins == 0:
        res.status = "FAIL"
        res.evidence = "no `test result:` lines parsed from log"
        return
    ok = failed == 0 and state.get("test_exit") == 0
    res.status = "PASS" if ok else "FAIL"
    res.earned = res.points if ok else 0
    res.evidence = (f"{passed} passed / {failed} failed across {bins} binaries "
                    f"(exit={state.get('test_exit')})")
    if not ok:
        fails = [ln for ln in state["test_out"].splitlines()
                 if "FAILED" in ln or "failures:" in ln][:5]
        res.evidence += "; e.g. " + " | ".join(fails)


def check_test_exit_wall(ctx, res, state):
    ok = state.get("test_exit") == 0
    res.status = "PASS" if ok else "FAIL"
    res.earned = res.points if ok else 0
    res.evidence = (f"exit={state.get('test_exit')} "
                    f"wall={state.get('test_wall', 0):.1f}s")


# --------------------------------------------------------------------------
# Task 3: bench + baseline
# --------------------------------------------------------------------------

BASELINE_RE = re.compile(r"benchmarks[\\/]baselines[\\/][\w\-.]+\.json")
VERDICT_RE = re.compile(r"WARNING|REGRESSION|CRITICAL|no regression|"
                        r"regressions?:\s*\d+|baseline", re.IGNORECASE)


def _tracked_status(ctx):
    r = run_child(["git", "status", "--porcelain", "--untracked-files=no"],
                  ctx.repo, 120)
    return r["out"]


SCORECARD_PATH = "docs/project/status/parser_performance_scorecard.json"


def _path_dirty(tracked_porcelain, rel_posix_path):
    """True when the porcelain status shows rel_posix_path as dirty.

    Compares full status rows (handles `R  old -> new` renames and simple
    C-quoting); the scorecard path has no special characters.
    """
    for line in tracked_porcelain.splitlines():
        if len(line) <= 3:
            continue
        paths = [p.strip().strip('"') for p in line[3:].split(" -> ")]
        if rel_posix_path in paths:
            return True
    return False


def _restore_bench_side_effects(ctx, before):
    """Restore tracked files the bench run rewrote (e.g. the generated
    parser scorecard). Only touches files that were clean before the run
    and dirty after it; returns the restored path list for the evidence."""
    after = _tracked_status(ctx)
    before_set = set(before.splitlines())
    after_set = set(after.splitlines())
    dirtied = sorted(a[3:] for a in (after_set - before_set)
                     if len(a) > 3 and not a.startswith("??"))
    restored = []
    for path in dirtied:
        r = run_child(["git", "checkout", "--", path], ctx.repo, 120)
        if r["exit"] == 0:
            restored.append(path)
    return restored, dirtied


def check_quick_bench(ctx, res, state):
    # Canonical `just bench-quick` path: bench-compare reads this exact file
    # and benchmarks/results/ is gitignored, so the tree stays clean.
    out_path = os.path.join(ctx.repo, "benchmarks", "results", "latest.json")
    # Direct-cargo fallback per the task instruction: `just` recipes are bash
    # and unusable from a bare PowerShell/Windows-Python spawn.
    argv = ["cargo", "xtask", "bench-run", "--quick", "--output", out_path]
    tracked_before = _tracked_status(ctx)
    if _path_dirty(tracked_before, SCORECARD_PATH):
        # The benchmark rewrites the tracked scorecard in place. With
        # uncommitted changes already present, running would either clobber
        # them (dirty-before is never restored) or lose a deletion (a ` D`
        # row rewritten to ` M` would be "restored" by checkout). Refuse.
        res.status = "FAIL"
        res.earned = 0
        res.evidence = (f"{SCORECARD_PATH} is dirty before the run; bench "
                        "refused (would overwrite uncommitted changes)")
        state["bench_exit"] = None
        state["bench_wall"] = 0.0
        state["bench_out"] = ""
        state["results_path"] = ""
        return
    r = ctx.run(argv, 1200, "t3-bench-run")
    restored, dirtied = _restore_bench_side_effects(ctx, tracked_before)
    state["bench_restored"] = restored
    state["bench_exit"] = r["exit"]
    state["bench_wall"] = r["wall_s"]
    state["bench_out"] = r["out"]
    wrote = os.path.isfile(out_path)
    state["results_path"] = out_path if wrote else ""
    ok = r["exit"] == 0 and wrote
    res.status = "PASS" if ok else "FAIL"
    res.earned = res.points if ok else 0
    fail_detail = ""
    m = re.search(r"benchmark categories failed: (.+)", r["out"])
    if m:
        fail_detail = f"; categories failed: {m.group(1).strip()}"
    res.evidence = (f"`{' '.join(argv)}` exit={r['exit']} wall={r['wall_s']:.1f}s; "
                    f"results {'written' if wrote else 'MISSING'}{fail_detail}")
    if r["exit"] != 0 and wrote:
        # A partial run still leaves comparable results; later steps proceed
        # against them while this step stays FAIL. The failure is data.
        res.evidence += " (partial results still comparable below)"
    if restored:
        res.evidence += (f"; bench rewrote tracked file(s) {restored}, "
                         f"restored via `git checkout --` (tree kept clean)")
    unrestored = [p for p in dirtied if p not in restored]
    if unrestored:
        res.evidence += f"; WARNING: unrestored tracked modifications: {unrestored}"


def check_baseline_named(ctx, res, state):
    if not state.get("results_path"):
        res.status = "FAIL"
        res.evidence = "quick bench produced no results file; no comparison run"
        return
    r = ctx.run(["cargo", "xtask", "bench-compare"], 600, "t3-bench-compare")
    state["compare_exit"] = r["exit"]
    state["compare_out"] = r["out"]
    # Compare prints `Baseline: <version> (<sha>)`; resolve to the file.
    m = re.search(r"Baseline:\s+(\S+)", ansi_clean(r["out"]))
    fname = None
    if m:
        cand = os.path.join(ctx.repo, "benchmarks", "baselines", m.group(1) + ".json")
        if os.path.isfile(cand):
            fname = os.path.relpath(cand, ctx.repo)
    if fname is None:
        # Fallback: a literal baselines path in the output.
        m2 = BASELINE_RE.search(r["out"])
        fname = m2.group(0) if m2 else None
    if fname:
        state["baseline_file"] = fname
        res.status = "PASS"
        res.earned = res.points
        res.evidence = f"compared against {fname} (exit={r['exit']})"
    else:
        res.status = "FAIL"
        res.evidence = (f"no baseline resolved from compare output "
                        f"(exit={r['exit']}); possible empty comparison. "
                        f"Tail: {tail(r['out'], 6)}")


def check_verdict_quoted(ctx, res, state):
    out = state.get("compare_out", "")
    if not out:
        res.status = "FAIL"
        res.evidence = "no compare output captured"
        return
    # Scope to the comparison report: cargo build warnings above it also
    # mention "baseline" and would pollute the verdict set.
    cut = out.find("Benchmark Comparison")
    if cut < 0:
        cut = out.find("bench-compare")
    report = out[cut:] if cut >= 0 else out
    verdicts = sorted(set(ansi_clean(ln).strip() for ln in report.splitlines()
                          if ansi_clean(ln).strip()
                          and VERDICT_RE.search(ansi_clean(ln))))
    state["verdict_lines"] = verdicts
    if verdicts:
        res.status = "PASS"
        res.earned = res.points
        res.evidence = "verdict lines: " + " | ".join(verdicts[:6])
    else:
        res.status = "FAIL"
        res.evidence = ("no WARNING/REGRESSION/CRITICAL/baseline verdict line found; "
                        f"tail: {tail(out, 6)}")


# --------------------------------------------------------------------------
# Task 4: bisect
# --------------------------------------------------------------------------

def check_bisect_clean(ctx, res, state):
    r = ctx.run(["git", "status", "--porcelain"], 120, "t4-status")
    head = ctx.run(["git", "rev-parse", "HEAD"], 60, "t4-head")
    branch = ctx.run(["git", "branch", "--show-current"], 60, "t4-branch")
    state["orig_head"] = head["out"].strip()
    state["orig_branch"] = branch["out"].strip()
    clean = r["exit"] == 0 and r["out"].strip() == ""
    state["tree_clean"] = clean
    res.status = "PASS" if clean else "FAIL"
    res.earned = res.points if clean else 0
    res.evidence = (f"tree {'clean' if clean else 'DIRTY'}; HEAD={state['orig_head'][:12]} "
                    f"branch={state['orig_branch'] or '(detached)'}; no stash used")


def _bisect_test_argv(ctx):
    failing = ctx.params.get("failing-test", "")
    parts = failing.split()
    if len(parts) != 2:
        return None
    pkg, name = parts
    return ["cargo", "test", "--locked", "-p", pkg, name]


def check_bisect_run(ctx, res, state):
    if not state.get("tree_clean"):
        res.status = "SKIP"
        res.evidence = "tree not clean; bisect refused (would endanger uncommitted work)"
        return
    argv = _bisect_test_argv(ctx)
    good = ctx.params.get("good-sha", "")
    if argv is None or not good:
        res.status = "NEEDS-PARAM"
        res.evidence = ("needs grader params: --param failing-test='<pkg> <test-name>' "
                        "--param good-sha=<sha>")
        return
    state["bisect_argv"] = argv
    start = ctx.run(["git", "bisect", "start", "HEAD", good, "--"], 120, "t4-bisect-start")
    if start["exit"] != 0:
        res.status = "FAIL"
        res.evidence = f"bisect start failed: {tail(start['out'], 4)}"
        return
    try:
        r = ctx.run(["git", "bisect", "run"] + argv, 2400, "t4-bisect-run")
        state["bisect_out"] = r["out"]
        m = re.search(r"^([0-9a-f]{40}) is the first bad commit",
                      r["out"], re.MULTILINE)
        if m:
            state["first_bad"] = m.group(1)
            res.status = "PASS"
            res.earned = res.points
            res.evidence = (f"single-test bisect cmd: {' '.join(argv)}; "
                            f"first bad: {m.group(1)[:12]}")
        else:
            res.status = "FAIL"
            res.evidence = (f"bisect run exit={r['exit']} but no 'first bad commit' "
                            f"verdict parsed; tail: {tail(r['out'], 6)}")
    finally:
        reset = ctx.run(["git", "bisect", "reset"], 120, "t4-bisect-reset")
        state["bisect_reset_exit"] = reset["exit"]


def check_bisect_reset(ctx, res, state):
    if state.get("bisect_reset_exit") is None:
        # No bisect was started (e.g. NEEDS-PARAM): verify state untouched.
        head = ctx.run(["git", "rev-parse", "HEAD"], 60, "t4-head-after")
        same = head["out"].strip() == state.get("orig_head")
        res.status = "PASS" if same else "FAIL"
        res.earned = res.points if same else 0
        res.evidence = (f"no bisect started; HEAD {'unchanged' if same else 'CHANGED'}")
        return
    head = ctx.run(["git", "rev-parse", "HEAD"], 60, "t4-head-after")
    status = ctx.run(["git", "status", "--porcelain"], 60, "t4-status-after")
    restored = (state["bisect_reset_exit"] == 0
                and head["out"].strip() == state.get("orig_head")
                and status["out"].strip() == "")
    res.status = "PASS" if restored else "FAIL"
    res.earned = res.points if restored else 0
    res.evidence = (f"bisect reset exit={state['bisect_reset_exit']}; "
                    f"HEAD restored={head['out'].strip() == state.get('orig_head')}; "
                    f"tree clean={status['out'].strip() == ''}")


# --------------------------------------------------------------------------
# Task 6: hygiene gate
# --------------------------------------------------------------------------

def _gate_list(spec, change_type, pkg):
    table = spec.get("gate_table", {})
    if change_type not in table:
        return None
    return [g.replace("<pkg>", pkg) for g in table[change_type]]


def check_gate_select(ctx, res, state, spec):
    ctype = ctx.params.get("change-type", "rust-prod")
    pkg = ctx.params.get("pkg", "perl-token")
    gates = _gate_list(spec, ctype, pkg)
    if gates is None:
        res.status = "FAIL"
        res.evidence = (f"unknown change-type {ctype!r}; known: "
                        f"{sorted(spec.get('gate_table', {}))}")
        return
    state["gates"] = gates
    state["change_type"] = ctype
    state["pkg"] = pkg
    res.status = "PASS"
    res.earned = res.points
    res.evidence = f"change-type={ctype} pkg={pkg}; gates={gates}"


def _run_gate(ctx, gate, idx):
    parts = gate.split()
    name = f"t6-gate{idx}-{parts[0]}"
    if parts[0] == "just":
        ctx.argv_log.append(parts)
        res, route = run_just(parts[1:], ctx.repo, 1200, ctx.log_path(name))
        res["route"] = route
        return res
    return ctx.run(parts, 1200, name)


def check_gate_order(ctx, res, state):
    gates = state.get("gates", [])
    results = []
    for i, gate in enumerate(gates):
        r = _run_gate(ctx, gate, i)
        results.append({"cmd": gate, "exit": r["exit"],
                        "wall_s": round(r["wall_s"], 1),
                        "route": r.get("route", "direct")})
        if r["exit"] != 0:
            break  # cheapest-first stops at the first falsifier
    state["gate_results"] = results
    ran = [g["cmd"] for g in results]
    res.status = "PASS"  # executed in spec-table order by construction
    res.earned = res.points
    res.evidence = f"executed cheapest-first: {ran}"


def check_gate_exits(ctx, res, state):
    results = state.get("gate_results", [])
    if not results:
        res.status = "FAIL"
        res.evidence = "no gates executed"
        return
    summary = "; ".join(f"{g['cmd']} -> exit={g['exit']} ({g['wall_s']}s)"
                        for g in results)
    # The graded behavior is complete reporting; greenness is recorded, not
    # conflated: a failing gate is data, and the evidence names it.
    res.status = "PASS"
    res.earned = res.points
    res.evidence = summary


def check_proof_not_run(ctx, res, state):
    skipped = ["workspace-wide proof (--workspace clippy/test)",
               "write-side recipes (status-update, public-api-update, bench-baseline)",
               "CI re-runs / hosted gates"]
    res.status = "PASS"
    res.earned = res.points
    res.evidence = "deliberately not run: " + "; ".join(skipped)


def check_no_workspace(ctx, res, state):
    wide = [a for a in ctx.argv_log if "--workspace" in a]
    ok = not wide
    res.status = "PASS" if ok else "FAIL"
    res.earned = res.points if ok else 0
    res.evidence = ("no --workspace in executed commands" if ok
                    else f"workspace-wide commands run: {wide}")


# --------------------------------------------------------------------------
# Task 7: owner docs
# --------------------------------------------------------------------------

def check_owning_crate(ctx, res, state):
    fpath = ctx.params.get("file-path",
                           "crates/perl-lsp-rs-core/src/providers/mod.rs")
    state["file_path"] = fpath
    m = re.match(r"crates/([^/]+)/", fpath.replace("\\", "/"))
    if not m:
        res.status = "FAIL"
        res.evidence = f"{fpath} is not under crates/<name>/"
        return
    crate = m.group(1)
    state["crate"] = crate
    exists = os.path.isdir(os.path.join(ctx.repo, "crates", crate))
    res.status = "PASS" if exists else "FAIL"
    res.earned = res.points if exists else 0
    res.evidence = f"owning crate: {crate} (dir {'exists' if exists else 'MISSING'})"


def check_owner_doc(ctx, res, state):
    crate = state.get("crate")
    if not crate:
        res.status = "FAIL"
        res.evidence = "no owning crate resolved"
        return
    doc = os.path.join(ctx.repo, "crates", crate, "CLAUDE.md")
    if not os.path.isfile(doc):
        res.status = "FAIL"
        res.evidence = f"{doc} MISSING (and AGENTS.md is not the convention here)"
        return
    with open(doc, encoding="utf-8", errors="replace") as fh:
        lines = fh.read().splitlines()
    state["owner_doc"] = doc
    res.status = "PASS"
    res.earned = res.points
    res.evidence = f"opened {doc} ({len(lines)} lines)"


CHECKS = {
    "toolchain": check_toolchain,
    "clean_package": check_clean_package,
    "admitted_attempt": check_admitted_attempt,
    "locked_build": check_locked_build,
    "fresh_binary": check_fresh_binary,
    "wall_time": check_wall_time,
    "test_scope": check_test_scope,
    "test_locked": check_test_locked,
    "test_redirect": check_test_redirect,
    "test_totals": check_test_totals,
    "test_exit_wall": check_test_exit_wall,
    "quick_bench": check_quick_bench,
    "baseline_named": check_baseline_named,
    "verdict_quoted": check_verdict_quoted,
    "bisect_clean": check_bisect_clean,
    "bisect_run": check_bisect_run,
    "bisect_reset": check_bisect_reset,
    "gate_select": check_gate_select,
    "gate_order": check_gate_order,
    "gate_exits": check_gate_exits,
    "proof_not_run": check_proof_not_run,
    "no_workspace": check_no_workspace,
    "owning_crate": check_owning_crate,
    "owner_doc": check_owner_doc,
}

# Checks taking (ctx, res, state, spec).
SPEC_CHECKS = {"gate_select"}


# --------------------------------------------------------------------------
# Task execution, scoring, reporting
# --------------------------------------------------------------------------

def run_task(spec, repo, params, tmpdir):
    ctx = Ctx(repo, params, tmpdir)
    result = TaskResult(spec)
    state = {}
    start = time.time()
    for step in spec["rubric"]:
        res = StepResult(step["step_id"], step["description"],
                         step["points"], step["mode"])
        if step["mode"] == "manual":
            res.status = "MANUAL"
            res.verify = step["verify"]
            res.evidence = step["evidence"]
        else:
            fn = CHECKS[step["check"]]
            t0 = time.time()
            try:
                if step["check"] in SPEC_CHECKS:
                    fn(ctx, res, state, spec)
                else:
                    fn(ctx, res, state)
            except Exception as exc:  # noqa: BLE001 - a crashing check is data
                res.status = "FAIL"
                res.evidence = f"check crashed: {exc!r}"
            res.wall_s = time.time() - t0
        result.steps.append(res)
    result.wall_s = time.time() - start
    state["task_wall"] = result.wall_s
    # wall_time evidence is recorded post-hoc (check ran before total known).
    for res, step in zip(result.steps, spec["rubric"]):
        if step.get("check") == "wall_time" and res.status == "PASS":
            res.evidence = f"task wall time {result.wall_s:.1f}s"
    earned = sum(s.earned for s in result.steps)
    auto_max = sum(s.points for s in result.steps if s.mode == "auto")
    if spec["automation"] == "manual-only":
        result.status = "MANUAL"
    elif earned >= spec["pass_threshold"]:
        result.status = "PASS"
    else:
        result.status = "BELOW_THRESHOLD"
    result.earned = earned
    result.auto_max = auto_max
    return result


def _bisect_reset_failed(spec, result):
    """True when the task ran a bisect_reset check that did not pass.

    A failed reset leaves HEAD mid-bisect (or BISECT state behind); later
    tasks must not run against that tree.
    """
    for rubric_step, step_result in zip(spec["rubric"], result.steps):
        if rubric_step.get("check") == "bisect_reset":
            return step_result.status != "PASS"
    return False


def print_task(result, verbose=True):
    spec = result.spec
    print(f"\n=== {spec['task_id']}: {spec['title']} "
          f"(automation: {spec['automation']}) ===")
    print(f"    goal: {spec['goal'][:150]}")
    for s in result.steps:
        mark = {"PASS": "pass", "FAIL": "FAIL", "MANUAL": "MANUAL",
                "NEEDS-PARAM": "NEEDS-PARAM", "SKIP": "SKIP"}.get(s.status, "?")
        print(f"  [{mark}] {s.step_id} {s.earned}/{s.points}pts: {s.description}")
        if verbose:
            if s.mode == "manual":
                print(f"         verify: {s.verify}")
                print(f"         record: {s.evidence}")
            elif s.evidence:
                for i, chunk in enumerate(
                        [s.evidence[j:j + 200]
                         for j in range(0, len(s.evidence), 200)]):
                    tag = "evidence: " if i == 0 else "          "
                    print(f"         {tag}{chunk}")
    print(f"  => {result.earned}/{spec['max_points']} "
          f"(auto {sum(s.earned for s in result.steps if s.mode == 'auto')}"
          f"/{result.auto_max}; threshold {spec['pass_threshold']}) "
          f"[{result.status}] wall={result.wall_s:.1f}s")


def print_table(results):
    print("\nPer-task score table")
    print(f"{'task':<24} {'earned':>6} {'auto':>9} "
          f"{'manual':>6} {'status':<15} {'wall':>8}")
    print("-" * 76)
    for r in results:
        auto_earned = sum(s.earned for s in r.steps if s.mode == "auto")
        manual_n = sum(1 for s in r.steps if s.mode == "manual")
        print(f"{r.spec['task_id']:<24} {r.earned:>3}/{r.spec['max_points']:<3} "
              f"{auto_earned:>3}/{r.auto_max:<4} "
              f"{manual_n:>6} {r.status:<15} {r.wall_s:>7.1f}s")
    print("-" * 76)


def _toml_str(s):
    return '"' + str(s).replace("\\", "\\\\").replace('"', '\\"') + '"'


def write_results_toml(path, results, repo, params, base_sha):
    with open(path, "w", encoding="utf-8") as fh:
        fh.write("# Battery run results (machine-readable, informational).\n")
        fh.write(f"schema_version = {_toml_str(SCHEMA_VERSION)}\n")
        fh.write(f"base_sha = {_toml_str(base_sha)}\n")
        fh.write(f"repo = {_toml_str(repo)}\n\n")
        for r in results:
            fh.write("[[task]]\n")
            fh.write(f"task_id = {_toml_str(r.spec['task_id'])}\n")
            fh.write(f"earned = {r.earned}\n")
            fh.write(f"max_points = {r.spec['max_points']}\n")
            fh.write(f"status = {_toml_str(r.status)}\n")
            fh.write(f"wall_s = {round(r.wall_s, 1)}\n")
            for s in r.steps:
                fh.write("[[task.step]]\n")
                fh.write(f"step_id = {_toml_str(s.step_id)}\n")
                fh.write(f"mode = {_toml_str(s.mode)}\n")
                fh.write(f"earned = {s.earned}\n")
                fh.write(f"points = {s.points}\n")
                fh.write(f"status = {_toml_str(s.status)}\n")
                fh.write(f"evidence = {_toml_str(s.evidence.splitlines()[0][:500] if s.evidence else '')}\n")
            fh.write("\n")


def base_sha_of(repo):
    try:
        out = subprocess.run(["git", "rev-parse", "HEAD"], cwd=repo,
                             capture_output=True, text=True,
                             timeout=60, check=False)
        return out.stdout.strip() or "unknown"
    except Exception:  # noqa: BLE001 - informational only
        return "unknown"


def main(argv=None):
    ap = argparse.ArgumentParser(description="Graded agentic task battery runner.")
    ap.add_argument("--check", action="store_true",
                    help="parse + validate all specs, run nothing")
    ap.add_argument("--task", default=None,
                    help="run only this task_id (default: all)")
    ap.add_argument("--param", action="append", default=[],
                    help="grader param k=v (repeatable)")
    ap.add_argument("--results-out", default=None,
                    help="write machine-readable results TOML here")
    ap.add_argument("--repo", default=DEFAULT_REPO,
                    help="repo root under test")
    ap.add_argument("--quiet", action="store_true",
                    help="score table only, no per-step evidence")
    args = ap.parse_args(argv)

    specs, errors = load_specs()
    if errors:
        print("SPEC ERRORS:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 2
    if args.check:
        print(f"OK: {len(specs)} specs parse and validate "
              f"({', '.join(s['task_id'] for s in specs)})")
        return 0

    params = {}
    for p in args.param:
        if "=" not in p:
            print(f"bad --param {p!r}, want k=v", file=sys.stderr)
            return 1
        k, v = p.split("=", 1)
        params[k] = v

    if args.task:
        specs = [s for s in specs if s["task_id"] == args.task]
        if not specs:
            print(f"unknown task {args.task}", file=sys.stderr)
            return 1

    repo = os.path.normpath(args.repo)
    sha = base_sha_of(repo)
    print(f"Battery run: repo={repo} base={sha}")
    print(f"params: {params or '(none)'}")
    results = []
    with tempfile.TemporaryDirectory(prefix="battery-") as tmpdir:
        for spec in specs:
            result = run_task(spec, repo, params, tmpdir)
            results.append(result)
            print_task(result, verbose=not args.quiet)
            if _bisect_reset_failed(spec, result):
                print("Stopping battery: bisect reset check failed; later "
                      "tasks would run against a bisected tree.",
                      file=sys.stderr)
                break
    print_table(results)
    if args.results_out:
        write_results_toml(args.results_out, results, repo, params, sha)
        print(f"results written to {args.results_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
