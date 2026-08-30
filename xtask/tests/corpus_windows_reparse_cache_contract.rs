//! Static topology contract for the Windows reparse proof workflow.
//!
//! This proves the declared YAML structure, pinned action identities, trigger and
//! permission shape, cache inputs/order, and non-vacuous command text. It does
//! not prove runtime cache restore/save provenance, discovery of arbitrary
//! shell-local writers, or trusted runner `cargo`/`PATH` resolution; those are
//! NOT_PROVEN here and require runtime or host evidence.

use std::{collections::BTreeSet, fs, path::PathBuf};

use anyhow::{Result, anyhow, ensure};
use serde_yaml_ng::Value;

const CACHE_ACTION: &str = "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6";
const CHECKOUT_ACTION: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const TOOLCHAIN_ACTION: &str = "dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772";
const CHECKOUT_REF: &str = "${{ github.event.pull_request.head.sha || github.sha }}";
const CHECKOUT_REF_LINE: &str =
    "          ref: ${{ github.event.pull_request.head.sha || github.sha }}\n";
const SHARED_KEY: &str = "corpus-windows-reparse-proof-${{ hashFiles('Cargo.lock') }}";
const TRUSTED_SAVE_IF: &str =
    "${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}";
const CORPUS_PROOF: &str = "cargo test --locked -p perl-corpus --test strict_sectioned_loading public_plain_loader_rejects_windows_reparse_point -- --exact --nocapture";
const XTASK_PROOF: &str = "cargo test --locked -p xtask --lib publication_drift::output_tests::dangling_protected_source_rejects_before_publication_write -- --exact --nocapture";
const CORPUS_PROOF_ANCHOR: &str = "          $output = cargo test --locked -p perl-corpus --test strict_sectioned_loading public_plain_loader_rejects_windows_reparse_point -- --exact --nocapture 2>&1\n";
const XTASK_PROOF_ANCHOR: &str = "          $output = cargo test --locked -p xtask --lib publication_drift::output_tests::dangling_protected_source_rejects_before_publication_write -- --exact --nocapture 2>&1\n";
const CORPUS_OUTPUT_REPLAY_ANCHOR: &str = "          $output = cargo test --locked -p perl-corpus --test strict_sectioned_loading public_plain_loader_rejects_windows_reparse_point -- --exact --nocapture 2>&1\n          $exitCode = $LASTEXITCODE\n          $output | ForEach-Object { $_ }\n";
const TOPOLOGY_EXECUTION_ANCHOR: &str = "for test_name in \"${selected_tests[@]}\"; do\n  test_output=\"$(mktemp)\"\n  if ! cargo test --locked -p perl-corpus --lib \"$test_name\" \\\n    -- --exact --nocapture \\\n    2>&1 | sed 's/\\r$//' | tee \"$test_output\"; then";
const TOPOLOGY_EXECUTION_SOURCE_ANCHOR: &str = "          for test_name in \"${selected_tests[@]}\"; do\n            test_output=\"$(mktemp)\"";
const TOPOLOGY_EXECUTION_COMMAND_SOURCE_ANCHOR: &str =
    "            if ! cargo test --locked -p perl-corpus --lib \"$test_name\" \\\n";
const TOPOLOGY_SHELL_SOURCE_ANCHOR: &str = "        shell: bash\n        run: |\n          set -euo pipefail\n\n          selected_tests=(";

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
        "workflow must expose only pull_request and workflow_dispatch"
    );
    ensure!(
        events.get("workflow_dispatch") == Some(&Value::Null),
        "workflow_dispatch must not declare inputs or payload"
    );
    ensure!(
        trigger.len() == 2 && trigger.get("types").is_none(),
        "pull_request must not add extra activity types"
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
    ensure!(
        jobs.len() == 1 && jobs.contains_key("windows-reparse-proof"),
        "workflow must not hide additional jobs"
    );
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
            step.get("uses").and_then(Value::as_str).is_none_or(|uses| {
                [CHECKOUT_ACTION, TOOLCHAIN_ACTION, CACHE_ACTION].contains(&uses)
            })
        }),
        "actions must use the approved immutable refs; alternate or local writers are not allowed"
    );
    ensure!(
        all_steps.iter().all(|step| {
            step.get("run").and_then(Value::as_str).is_none_or(|run| {
                let run = run.to_ascii_lowercase();
                !run.contains("actions/cache")
                    && !run.contains("swatinem/rust-cache")
                    && !run.contains("cache/save")
                    && !run.contains("cache/restore")
            })
        }),
        "shell bodies must not hide alternate cache writers"
    );
    ensure!(
        all_steps.iter().all(|step| {
            step.get("run").and_then(Value::as_str).is_none_or(|run| {
                let run = run.to_ascii_lowercase();
                !run.contains(".ps1")
                    && !run.contains("curl")
                    && !run.contains("invoke-webrequest")
                    && !run.contains("invoke-restmethod")
            })
        }),
        "shell bodies must not hide script or network cache writers"
    );
    ensure!(
        jobs.values()
            .filter_map(Value::as_mapping)
            .all(|candidate| candidate.get("permissions").is_none()),
        "no proof job may add permissions"
    );

    let cache_steps: Vec<_> = steps
        .iter()
        .filter(|step| step.get("uses").and_then(Value::as_str) == Some(CACHE_ACTION))
        .collect();
    ensure!(
        cache_steps.len() == 1,
        "the Windows proof job must have exactly one pinned rust-cache step"
    );
    for action in [CHECKOUT_ACTION, TOOLCHAIN_ACTION, CACHE_ACTION] {
        ensure!(
            all_steps
                .iter()
                .filter(|step| step.get("uses").and_then(Value::as_str) == Some(action))
                .count()
                == 1,
            "each approved action must occur exactly once: {action}"
        );
    }
    let checkout_step = steps
        .iter()
        .find(|step| step.get("uses").and_then(Value::as_str) == Some(CHECKOUT_ACTION))
        .ok_or_else(|| anyhow!("missing pinned checkout step"))?;
    let checkout_with = checkout_step
        .get("with")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("checkout must declare candidate inputs"))?;
    ensure!(
        checkout_with.get("ref").and_then(Value::as_str) == Some(CHECKOUT_REF),
        "checkout must use the exact candidate ref expression"
    );
    ensure!(
        checkout_with.get("persist-credentials") == Some(&Value::Bool(false)),
        "checkout must disable persisted credentials"
    );
    ensure!(cache_steps[0].get("if").is_none(), "cache restore must not be conditionally disabled");
    let cache_with = cache_steps[0]
        .get("with")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("the rust-cache step must declare inputs"))?;
    ensure!(
        cache_with.get("save-if").and_then(Value::as_str) == Some(TRUSTED_SAVE_IF),
        "rust-cache must save only for canonical default-branch refs"
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
        let matches: Vec<_> = steps
            .iter()
            .filter(|step| step.get("name").and_then(Value::as_str) == Some(name))
            .collect();
        ensure!(matches.len() == 1, "proof step name must be unique: {name}");
        matches.first().copied().ok_or_else(|| anyhow!("missing proof step: {name}"))
    };
    let require_command = |name: &str, command: &str| -> Result<()> {
        let step = named_step(name)?;
        ensure!(step.get("if").is_none(), "proof step must not be conditionally skipped: {name}");
        ensure!(
            step.get("continue-on-error").is_none()
                || step.get("continue-on-error") == Some(&Value::Bool(false)),
            "proof step must not continue on error: {name}"
        );
        let run = step.get("run").and_then(Value::as_str).unwrap_or_default();
        let expected_command = format!("$output = {command} 2>&1");
        let statements: Vec<_> =
            run.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
        let lower_statements: Vec<_> =
            statements.iter().map(|line| line.to_ascii_lowercase()).collect();
        let expected_command_lower = expected_command.to_ascii_lowercase();
        ensure!(
            lower_statements.first().map(String::as_str) == Some(expected_command_lower.as_str())
                && lower_statements.get(1).map(String::as_str) == Some("$exitcode = $lastexitcode")
                && lower_statements.get(2).map(String::as_str)
                    == Some("$output | foreach-object { $_ }"),
            "proof command must be the first top-level PowerShell statement: {name}"
        );
        let exit_guard = "if ($exitcode -ne 0) { exit $exitcode }";
        let exit_guard_index = lower_statements
            .iter()
            .position(|line| line == exit_guard)
            .ok_or_else(|| anyhow!("proof command must have an exit-code guard: {name}"))?;
        let running_count_index = lower_statements
            .iter()
            .position(|line| line.starts_with("$runningcount = @($output | select-string"))
            .ok_or_else(|| anyhow!("proof command must count executed tests: {name}"))?;
        let result_count_index = lower_statements
            .iter()
            .position(|line| line.starts_with("$resultcount = @($output | select-string"))
            .ok_or_else(|| anyhow!("proof command must count passing tests: {name}"))?;
        ensure!(
            exit_guard_index < running_count_index
                && exit_guard_index < result_count_index
                && lower_statements.iter().filter(|line| line.starts_with("$output =")).count()
                    == 1
                && !lower_statements.iter().any(|line| line.starts_with("$output +="))
                && !run.to_ascii_lowercase().contains("return")
                && !run.to_ascii_lowercase().contains("exit 0"),
            "proof command must fail before validating captured output: {name}"
        );
        ensure!(
            normalized(run).contains(&normalized(command)),
            "proof step must run its exact production command: {name}"
        );
        ensure!(
            run.contains("$runningCount = @($output | Select-String")
                && run.contains("$resultCount = @($output | Select-String")
                && run.contains("if ($runningCount -ne 1 -or $resultCount -ne 1)"),
            "proof step must require one executed passing test: {name}"
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
    let setup_indices: Vec<_> = [CHECKOUT_ACTION, TOOLCHAIN_ACTION]
        .iter()
        .map(|action| {
            steps
                .iter()
                .position(|step| step.get("uses").and_then(Value::as_str) == Some(action))
                .ok_or_else(|| anyhow!("missing setup action: {action}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let checkout_index =
        *setup_indices.first().ok_or_else(|| anyhow!("missing checkout ordering index"))?;
    let toolchain_index =
        *setup_indices.get(1).ok_or_else(|| anyhow!("missing toolchain ordering index"))?;
    ensure!(
        checkout_index < toolchain_index && toolchain_index < cache_index,
        "cache restore must follow checkout and toolchain in order"
    );
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
    ensure!(
        topology.get("continue-on-error").is_none()
            || topology.get("continue-on-error") == Some(&Value::Bool(false)),
        "topology proof must not continue on error"
    );
    let topology_raw = topology.get("run").and_then(Value::as_str).unwrap_or_default();
    let topology_run = normalized(topology_raw);
    ensure!(
        topology_raw.lines().map(str::trim).find(|line| !line.is_empty())
            == Some("set -euo pipefail"),
        "topology proof must begin with its fail-closed shell mode"
    );
    ensure!(
        !topology_raw.contains("continue")
            && !topology_raw.contains("break")
            && !topology_raw.contains("return")
            && !topology_raw.contains("exit 0")
            && !topology_raw.contains("unset selected_tests")
            && !topology_raw.contains("if false")
            && !topology_raw.contains("if [[ false"),
        "topology proof must not bypass execution with continue or false guards"
    );
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
        topology_run.contains(
            "cargo test --locked -p perl-corpus --lib -- --list \\ | sed 's/\\r$//' \\ | tee \"$test_list\""
        ),
        "topology proof must enumerate the real corpus test list"
    );
    let selected_start = topology_raw
        .find("selected_tests=(")
        .ok_or_else(|| anyhow!("topology proof must declare selected tests"))?;
    let selected_end = topology_raw[selected_start..]
        .find(")\n")
        .map(|offset| selected_start + offset)
        .ok_or_else(|| anyhow!("topology proof selected tests must be closed"))?;
    ensure!(
        topology_raw.matches("selected_tests=(").count() == 1
            && !topology_raw.contains("selected_tests=()"),
        "topology proof must not clear its selected test population"
    );
    let after_selected_declaration = &topology_raw[selected_end + 2..];
    ensure!(
        !after_selected_declaration.lines().map(str::trim).any(|line| {
            let line = line.to_ascii_lowercase();
            line.starts_with("selected_tests=")
                || line.starts_with("selected_tests+=")
                || line.starts_with("selected_tests[") && line.contains("]=")
                || line == "unset selected_tests"
                || line.starts_with("unset selected_tests[")
                || line.starts_with("unset 'selected_tests[")
                || line.starts_with("unset \"selected_tests[")
                || line.starts_with("declare selected_tests")
                || line.starts_with("declare -a selected_tests")
                || line.starts_with("typeset selected_tests")
                || line.starts_with("typeset -a selected_tests")
                || (line.contains("mapfile") || line.contains("readarray"))
                    && line.contains("selected_tests")
        }),
        "topology proof must not mutate selected tests after declaration"
    );
    let selected: BTreeSet<_> = topology_raw
        [selected_start + "selected_tests=(".len()..selected_end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let selected_count = topology_raw[selected_start + "selected_tests=(".len()..selected_end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .count();
    ensure!(
        selected_count == EXPECTED_TOPOLOGY_TESTS.len()
            && selected == BTreeSet::from(EXPECTED_TOPOLOGY_TESTS),
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
fn static_workflow_topology_matches_production_contract() -> Result<()> {
    validate_workflow(&actual_workflow()?)
}

#[test]
fn static_contract_rejects_structural_cache_and_permission_mutations() -> Result<()> {
    let source = actual_workflow()?;
    for (from, to) in [
        (CACHE_ACTION, "Swatinem/rust-cache@v2"),
        (CHECKOUT_ACTION, "actions/checkout@v7"),
        (TOOLCHAIN_ACTION, "dtolnay/rust-toolchain@master"),
        (CHECKOUT_ACTION, "./.github/actions/cache-writer"),
        (CHECKOUT_REF_LINE, "          ref: ${{ github.sha }}\n"),
        ("persist-credentials: false", "persist-credentials: true"),
        (
            "save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}",
            "save-if: true",
        ),
        (SHARED_KEY, "cross-branch-key"),
        (
            "      - name: Cache cargo dependencies\n",
            "      - name: Cache cargo dependencies\n        if: ${{ github.ref == 'refs/heads/main' }}\n",
        ),
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
        ("jobs:\n", "jobs:\n  hidden-cache-writer:\n    runs-on: ubuntu-latest\n    steps: []\n"),
    ] {
        ensure!(
            validate_workflow(&replace_once(&source, from, to)?).is_err(),
            "realistic workflow mutation must be rejected: {from}"
        );
    }
    Ok(())
}

#[test]
fn static_contract_rejects_structural_proof_and_trigger_mutations() -> Result<()> {
    let source = actual_workflow()?;
    for (from, to) in [
        (CORPUS_PROOF_ANCHOR, "        run: echo cargo test --locked -p perl-corpus"),
        (XTASK_PROOF_ANCHOR, "        run: echo cargo test --locked -p xtask"),
        (
            CORPUS_PROOF_ANCHOR,
            "          $output = @('running 1 test', 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.0s')\n",
        ),
        (
            "      - name: Verify exact candidate checkout\n",
            "      - name: Run non-skipping Windows reparse proof\n",
        ),
        (
            "      - name: Run non-skipping Windows reparse proof\n        shell: pwsh\n",
            "      - name: Run non-skipping Windows reparse proof\n        shell: pwsh\n        continue-on-error: true\n",
        ),
        (
            TOPOLOGY_SHELL_SOURCE_ANCHOR,
            "        shell: bash\n        if: ${{ false }}\n        run: |\n          set -euo pipefail\n\n          selected_tests=(",
        ),
        ("branches: [main, master]", "branches: [feature/cache]"),
        ("pull_request:", "pull_request_target:"),
        ("    branches: [main, master]\n", "    branches: [main, master]\n    types: [opened]\n"),
        ("  workflow_dispatch:\n", "  workflow_dispatch:\n  push:\n"),
        ("  workflow_dispatch:\n", "  workflow_dispatch:\n  schedule:\n    - cron: '0 0 * * *'\n"),
        (
            "  workflow_dispatch:\n",
            "  workflow_dispatch:\n    inputs:\n      reason:\n        required: false\n",
        ),
        (
            CORPUS_OUTPUT_REPLAY_ANCHOR,
            "          $output = cargo test --locked -p perl-corpus --test strict_sectioned_loading public_plain_loader_rejects_windows_reparse_point -- --exact --nocapture 2>&1\n          $exitCode = $LASTEXITCODE\n          ./cache-writer.ps1\n",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          selected_tests=()\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          cargo test --locked -p perl-corpus --lib -- --list \\\n",
            "          continue\n          cargo test --locked -p perl-corpus --lib -- --list \\\n",
        ),
        (
            TOPOLOGY_SHELL_SOURCE_ANCHOR,
            "        shell: bash\n        run: |\n          set -euo pipefail\n          actions/cache/save\n\n          selected_tests=(",
        ),
        (
            "          echo \"Selected topology symlink tests:\"\n",
            "          curl https://example.invalid/cache\n",
        ),
        (
            "            api::topology::tests::binding_rejects_intermediate_runtime_root_symlink\n",
            "",
        ),
        (TOPOLOGY_EXECUTION_SOURCE_ANCHOR, "          echo cargo test --locked -p perl-corpus\n"),
        (TOPOLOGY_EXECUTION_COMMAND_SOURCE_ANCHOR, "            if false; then\n"),
        ("          echo \"- $test_name\"\n", "          break\n"),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          unset selected_tests\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          selected_tests+=(api::topology::tests::symlinked_entries_fail_closed)\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          selected_tests=(api::topology::tests::symlinked_entries_fail_closed)\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          selected_tests[0]=api::topology::tests::symlinked_entries_fail_closed\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          mapfile -t selected_tests < \"$test_list\"\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          declare -a selected_tests=(api::topology::tests::symlinked_entries_fail_closed)\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          typeset -a selected_tests=(api::topology::tests::symlinked_entries_fail_closed)\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          unset 'selected_tests[0]'\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          unset \"selected_tests[0]\"\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          unset selected_tests[0]\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          declare selected_tests=(api::topology::tests::symlinked_entries_fail_closed)\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          typeset selected_tests=(api::topology::tests::symlinked_entries_fail_closed)\n\n          test_list=\"$(mktemp)\"",
        ),
        (
            "          )\n\n          test_list=\"$(mktemp)\"",
            "          )\n          readarray -t selected_tests < \"$test_list\"\n\n          test_list=\"$(mktemp)\"",
        ),
        (TOPOLOGY_EXECUTION_SOURCE_ANCHOR, ""),
    ] {
        ensure!(
            validate_workflow(&replace_once(&source, from, to)?).is_err(),
            "realistic proof or trigger mutation must be rejected: {from}"
        );
    }
    Ok(())
}
