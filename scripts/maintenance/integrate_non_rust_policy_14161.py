#!/usr/bin/env python3
from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE_HEAD = os.environ["SOURCE_HEAD"]
BASE_SHA = os.environ["BASE_SHA"]
TARGET_BRANCH = os.environ["TARGET_BRANCH"]


def run(*args: str, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(args), flush=True)
    subprocess.run(args, cwd=ROOT, env=env, check=True)


def output(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write(content)


def show(path: str) -> str:
    return output("git", "show", f"{SOURCE_HEAD}:{path}")


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected one literal match, found {count}: {old[:80]!r}")
    write(path, text.replace(old, new, 1))


def regex_once(path: str, pattern: str, replacement: str) -> None:
    text = read(path)
    normalized = replacement.replace("\\n", "\n").replace("\\\"", '"')
    updated, count = re.subn(
        pattern,
        lambda _match: normalized,
        text,
        count=1,
        flags=re.DOTALL,
    )
    if count != 1:
        raise RuntimeError(f"{path}: expected one regex match, found {count}: {pattern!r}")
    write(path, updated)


def require_absent(path: str, needle: str) -> None:
    if needle in read(path):
        raise RuntimeError(f"{path}: retired reference remains: {needle}")


# Rebuild from the reviewed semantic files while retaining current-main versions
# of every unrelated and concurrently changed surface.
for source_path in (
    "xtask/src/tasks/file_policy.rs",
    "xtask/tests/file_policy.rs",
    ".github/workflows/post-merge-status.yml",
    "docs/FILE_POLICY.md",
    "docs/development/FILE_POLICY_RAIL.md",
    "docs/policy/NON_RUST_LADDER.md",
    "docs/policy/NON_RUST_POLICY.md",
):
    write(source_path, show(source_path))

# The legacy --write spelling remains accepted for callers, but it is now an
# alias for target-local evidence and cannot recreate tracked authority.
file_policy = "xtask/src/tasks/file_policy.rs"
regex_once(
    file_policy,
    r"//! - `cargo xtask non-rust inventory --write`.*?//!\n(?=//! - `cargo xtask non-rust check)",
    "//! - `cargo xtask non-rust inventory --write` — compatibility alias for the same\\n"
    "//!   target-local outputs. It creates or updates no tracked inventory.\\n//!\\n",
)
regex_once(
    file_policy,
    r"/// Entry point for `cargo xtask non-rust inventory`\.\n///\n.*?(?=pub fn non_rust_inventory\()",
    "/// Entry point for `cargo xtask non-rust inventory`.\\n///\\n"
    "/// Writes current-tree Markdown and JSON evidence under `target/policy/`\\n"
    "/// and never modifies a tracked file. The legacy `--write` spelling is a\\n"
    "/// compatibility alias for this same target-local operation.\\n",
)
regex_once(
    file_policy,
    r"/// Regenerate the reference copy at `docs/policy/NON_RUST_INVENTORY\.md`\..*?\n"
    r"pub fn non_rust_inventory_write_docs\(root: &Path\) -> Result<\(\)> \{.*?\n\}\n\n/// Check",
    "/// Compatibility alias for the retired tracked-inventory writer.\\n"
    "///\\n"
    "/// Existing automation may keep passing `--write`, but the command now\\n"
    "/// emits only `target/policy/non-rust-inventory.{md,json}` and cannot\\n"
    "/// create or modify repository documentation.\\n"
    "pub fn non_rust_inventory_write_docs(root: &Path) -> Result<()> {\\n"
    "    eprintln!(\"warning: `non-rust inventory --write` is a compatibility alias; no tracked inventory is written\");\\n"
    "    non_rust_inventory(root)\\n"
    "}\\n\\n"
    "/// Check",
)
require_absent(file_policy, "docs/policy/NON_RUST_INVENTORY.md")

# Preserve current-main command additions while correcting the CLI contract.
main_rs = "xtask/src/main.rs"
replace_once(
    main_rs,
    "    /// Pass `--write` to also regenerate the committed snapshot at\n"
    "    /// `docs/policy/NON_RUST_INVENTORY.md`.\n",
    "    /// `--write` remains accepted as a compatibility alias, but still writes\n"
    "    /// only the target-local evidence files.\n",
)
replace_once(
    main_rs,
    "        /// Check classification and newly added files without rewriting outputs.\n"
    "        /// Require the generated Markdown snapshot to match the committed snapshot\n"
    "        /// after line-ending normalization.\n",
    "        /// Validate current-tree classification and newly added files against\n"
    "        /// the explicit comparison base.\n",
)
replace_once(
    main_rs,
    "        /// Also overwrite `docs/policy/NON_RUST_INVENTORY.md` with the\n"
    "        /// regenerated content.  Mutually exclusive with `--check`.\n",
    "        /// Compatibility alias for a target-local inventory run. No tracked\n"
    "        /// file is created or modified. Mutually exclusive with `--check`.\n",
)
require_absent(main_rs, "docs/policy/NON_RUST_INVENTORY.md")

# The focused integration test keeps the existing required aggregate as a
# transition backstop while proving the independently attributable exact-tree
# workflow and its evidence binding.
test_path = "xtask/tests/file_policy.rs"
replacement_test = r'''#[test]
fn non_rust_inventory_check_has_exact_tree_result_and_required_backstop() -> Result<()> {
    let root = project_root()?;
    let policy: Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(root.join(".ci/gate-policy.yaml"))?)?;
    let gate = policy
        .get("gates")
        .and_then(Value::as_sequence)
        .and_then(|gates| {
            gates.iter().find(|gate| {
                gate.get("name").and_then(Value::as_str) == Some("non_rust_inventory_check")
            })
        })
        .ok_or_else(|| eyre!("non_rust_inventory_check is missing from gate policy"))?;

    assert_eq!(gate.get("tier").and_then(Value::as_str), Some("merge_gate"));
    assert_eq!(gate.get("required").and_then(Value::as_bool), Some(true));
    assert_eq!(
        gate.get("command").and_then(Value::as_str),
        Some("cargo xtask non-rust inventory --check")
    );
    assert_eq!(gate.get("timeout_seconds").and_then(Value::as_u64), Some(300));
    assert_eq!(
        gate.get("budgets")
            .and_then(|budgets| budgets.get("max_duration_ms"))
            .and_then(Value::as_u64),
        Some(240_000)
    );

    let aggregate_mappings = policy
        .get("workflow_integration")
        .and_then(|integration| integration.get("job_mapping"))
        .and_then(|mapping| mapping.get("ci-gate"))
        .and_then(|job| job.get("gates"))
        .and_then(Value::as_sequence)
        .map(|gates| {
            gates
                .iter()
                .filter_map(Value::as_str)
                .filter(|name| *name == "non_rust_inventory_check")
                .count()
        })
        .unwrap_or_default();
    assert_eq!(
        aggregate_mappings, 1,
        "the existing required aggregate must remain a transition backstop"
    );

    let workflow_text =
        std::fs::read_to_string(root.join(".github/workflows/non-rust-policy.yml"))?;
    let workflow: Value = serde_yaml_ng::from_str(&workflow_text)?;
    let job = workflow
        .get("jobs")
        .and_then(|jobs| jobs.get("non-rust-policy"))
        .ok_or_else(|| eyre!("dedicated non-rust-policy job is missing"))?;
    ensure!(
        job.get("name").and_then(Value::as_str) == Some("Non-Rust policy exact-tree"),
        "dedicated result must be independently attributable"
    );

    let env = job.get("env").ok_or_else(|| eyre!("job env is missing"))?;
    ensure!(
        env.get("SUBJECT_SHA").and_then(Value::as_str).is_some_and(|value| {
            value.contains("github.event.pull_request.head.sha")
                && value.contains("github.event.merge_group.head_sha")
                && value.contains("inputs.head_sha")
        }),
        "job must select the exact PR, merge-group, or dispatch subject"
    );
    ensure!(
        env.get("BASE_SHA").and_then(Value::as_str).is_some_and(|value| {
            value.contains("github.event.pull_request.base.sha")
                && value.contains("github.event.merge_group.base_sha")
                && value.contains("inputs.base_sha")
                && value.contains("github.event.before")
        }),
        "job must select the explicit event comparison base"
    );

    let steps = job
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or_else(|| eyre!("job steps are missing"))?;
    let step_named = |name: &str| {
        steps.iter().find(|step| step.get("name").and_then(Value::as_str) == Some(name))
    };
    let checkout = step_named("Checkout exact non-Rust policy subject")
        .ok_or_else(|| eyre!("exact-subject checkout is missing"))?;
    ensure!(
        checkout.get("with").and_then(|with| with.get("ref")).and_then(Value::as_str)
            == Some("${{ env.SUBJECT_SHA }}"),
        "checkout must use the selected exact subject"
    );

    let binding = step_named("Bind policy evidence to the checked-out tree")
        .and_then(|step| step.get("run"))
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("candidate-binding step is missing"))?;
    ensure!(
        binding.contains("test \"$actual_sha\" = \"$SUBJECT_SHA\"")
            && binding.contains("git rev-parse --verify \"$BASE_SHA^{commit}\"")
            && binding.contains("SUBJECT_TREE_SHA=")
            && binding.contains("case \"$GITHUB_EVENT_NAME\" in"),
        "job must bind the selected commit, explicit base, and exact tree without confusing a PR head with GitHub's synthetic merge SHA"
    );

    let policy_step = step_named("Validate exact-tree non-Rust policy")
        .ok_or_else(|| eyre!("policy execution step is missing"))?;
    let policy_run = policy_step
        .get("run")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("policy command is missing"))?;
    ensure!(
        policy_run.contains(
            "cargo run --locked -p xtask -- gates --gate non_rust_inventory_check"
        ) && policy_run.contains("non-rust-policy-exact-tree.json"),
        "dedicated job must execute the governed gate and retain an exact-subject receipt"
    );
    ensure!(
        policy_step.get("env").and_then(|env| env.get("CI_SCOPE_BASE")).and_then(Value::as_str)
            == Some("${{ env.BASE_SHA }}"),
        "policy execution must receive the explicit comparison base"
    );

    let upload = step_named("Upload exact-tree non-Rust policy evidence")
        .ok_or_else(|| eyre!("artifact upload step is missing"))?;
    ensure!(
        upload.get("if").and_then(Value::as_str) == Some("always()"),
        "evidence must survive both pass and failure outcomes"
    );
    let artifact_paths = upload
        .get("with")
        .and_then(|with| with.get("path"))
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("artifact path set is missing"))?;
    for expected in [
        "target/policy/non-rust-inventory.md",
        "target/policy/non-rust-inventory.json",
        "target/policy/non-rust-policy-exact-tree.json",
    ] {
        ensure!(artifact_paths.contains(expected), "artifact set omits {expected}");
    }

    ensure!(
        !workflow_text.contains("\n    paths:") && !workflow_text.contains("\n      paths:"),
        "whole-tree policy cannot be hidden behind a path filter"
    );
    ensure!(
        std::fs::read_to_string(root.join(".github/workflows/ci.yml"))?
            .contains("non_rust_inventory_check"),
        "the required aggregate backstop must remain until settings promotion is proven"
    );
    Ok(())
}
'''
text = read(test_path)
text, count = re.subn(
    r"#\[test\]\nfn non_rust_inventory_check_is_wired_to_exact_tree_job\(\) -> Result<\(\)> \{.*\Z",
    replacement_test,
    text,
    count=1,
    flags=re.DOTALL,
)
if count != 1:
    raise RuntimeError(f"{test_path}: could not replace exact-tree wiring test")
text = text.replace(
    "    // docs/policy/NON_RUST_INVENTORY.md must NOT be rewritten by the\n"
    "    // non-`--write` path; the committed snapshot is updated only by\n"
    "    // `cargo xtask non-rust inventory --write`.\n",
    "    // No tracked documentation participates in this operation.\n",
)
write(test_path, text)
require_absent(test_path, "docs/policy/NON_RUST_INVENTORY.md")

# Retire the checked-in whole-tree projection and every direct writer/registry
# claim that would recreate it.
inventory_path = ROOT / "docs/policy/NON_RUST_INVENTORY.md"
if inventory_path.exists():
    inventory_path.unlink()

ignore_path = ".gitignore"
ignore_text = read(ignore_path)
ignore_block = (
    "\n# Legacy local export spelling retained by `cargo xtask non-rust inventory --write`.\n"
    "# Exact-tree policy evidence lives under target/policy/ and is not tracked.\n"
    "/docs/policy/NON_RUST_INVENTORY.md\n"
)
if "/docs/policy/NON_RUST_INVENTORY.md" not in ignore_text:
    write(ignore_path, ignore_text.rstrip("\n") + "\n" + ignore_block)

regex_once(
    "policy/generated-allowlist.toml",
    r"\n\[\[allow\]\]\nid = \"generated-non-rust-inventory\"\n.*?(?=\n\[\[allow\]\]|\Z)",
    "",
)
regex_once(
    "policy/tree-sitter-compat-inventory.toml",
    r"\n\[\[consumers\]\]\npath = \"docs/policy/NON_RUST_INVENTORY\.md\"\n.*?(?=\n\[\[consumers\]\]|\Z)",
    "",
)
replace_once(
    "tests/test_legacy_authority_banners.py",
    '    "docs/policy/NON_RUST_INVENTORY.md",\n',
    "",
)

# Correct the human-facing descriptions. The full inventory is an artifact of
# one exact tree, not a documentation page with implied live currentness.
replace_once(
    "docs/development/FILE_POLICY_RAIL.md",
    "current-tree Markdown/JSON generated under `target/policy/` for CI and documentation publication. `docs/policy/NON_RUST_INVENTORY.md` is only a reference snapshot, not byte-fresh merge authority.",
    "current-tree Markdown/JSON generated under `target/policy/` for exact-tree CI artifact publication. No full-tree inventory is tracked.",
)
replace_once(
    "docs/policy/NON_RUST_LADDER.md",
    "CI publishes both outputs and the documentation build includes the Markdown projection.",
    "the exact-tree CI lane publishes both outputs with a subject-bound receipt.",
)
replace_once(
    "docs/policy/NON_RUST_POLICY.md",
    "target/policy/non-rust-inventory.{md,json}  # current-tree generated inventory\n"
    "docs/policy/NON_RUST_INVENTORY.md  # optional reference snapshot; not merge authority",
    "target/policy/non-rust-inventory.{md,json}  # exact-tree generated evidence; not tracked",
)
for doc_path in (
    "docs/FILE_POLICY.md",
    "docs/development/FILE_POLICY_RAIL.md",
    "docs/policy/NON_RUST_LADDER.md",
    "docs/policy/NON_RUST_POLICY.md",
    ".github/workflows/post-merge-status.yml",
):
    require_absent(doc_path, "docs/policy/NON_RUST_INVENTORY.md")

# Retain the required aggregate lane, and register the named exact-tree result
# as an advisory duplicate until a separate settings transaction promotes it.
ci_lanes = "policy/ci-lanes.toml"
if "[lane.non_rust_policy]" not in read(ci_lanes):
    lane_block = '''[lane.non_rust_policy]
description = "Observe exact candidate-tree non-Rust classification and publish attributable evidence."
intent = "attributable file-policy enforcement"
runner = "ubuntu_24_04"
base_lem = 3
default_pr = true
blocking = false
paths = ["**"]
outputs = [
  "target/policy/non-rust-inventory.md",
  "target/policy/non-rust-inventory.json",
  "target/policy/non-rust-policy-exact-tree.json",
]
duplicate_of = ["gate:non_rust_inventory_check"]

'''
    replace_once(ci_lanes, "[lane.merge_gate_aggregate]\n", lane_block + "[lane.merge_gate_aggregate]\n")

lane_whitelist = "policy/ci-lane-whitelist.toml"
if 'id = "non_rust_policy"' not in read(lane_whitelist):
    whitelist_block = '''[[lane]]
id = "non_rust_policy"
workflow = ".github/workflows/non-rust-policy.yml"
job = "non-rust-policy"
tier = "merge_gate"
kind = "policy"
blocking = false
default_pr = true
runner = "ubuntu_24_04"
base_lem = 3
owner = "release/ci"
intent = "Observe the exact candidate tree against non-Rust classification policy and publish attributable evidence."
failure_mode = "A candidate adds an unclassified non-Rust path or the policy instrument cannot classify the exact merge subject."
proof_obligation = "Bind subject and base SHAs, run non_rust_inventory_check, and publish Markdown, JSON, and an exact-tree receipt."
evidence = [
  "target/policy/non-rust-inventory.md",
  "target/policy/non-rust-inventory.json",
  "target/policy/non-rust-policy-exact-tree.json",
]
allowed_triggers = ["pull_request", "push", "merge_group", "workflow_dispatch"]
expensive = false
duplicate_of = ["gate:non_rust_inventory_check"]
review_after = "2026-11-30"
expires = "2027-02-28"

'''
    replace_once(
        lane_whitelist,
        '[[lane]]\nid = "merge_gate_aggregate"\n',
        whitelist_block + '[[lane]]\nid = "merge_gate_aggregate"\n',
    )

replace_once(
    "scripts/ci/validate_gate_lane_mapping.py",
    '    "non_rust_inventory_check": {"lanes": ["merge_gate_shards"]},\n',
    '    "non_rust_inventory_check": {"lanes": ["merge_gate_shards", "non_rust_policy"]},\n',
)
replace_once(
    ".ci/gate-policy.yaml",
    '    description: "Scan and classify tracked non-Rust files and require the normalized committed snapshot to match"\n',
    '    description: "Validate current-tree non-Rust classification and emit reviewable inventory artifacts"\n',
)

economics_path = "docs/ci/gate-policy-economics.md"
economics = read(economics_path)
lane_count = len(re.findall(r"^\[lane\.", read(ci_lanes), flags=re.MULTILINE))
economics, count = re.subn(
    r"^- \d+ lanes in `policy/ci-lanes\.toml`$",
    f"- {lane_count} lanes in `policy/ci-lanes.toml`",
    economics,
    count=1,
    flags=re.MULTILINE,
)
if count != 1:
    raise RuntimeError(f"{economics_path}: lane count line not found")
if "| `non_rust_policy` |" not in economics:
    lines = economics.splitlines()
    for index, line in enumerate(lines):
        if line.startswith("| `merge_gate_shards` |"):
            lines.insert(index + 1, "| `non_rust_policy` | `non_rust_inventory_check` |")
            break
    else:
        raise RuntimeError(f"{economics_path}: merge_gate_shards row not found")
    economics = "\n".join(lines) + "\n"
write(economics_path, economics)

permanent_workflow = r'''name: Non-Rust policy exact-tree

on:
  pull_request:
    branches: [main, master]
    types: [opened, synchronize, reopened, ready_for_review]
  merge_group: {}
  push:
    branches: [main, master]
  workflow_dispatch:
    inputs:
      base_sha:
        description: Exact comparison-base commit SHA
        required: true
        type: string
      head_sha:
        description: Exact subject commit SHA; must equal the dispatch SHA
        required: true
        type: string

permissions:
  contents: read

concurrency:
  group: non-rust-policy-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' && github.event.action == 'synchronize' }}

jobs:
  non-rust-policy:
    name: Non-Rust policy exact-tree
    if: github.event_name != 'pull_request' || github.event.pull_request.draft != true
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    env:
      SUBJECT_SHA: >-
        ${{ github.event.pull_request.head.sha ||
            github.event.merge_group.head_sha ||
            inputs.head_sha ||
            github.sha }}
      BASE_SHA: >-
        ${{ github.event.pull_request.base.sha ||
            github.event.merge_group.base_sha ||
            inputs.base_sha ||
            github.event.before }}

    steps:
      - name: Checkout exact non-Rust policy subject
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          ref: ${{ env.SUBJECT_SHA }}
          fetch-depth: 0
          persist-credentials: false

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772 # stable (master)
        with:
          toolchain: 1.95.0

      - name: Bind policy evidence to the checked-out tree
        shell: bash
        run: |
          set -euo pipefail
          actual_sha="$(git rev-parse HEAD)"
          test "$actual_sha" = "$SUBJECT_SHA"
          case "$GITHUB_EVENT_NAME" in
            pull_request) ;;
            *) test "$SUBJECT_SHA" = "$GITHUB_SHA" ;;
          esac
          git rev-parse --verify "$BASE_SHA^{commit}" >/dev/null
          subject_tree_sha="$(git rev-parse "$SUBJECT_SHA^{tree}")"
          echo "SUBJECT_TREE_SHA=$subject_tree_sha" >> "$GITHUB_ENV"

      - name: Validate exact-tree non-Rust policy
        id: policy
        shell: bash
        env:
          CI_SCOPE_BASE: ${{ env.BASE_SHA }}
        run: |
          set +e
          cargo run --locked -p xtask -- gates --gate non_rust_inventory_check
          rc=$?
          set -e

          result=pass
          if [ "$rc" -ne 0 ]; then
            result=fail
          fi
          echo "result=$result" >> "$GITHUB_OUTPUT"
          echo "exit_code=$rc" >> "$GITHUB_OUTPUT"

          mkdir -p target/policy
          POLICY_RESULT="$result" POLICY_EXIT_CODE="$rc" python3 - <<'PY'
          import json
          import os
          from pathlib import Path

          receipt = {
              "schema": "non_rust_policy_exact_tree.v1",
              "event": os.environ["GITHUB_EVENT_NAME"],
              "subject_sha": os.environ["SUBJECT_SHA"],
              "subject_tree_sha": os.environ["SUBJECT_TREE_SHA"],
              "base_sha": os.environ["BASE_SHA"],
              "result": os.environ["POLICY_RESULT"],
              "exit_code": int(os.environ["POLICY_EXIT_CODE"]),
          }
          Path("target/policy/non-rust-policy-exact-tree.json").write_text(
              json.dumps(receipt, indent=2, sort_keys=True) + "\n",
              encoding="utf-8",
          )
          PY
          exit "$rc"

      - name: Publish exact-tree policy summary
        if: always()
        shell: bash
        run: |
          {
            echo "### Non-Rust policy exact-tree"
            echo
            echo "- Subject: \`$SUBJECT_SHA\`"
            echo "- Tree: \`${SUBJECT_TREE_SHA:-not_proven}\`"
            echo "- Base: \`$BASE_SHA\`"
            echo "- Result: \`${{ steps.policy.outputs.result || 'not_proven' }}\`"
            echo
            if [ -f target/policy/non-rust-inventory.md ]; then
              sed -n '1,24p' target/policy/non-rust-inventory.md
            else
              echo "Inventory output was not produced."
            fi
          } >> "$GITHUB_STEP_SUMMARY"

      - name: Upload exact-tree non-Rust policy evidence
        if: always()
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1
        with:
          name: non-rust-policy-${{ env.SUBJECT_SHA }}
          path: |
            target/policy/non-rust-inventory.md
            target/policy/non-rust-inventory.json
            target/policy/non-rust-policy-exact-tree.json
          if-no-files-found: error
          retention-days: 30
'''
write(".github/workflows/non-rust-policy.yml", permanent_workflow)

# The one-shot repair surface removes itself before the candidate commit.
for temporary_path in (
    ".github/workflows/repair-14374.yml",
    "scripts/maintenance/integrate_non_rust_policy_14161.py",
):
    path = ROOT / temporary_path
    if path.exists():
        path.unlink()

# Stage first so git-backed inventories observe the candidate tree, then
# regenerate dependent projections from their owning tools.
run("git", "add", "-A")
run("cargo", "run", "--locked", "-p", "xtask", "--", "compat-inventory")
run("git", "add", "-A")
run("cargo", "fmt", "-p", "xtask", "--", "--check")
run("git", "diff", "--cached", "--check")

run("git", "config", "user.name", "EffortlessSteven")
run("git", "config", "user.email", "git@effortlesssteven.com")
run("git", "commit", "-m", "ci(policy): integrate exact-tree non-Rust policy (#14161)")

# Execute proof against the exact candidate commit. Nothing is pushed if any
# command fails.
run("cargo", "test", "-p", "xtask", "--locked", "--test", "file_policy")
run("cargo", "test", "-p", "xtask", "--locked", "--lib", "file_policy")
run("python3", "-m", "unittest", "tests/test_legacy_authority_banners.py")
run("python3", "scripts/ci/validate_gate_lane_mapping.py", "--strict", "--workflow", ".github/workflows/ci.yml")
run("cargo", "run", "--locked", "-p", "xtask", "--", "gate-policy", "check")
run("cargo", "run", "--locked", "-p", "xtask", "--", "check-generated")
run("cargo", "run", "--locked", "-p", "xtask", "--", "check-file-policy")
check_env = os.environ.copy()
check_env["CI_SCOPE_BASE"] = BASE_SHA
run(
    "cargo",
    "run",
    "--locked",
    "-p",
    "xtask",
    "--",
    "non-rust",
    "inventory",
    "--check",
    env=check_env,
)
run("git", "diff", "HEAD^", "--check")
if output("git", "status", "--porcelain"):
    raise RuntimeError("verification changed the candidate worktree")

run("git", "push", "origin", f"HEAD:{TARGET_BRANCH}")
print("integration repair pushed", output("git", "rev-parse", "HEAD").strip())
