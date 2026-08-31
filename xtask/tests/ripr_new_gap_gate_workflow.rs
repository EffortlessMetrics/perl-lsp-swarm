//! Contract tests for ready-for-review RIPR workflow routing.

use std::{fs, path::PathBuf, process::Command};

use anyhow::{Context, Result, anyhow, bail};
use serde_yaml_ng::Value;

#[path = "support/workflow_bash.rs"]
mod workflow_bash;

use workflow_bash::bash_executable;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn evaluate_run_block() -> Result<String> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let yaml: Value = serde_yaml_ng::from_str(&workflow)?;
    yaml.get("jobs")
        .and_then(|jobs| jobs.get("ripr"))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("name").and_then(Value::as_str) == Some("Evaluate routed result")
            })
        })
        .and_then(|step| step.get("run"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("ripr gate Evaluate routed result run block is missing"))
}

fn run_gate_with_fake_gh(log: Option<&str>) -> Result<(std::process::Output, String, String)> {
    let root = project_root()?;
    let sandbox = tempfile::tempdir().context("creating gate workflow sandbox")?;
    let classifier_dir = sandbox.path().join("scripts/ci");
    fs::create_dir_all(&classifier_dir)?;
    fs::copy(
        root.join("scripts/ci/classify-ripr-lane-termination"),
        classifier_dir.join("classify-ripr-lane-termination"),
    )?;
    let summary = sandbox.path().join("summary.md");
    let calls = sandbox.path().join("log-fetch-calls");
    let classification = sandbox.path().join("ripr-gate-classification.env");
    let run = evaluate_run_block()?;
    let fake = r#"
sleep() { :; }
gh() {
  local url="$2"
  case "$url" in
    */jobs?per_page=100)
      printf '98765\n'
      return
      ;;
    */logs)
      local count=0
      if [ -f "$FAKE_CALLS" ]; then count=$(cat "$FAKE_CALLS"); fi
      printf '%s' "$((count + 1))" > "$FAKE_CALLS"
      if [ "$FAKE_FETCH" = "fail" ]; then return 1; fi
      printf '%s' "$FAKE_LOG"
      return
      ;;
  esac
  return 1
}
"#;
    let script = format!("{fake}\n{run}");
    let output = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-c", &script])
        .current_dir(sandbox.path())
        .env("ROUTE_RESULT", "success")
        .env("ROUTER_TARGET", "github")
        .env("ROUTER_REASON", "test")
        .env("CX53_RESULT", "skipped")
        .env("CX43_RESULT", "skipped")
        .env("GITHUB_RESULT", "failure")
        .env("FALLBACK_RESULT", "skipped")
        .env("GITHUB_REPOSITORY", "EffortlessMetrics/perl-lsp-swarm")
        .env("GITHUB_RUN_ID", "4242")
        .env("GITHUB_SHA", "0123456789abcdef0123456789abcdef01234567")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("FAKE_FETCH", if log.is_some() { "success" } else { "fail" })
        .env("FAKE_LOG", log.unwrap_or(""))
        .env("FAKE_CALLS", &calls)
        .output()
        .context("executing the real ripr gate run block")?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let calls_text = fs::read_to_string(&calls).unwrap_or_default();
    let classification_text = fs::read_to_string(&classification).unwrap_or_default();
    Ok((output, format!("{combined}\n{calls_text}"), classification_text))
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
fn ripr_infra_retry_is_bounded_and_gate_classified() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let gate = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let retry = fs::read_to_string(root.join(".github/workflows/ripr-infra-retry.yml"))?;

    // #6807 slice 2: the gate remains the single eviction classifier and
    // hands its verdict to the retry workflow strictly as data.
    let evaluate_step =
        workflow_step(&gate, "Evaluate routed result").ok_or("missing evaluate step")?;
    assert!(
        evaluate_step.contains("classification=infra-no-proof")
            && evaluate_step.contains("> ripr-gate-classification.env")
            && evaluate_step.contains("head_sha=${GITHUB_SHA}")
            && evaluate_step.contains("run_id=${GITHUB_RUN_ID}"),
        "the ripr gate must emit its infra-no-proof verdict as a data file for the retry workflow"
    );
    let upload_step = workflow_step(&gate, "Upload gate classification")
        .ok_or("missing classification upload")?;
    assert!(
        upload_step.contains("if: failure()")
            && upload_step.contains("name: ripr-gate-classification")
            && upload_step.contains("if-no-files-found: ignore"),
        "genuine ripr failures produce no classification file, so the upload must tolerate its absence"
    );

    // The retry workflow fires on completed failing ripr runs only.
    assert!(
        retry.contains("workflow_run:")
            && retry.contains("workflows: [ripr]")
            && retry.contains("types: [completed]")
            && retry.contains("github.event.workflow_run.conclusion == 'failure'"),
        "ripr-infra-retry must trigger on completed failing ripr runs"
    );
    // Bound: exactly one automatic retry; attempt 2+ takes the manual path.
    assert!(
        retry.contains("[ \"${RUN_ATTEMPT}\" != \"1\" ]"),
        "ripr-infra-retry must bound the automatic retry to run attempt 1"
    );
    // The verdict is consumed strictly as data: exact-line grep, no source,
    // and the rerun target is the event-provided run id.
    assert!(
        retry.contains("grep -qx 'classification=infra-no-proof'"),
        "ripr-infra-retry must match the classification line exactly"
    );
    assert!(
        !retry.contains("actions/checkout"),
        "ripr-infra-retry runs with actions:write on the default branch and must never check out candidate code"
    );
    assert!(
        retry.contains("RUN_ID: ${{ github.event.workflow_run.id }}")
            && retry.contains("actions/runs/${RUN_ID}/rerun-failed-jobs"),
        "ripr-infra-retry must rerun failed jobs of the event run id, not an artifact-provided id"
    );
    // Artifact/run coherence is proven by the recorded run id, not by head
    // SHA: for pull_request runs the gate's GITHUB_SHA is the evaluated
    // refs/pull/<n>/merge commit while workflow_run.head_sha is the PR branch
    // tip, so a head comparison would skip every genuine PR eviction.
    assert!(
        retry.contains("[ \"${gate_run_id}\" != \"${RUN_ID}\" ]"),
        "ripr-infra-retry must verify the classification run id matches the event run"
    );
    assert!(
        !retry.contains("[ \"${gate_head}\" != \"${HEAD_SHA}\" ]"),
        "ripr-infra-retry must not gate the retry on a head-SHA comparison (merge ref vs branch tip)"
    );

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
fn ripr_infra_classifier_is_shared_tested_and_boundary_documented()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let gate = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let retry = fs::read_to_string(root.join(".github/workflows/ripr-infra-retry.yml"))?;
    let classifier = fs::read_to_string(root.join("scripts/ci/classify-ripr-lane-termination"))?;
    let self_test =
        fs::read_to_string(root.join("scripts/tests/test-classify-ripr-lane-termination.sh"))?;
    let whitelist = fs::read_to_string(root.join("policy/ci-lane-whitelist.toml"))?;

    // #12563 complement to #12771: the gate must run the ONE classifier from
    // the tested script instead of an inline grep twin, and must echo its
    // machine-checkable verdict into the run log for auditability.
    assert!(
        gate.contains("bash scripts/ci/classify-ripr-lane-termination"),
        "gate must classify lane termination through the shared tested script"
    );
    assert!(
        !gate.contains("eviction_matches=$(grep -cE"),
        "inline grep classification twins the tested classifier and must not return"
    );
    for verdict in [
        "RIPR_GATE_VERDICT=infra-no-proof",
        "RIPR_GATE_VERDICT=ripr-failure",
        "RIPR_GATE_VERDICT=cancelled-no-verdict",
        "RIPR_GATE_VERDICT=neutral-router-skipped",
        "RIPR_GATE_VERDICT=router-not-success",
        "RIPR_GATE_VERDICT=success",
    ] {
        assert!(
            gate.contains(verdict),
            "every gate outcome must emit a distinctive machine-checkable verdict token"
        );
    }
    assert!(
        gate.contains("RIPR_GATE_DECISION boundary="),
        "each infra-no-proof application must document its decision boundary in the run log"
    );

    // Classifier boundary: precedence between genuine reds and teardown
    // evidence is encoded in code order, not prose.
    let gap_rule =
        classifier.find("gap_hits").ok_or("classifier must evaluate genuine gap receipts first")?;
    let infra_class = classifier
        .find("\"infra-no-proof\"")
        .ok_or("classifier must assign the infra-no-proof class")?;
    assert!(
        gap_rule < infra_class,
        "genuine gap receipt evaluation must precede any infra classification"
    );
    for marker in [
        "The runner has received a shutdown signal",
        "Process completed with exit code 143.",
        "The operation was canceled",
        "quality gate failed; see receipt",
    ] {
        assert!(
            classifier.contains(marker),
            "classifier must pin its exact evidence markers: {marker}"
        );
    }

    // Responder: the exhausted retry bound surfaces NOT_PROVEN loudly rather
    // than as a silent notice.
    assert!(
        retry.contains("RIPR_GATE_VERDICT=not-proven-infra-retry-exhausted"),
        "attempt >= 2 must surface NOT_PROVEN with a machine-checkable verdict token"
    );
    assert!(
        retry.contains("[ \"${RUN_ATTEMPT}\" != \"1\" ]"),
        "the single automatic retry stays bounded to attempt 1"
    );

    // The fixture suite pins the discriminator: real failure with teardown
    // noise present still classifies ripr-failure.
    assert!(
        self_test.contains("DISCRIMINATOR: genuine gap receipt outranks later teardown marker"),
        "self-test must prove real failures are never classified infra"
    );
    assert!(
        self_test.contains("empty log fails closed to ripr-failure"),
        "self-test must prove absent evidence fails closed"
    );

    // Lane hygiene: the privileged responder must carry a whitelist entry.
    assert!(
        whitelist.contains("workflow = \".github/workflows/ripr-infra-retry.yml\""),
        "ripr-infra-retry must be governed by a ci-lane-whitelist entry"
    );

    Ok(())
}

#[test]
fn ripr_gate_retrieval_reaches_classifier_and_failed_fetch_fails_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let evicted_log = concat!(
        "##[error]The runner has received a shutdown signal.\n",
        "##[error]The operation was canceled.\n"
    );
    let (success, success_output, classification) = run_gate_with_fake_gh(Some(evicted_log))?;
    if success.status.success() {
        bail!("a classified lane failure must keep the gate red");
    }
    if !success_output.contains("classification=infra-no-proof")
        || !success_output.contains("RIPR_GATE_VERDICT=infra-no-proof")
    {
        bail!(
            "successful log retrieval must reach the shared classifier and gate verdict:\n{success_output}"
        );
    }
    if !classification.contains("classification=infra-no-proof")
        || !classification.contains("run_id=4242")
    {
        bail!(
            "infra classification artifact must be written by the actual gate block:\n{classification}"
        );
    }

    let genuine_failure_log = concat!(
        "quality gate failed; see receipt target/receipts/quality/quality-gate-ripr.json\n",
        "##[error]The runner has received a shutdown signal.\n"
    );
    let (genuine, genuine_output, genuine_classification) =
        run_gate_with_fake_gh(Some(genuine_failure_log))?;
    if genuine.status.success()
        || !genuine_output.contains("classification=ripr-failure")
        || !genuine_output.contains("RIPR_GATE_VERDICT=ripr-failure")
    {
        bail!(
            "a retrieved genuine gap must remain fail-closed even with teardown noise:\n{genuine_output}"
        );
    }
    if !genuine_classification.is_empty() {
        bail!(
            "a genuine ripr failure must not create an infra retry artifact:\n{genuine_classification}"
        );
    }

    let (failed, failed_output, no_classification) = run_gate_with_fake_gh(None)?;
    if failed.status.success() {
        bail!("an unretrievable lane log must keep the gate red");
    }
    if !failed_output.contains("classification=ripr-failure")
        || !failed_output.contains("was not retrievable after 5 attempts")
        || !failed_output.contains("RIPR_GATE_VERDICT=ripr-failure")
    {
        bail!(
            "failed retrieval must emit an explicit fail-closed classification and warning:\n{failed_output}"
        );
    }
    if !no_classification.is_empty() {
        bail!("failed retrieval must not create an infra retry artifact:\n{no_classification}");
    }
    let fetch_attempts =
        failed_output.lines().last().ok_or("fake gh did not record log fetch attempts")?;
    if fetch_attempts != "5" {
        bail!("log retrieval retry must be bounded at five attempts, got {fetch_attempts}");
    }

    Ok(())
}
