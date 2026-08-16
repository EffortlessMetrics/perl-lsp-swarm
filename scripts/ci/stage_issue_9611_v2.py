#!/usr/bin/env python3
"""Build and publish the bounded issue #9611 candidate from an exact main SHA.

This helper exists only on the construction branch. It patches a checkout,
verifies the semantic boundary, runs the focused proof, and then creates a clean
six-file commit whose parent is BASE_SHA. No construction file is copied into
the candidate branch.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import subprocess
import urllib.error
import urllib.request
from pathlib import Path

BASE_SHA = "ce572efb53a98596a1f1daf03f52e5675d4138d7"
TARGET_BRANCH = "codex/9611-retire-duplicate-crate-count-v2"
ACTIVE_GATE = "published_crate_count_pr_fast"
OBSOLETE_GATE = "published_crate_count"
CHANGED_PATHS = (
    ".ci/gate-policy.yaml",
    ".github/workflows/ci.yml",
    "docs/ci/gate-policy-economics.md",
    "docs/reference/CI_ARCHITECTURE.md",
    "scripts/ci/validate_gate_lane_mapping.py",
    "xtask/tests/published_crate_count_gate_integration.rs",
)

OBSOLETE_GATE_BLOCK = """  - name: published_crate_count
    tier: merge_gate
    description: \"Ratchet: published crate count must not exceed baseline\"
    required: true
    command: cargo xtask published-crate-count
    timeout_seconds: 30
    retry_count: 0
    budgets:
      max_duration_ms: 5000
    quarantine: true  # Until collapse completes (~30-31 crates)
    tags:
      - ratchet
      - microcrate
      - collapse

"""

TEST_SOURCE = r'''//! Integration contract for the one active published-crate-count gate.
//!
//! The transition-era duplicate `published_crate_count` merge-gate row was
//! quarantined while the workspace still carried 81 publishable crates. The
//! collapse is complete and `published_crate_count_pr_fast` now owns the same
//! xtask predicate at the current exact baseline.

use serde_yaml_ng::Value;
use std::fs;
use std::path::{Path, PathBuf};

const ACTIVE_GATE: &str = "published_crate_count_pr_fast";
const OBSOLETE_GATE: &str = "published_crate_count";

fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask must be a direct workspace member".to_string())
}

fn read_workspace_file(relative: &str) -> Result<String, String> {
    let path = workspace_root()?.join(relative);
    fs::read_to_string(&path).map_err(|error| format!("failed to read {path:?}: {error}"))
}

fn load_gate_policy_yaml() -> Result<Value, String> {
    serde_yaml_ng::from_str(&read_workspace_file(".ci/gate-policy.yaml")?)
        .map_err(|error| format!("gate-policy.yaml must be valid YAML: {error}"))
}

fn gates(policy: &Value) -> Result<&[Value], String> {
    policy
        .get("gates")
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
        .ok_or_else(|| "gate-policy.yaml must contain a gates sequence".to_string())
}

fn gate_named<'a>(policy: &'a Value, name: &str) -> Result<&'a Value, String> {
    gates(policy)?
        .iter()
        .find(|gate| gate.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("gate {name:?} must exist"))
}

fn string_field<'a>(gate: &'a Value, field: &str) -> Result<&'a str, String> {
    gate.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("gate field {field:?} must be a string"))
}

fn bool_field(gate: &Value, field: &str) -> Result<bool, String> {
    gate.get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("gate field {field:?} must be a boolean"))
}

#[test]
fn active_pr_fast_gate_owns_the_published_crate_count_predicate() -> Result<(), String> {
    let policy = load_gate_policy_yaml()?;
    let active_count = gates(&policy)?
        .iter()
        .filter(|gate| gate.get("name").and_then(Value::as_str) == Some(ACTIVE_GATE))
        .count();
    if active_count != 1 {
        return Err(format!("{ACTIVE_GATE} must exist exactly once; found {active_count}"));
    }

    let gate = gate_named(&policy, ACTIVE_GATE)?;
    if string_field(gate, "tier")? != "pr_fast" {
        return Err("active crate-count gate must remain in pr_fast".to_string());
    }
    if !bool_field(gate, "required")? {
        return Err("active crate-count gate must remain required".to_string());
    }
    if bool_field(gate, "quarantine")? {
        return Err("active crate-count gate must remain non-quarantined".to_string());
    }
    if string_field(gate, "command")? != "just ci-published-crate-count" {
        return Err("active gate must keep the canonical just recipe".to_string());
    }
    Ok(())
}

#[test]
fn obsolete_quarantined_merge_gate_does_not_return() -> Result<(), String> {
    let policy = load_gate_policy_yaml()?;
    if gates(&policy)?
        .iter()
        .any(|gate| gate.get("name").and_then(Value::as_str) == Some(OBSOLETE_GATE))
    {
        return Err(
            "the duplicate quarantined published_crate_count merge gate must stay retired"
                .to_string(),
        );
    }
    Ok(())
}

#[test]
fn obsolete_gate_is_not_selected_by_a_workflow_matrix() -> Result<(), String> {
    let workflow = read_workspace_file(".github/workflows/ci.yml")?;
    for line in workflow.lines() {
        let trimmed = line.trim_start();
        let matrix = trimmed
            .strip_prefix("gates: ")
            .or_else(|| trimmed.strip_prefix("- gates: "));
        if matrix.is_some_and(|value| value.split_whitespace().any(|token| token == OBSOLETE_GATE))
        {
            return Err(format!(
                "workflow matrix still selects obsolete gate {OBSOLETE_GATE:?}: {trimmed}"
            ));
        }
    }
    Ok(())
}

#[test]
fn just_recipe_still_delegates_to_the_xtask_ratchet() -> Result<(), String> {
    let justfile = read_workspace_file("justfile")?;
    let marker = "ci-published-crate-count:";
    let body = justfile
        .split_once(marker)
        .map(|(_, body)| body)
        .ok_or_else(|| "ci-published-crate-count recipe must exist".to_string())?;
    let recipe = body.split_once("\n\n").map_or(body, |(recipe, _)| recipe);
    if !recipe.lines().any(|line| {
        matches!(
            line.trim(),
            "cargo xtask published-crate-count" | "@cargo xtask published-crate-count"
        )
    }) {
        return Err(
            "ci-published-crate-count must delegate to cargo xtask published-crate-count"
                .to_string(),
        );
    }
    Ok(())
}
'''


def fail(message: str) -> None:
    raise SystemExit(message)


def replace_once(path: Path, old: str, new: str, label: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        fail(f"{label}: expected exactly one match in {path}, found {count}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def patch() -> None:
    gate_policy = Path(".ci/gate-policy.yaml")
    replace_once(
        gate_policy,
        OBSOLETE_GATE_BLOCK,
        "",
        "obsolete gate block",
    )

    replace_once(
        Path(".github/workflows/ci.yml"),
        "gates: msrv_authority_sync docs_build adr_link_check "
        "non_rust_inventory_check v2_bundle_sync nested_lock_check "
        "published_crate_count version_sync",
        "gates: msrv_authority_sync docs_build adr_link_check "
        "non_rust_inventory_check v2_bundle_sync nested_lock_check version_sync",
        "policy shard member",
    )

    replace_once(
        Path("scripts/ci/validate_gate_lane_mapping.py"),
        '    "published_crate_count": {"lanes": ["release_check"]},\n',
        "",
        "duplicate lane mapping",
    )

    policy_text = gate_policy.read_text(encoding="utf-8")
    if policy_text.count(f"  - name: {ACTIVE_GATE}\n") != 1:
        fail(f"{ACTIVE_GATE} must remain exactly once")
    if policy_text.count(f"  - name: {OBSOLETE_GATE}\n") != 0:
        fail(f"{OBSOLETE_GATE} must be absent after patch")
    gate_count = sum(
        line.startswith("  - name: ") for line in policy_text.splitlines()
    )

    economics = Path("docs/ci/gate-policy-economics.md")
    economics_text = economics.read_text(encoding="utf-8")
    old_row = (
        "| `release_check` | `published_crate_count`, `release_build`, "
        "`version_sync`, `sbom_verify`, `determinism_check`, "
        "`inline_completion_binary_smoke` |"
    )
    new_row = (
        "| `release_check` | `release_build`, `version_sync`, `sbom_verify`, "
        "`determinism_check`, `inline_completion_binary_smoke` |"
    )
    if economics_text.count(old_row) != 1:
        fail("release_check docs row did not match exactly once")
    economics_text = economics_text.replace(old_row, new_row, 1)
    economics_text, count_a = re.subn(
        r"- \d+ gates in `\.ci/gate-policy\.yaml`",
        f"- {gate_count} gates in `.ci/gate-policy.yaml`",
        economics_text,
        count=1,
    )
    economics_text, count_b = re.subn(
        r"- \d+ / \d+ gates have at least one lane mapping",
        f"- {gate_count} / {gate_count} gates have at least one lane mapping",
        economics_text,
        count=1,
    )
    if (count_a, count_b) != (1, 1):
        fail(f"gate-count docs update failed: {(count_a, count_b)}")
    economics.write_text(economics_text, encoding="utf-8")

    architecture = Path("docs/reference/CI_ARCHITECTURE.md")
    architecture_text = architecture.read_text(encoding="utf-8")
    for label, old in (
        (
            "required gate table row",
            "| `published_crate_count` | `xtask published-crate-count` | "
            "Crate count ratchet |\n",
        ),
        (
            "advisory gate table row",
            "| `published_crate_count` | Quarantined until collapse completes "
            "(~30–31 target crates) |\n",
        ),
    ):
        count = architecture_text.count(old)
        if count != 1:
            fail(f"{label}: expected one match, found {count}")
        architecture_text = architecture_text.replace(old, "", 1)
    architecture.write_text(architecture_text, encoding="utf-8")

    Path("xtask/tests/published_crate_count_gate_integration.rs").write_text(
        TEST_SOURCE,
        encoding="utf-8",
    )

    changed = {
        line
        for line in subprocess.check_output(
            ["git", "diff", "--name-only"], text=True
        ).splitlines()
        if line
    }
    expected = set(CHANGED_PATHS)
    if changed != expected:
        fail(
            "candidate path set drifted: "
            f"missing={sorted(expected - changed)} extra={sorted(changed - expected)}"
        )

    diff = subprocess.check_output(["git", "diff", "--numstat"], text=True)
    numstat = {}
    for line in diff.splitlines():
        added, deleted, path = line.split("\t", 2)
        numstat[path] = (int(added), int(deleted))
    gate_add, gate_delete = numstat[".ci/gate-policy.yaml"]
    if gate_add != 0 or not 12 <= gate_delete <= 20:
        fail(
            "gate-policy edit exceeded bounded row retirement: "
            f"additions={gate_add} deletions={gate_delete}"
        )


def run(command: list[str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, check=True)


def verify() -> None:
    run(["git", "diff", "--check"])
    run(["cargo", "fmt", "--all", "--", "--check"])
    run(
        [
            "python3",
            "scripts/ci/validate_gate_lane_mapping.py",
            "--strict",
        ]
    )
    run(["cargo", "xtask", "gate-policy", "check"])
    run(["cargo", "xtask", "docs-check"])
    run(
        [
            "python3",
            "scripts/ci/validate_policy_checks_inventory.py",
            "--check",
        ]
    )
    run(
        [
            "cargo",
            "test",
            "-p",
            "xtask",
            "--test",
            "published_crate_count_gate_integration",
            "--locked",
        ]
    )
    run(
        [
            "cargo",
            "run",
            "-p",
            "xtask",
            "--locked",
            "--",
            "published-crate-count",
        ]
    )


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


def publish() -> None:
    base_commit = request("GET", f"/git/commits/{BASE_SHA}")
    if not isinstance(base_commit, dict):
        fail("base commit response must be an object")
    tree = base_commit.get("tree")
    if not isinstance(tree, dict) or not isinstance(tree.get("sha"), str):
        fail("base commit response lacks tree.sha")

    entries = []
    for raw_path in CHANGED_PATHS:
        path = Path(raw_path)
        blob = request(
            "POST",
            "/git/blobs",
            {
                "content": base64.b64encode(path.read_bytes()).decode("ascii"),
                "encoding": "base64",
            },
        )
        if not isinstance(blob, dict) or not isinstance(blob.get("sha"), str):
            fail(f"blob response for {raw_path} lacks sha")
        entries.append(
            {
                "path": raw_path,
                "mode": "100755" if raw_path.startswith("scripts/ci/") else "100644",
                "type": "blob",
                "sha": blob["sha"],
            }
        )

    candidate_tree = request(
        "POST",
        "/git/trees",
        {"base_tree": tree["sha"], "tree": entries},
    )
    if not isinstance(candidate_tree, dict) or not isinstance(
        candidate_tree.get("sha"), str
    ):
        fail("candidate tree response lacks sha")

    commit = request(
        "POST",
        "/git/commits",
        {
            "message": "fix(ci): retire duplicate quarantined crate-count gate (#9611)",
            "tree": candidate_tree["sha"],
            "parents": [BASE_SHA],
        },
    )
    if not isinstance(commit, dict) or not isinstance(commit.get("sha"), str):
        fail("candidate commit response lacks sha")

    request(
        "PATCH",
        f"/git/refs/heads/{TARGET_BRANCH}",
        {"sha": commit["sha"], "force": True},
    )

    receipt = {
        "schema_version": 2,
        "issue": 9611,
        "base_sha": BASE_SHA,
        "branch": TARGET_BRANCH,
        "head_sha": commit["sha"],
        "paths": list(CHANGED_PATHS),
    }
    output = Path("target/9611-v2/result.json")
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps(receipt, sort_keys=True))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("patch", "verify", "publish"))
    args = parser.parse_args()
    if args.mode == "patch":
        patch()
    elif args.mode == "verify":
        verify()
    else:
        publish()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
