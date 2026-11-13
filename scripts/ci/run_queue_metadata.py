#!/usr/bin/env python3
"""Run compilation-compatible G metadata checks once, with per-command evidence.

These candidate-produced execution records are diagnostic, not trusted admission
or finalizer authority. Native tool receipts retain their original semantics.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time


def context(env: dict[str, str]) -> dict[str, str]:
    if env.get("GITHUB_EVENT_NAME") != "merge_group":
        raise ValueError("metadata cluster requires merge_group")
    event = json.loads(Path(env["GITHUB_EVENT_PATH"]).read_text())
    group = event["merge_group"]
    if event.get("action") != "checks_requested":
        raise ValueError("metadata cluster requires checks_requested")
    sha = env.get("GITHUB_SHA", "")
    base = group.get("base_sha", "")
    workflow_sha = env.get("GITHUB_WORKFLOW_SHA", "")
    if any(not re.fullmatch(r"[0-9a-f]{40}", x) for x in (sha, base, workflow_sha)):
        raise ValueError("missing exact commit identity")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if sha != group.get("head_sha") or sha != head:
        raise ValueError("event, checkout and G identities disagree")
    subprocess.run(["git", "merge-base", "--is-ancestor", base, sha], check=True)
    for key in ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"):
        if not re.fullmatch(r"[1-9][0-9]*", env.get(key, "")):
            raise ValueError("missing positive run/attempt identity")
    for key in ("GITHUB_REPOSITORY", "GITHUB_WORKFLOW_REF", "GITHUB_JOB", "RUNNER_TEMP"):
        if not env.get(key):
            raise ValueError(f"missing {key}")
    return dict(sha=sha, base_sha=base, run_id=env["GITHUB_RUN_ID"],
                run_attempt=env["GITHUB_RUN_ATTEMPT"], repository=env["GITHUB_REPOSITORY"],
                workflow_ref=env["GITHUB_WORKFLOW_REF"], workflow_sha=workflow_sha,
                job=env["GITHUB_JOB"])


def execute(command: list[str], timeout: int, env: dict[str, str]) -> tuple[str, int | None]:
    try:
        process = subprocess.Popen(command, start_new_session=True, env=env)
    except OSError:
        return "instrument_failure", None
    try:
        code = process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()
        return "timed_out", None
    return ("success" if code == 0 else "failure"), code


def run(env: dict[str, str]) -> int:
    bound = context(env)
    out = Path(env["RUNNER_TEMP"]) / "queue-metadata" / bound["sha"] / bound["run_id"] / bound["run_attempt"]
    # A reused attempt directory cannot supply stale tool receipts.
    out.mkdir(parents=True, exist_ok=False)
    build_env = dict(env, CARGO_TARGET_DIR=str(Path("target").resolve()))
    # Every Cargo invocation in this job uses this same compiler/profile/features
    # and dependency closure. No cross-job executable or verdict cache is used.
    checks = [
        ("setup-perl-pin-regression", ["python3", "-m", "unittest", "scripts/ci/test_action_pin_setup_perl_sites_18049.py"], None, 120),
        ("build", ["cargo", "build", "--locked", "-p", "xtask", "--bin", "xtask", "--bin", "action-pin-provenance"], None, 900),
        ("workflow-trigger-lint", ["target/debug/xtask", "workflow-trigger-lint", "--policy", ".ci/policies/required-checks.toml", "--receipt", str(out / "workflow-trigger-lint.json"), "--format", "json"], "workflow-trigger-lint.json", 120),
        ("action-pin-provenance", ["target/debug/action-pin-provenance", "--base", bound["base_sha"], "--receipt", str(out / "action-pin-provenance.json")], "action-pin-provenance.json", 120),
        ("methodology-gate", ["target/debug/xtask", "methodology-gate", "--fixture", env["GITHUB_EVENT_PATH"], "--receipt", str(out / "methodology-gate.json")], "methodology-gate.json", 120),
        ("agent-capabilities", ["target/debug/xtask", "check-agent-capabilities"], None, 120),
    ]
    success = True
    built = False
    for name, command, native_receipt, timeout in checks:
        print(f"::group::{name}", flush=True)
        started = time.monotonic()
        if name not in ("setup-perl-pin-regression", "build") and not built:
            status, code = "not_run", None
        else:
            status, code = execute(command, timeout, build_env)
        digest = None
        if native_receipt and status == "success":
            path = out / native_receipt
            if not path.is_file() or path.is_symlink() or path.stat().st_size == 0:
                status = "missing_receipt"
            else:
                digest = hashlib.sha256(path.read_bytes()).hexdigest()
        record = dict(bound, schema="queue-metadata-execution.v1", check=name,
                      command=command, status=status, exit_code=code,
                      elapsed_seconds=round(time.monotonic() - started, 3),
                      native_receipt=native_receipt, native_receipt_sha256=digest)
        (out / f"{name}.execution.json").write_text(json.dumps(record, indent=2) + "\n")
        print(f"{name}: {status} (exit={code})\n::endgroup::", flush=True)
        if name == "build":
            built = status == "success"
        success = success and status == "success"
    return 0 if success else 1


if __name__ == "__main__":
    try:
        sys.exit(run(dict(os.environ)))
    except (KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"queue metadata refused: {error}", file=sys.stderr)
        sys.exit(1)
