//! Contract tests for ready-for-review RIPR workflow routing.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn ripr_workflow_runs_on_ready_for_review_without_path_filter()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let xtask_main = fs::read_to_string(root.join("xtask/src/main.rs"))?;

    assert!(workflow.contains("pull_request:"), "ripr.yml must run from pull_request events");
    assert!(
        workflow.contains("cancel-in-progress: false"),
        "RIPR evidence runs must queue newer heads instead of cancelling active analysis"
    );
    assert!(
        !workflow.contains("cancel-in-progress: ${{ github.event_name == 'pull_request' && github.event.action == 'synchronize' }}"),
        "RIPR must not turn normal PR synchronization into a cancelled no-verdict"
    );
    assert!(
        workflow.contains("types: [opened, synchronize, reopened, ready_for_review]"),
        "ripr.yml must rerun when a draft PR becomes ready for review because the job skips draft PRs"
    );
    assert!(
        !workflow.contains("\n    paths:"),
        "ripr.yml must not path-filter the ready-for-review proof run"
    );
    assert!(
        workflow.contains("if: github.event.pull_request.draft != true"),
        "ripr.yml may skip draft PRs while they are still draft"
    );
    let gate_step = workflow_step(&workflow, "Enforce new RIPR gap quality gate")
        .ok_or("missing RIPR gate step")?;
    assert!(
        !gate_step.contains("continue-on-error: true"),
        "RIPR workflow is now promoted past PR1 routing-only mode and must block new-gap failures"
    );
    assert!(
        workflow.contains("cargo xtask ripr-pr --base") && workflow.contains("target/ripr/pr/**"),
        "ripr.yml must produce and upload diff-scoped RIPR PR receipts"
    );
    assert!(
        workflow.contains("PR_HEAD_SHA: ${{ github.event_name == 'pull_request' && github.event.pull_request.head.sha || '' }}")
            && workflow.matches("--pr-head \"$PR_HEAD_SHA\"").count() >= 8,
        "every receipt generation/check route must carry the PR head separately from evaluated HEAD"
    );
    assert!(
        workflow.contains("cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json")
            && workflow.contains(
                "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check"
            )
            && workflow.contains("target/receipts/quality/ripr-plus.json"),
        "ripr.yml must generate, check, and upload the repo-wide RIPR+ receipt"
    );
    assert!(
        xtask_main.contains("RiprPlus")
            && xtask_main.contains("ripr_plus(&root, &receipt, &suppressions, check)")
            && xtask_main.contains("default_value = \"policy/ripr-suppressions.toml\""),
        "PR1 workflow must not call a missing `cargo xtask ripr-plus` command"
    );
    assert!(
        workflow.contains("cargo xtask ripr-review-comments --base")
            && workflow.contains("target/ripr/review/**"),
        "ripr.yml must generate and upload review-guidance receipts"
    );
    let validate_step = workflow_step(&workflow, "Validate PR evidence contracts")
        .ok_or("missing validate step")?;
    for check_command in [
        "cargo xtask ripr-pr --base",
        "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check",
        "cargo xtask ripr-review-comments --base",
    ] {
        assert!(
            validate_step.contains(check_command) && validate_step.contains("--check"),
            "PR1 validate step must check `{check_command}`"
        );
    }
    let upload_step =
        workflow_step(&workflow, "Upload ripr PR evidence").ok_or("missing upload step")?;
    for artifact_path in [
        "target/ripr/pr/**",
        "target/ripr/review/**",
        "target/xtask/impacted-evidence/**",
        "target/receipts/quality/ripr-plus.json",
    ] {
        assert!(
            upload_step.contains(artifact_path),
            "PR1 upload step must include `{artifact_path}`"
        );
    }
    assert!(
        upload_step.contains("if-no-files-found: error"),
        "RIPR proof artifacts are required after PR8"
    );
    let summary_step =
        workflow_step(&workflow, "Append PR evidence summary").ok_or("missing summary step")?;
    assert!(
        summary_step.contains("if: always()") && summary_step.contains("target/ripr/pr/summary.md"),
        "RIPR summary step must publish PR evidence even when earlier receipt steps fail"
    );

    Ok(())
}

#[test]
fn ripr_self_hosted_preflight_falls_back_when_required_image_is_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;

    assert_eq!(
        workflow
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter(|line| {
                line.trim_start().starts_with("if ! docker image inspect em-ci-rust:1.95")
            })
            .count(),
        2,
        "CX53 and CX43 preflight must both check the required Docker image before running ripr"
    );
    assert!(
        workflow.contains("Required Docker image em-ci-rust:1.95 is missing on CX53")
            && workflow.contains("Required Docker image em-ci-rust:1.95 is missing on CX43"),
        "missing self-hosted Rust image must be reported as preflight failure"
    );
    assert!(
        workflow.contains("needs.ripr-cx53.outputs.preflight_ok == 'false'")
            && workflow.contains("needs.ripr-cx43.outputs.preflight_ok == 'false'"),
        "preflight_ok=false must route the run to the GitHub-hosted fallback"
    );

    Ok(())
}

#[test]
fn ripr_docs_describe_unfiltered_ready_for_review_receipt_routing()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let docs = fs::read_to_string(root.join("docs/ci/ripr.md"))?;
    let posture = section_block(&docs, "## Current routing posture")
        .ok_or("docs/ci/ripr.md is missing the current routing posture section")?;
    let when_it_runs = section_block(&docs, "## When it runs")
        .ok_or("docs/ci/ripr.md is missing the When it runs section")?;
    let behavior = section_block(&docs, "## Behavior")
        .ok_or("docs/ci/ripr.md is missing the Behavior section")?;

    assert!(
        posture.contains("blocks PRs that introduce")
            && posture.contains("Repo-wide")
            && posture.contains("RIPR+ total zero remains a burn-down target"),
        "RIPR docs must describe the promoted new-gap blocking posture without final total-zero enforcement"
    );
    assert!(
        when_it_runs.contains("Every PR targeting `master` or `main`"),
        "RIPR docs must describe the workflow as an every-PR proof run"
    );
    assert!(
        when_it_runs.contains("No path filter is applied")
            && when_it_runs.contains("docs-only")
            && when_it_runs.contains("policy-only")
            && when_it_runs.contains("workflow-only"),
        "RIPR docs must make docs/policy/workflow-only PR coverage explicit"
    );
    assert!(
        when_it_runs.contains("ready_for_review"),
        "RIPR docs must say draft PRs run the workflow when they become ready"
    );
    assert!(
        behavior.contains("target/ripr/pr/")
            && behavior.contains("target/receipts/quality/ripr-plus.json")
            && behavior.contains("target/ripr/review/"),
        "RIPR docs must name diff-scoped, repo-wide, and review-guidance receipts"
    );
    for forbidden in ["Blocks merges", "quality-gate --mode enforce "] {
        assert!(
            !docs.contains(forbidden),
            "RIPR docs must not carry final quality-gate text `{forbidden}`"
        );
    }

    Ok(())
}

#[test]
fn ripr_docs_use_direct_local_proof_commands() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let docs = fs::read_to_string(root.join("docs/ci/ripr.md"))?;
    let block = fenced_block_after(&docs, "## Running locally")
        .ok_or("docs/ci/ripr.md is missing the Running locally command block")?;
    let commands = block.lines().filter(|line| !line.trim().is_empty()).collect::<Vec<_>>();
    assert!(!commands.is_empty(), "RIPR local proof block must list commands");
    for command in &commands {
        let direct = command.starts_with("cargo install ripr ")
            || command.starts_with("cargo xtask ")
            || *command == "ripr doctor";
        assert!(direct, "RIPR local proof command must be directly executable: {command}");
        assert_ne!(
            command.split_whitespace().next(),
            Some("rtk"),
            "RIPR local proof command must not use the retired RTK wrapper: {command}"
        );
        assert!(
            !command.contains("quality-gate --mode enforce "),
            "RIPR local proof commands must not run final enforcement before burn-down: {command}"
        );
    }
    for required in [
        "cargo xtask ripr-pr --base origin/HEAD --head HEAD",
        "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json",
        "cargo xtask ripr-review-comments --base origin/HEAD --head HEAD",
        "cargo xtask quality-gate --mode enforce-new-ripr",
        "cargo xtask ripr-pr --base origin/HEAD --head HEAD --check",
        "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check",
        "cargo xtask ripr-review-comments --base origin/HEAD --head HEAD --check",
        "cargo xtask quality-gate --mode enforce-new-ripr",
    ] {
        assert!(
            commands.iter().any(|command| command.contains(required)),
            "RIPR local proof block must include `{required}`"
        );
    }

    Ok(())
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

fn section_block<'a>(content: &'a str, heading: &str) -> Option<&'a str> {
    let start = content.find(heading)?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(heading.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(
            |(offset, line)| {
                if line.starts_with("## ") && line != heading { Some(offset) } else { None }
            },
        )
        .unwrap_or(rest.len());
    Some(&rest[..next])
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

#[test]
fn ripr_gate_classifies_infra_termination_with_bounded_same_head_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let classifier = fs::read_to_string(root.join("scripts/ci/classify-ripr-lane-termination"))?;
    let decider = fs::read_to_string(root.join("scripts/ci/ripr-bounded-retry"))?;
    let responder = fs::read_to_string(root.join(".github/workflows/ripr-retry.yml"))?;
    let self_test =
        fs::read_to_string(root.join("scripts/tests/test-classify-ripr-lane-termination.sh"))?;

    // The gate must evaluate non-success lanes through the shared, tested
    // decision table, and carry the single-retry budget as an explicit input
    // (#6807, #12563). Infra-no-verdict is never inferred from silence: it
    // requires the classifier's teardown evidence.
    assert!(
        workflow.contains("scripts/ci/ripr-bounded-retry --decide")
            && workflow.contains("bash scripts/ci/ripr-bounded-retry --decide"),
        "ripr gate must decide lane outcomes through scripts/ci/ripr-bounded-retry"
    );
    assert!(
        workflow.contains("RUN_ATTEMPT: ${{ github.run_attempt }}"),
        "gate must know its run attempt so the automatic same-head retry stays bounded to one"
    );
    // The decider owns the four machine-checkable lane-outcome tokens and
    // emits them under the RIPR_GATE_VERDICT= key; the gate echoes every
    // application's verdict into its run log uniformly.
    for verdict in [
        "infra-retry-requested",
        "not-proven-infra-retry-exhausted",
        "cancelled-no-verdict",
        "ripr-failure",
    ] {
        assert!(
            decider.contains(verdict),
            "each lane outcome must have a distinctive machine-checkable verdict token"
        );
    }
    assert!(
        decider.contains("RIPR_GATE_VERDICT=%s"),
        "decision table must emit its tokens under the machine-checkable RIPR_GATE_VERDICT key"
    );
    assert!(
        workflow.contains("echo \"RIPR_GATE_VERDICT=${decide_verdict:-unknown}\""),
        "gate must echo the decision's verdict token for auditability in the run log"
    );
    for loud in [
        "RIPR_GATE_VERDICT=infra-retry-requested",
        "RIPR_GATE_VERDICT=not-proven-infra-retry-exhausted",
    ] {
        assert!(
            workflow.contains(loud),
            "retry-arming and NOT_PROVEN outcomes must be loud literals in the gate log: {loud}"
        );
    }

    // Classifier boundary: a genuine terminal red outranks infra markers, so
    // a real failure keeps redding the gate even when the runner is torn
    // down during artifact upload. The source must encode that precedence,
    // not merely mention both classes. Compare CODE positions, not prose:
    // the boundary rule guards the verdict selection itself.
    let gap_receipt_rule = classifier
        .find("if [ \"$gap_hits\" -gt 0 ]")
        .ok_or("classifier must evaluate genuine gap receipts as its first rule")?;
    let infra_branch = classifier
        .find("verdict=\"infra-eviction-shutdown-signal\"")
        .ok_or("classifier must assign the shutdown-signal infra class")?;
    assert!(
        gap_receipt_rule < infra_branch,
        "genuine-gap-receipt precedence must be evaluated before any infra classification"
    );
    for evidence in [
        "The runner has received a shutdown signal",
        "Process completed with exit code 143.",
        "The operation was canceled",
    ] {
        assert!(
            classifier.contains(evidence),
            "classifier must require positive teardown evidence: {evidence}"
        );
    }

    // Decision table: retry arms only on attempt 1, exhausted budgets surface
    // NOT_PROVEN loudly, and the rerun endpoint is used at most once.
    assert!(
        decider.contains("if [ \"$run_attempt\" -eq 1 ]; then"),
        "exactly one automatic same-head retry: armed only on attempt 1"
    );
    assert!(
        decider.contains("not-proven-infra-retry-exhausted"),
        "a second eviction must surface NOT_PROVEN loudly instead of looping"
    );
    assert_eq!(
        decider.matches("gh api -X POST \"${api}/rerun-failed-jobs\"").count(),
        1,
        "the bounded retry must post the rerun request from exactly one code site"
    );

    // Responder: fired once per completed ripr run, gated to failed first
    // attempts in this repository, executing the same decision table.
    assert!(
        responder.contains("workflows: [ripr]") && responder.contains("types: [completed]"),
        "responder must trigger on completed ripr runs"
    );
    assert!(
        responder.contains("run_attempt == 1") && responder.contains("conclusion == 'failure'"),
        "responder must bound itself to failed first attempts"
    );
    assert!(
        responder.contains("repository.full_name == github.repository"),
        "responder must stay scoped to runs owned by this repository"
    );
    assert!(
        responder.contains("scripts/ci/ripr-bounded-retry arm-retry"),
        "responder must execute the shared bounded-retry decision table"
    );

    // The discriminator suite must pin the load-bearing behaviors: real reds
    // still red, only teardown evidence retries, retries do not loop.
    assert!(
        self_test.contains("DISCRIMINATOR: real failure never retried despite teardown marker"),
        "self-test must prove a genuine failure still redding the gate with teardown noise present"
    );
    assert!(
        self_test.contains("attempt-2 eviction exhausts retry budget to NOT_PROVEN"),
        "self-test must prove the single-retry bound"
    );

    Ok(())
}
