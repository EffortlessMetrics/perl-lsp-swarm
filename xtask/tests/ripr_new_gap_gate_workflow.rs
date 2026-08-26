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

#[test]
fn hosted_cancellation_retries_are_bounded_and_visible() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;

    assert!(workflow.contains("ripr-github-retry-1:"), "missing retry-1 job");
    assert!(workflow.contains("ripr-github-retry-2:"), "missing retry-2 job");
    assert!(
        !workflow.contains("ripr-github-retry-3"),
        "the cancellation-retry chain must stop after two retries (#6807)"
    );
    assert_eq!(
        workflow.matches("- name: Backoff before hosted retry").count(),
        2,
        "both retry attempts must apply a visible backoff"
    );

    // Every retry is visible in logs, names, artifacts, and summaries (#6807).
    for visible in [
        "name: ripr+ on GitHub Hosted (retry 1/2)",
        "name: ripr+ on GitHub Hosted (retry 2/2)",
        "name: ripr-pr-evidence-retry-1",
        "name: ripr-pr-evidence-retry-2",
        "echo \"- retry1_result: \\`${RETRY1_RESULT:-}\\`\"",
        "echo \"- retry2_result: \\`${RETRY2_RESULT:-}\\`\"",
        "RUNNER-SHUTDOWN-CANCELLATION recovery",
        "attempt ${RIPR_ATTEMPT}/3",
    ] {
        assert!(workflow.contains(visible), "retry visibility receipt missing: {visible}");
    }

    Ok(())
}

#[test]
fn hosted_retry_conditions_admit_only_cancellation_class_outcomes()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;

    let retry_1_if = job_condition(&workflow, "ripr-github-retry-1")?;
    let retry_2_if = job_condition(&workflow, "ripr-github-retry-2")?;

    // Retry fires ONLY on its predecessor's cancellation with no TIMEOUT-BUDGET
    // marker; failure, success, and skipped predecessors never chain forward.
    for condition in [&retry_1_if, &retry_2_if] {
        assert!(
            condition.contains("== 'cancelled'"),
            "retry conditions must require a cancelled predecessor: {condition}"
        );
        assert!(
            condition.contains("outputs.deadline_exceeded != 'true'"),
            "retry conditions must exclude TIMEOUT-BUDGET outcomes: {condition}"
        );
        assert!(
            !condition.contains("'failure'"),
            "test/gate/lint failures must fail fast with zero retries: {condition}"
        );
        assert!(
            !condition.contains("!= 'success'"),
            "retries must key on cancelled exactly, not on not-success: {condition}"
        );
        assert!(
            condition.contains("needs.route-ripr.outputs.target == 'github'"),
            "the retry policy is scoped to the GitHub-hosted lane only: {condition}"
        );
        // Evidence gate (#6807 review): a bare 'cancelled' conclusion no
        // longer admits a retry by itself — the predecessor's
        // cancellation_class must be either a positively detected
        // runner-shutdown signature or empty because the runner died before
        // it could classify. Manual/API cancellations classify as
        // manual-or-api-cancellation and stop the chain.
        for allowed in ["outputs.cancellation_class == 'runner-shutdown'", "outputs.cancellation_class == ''"] {
            assert!(
                condition.contains(allowed),
                "retry conditions must admit only evidenced cancellation classes ({allowed}): {condition}"
            );
        }
    }
    assert!(
        retry_1_if.contains("needs.ripr-github.result == 'cancelled'"),
        "retry 1 must gate on the primary attempt: {retry_1_if}"
    );
    assert!(
        retry_2_if.contains("needs.ripr-github-retry-1.result == 'cancelled'"),
        "retry 2 must gate on retry 1 so a real verdict stops the chain: {retry_2_if}"
    );

    // The blanket self-hosted cancelled loop must not observe the hosted chain,
    // or a recovered run would be double-failed.
    let blanket_loop = workflow
        .lines()
        .find(|line| line.starts_with("          for lane_result in "))
        .ok_or("missing blanket cancelled-lane loop")?;
    assert!(
        blanket_loop.contains("$FALLBACK_RESULT") && !blanket_loop.contains("$GITHUB_RESULT"),
        "hosted cancellations belong to the decision table, not the blanket loop: {blanket_loop}"
    );

    Ok(())
}

#[test]
fn github_chain_decision_table_executes_with_bash() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;

    const BEGIN: &str = "# >>> ripr github-chain decision table";
    const END: &str = "# <<< ripr github-chain decision table";
    assert!(
        workflow.contains(BEGIN) && workflow.contains(END),
        "decision-table sentinels must delimit the executable classification block"
    );
    let script = extract_decision_table(&workflow)?;

    let Some(bash) = functional_bash() else {
        // CI runs this suite on Linux where bash always exists; Windows dev
        // environments without a usable bash still keep every string-contract
        // test above as the classification surface.
        eprintln!("skipping decision-table execution: no usable bash available");
        return Ok(());
    };

    let temp = std::env::temp_dir().join(format!("ripr-decision-table-{}", std::process::id()));
    fs::create_dir_all(&temp)?;
    // RAII cleanup guard: the temp dir is removed when this test scope ends,
    // whether the combos pass, fail, or panic (review disposition).
    struct TempCleanup(std::path::PathBuf);
    impl Drop for TempCleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = TempCleanup(temp.clone());
    let script_path = temp.join("table.sh");
    fs::write(&script_path, script)?;
    // Git-bash on Windows rejects backslash paths in argv; forward slashes
    // work on every platform bash supports.
    let script_arg = script_path.to_string_lossy().replace('\\', "/");

    struct Combo {
        attempts: (&'static str, &'static str, &'static str),
        expect_success: bool,
        exhausted_incident: bool,
    }
    let combos = [
        Combo {
            attempts: ("success", "skipped", "skipped"),
            expect_success: true,
            exhausted_incident: false,
        },
        Combo {
            attempts: ("cancelled", "success", "skipped"),
            expect_success: true,
            exhausted_incident: false,
        },
        Combo {
            attempts: ("cancelled", "cancelled", "success"),
            expect_success: true,
            exhausted_incident: false,
        },
        Combo {
            attempts: ("failure", "skipped", "skipped"),
            expect_success: false,
            exhausted_incident: false,
        },
        Combo {
            attempts: ("cancelled", "failure", "skipped"),
            expect_success: false,
            exhausted_incident: false,
        },
        Combo {
            // Defensive: a cancelled attempt whose successor was skipped (e.g.
            // gating regression) must stay visibly red without incident naming.
            attempts: ("cancelled", "skipped", "skipped"),
            expect_success: false,
            exhausted_incident: false,
        },
        Combo {
            attempts: ("cancelled", "cancelled", "cancelled"),
            expect_success: false,
            exhausted_incident: true,
        },
    ];

    let mut failures = Vec::new();
    for combo in &combos {
        let output = std::process::Command::new(&bash)
            .arg(&script_arg)
            .env("ROUTER_TARGET", "github")
            .env("GITHUB_RESULT", combo.attempts.0)
            .env("RETRY1_RESULT", combo.attempts.1)
            .env("RETRY2_RESULT", combo.attempts.2)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}{stderr}");
        if output.status.success() != combo.expect_success {
            // Bounded stderr diagnostic: surface the first non-empty stderr
            // line so CI failures are diagnosable without unbounded dumps
            // (review disposition).
            let stderr_summary = stderr
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .chars()
                .take(200)
                .collect::<String>();
            failures.push(format!(
                "attempts {:?}: expected {}, got exit {:?} (stderr: {})",
                combo.attempts,
                if combo.expect_success { "success" } else { "failure" },
                output.status.code(),
                stderr_summary
            ));
        }
        let names_exhausted = combined.contains("RUNNER-SHUTDOWN-CANCELLATION exhausted");
        if names_exhausted != combo.exhausted_incident {
            failures.push(format!(
                "attempts {:?}: incident naming mismatch (present: {names_exhausted})",
                combo.attempts
            ));
        }
        if !combo.exhausted_incident && combined.contains("RUNNER-SHUTDOWN-CANCELLATION") {
            failures.push(format!(
                "attempts {:?}: incident class must be named only on exhaustion",
                combo.attempts
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "decision-table behavior drifted from the cancellation policy: {failures:?}"
    );

    Ok(())
}

#[test]
fn hosted_timeout_budget_discriminator_is_wired_into_every_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&workflow).map_err(|error| {
        format!("ripr.yml must stay parseable YAML after the retry edits: {error}")
    })?;

    // The self-enforced budget (2400s) is derived from lane start in all three
    // attempts, so a genuine timeout can never surface as 'cancelled'.
    assert_eq!(
        workflow.matches("RIPR_LANE_DEADLINE_EPOCH=$((lane_start_epoch + 2400))").count(),
        3,
        "every hosted attempt must enforce the 2400s TIMEOUT-BUDGET"
    );
    assert_eq!(
        workflow.matches("timeout --kill-after=30s \"${remaining}s\" \"$@\"").count(),
        3,
        "every attempt must install the GNU timeout guard"
    );
    for job in ["ripr-github", "ripr-github-retry-1", "ripr-github-retry-2"] {
        let body = job_block(&workflow, job)?;
        assert!(
            body.contains("deadline_exceeded: ${{ steps.classify.outputs.deadline_exceeded }}"),
            "{job} must publish the deadline classification output"
        );
        assert!(
            body.contains("lane_run \"Evaluate quality gate\""),
            "{job} quality-gate evaluation must run under the deadline guard"
        );
    }

    Ok(())
}

fn extract_decision_table(workflow: &str) -> Result<String, Box<dyn std::error::Error>> {
    const BEGIN: &str = "# >>> ripr github-chain decision table";
    const END: &str = "# <<< ripr github-chain decision table";
    let start = workflow.find(BEGIN).ok_or("missing begin sentinel")?;
    let end = workflow.find(END).ok_or("missing end sentinel")?;
    if end <= start {
        return Err("decision-table sentinels are out of order".into());
    }
    let body = workflow[start..end].lines().skip(1).collect::<Vec<_>>().join("\n");
    Ok(format!("set -euo pipefail\ncase \"${{ROUTER_TARGET}}\" in\n{body}\nesac\n"))
}

fn functional_bash() -> Option<std::path::PathBuf> {
    // On Windows the PATH may expose a WSL bash stub that cannot open Win32
    // paths; prefer an MSYS/Git-bash that shares the filesystem view.
    let candidates: &[&str] = if cfg!(windows) {
        &[r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files\Git\usr\bin\bash.exe", "bash"]
    } else {
        &["bash"]
    };
    candidates.iter().find_map(|candidate| {
        std::process::Command::new(candidate)
            .arg("-c")
            .arg("echo ok")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|_| std::path::PathBuf::from(candidate))
    })
}

/// Return the raw YAML text of one top-level workflow job (two-space indent).
fn job_block<'a>(content: &'a str, job: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let needle = format!("  {job}:\n");
    let start = content.find(&needle).ok_or_else(|| format!("missing top-level job `{job}`"))?;
    let rest = &content[start..];
    let end = rest
        .match_indices('\n')
        .skip(1)
        .find_map(|(offset, _)| {
            let line_start = offset + 1;
            let line = &rest[line_start..];
            // The next key indented exactly two spaces begins the next job.
            let is_next_job = line.starts_with("  ")
                && !line[2..].starts_with(' ')
                && !line[2..].starts_with('#');
            is_next_job.then_some(line_start)
        })
        .unwrap_or(rest.len());
    Ok(&rest[..end])
}

fn job_condition(content: &str, job: &str) -> Result<String, Box<dyn std::error::Error>> {
    let block = job_block(content, job)?;
    let start = block.find("if: >-").ok_or_else(|| format!("job `{job}` has no multiline if"))?;
    let body = &block[start + "if: >-".len()..];
    let mut condition = String::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        // The evidence-gate conjunct is parenthesized across lines; keep
        // parsing through the opening parenthesis and outputs. terms instead
        // of truncating the condition mid-expression.
        if !trimmed.starts_with("needs.")
            && !trimmed.starts_with("always()")
            && !trimmed.starts_with("outputs.")
            && trimmed != "("
        {
            break;
        }
        condition.push_str(trimmed.trim_end_matches("&&"));
        condition.push(' ');
    }
    Ok(condition)
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
