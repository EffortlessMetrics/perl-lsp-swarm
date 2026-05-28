//! Contract tests for first blocking proof-lane CI wiring.

use std::{error::Error, fs, path::PathBuf};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn project_root() -> TestResult<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn ripr_workflow_blocks_new_gaps_and_requires_receipts() -> TestResult {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;

    assert!(workflow.contains("name: ripr+ New Gap Gate"), "RIPR check name must be explicit");
    assert!(
        !workflow.contains("continue-on-error: true"),
        "RIPR new-gap gate must be blocking after PR8"
    );

    let gate_step = workflow_step(&workflow, "Enforce new RIPR gap quality gate")
        .ok_or("missing RIPR quality-gate step")?;
    for required in [
        "cargo xtask quality-gate",
        "--mode enforce-new-ripr",
        "--ripr-receipt target/receipts/quality/ripr-plus.json",
        "--ripr-pr-receipt target/ripr/pr/repo-exposure.json",
        "--review-receipt target/ripr/review/comments.json",
        "--receipt target/receipts/quality/quality-gate-ripr.json",
        "--summary target/receipts/quality/quality-gate-ripr.md",
        "--check",
    ] {
        assert!(gate_step.contains(required), "RIPR gate step missing `{required}`");
    }

    let summary_step =
        workflow_step(&workflow, "Append PR evidence summary").ok_or("missing summary step")?;
    assert!(
        summary_step.contains("target/receipts/quality/quality-gate-ripr.md")
            && summary_step.contains("GITHUB_STEP_SUMMARY"),
        "RIPR workflow must append quality-gate Markdown to the GitHub summary"
    );

    let upload_step =
        workflow_step(&workflow, "Upload ripr PR evidence").ok_or("missing RIPR upload step")?;
    for required in [
        "target/ripr/pr/**",
        "target/ripr/review/**",
        "target/receipts/quality/ripr-plus.json",
        "target/receipts/quality/quality-gate-ripr.json",
        "target/receipts/quality/quality-gate-ripr.md",
        "if-no-files-found: error",
    ] {
        assert!(upload_step.contains(required), "RIPR upload step missing `{required}`");
    }

    Ok(())
}

#[test]
fn coverage_workflow_blocks_patch_coverage_and_requires_receipts() -> TestResult {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ci-nightly.yml"))?;
    let justfile = fs::read_to_string(root.join("justfile"))?;

    let coverage_job = yaml_job(&workflow, "test-coverage:").ok_or("missing coverage job")?;
    assert!(
        coverage_job.contains("name: Codecov / Patch 95"),
        "coverage job must have a branch-protection-ready check name"
    );
    assert!(
        coverage_job.contains(
            "(github.event_name == 'pull_request' && github.event.pull_request.draft != true)"
        ) && !coverage_job.contains("ci:coverage"),
        "patch coverage must be a front-door PR gate, not label-gated"
    );
    assert!(
        workflow.contains("types: [opened, synchronize, reopened, ready_for_review, labeled]"),
        "coverage workflow must rerun when a draft PR becomes ready"
    );
    assert!(
        coverage_job.contains("just coverage-proof \"origin/$base_ref\""),
        "coverage job must run the local coverage proof wrapper"
    );
    assert!(
        coverage_job.contains("target/receipts/quality/quality-gate-coverage.md")
            && coverage_job.contains("GITHUB_STEP_SUMMARY"),
        "coverage job must append quality-gate Markdown to the GitHub summary"
    );
    assert!(
        coverage_job.contains("uses: codecov/codecov-action@")
            && coverage_job.contains("files: target/lcov.info")
            && coverage_job.contains("fail_ci_if_error: true")
            && !coverage_job.contains("continue-on-error: true"),
        "Codecov upload must be a blocking patch-coverage surface"
    );
    let upload_step = workflow_step(coverage_job, "Upload coverage proof artifacts")
        .ok_or("missing coverage proof artifact upload step")?;
    for required in [
        "target/lcov.info",
        "target/receipts/quality/coverage-baseline.json",
        "target/receipts/quality/quality-gate-coverage.json",
        "target/receipts/quality/quality-gate-coverage.md",
        "if-no-files-found: error",
    ] {
        assert!(upload_step.contains(required), "coverage upload step missing `{required}`");
    }

    let coverage_proof =
        just_recipe(&justfile, "coverage-proof").ok_or("missing coverage-proof recipe")?;
    for required in [
        "cargo llvm-cov --workspace",
        "--lcov --output-path target/lcov.info",
        "cargo xtask coverage-baseline",
        "--patch-base \"{{base}}\"",
        "--scope workspace",
        "cargo xtask quality-gate",
        "--mode enforce-patch-coverage",
        "--receipt target/receipts/quality/quality-gate-coverage.json",
        "--summary target/receipts/quality/quality-gate-coverage.md",
        "--check",
    ] {
        assert!(coverage_proof.contains(required), "coverage-proof missing `{required}`");
    }

    Ok(())
}

#[test]
fn docs_describe_transitional_blocking_contract() -> TestResult {
    let root = project_root()?;
    let ripr_doc = fs::read_to_string(root.join("docs/ci/ripr.md"))?;
    let coverage_doc = fs::read_to_string(root.join("docs/how-to/COVERAGE.md"))?;
    let status_doc =
        fs::read_to_string(root.join("docs/project/status/coverage_and_ripr_enforcement.md"))?;

    assert!(
        ripr_doc.contains("blocks PRs that introduce")
            && ripr_doc.contains("not require repo-wide RIPR+ total zero"),
        "RIPR docs must distinguish new-gap blocking from final total-zero enforcement"
    );
    assert!(
        coverage_doc.contains("coverage proof workflow now runs the patch coverage quality gate")
            && coverage_doc.contains("just coverage-proof <base>")
            && coverage_doc.contains("Project coverage remains informational during burn-down"),
        "coverage docs must describe PR8 patch enforcement and transitional project coverage"
    );
    assert!(
        status_doc.contains("quality-gate")
            && status_doc.contains("Markdown summaries")
            && status_doc.contains("repo-wide RIPR+ zero and project coverage 95% remain")
            && status_doc.contains("burn-down targets"),
        "status doc must keep final targets separate from transitional enforcement"
    );

    Ok(())
}

fn workflow_step<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("- name: {name}");
    let start = content.find(&needle)?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(needle.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(
            |(offset, line)| {
                if line.trim_start().starts_with("- name:") { Some(offset) } else { None }
            },
        )
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

fn yaml_job<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let start = content.find(name)?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(name.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(|(offset, line)| {
            let is_job = line.starts_with("  ") && !line.starts_with("    ") && line.ends_with(':');
            if is_job { Some(offset) } else { None }
        })
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

fn just_recipe<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let start = content.find(&format!("{name} "))?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(name.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(|(offset, line)| {
            if !line.trim().is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
                Some(offset)
            } else {
                None
            }
        })
        .unwrap_or(rest.len());
    Some(&rest[..next])
}
