#!/usr/bin/env python3
from __future__ import annotations
import base64, gzip, json, os, subprocess, urllib.error, urllib.request
from pathlib import Path

BASE_SHA = "66482bd58313cbc578254835e2703ea914dcac43"
TARGET_BRANCH = "codex/p2-live-enforcement-model"
PAYLOADS = {
    "scripts/ci/reconcile_github_enforcement_snapshot.py": "construction/9152/reconcile.py.gz",
    "scripts/ci/test_reconcile_github_enforcement_snapshot.py": "construction/9152/test.py.gz",
    "docs/ci/github-enforcement-snapshot.md": "construction/9152/docs.md.gz",
    ".github/workflows/gate-enforcement-contract.yml": "construction/9152/workflow.yml.gz",
}

def request(method: str, path: str, payload=None):
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(
        f"https://api.github.com/repos/{os.environ['GITHUB_REPOSITORY']}{path}",
        data=data,
        method=method,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {os.environ['GITHUB_TOKEN']}",
            "X-GitHub-Api-Version": "2022-11-28",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        raise SystemExit(
            f"GitHub API {method} {path} failed: {error.code} "
            f"{error.read().decode(errors='replace')}"
        ) from error

def materialize() -> None:
    for raw_path, payload_path in PAYLOADS.items():
        path = Path(raw_path)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(gzip.decompress(Path(payload_path).read_bytes()))

def publish() -> None:
    commit = request("GET", f"/git/commits/{BASE_SHA}")
    entries = []
    for raw_path in sorted(PAYLOADS):
        blob = request(
            "POST",
            "/git/blobs",
            {
                "content": base64.b64encode(Path(raw_path).read_bytes()).decode(),
                "encoding": "base64",
            },
        )
        entries.append(
            {
                "path": raw_path,
                "mode": "100755" if raw_path.startswith("scripts/ci/") else "100644",
                "type": "blob",
                "sha": blob["sha"],
            }
        )
    tree = request(
        "POST",
        "/git/trees",
        {"base_tree": commit["tree"]["sha"], "tree": entries},
    )
    created = request(
        "POST",
        "/git/commits",
        {
            "message": "feat(ci): model the live GitHub enforcement union",
            "tree": tree["sha"],
            "parents": [BASE_SHA],
        },
    )
    request(
        "POST",
        "/git/refs",
        {"ref": f"refs/heads/{TARGET_BRANCH}", "sha": created["sha"]},
    )
    result = {"base": BASE_SHA, "branch": TARGET_BRANCH, "head": created["sha"]}
    Path("target/9152-result.json").parent.mkdir(parents=True, exist_ok=True)
    Path("target/9152-result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, sort_keys=True))

def main() -> None:
    materialize()
    subprocess.run(
        [
            "python3",
            "-m",
            "unittest",
            "-v",
            "scripts/ci/test_reconcile_github_enforcement_snapshot.py",
        ],
        check=True,
    )
    subprocess.run(
        [
            "python3",
            "-m",
            "py_compile",
            "scripts/ci/reconcile_github_enforcement_snapshot.py",
            "scripts/ci/test_reconcile_github_enforcement_snapshot.py",
        ],
        check=True,
    )
    subprocess.run(["git", "diff", "--check"], check=True)
    publish()

if __name__ == "__main__":
    main()
