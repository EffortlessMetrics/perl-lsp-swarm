#!/usr/bin/env python3
"""Prepare canonical receipt custody; this does not verify product semantics.

Inactive until an immutable central Perl lane supplies a fresh private directory.
The Rust producer remains the owner of rust_small_proof.v1 verification.
"""
from __future__ import annotations

import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys

MAX_RECEIPT_BYTES = 1024 * 1024


def regular_bytes(path: Path, limit: int) -> tuple[bytes, os.stat_result]:
    for parent in (path, *path.parents):
        if parent.is_symlink():
            raise ValueError("linked custody path")
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise ValueError("receipt must be a single-link regular file")
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            data = stream.read(limit + 1)
        if not data or len(data) > limit:
            raise ValueError("receipt size outside bound")
        after = os.fstat(descriptor)
        custody = ("st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns", "st_nlink")
        if any(getattr(after, key) != getattr(metadata, key) for key in custody) or len(data) != metadata.st_size:
            raise ValueError("receipt changed during export")
        return data, metadata
    finally:
        os.close(descriptor)


def export(environment: dict[str, str], *, acquire_plan=None, expected_process_status=None,
           command_receipt: Path | None = None) -> dict:
    # The installed `python3 -I` exporter stays stdlib-only. The bridge is an
    # in-process trusted-control composition, never a new script/path override.
    hooks = {}
    if acquire_plan is not None:
        if not __package__:
            raise ValueError('subject bridge requires trusted in-process composition')
        from .rust_small_subject import product_subject
        from .rust_small_evidence import unique_object
        hooks['object_pairs_hook'] = unique_object
    required = ("GITHUB_WORKSPACE", "RUNNER_TEMP", "GITHUB_REPOSITORY", "GITHUB_SHA",
                "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "EM_CI_ARTIFACT_DIR",
                "GITHUB_JOB", "GITHUB_WORKFLOW", "GITHUB_WORKFLOW_REF", "GITHUB_EVENT_NAME", "GITHUB_REF")
    if any(not environment.get(key) for key in required):
        raise ValueError("central custody environment missing")
    if environment["GITHUB_REPOSITORY"] != "EffortlessMetrics/perl-lsp-swarm":
        raise ValueError("unsupported receipt repository")
    if not re.fullmatch("[0-9a-f]{40}", environment["GITHUB_SHA"]):
        raise ValueError("exact source SHA missing")
    for key in ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"):
        if not re.fullmatch("[1-9][0-9]*", environment[key]):
            raise ValueError("native run identity missing")
    root = Path(environment["EM_CI_ARTIFACT_DIR"])
    temporary = Path(environment["RUNNER_TEMP"])
    if not root.is_absolute() or not temporary.is_absolute() or ".." in root.parts or ".." in temporary.parts:
        raise ValueError("private custody paths must be absolute")
    for path in (root, *root.parents):
        if path.is_symlink():
            raise ValueError("linked custody root")
    if root == temporary or not root.is_relative_to(temporary) or not root.is_dir():
        raise ValueError("central custody root outside runner temp")
    if any(root.iterdir()):
        raise ValueError("central custody root is not fresh")
    directory = root.stat()
    if directory.st_uid != os.geteuid() or directory.st_mode & 0o077:
        raise ValueError("central custody root is not private to this owner")
    if environment["GITHUB_JOB"] not in ("standard", "hosted", "hosted-fallback"):
        raise ValueError("unsupported standard producer job")
    context_bytes, _ = regular_bytes(temporary / "em-ci-job-context.json", 16384)
    context = json.loads(context_bytes, **hooks)
    expected = {
        "schema": "em-ci-job-context.v1", "repository": environment["GITHUB_REPOSITORY"],
        "sha": environment["GITHUB_SHA"], "run_id": environment["GITHUB_RUN_ID"],
        "run_attempt": environment["GITHUB_RUN_ATTEMPT"], "profile": "standard",
        "job": environment["GITHUB_JOB"], "workflow": environment["GITHUB_WORKFLOW"],
        "workflow_ref": environment["GITHUB_WORKFLOW_REF"], "event": environment["GITHUB_EVENT_NAME"],
        "ref": environment["GITHUB_REF"],
    }
    if any(context.get(key) != value for key, value in expected.items()):
        raise ValueError("central producer identity mismatch")
    acquired = product_subject(environment, context, acquire_plan) if acquire_plan is not None else None
    entered = datetime.datetime.strptime(context["proof_started_at"], "%Y-%m-%dT%H:%M:%SZ")
    entered_ns = int(entered.replace(tzinfo=datetime.timezone.utc).timestamp()) * 10**9
    source = Path(environment["GITHUB_WORKSPACE"]) / "target/receipts/rust-small-proof.json"
    data, metadata = regular_bytes(source, MAX_RECEIPT_BYTES)
    if metadata.st_mtime_ns < entered_ns:
        raise ValueError("receipt predates this proof entry")
    receipt = json.loads(data)
    if receipt.get("schema_version") != "rust_small_proof.v1":
        raise ValueError("canonical receipt schema missing")
    subject = receipt.get("subject", {})
    proof_sha = acquired['head'] if acquired is not None else environment["GITHUB_SHA"]
    if subject.get("git_sha") != proof_sha or subject.get("worktree_dirty") is not False:
        raise ValueError("canonical receipt source mismatch")
    if receipt.get("result") not in ("success", "product_failure", "not_completed", "instrument_failure"):
        raise ValueError("canonical receipt outcome missing")
    if expected_process_status is not None:
        if (type(expected_process_status) is not int or not 0 <= expected_process_status <= 255
                or (expected_process_status == 0) != (receipt['result'] == 'success')):
            raise ValueError('canonical producer exit differs from receipt outcome')
    manifest = {
        "schema": "perl-rust-small-export.v1", **{key: expected[key] for key in
            ("repository", "sha", "run_id", "run_attempt", "profile")},
        "proof_started_at": context["proof_started_at"], "result": receipt["result"],
        "member": {"path": "rust-small-proof.json", "bytes": len(data),
                   "sha256": hashlib.sha256(data).hexdigest()},
    }
    command_data = None
    if command_receipt is not None:
        # Trusted in-process producer supplies this path; dispatch/environment
        # cannot select another receipt or activate this optional extension.
        if (not isinstance(command_receipt, Path) or not command_receipt.is_absolute()
                or not command_receipt.is_relative_to(temporary)
                or command_receipt.resolve().is_relative_to(Path(environment['GITHUB_WORKSPACE']).resolve())
                or context.get('product_workspace') != str(Path(environment['GITHUB_WORKSPACE']).resolve())):
            raise ValueError('command proof lacks native workspace/private temporary custody')
        command_data, command_metadata = regular_bytes(command_receipt, MAX_RECEIPT_BYTES)
        command_proof = json.loads(command_data, **hooks)
        if (command_metadata.st_mtime_ns < entered_ns
                or command_proof.get('schema_version') != 'command-proof-set.v1'
                or not isinstance(command_proof.get('commands'), list) or not command_proof['commands']
                or not isinstance(command_proof.get('result'), str)):
            raise ValueError('command proof missing actual owner schema/generation')
        # Export is custody, not a new result classifier. Existing command owner
        # retains every failure; readiness consumes complete actual SUCCESS.
        manifest['command_member'] = dict(path='command-proof-set.json', bytes=len(command_data),
                                          sha256=hashlib.sha256(command_data).hexdigest())
        if regular_bytes(command_receipt, MAX_RECEIPT_BYTES)[0] != command_data:
            raise ValueError('command proof changed during custody preparation')
    if acquired is not None and product_subject(environment, context, acquire_plan) != acquired:
        raise ValueError('acquired subject changed during export')
    # Exclusive writes into an immutable-owner-created fresh root. A failed
    # export can leave partial staging; central publication must reject it.
    if command_data is not None:
        with (root / 'command-proof-set.json').open('xb') as stream:
            stream.write(command_data)
    with (root / "rust-small-proof.json").open("xb") as stream:
        stream.write(data)
    with (root / "perl-rust-small-export.json").open("x", encoding="utf-8") as stream:
        json.dump(manifest, stream, sort_keys=True)
        stream.write("\n")
    return manifest


def main() -> int:
    try:
        export(dict(os.environ))
    except (ValueError, KeyError, TypeError, OSError):
        print("::error::canonical Rust Small receipt export NOT_PROVEN", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
