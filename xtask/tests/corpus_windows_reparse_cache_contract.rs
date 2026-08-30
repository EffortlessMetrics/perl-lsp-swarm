//! Contract for the Windows reparse proof workflow's cache and proof boundary.

use std::{collections::BTreeSet, fs, path::PathBuf};

use anyhow::{Result, anyhow, ensure};
use serde_yaml_ng::Value;

const CACHE_ACTION: &str = "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6";
const SHARED_KEY: &str = "corpus-windows-reparse-proof-${{ hashFiles('Cargo.lock') }}";
const TRUSTED_SAVE_IF: &str =
    "${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}";
const CORPUS_PROOF: &str = "cargo test --locked -p perl-corpus --test strict_sectioned_loading public_plain_loader_rejects_windows_reparse_point -- --exact --nocapture";
const XTASK_PROOF: &str = "cargo test --locked -p xtask --lib dangling_protected_source_rejects_before_publication_write -- --nocapture";
const CORPUS_PROOF_ANCHOR: &str = "        run: >-\n          cargo test --locked -p perl-corpus\n          --test strict_sectioned_loading\n          public_plain_loader_rejects_windows_reparse_point\n          -- --exact --nocapture";
const XTASK_PROOF_ANCHOR: &str = "        run: >-\n          cargo test --locked -p xtask --lib\n          dangling_protected_source_rejects_before_publication_write\n          -- --nocapture";
const TOPOLOGY_EXECUTION_ANCHOR: &str = "for test_name in \"${selected_tests[@]}\"; do\n  test_output=\"$(mktemp)\"\n  if ! cargo test --locked -p perl-corpus --lib \"$test_name\" \\\n    -- --exact --nocapture \\\n    2>&1 | sed 's/\\r$//' | tee \"$test_output\"; then";
const TOPOLOGY_EXECUTION_SOURCE_ANCHOR: &str = "          for test_name in \"${selected_tests[@]}\"; do\n            test_output=\"$(mktemp)\"";

const EXPECTED_TOPOLOGY_TESTS: [&str; 10] = [
    "api::topology::tests::binding_rejects_intermediate_runtime_root_symlink",
    "api::topology::tests::excluded_metadata_symlink_does_not_block_discovery",
    "api::topology::tests::dangling_excluded_metadata_symlink_does_not_block_discovery",
    "api::topology::tests::symlinked_entries_fail_closed",
    "api::topology::tests::dangling_selected_symlink_fails_as_symlink_unsupported",
    "api::topology::tests::symlinked_test_directory_cannot_hide_selected_descendants",
    "api::topology::tests::symlinked_fuzz_directory_cannot_hide_selected_descendants",
    "api::topology::tests::symlinked_directory_target_inside_root_still_fails_closed",
    "api::topology::tests::symlinked_directory_target_outside_root_fails_closed",
    "api::topology::tests::nested_intermediate_directory_symlink_fails_closed",
];

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn actual_workflow() -> Result<String> {
    Ok(fs::read_to_string(
        project_root().join(".github/workflows/corpus-windows-reparse-proof.yml"),
    )?)
}

fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn replace_once(source: &str, from: &str, to: &str) -> Result<String> {
    ensure!(source.matches(from).count() == 1, "fixture anchor must occur exactly once: {from}");
    Ok(source.replacen(from, to, 1))
}

fn validate_workflow(source: &str) -> Result<()> {
    let workflow: Value = serde_yaml_ng::from_str(source)?;
    let events = workflow
        .get("on")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must declare a mapping of events"))?;
    let trigger = events
        .get("pull_request")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must have a structured pull_request trigger"))?;
    let branches: BTreeSet<_> = trigger
        .get("branches")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("pull_request must declare branches"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    ensure!(
        branches == BTreeSet::from(["main", "master"]),
        "pull_request must target only main and master"
    );
    ensure!(
        events.len() == 2
            && events.contains_key("pull_request")
            && events.contains_key("workflow_dispatch"),
        "workflow must not add extra trigger types"
    );
    let paths: BTreeSet<_> = trigger
        .get("paths")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("pull_request must declare paths"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    ensure!(
        paths
            == BTreeSet::from([
                "Cargo.lock",
                "Cargo.toml",
                ".github/workflows/corpus-windows-reparse-proof.yml",
                "crates/perl-corpus/**",
                "xtask/src/publication_drift/**",
            ]),
        "pull_request paths must preserve the Windows proof trigger scope"
    );
    ensure!(events.get("workflow_dispatch").is_some(), "workflow_dispatch must remain available");
    ensure!(events.get("pull_request_target").is_none(), "pull_request_target is not allowed");

    let permissions = workflow
        .get("permissions")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must declare permissions"))?;
    ensure!(
        permissions.get("contents").and_then(Value::as_str) == Some("read"),
        "the workflow must grant contents read permission"
    );
    ensure!(permissions.len() == 1, "workflow permissions must not grant extra scopes");
    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must declare jobs"))?;
    let job = jobs
        .get("windows-reparse-proof")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the workflow must declare the Windows reparse proof job"))?;
    ensure!(job.get("permissions").is_none(), "the proof job must not add permissions");
    ensure!(
        job.get("runs-on").and_then(Value::as_str) == Some("windows-2022"),
        "the Windows proof must run on windows-2022"
    );

    let steps = job
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("the Windows proof job must declare steps"))?;
    let all_steps: Vec<_> = jobs
        .values()
        .filter_map(Value::as_mapping)
        .filter_map(|candidate| candidate.get("steps"))
        .filter_map(Value::as_sequence)
        .flat_map(|candidate| candidate.iter())
        .collect();
    ensure!(
        all_steps.iter().all(|step| {
            !step
                .get("uses")
                .and_then(Value::as_str)
                .is_some_and(|uses| uses.starts_with("actions/cache"))
        }),
        "alternate actions/cache writers are not allowed"
    );

    let cache_steps: Vec<_> = steps
        .iter()
        .filter(|step| step.get("uses").and_then(Value::as_str) == Some(CACHE_ACTION))
        .collect();
    ensure!(
        cache_steps.len() == 1,
        "the Windows proof job must have exactly one pinned rust-cache step"
    );
    let cache_with = cache_steps[0]
        .get("with")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the rust-cache step must declare inputs"))?;
    ensure!(
        cache_with.get("save-if").and_then(Value::as_str) == Some(TRUSTED_SAVE_IF),
        "rust-cache must save only for the canonical main/master refs"
    );
    ensure!(
        cache_with.get("cache-on-failure") == Some(&Value::Bool(true)),
        "rust-cache failure reuse must remain enabled"
    );
    ensure!(
        cache_with.get("shared-key").and_then(Value::as_str) == Some(SHARED_KEY),
        "rust-cache must use the exact shared key"
    );

    let named_step = |name: &str| -> Result<&Value> {
        steps
            .iter()
            .find(|step| step.get("name").and_then(Value::as_str) == Some(name))
            .ok_or_else(|| anyhow!("missing proof step: {name}"))
    };
    let require_command = |name: &str, command: &str| -> Result<()> {
        let step = named_step(name)?;
        ensure!(step.get("if").is_none(), "proof step must not be conditionally skipped: {name}");
        ensure!(
            normalized(step.get("run").and_then(Value::as_str).unwrap_or_default())
                == normalized(command),
            "proof step must run its exact production command: {name}"
        );
        ensure!(
            step.get("env")
                .and_then(Value::as_mapping)
                .and_then(|env| env.get("PLSW_REQUIRE_SYMLINK_PRIVILEGE"))
                .and_then(Value::as_str)
                == Some("1"),
            "proof step must require real Windows symlink privilege: {name}"
        );
        Ok(())
    };
    require_command("Run non-skipping Windows reparse proof", CORPUS_PROOF)?;
    require_command("Run non-skipping xtask reparse proof", XTASK_PROOF)?;

    let cache_index = steps
        .iter()
        .position(|step| step.get("uses").and_then(Value::as_str) == Some(CACHE_ACTION))
        .ok_or_else(|| anyhow!("missing pinned cache step"))?;
    for name in [
        "Run non-skipping Windows reparse proof",
        "Run non-skipping xtask reparse proof",
        "Run exact non-skipping perl-corpus topology proofs",
    ] {
        let index = steps
            .iter()
            .position(|step| step.get("name").and_then(Value::as_str) == Some(name))
            .ok_or_else(|| anyhow!("missing proof step: {name}"))?;
        ensure!(cache_index < index, "cache restore must precede proof execution: {name}");
    }

    let topology = named_step("Run exact non-skipping perl-corpus topology proofs")?;
    ensure!(topology.get("if").is_none(), "topology proof must not be conditionally skipped");
    let topology_raw = topology.get("run").and_then(Value::as_str).unwrap_or_default();
    let topology_run = normalized(topology_raw);
    for command in [
        "set -euo pipefail",
        "cargo test --locked -p perl-corpus --lib -- --list",
        "cargo test --locked -p perl-corpus --lib \"$test_name\" \\\n              -- --exact --nocapture \\",
        "running_count=",
        "result_count=",
    ] {
        ensure!(
            topology_run.contains(&normalized(command)),
            "topology proof is missing its production guard: {command}"
        );
    }
    ensure!(
        topology_run.contains("cargo test --locked -p perl-corpus --lib -- --list \\ | sed 's/\\r$//' \\ | tee \"$test_list\""),
        "topology proof must enumerate the real corpus test list"
    );
    let selected_start = topology_raw
        .find("selected_tests=(")
        .ok_or_else(|| anyhow!("topology proof must declare selected tests"))?;
    let selected_end = topology_raw[selected_start..]
        .find(")\n")
        .map(|offset| selected_start + offset)
        .ok_or_else(|| anyhow!("topology proof selected tests must be closed"))?;
    let selected: BTreeSet<_> = topology_raw
        [selected_start + "selected_tests=(".len()..selected_end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    ensure!(
        selected == BTreeSet::from(EXPECTED_TOPOLOGY_TESTS),
        "topology proof must select the complete production corpus"
    );
    let loop_marker = "for test_name in \"${selected_tests[@]}\"; do";
    ensure!(
        topology_raw.matches(loop_marker).count() == 2,
        "topology proof must retain both production loops"
    );
    let execution_loop_start = topology_raw
        .match_indices(loop_marker)
        .nth(1)
        .map(|(index, _)| index)
        .ok_or_else(|| anyhow!("topology proof must retain its execution loop"))?;
    let execution_loop = &topology_raw[execution_loop_start..];
    ensure!(
        execution_loop.contains(TOPOLOGY_EXECUTION_ANCHOR),
        "topology proof must execute the real selected-test command"
    );
    ensure!(
        execution_loop.contains("running_count=\"$(grep -Ec '^running 1 test$'"),
        "topology proof must verify one executed test"
    );
    ensure!(
        execution_loop.contains("result_count=\"$(grep -Ec '^test result: ok\\. 1 passed;"),
        "topology proof must verify the exact passing result"
    );
    ensure!(
        execution_loop
            .contains("if [[ \"$running_count\" -ne 1 || \"$result_count\" -ne 1 ]]; then"),
        "topology proof must fail on skipped or multiply-run tests"
    );
    ensure!(
        topology
            .get("env")
            .and_then(Value::as_mapping)
            .and_then(|env| env.get("PLSW_REQUIRE_SYMLINK_PRIVILEGE"))
            .and_then(Value::as_str)
            == Some("1"),
        "topology proof must require real Windows symlink privilege"
    );
    Ok(())
}

#[test]
fn corpus_windows_reparse_workflow_matches_production_contract() -> Result<()> {
    validate_workflow(&actual_workflow()?)
}

#[test]
fn contract_rejects_realistic_cache_and_permission_mutations() -> Result<()> {
    let source = actual_workflow()?;
    for (from, to) in [
        (CACHE_ACTION, "Swatinem/rust-cache@v2"),
        (
            "save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}",
            "save-if: true",
        ),
        (SHARED_KEY, "cross-branch-key"),
        ("permissions:\n  contents: read", "permissions:\n  contents: read\n  actions: write"),
        (
            "      - name: Cache cargo dependencies",
            "      - uses: actions/cache/save@v4\n        with:\n          path: target\n          key: decoy\n\n      - name: Cache cargo dependencies",
        ),
        (
            "  windows-reparse-proof:\n    name:",
            "  windows-reparse-proof:\n    permissions:\n      contents: write\n    name:",
        ),
        (
            "  windows-reparse-proof:\n    name:",
            "  windows-reparse-proof:\n    permissions:\n      id-token: write\n    name:",
        ),
    ] {
        ensure!(
            validate_workflow(&replace_once(&source, from, to)?).is_err(),
            "realistic workflow mutation must be rejected: {from}"
        );
    }
    Ok(())
}

#[test]
fn contract_rejects_decoy_commands_and_trigger_mutations() -> Result<()> {
    let source = actual_workflow()?;
    for (from, to) in [
        (CORPUS_PROOF_ANCHOR, "        run: echo cargo test --locked -p perl-corpus"),
        (XTASK_PROOF_ANCHOR, "        run: echo cargo test --locked -p xtask"),
        ("branches: [main, master]", "branches: [feature/cache]"),
        ("pull_request:", "pull_request_target:"),
        ("  workflow_dispatch:\n", "  workflow_dispatch:\n  push:\n"),
        ("  workflow_dispatch:\n", "  workflow_dispatch:\n  schedule:\n    - cron: '0 0 * * *'\n"),
        (
            "            api::topology::tests::binding_rejects_intermediate_runtime_root_symlink\n",
            "",
        ),
        (TOPOLOGY_EXECUTION_SOURCE_ANCHOR, "          echo cargo test --locked -p perl-corpus\n"),
        (TOPOLOGY_EXECUTION_SOURCE_ANCHOR, ""),
    ] {
        ensure!(
            validate_workflow(&replace_once(&source, from, to)?).is_err(),
            "realistic proof or trigger mutation must be rejected: {from}"
        );
    }
    Ok(())
}
