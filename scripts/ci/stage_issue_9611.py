#!/usr/bin/env python3
"""Temporary staging helper for issue #9611.

This file exists only on the construction branch. The connected GitHub app
uses the emitted blob manifest to rebuild the actual candidate from current
main without this helper or its workflow.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import urllib.request
from pathlib import Path

CHANGED_PATHS = [
    ".ci/gate-policy.yaml",
    ".github/workflows/ci.yml",
    "scripts/ci/validate_gate_lane_mapping.py",
    "docs/ci/gate-policy-economics.md",
    "docs/reference/CI_ARCHITECTURE.md",
    "xtask/tests/published_crate_count_gate_integration.rs",
]

TEST_SOURCE = r'''//! Integration contract for the one active published-crate-count gate.
//!
//! The transition-era duplicate `published_crate_count` merge-gate row was
//! quarantined while the workspace still carried 81 publishable crates. The
//! collapse is complete and `published_crate_count_pr_fast` now owns the same
//! xtask predicate at the current exact baseline.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_yaml_ng::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should be a direct workspace member")
        .to_path_buf()
}

fn load_gate_policy_yaml() -> Value {
    let path = workspace_root().join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Failed to read {:?} - file must exist", path));
    serde_yaml_ng::from_str(&content).expect("gate-policy.yaml must be valid YAML")
}

fn gates(policy: &Value) -> Result<&Vec<Value>, String> {
    policy
        .get("gates")
        .and_then(Value::as_sequence)
        .ok_or_else(|| "gate-policy.yaml must contain a gates sequence".to_string())
}

fn gate_named<'a>(policy: &'a Value, name: &str) -> Result<&'a Value, String> {
    gates(policy)?
        .iter()
        .find(|gate| gate.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("gate {name:?} must exist"))
}

#[test]
fn active_pr_fast_gate_owns_the_published_crate_count_predicate() -> Result<(), String> {
    let policy = load_gate_policy_yaml();
    let gate = gate_named(&policy, "published_crate_count_pr_fast")?;

    if gate.get("tier").and_then(Value::as_str) != Some("pr_fast") {
        return Err("active crate-count gate must remain in pr_fast".to_string());
    }
    if gate.get("required").and_then(Value::as_bool) != Some(true) {
        return Err("active crate-count gate must remain required".to_string());
    }
    if gate.get("quarantine").and_then(Value::as_bool) == Some(true) {
        return Err("active crate-count gate must not be quarantined".to_string());
    }
    if gate.get("command").and_then(Value::as_str) != Some("just ci-published-crate-count") {
        return Err("active gate must keep the canonical just recipe".to_string());
    }
    Ok(())
}

#[test]
fn obsolete_quarantined_merge_gate_does_not_return() -> Result<(), String> {
    let policy = load_gate_policy_yaml();
    if gates(&policy)?.iter().any(|gate| {
        gate.get("name").and_then(Value::as_str) == Some("published_crate_count")
    }) {
        return Err(
            "the duplicate quarantined published_crate_count merge gate must stay retired"
                .to_string(),
        );
    }
    Ok(())
}

#[test]
fn just_recipe_still_delegates_to_the_xtask_ratchet() -> Result<(), String> {
    let justfile = fs::read_to_string(workspace_root().join("justfile"))
        .map_err(|error| format!("failed to read justfile: {error}"))?;
    let expected = "ci-published-crate-count:\n    cargo xtask published-crate-count";
    if !justfile.contains(expected) {
        return Err(
            "ci-published-crate-count must delegate to cargo xtask published-crate-count"
                .to_string(),
        );
    }
    Ok(())
}
'''


def replace_once(path: Path, old: str, new: str, label: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(
            f"{label}: expected exactly one match in {path}, found {count}"
        )
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def patch() -> None:
    gate_policy = Path(".ci/gate-policy.yaml")
    policy_text = gate_policy.read_text(encoding="utf-8")
    pattern = re.compile(
        r"^  - name: published_crate_count\n.*?(?=^  - name: version_sync\n)",
        re.MULTILINE | re.DOTALL,
    )
    policy_text, count = pattern.subn("", policy_text)
    if count != 1:
        raise SystemExit(
            f"gate-policy duplicate block: expected one match, found {count}"
        )
    gate_policy.write_text(policy_text, encoding="utf-8")

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
        raise SystemExit("release_check docs row did not match exactly once")
    economics_text = economics_text.replace(old_row, new_row, 1)
    gate_count = sum(
        1 for line in policy_text.splitlines() if line.startswith("  - name: ")
    )
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
        raise SystemExit(f"gate-count docs update failed: {(count_a, count_b)}")
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
            raise SystemExit(f"{label}: expected one match, found {count}")
        architecture_text = architecture_text.replace(old, "", 1)
    architecture.write_text(architecture_text, encoding="utf-8")

    Path("xtask/tests/published_crate_count_gate_integration.rs").write_text(
        TEST_SOURCE,
        encoding="utf-8",
    )


def create_blob(path: Path, token: str, repository: str) -> str:
    payload = json.dumps(
        {
            "content": base64.b64encode(path.read_bytes()).decode("ascii"),
            "encoding": "base64",
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        f"https://api.github.com/repos/{repository}/git/blobs",
        data=payload,
        method="POST",
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.load(response)
    return str(result["sha"])


def create_blobs() -> None:
    token = os.environ["GITHUB_TOKEN"]
    repository = os.environ["GITHUB_REPOSITORY"]
    blobs = {}
    for raw_path in CHANGED_PATHS:
        path = Path(raw_path)
        data = path.read_bytes()
        blobs[raw_path] = {
            "blob_sha": create_blob(path, token, repository),
            "size": len(data),
            "mode": "100755" if os.access(path, os.X_OK) else "100644",
        }
    receipt = {
        "schema_version": 1,
        "source_sha": os.environ["GITHUB_SHA"],
        "issue": 9611,
        "blobs": blobs,
    }
    output = Path("target/9611/blobs.json")
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(receipt, sort_keys=True))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("patch", "create-blobs"))
    args = parser.parse_args()
    if args.mode == "patch":
        patch()
    else:
        create_blobs()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
