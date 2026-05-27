//! Contract tests for the first blocking quality-gate CI wiring slice.

use std::{error::Error, fs, path::PathBuf};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn project_root() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn ripr_workflow_and_policy_ledgers_mark_new_gap_gate_blocking() -> TestResult {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let lanes = fs::read_to_string(root.join("policy/ci-lanes.toml"))?;
    let whitelist = fs::read_to_string(root.join("policy/ci-lane-whitelist.toml"))?;
    let ripr_docs = fs::read_to_string(root.join("docs/ci/ripr.md"))?;

    assert!(
        workflow.contains("name: CI / ripr+ New Gap Gate")
            && workflow.contains("--mode enforce-new-ripr")
            && !workflow.contains("continue-on-error: true"),
        "RIPR workflow must expose and enforce the blocking new-gap quality gate"
    );
    assert!(
        workflow.contains("Verify required RIPR proof artifacts")
            && workflow.contains("target/ripr/pr/repo-exposure.json")
            && workflow.contains("target/ripr/review/comments.json")
            && workflow.contains("target/receipts/quality/ripr-plus.json")
            && workflow.contains("target/receipts/quality/quality-gate.json")
            && workflow.contains("target/receipts/quality/quality-gate.md")
            && workflow.contains("if-no-files-found: error"),
        "RIPR workflow must hard-fail missing required proof artifacts"
    );

    let lane_block = table_block(&lanes, "[lane.ripr_advisory]")
        .ok_or("policy/ci-lanes.toml is missing [lane.ripr_advisory]")?;
    assert!(
        lane_block.contains("blocking = true")
            && lane_block.contains("target/receipts/quality/ripr-plus.json")
            && lane_block.contains("target/receipts/quality/quality-gate.json")
            && lane_block.contains("target/receipts/quality/quality-gate.md"),
        "ci-lanes ripr lane must list the blocking new-gap receipts"
    );

    let whitelist_block = lane_array_block(&whitelist, "id = \"ripr_advisory\"")
        .ok_or("policy/ci-lane-whitelist.toml is missing ripr_advisory lane")?;
    assert!(
        whitelist_block.contains("blocking = true")
            && whitelist_block.contains("new severe gaps")
            && whitelist_block.contains("target/receipts/quality/ripr-plus.json")
            && whitelist_block.contains("target/receipts/quality/quality-gate.json")
            && whitelist_block.contains("target/receipts/quality/quality-gate.md"),
        "ci-lane-whitelist ripr lane must describe the blocking new-gap proof obligation"
    );

    assert!(
        ripr_docs.contains("`rtk cargo xtask quality-gate --mode enforce-new-ripr`")
            && ripr_docs.contains("Blocks merges when diff-scoped PR evidence is missing")
            && ripr_docs.contains("Fails explicitly if any required proof artifact is absent")
            && ripr_docs.contains("target/receipts/quality/quality-gate.json")
            && ripr_docs.contains("target/receipts/quality/quality-gate.md"),
        "docs/ci/ripr.md must describe the blocking quality-gate posture and required receipts"
    );
    let local_commands = fenced_block_after(&ripr_docs, "## Running locally")
        .ok_or("docs/ci/ripr.md is missing the Running locally command block")?;
    let quality_gate = local_commands
        .lines()
        .find(|command| command.contains("cargo xtask quality-gate --mode enforce-new-ripr"))
        .ok_or("RIPR docs must show the local enforce-new-ripr quality-gate command")?;
    for required in [
        "--ripr-receipt target/receipts/quality/ripr-plus.json",
        "--ripr-pr-receipt target/ripr/pr/repo-exposure.json",
        "--review-receipt target/ripr/review/comments.json",
        "--coverage-receipt target/receipts/quality/coverage-baseline.json",
        "--codecov codecov.yml",
        "--exceptions policy/quality-gate-exceptions.toml",
        "--receipt target/receipts/quality/quality-gate.json",
        "--summary target/receipts/quality/quality-gate.md",
        "--check",
    ] {
        assert!(
            quality_gate.contains(required),
            "RIPR local quality-gate command must include {required}: {quality_gate}"
        );
    }

    Ok(())
}

#[test]
fn coverage_workflow_and_policy_ledgers_mark_patch_gate_blocking() -> TestResult {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ci-nightly.yml"))?;
    let lanes = fs::read_to_string(root.join("policy/ci-lanes.toml"))?;
    let whitelist = fs::read_to_string(root.join("policy/ci-lane-whitelist.toml"))?;
    let rollout = fs::read_to_string(root.join("docs/ci/codecov-rollout.md"))?;
    let coverage_doc = fs::read_to_string(root.join("docs/how-to/COVERAGE.md"))?;
    let coverage_readme = fs::read_to_string(root.join(".ci/README-coverage.md"))?;
    let job = table_block(&workflow, "  test-coverage:")
        .ok_or("ci-nightly.yml is missing the test-coverage job")?;

    assert!(
        job.contains("name: Codecov / Patch 95"),
        "test-coverage must expose the blocking patch target in its PR check name"
    );
    assert!(
        job.contains("github.event_name == 'pull_request'")
            && !job.contains("contains(github.event.pull_request.labels.*.name, 'ci:coverage')"),
        "test-coverage must run on every PR, not only label-gated PRs"
    );
    assert!(
        !job.contains("continue-on-error: true") && job.contains("fail_ci_if_error: true"),
        "Codecov upload failures must not be hidden from the required patch gate"
    );
    assert!(
        job.contains("Run parser branch coverage ratchet")
            && job.contains("just coverage-branch-gate")
            && job.contains("Generate coverage proof LCOV")
            && job.contains("just coverage-proof-lcov"),
        "coverage workflow must keep parser ratchet and proof LCOV generation distinct"
    );
    assert!(
        job.contains("cargo xtask coverage-baseline")
            && job.contains("--lcov lcov.info")
            && job.contains("target/receipts/quality/coverage-baseline.json")
            && job.contains("--check"),
        "coverage workflow must emit and validate the coverage receipt consumed by quality-gate"
    );
    assert!(
        job.contains("cargo xtask quality-gate")
            && job.contains("--mode enforce-patch-coverage")
            && job.contains("--patch-status-source codecov")
            && job.contains("target/receipts/quality/coverage-quality-gate.json")
            && job.contains("target/receipts/quality/coverage-quality-gate.md"),
        "coverage workflow must run the patch coverage quality-gate mode"
    );
    assert!(
        job.contains("QUALITY_GATE_STATUS=$?")
            && job.contains("exit \"$QUALITY_GATE_STATUS\"")
            && job.contains(
                "cat target/receipts/quality/coverage-quality-gate.md >> \"$GITHUB_STEP_SUMMARY\""
            ),
        "coverage workflow must publish repair guidance and preserve the gate exit status"
    );
    assert!(
        job.contains("Verify coverage proof artifacts")
            && job.contains("lcov.info")
            && job.contains("target/receipts/quality/coverage-baseline.json")
            && job.contains("target/receipts/quality/coverage-quality-gate.json")
            && job.contains("target/receipts/quality/coverage-quality-gate.md")
            && job.contains("if-no-files-found: error"),
        "coverage workflow must hard-fail missing coverage proof artifacts"
    );

    let lane_block = table_block(&lanes, "[lane.coverage]")
        .ok_or("policy/ci-lanes.toml is missing [lane.coverage]")?;
    assert!(
        lane_block.contains("default_pr = true")
            && lane_block.contains("blocking = true")
            && lane_block.contains("Codecov patch 95")
            && lane_block.contains("target/receipts/quality/coverage-baseline.json")
            && lane_block.contains("target/receipts/quality/coverage-quality-gate.json"),
        "ci-lanes coverage lane must be a default blocking PR gate"
    );

    let whitelist_block = lane_array_block(&whitelist, "id = \"coverage\"")
        .ok_or("policy/ci-lane-whitelist.toml is missing coverage lane")?;
    assert!(
        whitelist_block.contains("default_pr = true")
            && whitelist_block.contains("blocking = true")
            && whitelist_block.contains("tier = \"frontdoor\"")
            && whitelist_block.contains("patch coverage >=95%")
            && whitelist_block.contains("target/receipts/quality/coverage-baseline.json")
            && whitelist_block.contains("target/receipts/quality/coverage-quality-gate.json"),
        "ci-lane-whitelist coverage lane must be a frontdoor blocking PR gate"
    );

    assert!(
        rollout.contains("Coverage flag uploaded   | `parser,xtask`")
            && rollout.contains("target/receipts/quality/coverage-quality-gate.json")
            && rollout.contains("target/receipts/quality/coverage-quality-gate.md")
            && rollout.contains("Keep the `test-coverage` job on every pull request")
            && rollout.contains("--patch-status-source codecov"),
        "Codecov rollout docs must describe the PR8 workflow wiring and coverage quality-gate receipts"
    );
    assert!(
        coverage_doc.contains("**On every PR**")
            && coverage_doc.contains("rtk just coverage-proof-lcov")
            && coverage_doc.contains("Run quality gate")
            && coverage_doc.contains("quality-gate --mode")
            && coverage_doc.contains("enforce-patch-coverage"),
        "coverage how-to must describe the default PR coverage workflow and quality-gate step"
    );
    assert!(
        coverage_readme.contains("rtk just coverage-proof-lcov"),
        "coverage README must show the proof LCOV command used by the CI wiring slice"
    );

    Ok(())
}

#[test]
fn coverage_proof_lcov_includes_xtask_proof_rail() -> TestResult {
    let root = project_root()?;
    let justfile = fs::read_to_string(root.join("justfile"))?;
    let recipe = just_recipe(&justfile, "coverage-proof-lcov:")
        .ok_or("justfile is missing coverage-proof-lcov recipe")?;

    assert!(
        recipe.contains("cargo llvm-cov")
            && recipe.contains("-p perl-parser")
            && recipe.contains("-p xtask")
            && recipe.contains("--output-path lcov.info"),
        "coverage-proof-lcov must generate Codecov/quality-gate LCOV from parser and xtask"
    );
    assert!(
        recipe.contains("crates/tree-sitter-perl-c/")
            && recipe.contains("archive|tests|benches|examples"),
        "coverage-proof-lcov must preserve established generated/test/legacy exclusions"
    );

    Ok(())
}

#[test]
fn evidence_lane_docs_do_not_classify_blocking_quality_gates_as_advisory() -> TestResult {
    let root = project_root()?;
    let docs = fs::read_to_string(root.join("docs/ci/test-evidence-lanes.md"))?;

    assert!(
        docs.contains("Diff-scoped new RIPR+ gaps are blocking now"),
        "test-evidence-lanes must state that diff-scoped new RIPR+ gaps block PRs"
    );
    assert!(
        docs.contains("patch coverage") && docs.contains("95%"),
        "test-evidence-lanes must mention the blocking patch coverage target"
    );
    assert!(
        !docs.contains("- `ripr` (static oracle-gap detection)."),
        "test-evidence-lanes must not classify the active RIPR new-gap gate as never-blocking"
    );

    Ok(())
}

fn table_block<'a>(content: &'a str, header: &str) -> Option<&'a str> {
    let start = content.find(header)?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(header.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(|(offset, line)| {
            if line.starts_with("[lane.")
                || line.starts_with("[[")
                || (line.starts_with("  ") && line.ends_with(':') && !line.starts_with("    "))
            {
                Some(offset)
            } else {
                None
            }
        })
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

fn just_recipe<'a>(content: &'a str, header: &str) -> Option<&'a str> {
    let start = content.find(header)?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(header.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(|(offset, line)| {
            if !line.trim().is_empty()
                && !line.starts_with(' ')
                && !line.starts_with('\t')
                && line.ends_with(':')
            {
                Some(offset)
            } else {
                None
            }
        })
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

fn lane_array_block<'a>(content: &'a str, needle: &str) -> Option<&'a str> {
    let needle_pos = content.find(needle)?;
    let before = &content[..needle_pos];
    let start = before.rfind("[[lane]]")?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan("[[lane]]".len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(|(offset, line)| if line == "[[lane]]" { Some(offset) } else { None })
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

fn fenced_block_after<'a>(content: &'a str, heading: &str) -> Option<&'a str> {
    let start = content.find(heading)?;
    let rest = &content[start..];
    let fence_start = rest.find("```bash")? + "```bash".len();
    let after_start = &rest[fence_start..];
    let body_start = after_start.strip_prefix('\n').unwrap_or(after_start);
    let fence_end = body_start.find("```")?;
    Some(&body_start[..fence_end])
}
