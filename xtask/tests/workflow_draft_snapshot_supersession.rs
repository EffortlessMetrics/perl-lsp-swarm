//! Contract tests for the draft-snapshot decision in both routed result gates.
//!
//! `IS_DRAFT_PR` is a webhook snapshot. A run created while a pull request was a
//! draft can finish after it is ready and publish a verdict about a state that
//! no longer exists, overwriting the verdict of the run that actually produced
//! proof. Both `Perl LSP Rust Small Result` and `ripr+ New Gap Gate` are
//! branch-protection required checks, and in both the decision is shell inside a
//! workflow. Asserting on the YAML text still passes when a comparison is
//! inverted, so these tests extract the real `run:` block and execute it under
//! Actions bash semantics with `gh` shimmed.

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

const EVALUATE_STEP: &str = "Evaluate routed result";

/// One required check's aggregator: where its decision lives and what it calls
/// its verdict.
#[derive(Clone, Copy)]
struct Gate {
    workflow: &'static str,
    job: &'static str,
    check_name: &'static str,
    verdict_var: &'static str,
}

const RUST_SMALL: Gate = Gate {
    workflow: ".github/workflows/em-ci-routed-rust.yml",
    job: "rust-small-result",
    check_name: "Perl LSP Rust Small Result",
    verdict_var: "RUST_SMALL_GATE_VERDICT",
};

const RIPR: Gate = Gate {
    workflow: ".github/workflows/ripr.yml",
    job: "ripr",
    check_name: "ripr+ New Gap Gate",
    verdict_var: "RIPR_GATE_VERDICT",
};

fn project_root() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("CARGO_MANIFEST_DIR has no parent"))?
        .to_path_buf())
}

fn workflow_yaml(gate: Gate) -> Result<Value> {
    let root = project_root()?;
    let text = fs::read_to_string(root.join(gate.workflow))
        .with_context(|| format!("reading {}", gate.workflow))?;
    serde_yaml_ng::from_str(&text).with_context(|| format!("parsing {}", gate.workflow))
}

fn evaluate_step(gate: Gate) -> Result<Value> {
    workflow_yaml(gate)?
        .get("jobs")
        .and_then(|jobs| jobs.get(gate.job))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps
                .iter()
                .find(|step| step.get("name").and_then(Value::as_str) == Some(EVALUATE_STEP))
        })
        .cloned()
        .ok_or_else(|| anyhow!("{} step {EVALUATE_STEP} is missing", gate.job))
}

fn evaluate_run_block(gate: Gate) -> Result<String> {
    evaluate_step(gate)?
        .get("run")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{EVALUATE_STEP} run block is missing in {}", gate.workflow))
}

fn evaluate_env(gate: Gate, key: &str) -> Result<String> {
    evaluate_step(gate)?
        .get("env")
        .and_then(|env| env.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{} {EVALUATE_STEP} env.{key} is missing", gate.workflow))
}

fn require_real_jq() -> Result<()> {
    let output = Command::new("jq").arg("--version").output().context("locating jq")?;
    if !output.status.success() {
        bail!("these contract tests require a working jq executable");
    }
    Ok(())
}

/// What the shimmed `gh` answers for each of the two reads the decision makes.
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

impl Evaluation {
    fn verdict_is(&self, gate: Gate, verdict: &str) -> bool {
        self.stdout.contains(&format!("{}={verdict}", gate.verdict_var))
    }
}

fn evaluate(
    gate: Gate,
    route_result: &str,
    is_draft: &str,
    shim: GhShim<'_>,
) -> Result<Evaluation> {
    // The env bindings are asserted separately, on purpose: bailing here would
    // make every behavioral case fail for that one reason, and the suite could
    // no longer tell a reverted decision from a dropped env key.
    require_real_jq()?;

    let sandbox = tempfile::tempdir().context("creating the aggregator sandbox")?;
    let bin = sandbox.path().join("bin");
    fs::create_dir_all(&bin)?;
    write_gh_shim(&bin, shim)?;
    let summary = sandbox.path().join("summary.md");
    fs::write(&summary, "")?;

    // Far enough ahead that the RIPR gate's own deadline guard never fires; the
    // Rust Small block ignores it.
    let deadline =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + 3600;

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new(bash_executable())
        .arg("-c")
        .arg(evaluate_run_block(gate)?)
        .env("PATH", path)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("GITHUB_REPOSITORY", "EffortlessMetrics/perl-lsp-swarm")
        .env("GH_TOKEN", "shim")
        .env("CHECK_NAME", gate.check_name)
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
        .env("SELFHOSTED_RESULT", "skipped")
        .env("GITHUB_RESULT", "skipped")
        .env("FALLBACK_RESULT", "skipped")
        .env("RIPR_GATE_TIMEOUT_SECONDS", "300")
        .env("RIPR_GATE_FINALIZATION_RESERVE_SECONDS", "60")
        .env("RIPR_GATE_DEADLINE_EPOCH", deadline.to_string())
        .output()
        .context("executing the aggregator run block")?;

    Ok(Evaluation {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        summary: fs::read_to_string(&summary).unwrap_or_default(),
    })
}

/// The whole decision table for one gate, so both required checks are held to
/// exactly the same rule rather than drifting apart.
fn assert_draft_snapshot_contract(gate: Gate) -> Result<()> {
    // Measured on #16094: the run that executed the lane published a success,
    // and a run created while the pull request was still a draft then finished
    // and replaced it. The stale run mirrors that success instead.
    let run = evaluate(
        gate,
        "skipped",
        "true",
        GhShim { live_draft: Some("false"), published: Some("success") },
    )?;
    if run.code != 0 || !run.verdict_is(gate, "superseded-draft-snapshot") {
        bail!(
            "{}: a stale snapshot must mirror a published success: {}",
            gate.workflow,
            run.stdout
        );
    }
    if run.stdout.contains("draft-no-proof") {
        bail!(
            "{}: a stale snapshot must not reach the draft verdict: {}",
            gate.workflow,
            run.stdout
        );
    }

    // Only an authoritative success is mirrored. Ordering between the two runs
    // is not guaranteed in either direction, so a published red, a run that
    // reached no verdict, an unreadable read and an empty read must all stay
    // non-successful — AGENTS.md makes missing evidence NOT_PROVEN.
    for published in [Some("failure"), Some("cancelled"), Some(""), None] {
        let run =
            evaluate(gate, "skipped", "true", GhShim { live_draft: Some("false"), published })?;
        if run.code == 0 || !run.verdict_is(gate, "superseded-no-proof") {
            bail!(
                "{}: published={published:?} must stay NOT_PROVEN: {}",
                gate.workflow,
                run.stdout
            );
        }
    }
    let run = evaluate(
        gate,
        "skipped",
        "true",
        GhShim { live_draft: Some("false"), published: Some("") },
    )?;
    if !run.summary.contains("superseded-no-proof (NOT_PROVEN; published: `none`)") {
        bail!("{}: the summary must record why it is not proven: {}", gate.workflow, run.summary);
    }

    // A pull request that is still a draft is unchanged, and a draft state that
    // cannot be read is not evidence that the snapshot is stale.
    for live_draft in [Some("true"), None] {
        let run =
            evaluate(gate, "skipped", "true", GhShim { live_draft, published: Some("success") })?;
        if run.code == 0 || !run.verdict_is(gate, "draft-no-proof") {
            bail!(
                "{}: live_draft={live_draft:?} must keep the draft verdict: {}",
                gate.workflow,
                run.stdout
            );
        }
    }

    // The pre-existing non-draft skipped route is untouched.
    let run = evaluate(
        gate,
        "skipped",
        "false",
        GhShim { live_draft: Some("false"), published: Some("failure") },
    )?;
    if run.code != 0 || !run.stdout.contains("neutral") {
        bail!("{}: the non-draft skipped route changed: {}", gate.workflow, run.stdout);
    }

    Ok(())
}

/// The reads the decision depends on need a token, the check's own name, and the
/// subject's identity. A silent removal would make every draft snapshot fail
/// closed again without any behavioral test noticing.
fn assert_live_read_bindings(gate: Gate) -> Result<()> {
    if evaluate_env(gate, "GH_TOKEN")? != "${{ github.token }}" {
        bail!("{}: {EVALUATE_STEP} must bind GH_TOKEN to github.token", gate.workflow);
    }
    if evaluate_env(gate, "CHECK_NAME")? != gate.check_name {
        bail!("{}: CHECK_NAME must name this gate's own check", gate.workflow);
    }
    for key in ["PR_NUMBER", "HEAD_SHA"] {
        evaluate_env(gate, key)?;
    }
    // A job-level `permissions:` block replaces the workflow-level one outright
    // rather than adding to it, so the effective scopes are the job's when it
    // declares any. Asserting on the workflow level alone would have passed
    // while the RIPR gate job silently dropped both reads to 403.
    let yaml = workflow_yaml(gate)?;
    let permissions = yaml
        .get("jobs")
        .and_then(|jobs| jobs.get(gate.job))
        .and_then(|job| job.get("permissions"))
        .or_else(|| yaml.get("permissions"))
        .ok_or_else(|| anyhow!("{} must declare permissions for {}", gate.workflow, gate.job))?;
    for scope in ["checks", "pull-requests"] {
        let granted = permissions.get(scope).and_then(Value::as_str);
        if granted != Some("read") {
            bail!(
                "{}: job {} must grant `{scope}: read`; got {granted:?}",
                gate.workflow,
                gate.job
            );
        }
    }
    Ok(())
}

#[test]
fn rust_small_result_honours_the_draft_snapshot_contract() -> Result<()> {
    assert_draft_snapshot_contract(RUST_SMALL)
}

#[test]
fn rust_small_result_declares_what_the_live_reads_need() -> Result<()> {
    assert_live_read_bindings(RUST_SMALL)
}

#[test]
fn ripr_new_gap_gate_honours_the_draft_snapshot_contract() -> Result<()> {
    assert_draft_snapshot_contract(RIPR)
}

#[test]
fn ripr_new_gap_gate_declares_what_the_live_reads_need() -> Result<()> {
    assert_live_read_bindings(RIPR)
}
