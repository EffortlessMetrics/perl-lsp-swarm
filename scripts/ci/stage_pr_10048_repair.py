#!/usr/bin/env python3
"""Materialize, verify, and publish the reviewed PR #10048 contract repair.

This helper and its compressed payloads exist only on the construction branch.
The published commit contains only the three reviewed production files and is a
fast-forward child of the exact PR head reviewed before construction began.
"""

from __future__ import annotations

import argparse
import base64
import gzip
import hashlib
import json
import os
import subprocess
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

BASE_SHA = "fb5f61279e278c1fc8e4af1653f44f91a2ab8c06"
TARGET_BRANCH = "codex/p2-live-enforcement-model"
PAYLOADS = {
    "scripts/ci/reconcile_github_enforcement_snapshot.py": (
        "construction/10048/reconcile.py.gz",
        "8d5dfc5a5904bd5979a5b25ba19420c2c0a72f3d211496551ced6cd7d38454c5",
    ),
    "scripts/ci/test_reconcile_github_enforcement_snapshot.py": (
        "construction/10048/test.py.gz",
        "082241c07f101c1bec3e9a39d9b7e33ee000ea75507c4592259e5a138528f6ac",
    ),
    "docs/ci/github-enforcement-snapshot.md": (
        "construction/10048/docs.md.gz",
        "8b6946215e0b0707b7c4f054da1398498aa909c5dddbf101cfb981417d8e24ad",
    ),
}


def fail(message: str) -> None:
    raise SystemExit(message)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def materialize() -> None:
    for raw_path, (payload_path, expected_digest) in PAYLOADS.items():
        data = gzip.decompress(Path(payload_path).read_bytes())
        actual_digest = sha256(data)
        if actual_digest != expected_digest:
            fail(
                f"payload digest mismatch for {raw_path}: "
                f"expected={expected_digest} actual={actual_digest}"
            )
        path = Path(raw_path)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        print(f"materialized {raw_path} sha256={actual_digest}")


def run(command: list[str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, check=True)


def verify() -> None:
    changed = {
        line
        for line in subprocess.check_output(
            ["git", "diff", "--name-only"], text=True
        ).splitlines()
        if line
    }
    expected = set(PAYLOADS)
    if changed != expected:
        fail(
            "reviewed production path set drifted: "
            f"missing={sorted(expected - changed)} extra={sorted(changed - expected)}"
        )

    for raw_path, (_, expected_digest) in PAYLOADS.items():
        actual_digest = sha256(Path(raw_path).read_bytes())
        if actual_digest != expected_digest:
            fail(
                f"working-file digest mismatch for {raw_path}: "
                f"expected={expected_digest} actual={actual_digest}"
            )

    run(["git", "diff", "--check"])
    run(
        [
            "python3",
            "-m",
            "py_compile",
            "scripts/ci/reconcile_github_enforcement_snapshot.py",
            "scripts/ci/test_reconcile_github_enforcement_snapshot.py",
        ]
    )
    run(
        [
            "python3",
            "-m",
            "unittest",
            "-v",
            "scripts/ci/test_reconcile_github_enforcement_snapshot.py",
        ]
    )
    run(
        [
            "python3",
            "-m",
            "unittest",
            "-v",
            "scripts/ci/test_validate_gate_enforcement_contract.py",
        ]
    )

    script = Path("scripts/ci/reconcile_github_enforcement_snapshot.py").read_text(
        encoding="utf-8"
    )
    forbidden = ("urllib.request", "requests.", "httpx.", "subprocess.run([\"gh\"")
    present = [token for token in forbidden if token in script]
    if present:
        fail(f"offline model acquired a network/client path: {present}")


def request(method: str, path: str, payload: object | None = None) -> object:
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        f"https://api.github.com/repos/{os.environ['GITHUB_REPOSITORY']}{path}",
        data=data,
        method=method,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {os.environ['GITHUB_TOKEN']}",
            "Content-Type": "application/json",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        fail(
            f"GitHub API {method} {path} failed: {error.code} "
            f"{error.read().decode(errors='replace')}"
        )


def require_object(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        fail(f"{label} response must be an object")
    return value


def require_sha(value: object, label: str) -> str:
    if not isinstance(value, str) or len(value) != 40:
        fail(f"{label} must be a 40-character SHA")
    return value


def publish() -> None:
    encoded_branch = urllib.parse.quote(TARGET_BRANCH, safe="")
    target_ref = require_object(
        request("GET", f"/git/ref/heads/{encoded_branch}"), "target ref"
    )
    ref_object = require_object(target_ref.get("object"), "target ref object")
    current_head = require_sha(ref_object.get("sha"), "target ref object.sha")
    if current_head != BASE_SHA:
        fail(
            "refusing to overwrite concurrent PR work: "
            f"expected={BASE_SHA} observed={current_head}"
        )

    base_commit = require_object(
        request("GET", f"/git/commits/{BASE_SHA}"), "base commit"
    )
    base_tree = require_object(base_commit.get("tree"), "base commit tree")
    base_tree_sha = require_sha(base_tree.get("sha"), "base commit tree.sha")

    entries = []
    output_digests: dict[str, str] = {}
    for raw_path in sorted(PAYLOADS):
        data = Path(raw_path).read_bytes()
        output_digests[raw_path] = sha256(data)
        blob = require_object(
            request(
                "POST",
                "/git/blobs",
                {
                    "content": base64.b64encode(data).decode("ascii"),
                    "encoding": "base64",
                },
            ),
            f"blob {raw_path}",
        )
        blob_sha = require_sha(blob.get("sha"), f"blob {raw_path}.sha")
        entries.append(
            {
                "path": raw_path,
                "mode": "100755" if raw_path.startswith("scripts/ci/") else "100644",
                "type": "blob",
                "sha": blob_sha,
            }
        )

    tree = require_object(
        request(
            "POST",
            "/git/trees",
            {"base_tree": base_tree_sha, "tree": entries},
        ),
        "candidate tree",
    )
    tree_sha = require_sha(tree.get("sha"), "candidate tree.sha")
    commit = require_object(
        request(
            "POST",
            "/git/commits",
            {
                "message": "fix(ci): make live enforcement snapshot consumable by P3",
                "tree": tree_sha,
                "parents": [BASE_SHA],
            },
        ),
        "candidate commit",
    )
    head_sha = require_sha(commit.get("sha"), "candidate commit.sha")

    request(
        "PATCH",
        f"/git/refs/heads/{encoded_branch}",
        {"sha": head_sha, "force": False},
    )

    receipt = {
        "schema_version": 1,
        "pull_request": 10048,
        "base_sha": BASE_SHA,
        "branch": TARGET_BRANCH,
        "head_sha": head_sha,
        "paths": sorted(PAYLOADS),
        "sha256": output_digests,
    }
    output = Path("target/10048-repair/result.json")
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps(receipt, sort_keys=True))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("materialize", "verify", "publish"))
    args = parser.parse_args()
    if args.mode == "materialize":
        materialize()
    elif args.mode == "verify":
        verify()
    else:
        publish()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
