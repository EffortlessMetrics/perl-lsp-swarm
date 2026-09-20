//! Contract tests for the `Perl LSP Rust Small Result` verdict block.
//!
//! The aggregator decides a branch-protection required check, and the branch
//! that decides it is shell inside a workflow. Asserting on the YAML text still
//! passes when a comparison is inverted, so these tests extract the real `run:`
//! block and execute it under Actions bash semantics with `gh` shimmed, the way
//! `ripr_new_gap_gate_workflow.rs` does for the RIPR gate.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};
use serde_yaml_ng::Value;

#[path = "support/workflow_bash.rs"]
mod workflow_bash;

use workflow_bash::bash_executable;

const WORKFLOW: &str = ".github/workflows/em-ci-routed-rust.yml";
const RESULT_JOB: &str = "rust-small-result";
const EVALUATE_STEP: &str = "Evaluate routed result";

fn project_root() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("CARGO_MANIFEST_DIR has no parent"))?
        .to_path_buf())
}

fn workflow_yaml() -> Result<Value> {
    let root = project_root()?;
    let text =
        fs::read_to_string(root.join(WORKFLOW)).with_context(|| format!("reading {WORKFLOW}"))?;
    serde_yaml_ng::from_str(&text).with_context(|| format!("parsing {WORKFLOW}"))
}

fn evaluate_step() -> Result<Value> {
    workflow_yaml()?
        .get("jobs")
        .and_then(|jobs| jobs.get(RESULT_JOB))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps
                .iter()
                .find(|step| step.get("name").and_then(Value::as_str) == Some(EVALUATE_STEP))
        })
        .cloned()
        .ok_or_else(|| anyhow!("{RESULT_JOB} step {EVALUATE_STEP} is missing"))
}

fn evaluate_run_block() -> Result<String> {
    evaluate_step()?
        .get("run")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{EVALUATE_STEP} run block is missing"))
}

fn evaluate_env(key: &str) -> Result<String> {
    evaluate_step()?
        .get("env")
        .and_then(|env| env.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{EVALUATE_STEP} env.{key} binding is missing"))
}

fn require_real_jq() -> Result<()> {
    let output = Command::new("jq").arg("--version").output().context("locating jq")?;
    if !output.status.success() {
        bail!("these contract tests require a working jq executable");
    }
    Ok(())
}

/// What the shimmed `gh` should answer for each of the two reads the block makes.
#[derive(Clone, Copy)]
struct GhShim<'a> {
    /// `.draft` for the pull request, or `None` to make the read fail.
    live_draft: Option<&'a str>,
    /// The conclusion another run has already published, `Some("")` for none,
    /// or `None` to make the read fail.
    published: Option<&'a str>,
}

fn write_gh_shim(dir: &Path, shim: GhShim<'_>) -> Result<()> {
    let draft = match shim.live_draft {
        Some(value) => format!("printf '%s\\n' '{value}'"),
        None => "exit 1".to_string(),
    };
    let published = match shim.published {
        Some("") => "exit 0".to_string(),
        Some(value) => format!("printf '%s\\n' '{value}'"),
        None => "exit 1".to_string(),
    };
    let script = format!(
        "#!/usr/bin/env bash\n\
         set -u\n\
         for arg in \"$@\"; do\n\
         \x20 case \"$arg\" in\n\
         \x20   */pulls/*) {draft}; exit 0 ;;\n\
         \x20   */check-runs*) {published}; exit 0 ;;\n\
         \x20 esac\n\
         done\n\
         echo \"unexpected gh invocation: $*\" >&2\n\
         exit 64\n"
    );
    let path = dir.join("gh");
    fs::write(&path, script)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

struct Evaluation {
    code: i32,
    stdout: String,
    summary: String,
}

fn evaluate(route_result: &str, is_draft: &str, shim: GhShim<'_>) -> Result<Evaluation> {
    // The GH_TOKEN binding is asserted separately, on purpose: bailing here
    // would make every behavioral case below fail for that one reason, and the
    // suite could no longer tell a reverted decision from a dropped env key.
    require_real_jq()?;

    let sandbox = tempfile::tempdir().context("creating the aggregator sandbox")?;
    let bin = sandbox.path().join("bin");
    fs::create_dir_all(&bin)?;
    write_gh_shim(&bin, shim)?;
    let summary = sandbox.path().join("summary.md");
    fs::write(&summary, "")?;

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new(bash_executable())
        .arg("-c")
        .arg(evaluate_run_block()?)
        .env("PATH", path)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("GITHUB_REPOSITORY", "EffortlessMetrics/perl-lsp-swarm")
        .env("GH_TOKEN", "shim")
        .env("CHECK_NAME", "Perl LSP Rust Small Result")
        .env("PR_NUMBER", "16094")
        .env("HEAD_SHA", "c9bf380f4")
        .env("IS_DRAFT_PR", is_draft)
        .env("ROUTE_RESULT", route_result)
        .env("ROUTER_TARGET", "")
        .env("ROUTER_REASON", "")
        .env("ROUTER_ERROR", "")
        .env("ROUTER_FALLBACK_ALLOWED", "")
        .env("CX53_RESULT", "skipped")
        .env("CX43_RESULT", "skipped")
        .env("GITHUB_RESULT", "skipped")
        .env("FALLBACK_RESULT", "skipped")
        .output()
        .context("executing the aggregator run block")?;

    Ok(Evaluation {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        summary: fs::read_to_string(&summary).unwrap_or_default(),
    })
}

/// The defect measured on #16094: a run created while the pull request was a
/// draft finishes after it is ready, and its stale snapshot must not claim a
/// verdict. Against the pre-#16101 block this exits 1 with `draft-no-proof`.
#[test]
fn a_stale_draft_snapshot_defers_instead_of_failing_a_ready_pull_request() -> Result<()> {
    let run = evaluate(
        "skipped",
        "true",
        GhShim { live_draft: Some("false"), published: Some("success") },
    )?;
    assert_eq!(
        run.code, 0,
        "a superseded draft snapshot must not red a ready pull request: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains("RUST_SMALL_GATE_VERDICT=superseded-draft-snapshot"),
        "verdict must name the supersession: {}",
        run.stdout
    );
    assert!(
        !run.stdout.contains("draft-no-proof"),
        "the stale snapshot must not reach the draft verdict: {}",
        run.stdout
    );
    Ok(())
}

/// Deferring must not become a way to publish a pass over someone else's red.
/// Ordering between the two runs is not guaranteed, so when the run that
/// produced proof has already failed, the stale one mirrors that failure.
#[test]
fn a_stale_draft_snapshot_mirrors_a_published_failure() -> Result<()> {
    let run = evaluate(
        "skipped",
        "true",
        GhShim { live_draft: Some("false"), published: Some("failure") },
    )?;
    assert_eq!(
        run.code, 1,
        "a published failure must not be overwritten with a pass: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains("reported failure"),
        "the error must name what it is mirroring: {}",
        run.stdout
    );
    Ok(())
}

/// Nothing has published yet: the run that produces proof will post after us,
/// so deferring is safe and is what keeps a ready pull request from going red.
#[test]
fn a_stale_draft_snapshot_defers_when_no_conclusion_has_been_published() -> Result<()> {
    let run =
        evaluate("skipped", "true", GhShim { live_draft: Some("false"), published: Some("") })?;
    assert_eq!(run.code, 0, "deferring must be the default: {}", run.stdout);
    assert!(
        run.summary.contains("published: `none`"),
        "the summary must record that nothing was published yet: {}",
        run.summary
    );
    Ok(())
}

/// The draft posture itself is unchanged: a pull request that is still a draft
/// still has no proof, and the required check still says so.
#[test]
fn a_pull_request_that_is_still_a_draft_still_fails_closed() -> Result<()> {
    let run = evaluate(
        "skipped",
        "true",
        GhShim { live_draft: Some("true"), published: Some("success") },
    )?;
    assert_eq!(run.code, 1, "a real draft must still be NOT_PROVEN: {}", run.stdout);
    assert!(
        run.stdout.contains("RUST_SMALL_GATE_VERDICT=draft-no-proof"),
        "a real draft must keep the draft verdict: {}",
        run.stdout
    );
    Ok(())
}

/// An unreadable draft state is not evidence that the snapshot is stale, so it
/// keeps the stricter verdict rather than inventing a deferral.
#[test]
fn an_unreadable_draft_state_keeps_the_draft_verdict() -> Result<()> {
    let run = evaluate("skipped", "true", GhShim { live_draft: None, published: Some("success") })?;
    assert_eq!(run.code, 1, "a failed read must fail closed: {}", run.stdout);
    assert!(
        run.stdout.contains("RUST_SMALL_GATE_VERDICT=draft-no-proof"),
        "a failed read must keep the draft verdict: {}",
        run.stdout
    );
    Ok(())
}

/// The pre-existing non-draft skipped route is untouched.
#[test]
fn the_non_draft_skipped_route_is_unchanged() -> Result<()> {
    let run = evaluate(
        "skipped",
        "false",
        GhShim { live_draft: Some("false"), published: Some("failure") },
    )?;
    assert_eq!(run.code, 0, "a non-draft skipped router is still neutral: {}", run.stdout);
    assert!(
        run.stdout.contains("neutral"),
        "the non-draft notice must still be emitted: {}",
        run.stdout
    );
    Ok(())
}

/// The reads the fix depends on need a token and repository-scoped permissions
/// the workflow must actually declare; a silent removal would make every draft
/// snapshot fail closed again without any behavioral test noticing.
#[test]
fn the_workflow_declares_what_the_live_reads_need() -> Result<()> {
    if evaluate_env("GH_TOKEN")? != "${{ github.token }}" {
        bail!("{EVALUATE_STEP} must bind GH_TOKEN to github.token");
    }
    for key in ["CHECK_NAME", "PR_NUMBER", "HEAD_SHA"] {
        evaluate_env(key)?;
    }
    let yaml = workflow_yaml()?;
    let permissions = yaml
        .get("permissions")
        .ok_or_else(|| anyhow!("{WORKFLOW} must declare workflow-level permissions"))?;
    for scope in ["checks", "pull-requests"] {
        let granted = permissions.get(scope).and_then(Value::as_str);
        if granted != Some("read") {
            bail!("{WORKFLOW} must grant `{scope}: read`; got {granted:?}");
        }
    }
    Ok(())
}
