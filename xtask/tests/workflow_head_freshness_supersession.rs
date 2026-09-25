//! Contract tests for ripr's head-freshness precondition (#16141).
//!
//! `ripr.yml` sets `cancel-in-progress: false` deliberately (#5460): an active
//! analysis must not be terminated into a false red on a required context. The
//! cost was paid by the whole run — a run created for a head the pull request
//! has since replaced ran to completion, held the concurrency group, and
//! produced a verdict for a SHA nobody will merge. Measured over 2026-09-20
//! 06:07-16:26Z: 108 of the 216 ripr runs that produced a verdict analyzed an
//! already-replaced head, 6,045 runner-minutes across 51 pull requests.
//!
//! `Check head freshness` in `route-ripr` draws the distinction the workflow
//! did not: *terminating* an analysis is the false-red hazard, *declining to
//! start* one for a head that is already stale is not.
//!
//! Two properties carry the whole change, and both are shell inside a workflow,
//! where a YAML assertion still passes when a comparison is inverted. So these
//! extract the real `run:` blocks and execute them under Actions bash semantics
//! with `gh` shimmed, as `workflow_draft_snapshot_supersession.rs` does for the
//! draft-snapshot decision it is the sibling of:
//!
//!   1. **The router fails open.** Only a successful live read that disagrees
//!      with this run's snapshot routes `superseded`. Every uncertainty — an
//!      unreadable API, a non-sha body, an event with no pull request — routes
//!      normally. Declaring supersession wrongly would skip analysis for the
//!      head that actually merges and leave the required context unsatisfied on
//!      it, which is worse than the waste this removes.
//!
//!   2. **The gate fails closed.** No lane ran, so no proof exists for this SHA
//!      and the verdict is NOT_PROVEN. The red lands on a commit branch
//!      protection never consults, while the run for the live head publishes
//!      that head's verdict. A pass would leave a required-context success on an
//!      unanalyzed commit that a later force-push back to it would make live,
//!      and no later failure supersedes a success on a required context.
//!
//! Unix-only by construction, for the same reason the draft-snapshot suite is:
//! the `gh` shim is an extensionless executable placed on a colon-separated
//! `PATH` with a Unix mode bit, none of which reaches a Windows host. Every ripr
//! lane is linux, so the shell under proof loses no coverage.
#![cfg(unix)]

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

const WORKFLOW: &str = ".github/workflows/ripr.yml";
const FRESHNESS_STEP: &str = "Check head freshness";
const ROUTE_STEP: &str = "Decide target runner";
const EVALUATE_STEP: &str = "Evaluate routed result";

/// The snapshot this run carries, and the head it is compared against.
const SNAPSHOT_HEAD: &str = "1111111111111111111111111111111111111111";
const LIVE_HEAD: &str = "2222222222222222222222222222222222222222";

fn project_root() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("CARGO_MANIFEST_DIR has no parent"))?
        .to_path_buf())
}

fn workflow_yaml() -> Result<Value> {
    let source = fs::read_to_string(project_root()?.join(WORKFLOW))
        .with_context(|| format!("reading {WORKFLOW}"))?;
    Ok(serde_yaml_ng::from_str(&source)?)
}

fn job(name: &str) -> Result<Value> {
    workflow_yaml()?
        .get("jobs")
        .and_then(|jobs| jobs.get(name))
        .cloned()
        .ok_or_else(|| anyhow!("job {name} is missing from {WORKFLOW}"))
}

fn step(job_name: &str, step_name: &str) -> Result<Value> {
    job(job_name)?
        .get("steps")
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|s| s.get("name").and_then(Value::as_str) == Some(step_name)).cloned()
        })
        .ok_or_else(|| anyhow!("{job_name} step {step_name} is missing"))
}

fn run_block(job_name: &str, step_name: &str) -> Result<String> {
    step(job_name, step_name)?
        .get("run")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{job_name} step {step_name} has no run block"))
}

fn require_real_jq() -> Result<()> {
    let jq = Command::new("jq").arg("--version").output().context("looking for jq")?;
    if !jq.status.success() {
        bail!("these contract tests require a working jq executable");
    }
    Ok(())
}

/// A `gh` that answers the one call the freshness step makes.
///
/// `mode` is the answer under proof: a sha to return, `fail` to exit non-zero
/// as a 401/403/timeout does, or any other string to return a body that is not
/// a commit sha.
fn write_gh_shim(dir: &Path, mode: &str) -> Result<()> {
    let script = if mode == "fail" {
        "#!/usr/bin/env bash\necho 'gh: HTTP 403 Resource not accessible' >&2\nexit 1\n".to_owned()
    } else {
        format!("#!/usr/bin/env bash\nprintf '%s' {mode:?}\n")
    };
    let path = dir.join("gh");
    fs::write(&path, script)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

struct Routing {
    code: i32,
    stdout: String,
    outputs: String,
}

impl Routing {
    /// What `route-ripr` will publish as its `target` output.
    ///
    /// Empty means the freshness step wrote nothing, so `Decide target runner`
    /// runs and the normal routing decides.
    fn target(&self) -> String {
        self.outputs
            .lines()
            .find_map(|line| line.strip_prefix("target="))
            .unwrap_or_default()
            .to_owned()
    }
}

fn check_freshness(pr_number: &str, snapshot: &str, gh_mode: &str) -> Result<Routing> {
    let sandbox = tempfile::tempdir().context("creating the router sandbox")?;
    let bin = sandbox.path().join("bin");
    fs::create_dir_all(&bin)?;
    write_gh_shim(&bin, gh_mode)?;

    let outputs = sandbox.path().join("github-output");
    let summary = sandbox.path().join("summary.md");
    fs::write(&outputs, "")?;
    fs::write(&summary, "")?;

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new(bash_executable())
        .arg("-c")
        .arg(run_block("route-ripr", FRESHNESS_STEP)?)
        .env("PATH", path)
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("GITHUB_REPOSITORY", "EffortlessMetrics/perl-lsp-swarm")
        .env("GH_TOKEN", "shim")
        .env("PR_NUMBER", pr_number)
        .env("SNAPSHOT_HEAD_SHA", snapshot)
        .output()
        .context("executing the freshness run block")?;

    Ok(Routing {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        outputs: fs::read_to_string(&outputs).unwrap_or_default(),
    })
}

/// The one disagreement that is proof: a successful live read naming a
/// different head than this run's snapshot.
#[test]
fn a_superseded_head_routes_superseded_and_starts_no_lane() -> Result<()> {
    let routed = check_freshness("16141", SNAPSHOT_HEAD, LIVE_HEAD)?;

    assert_eq!(
        0, routed.code,
        "the router must succeed so the gate can read its target:\n{}",
        routed.stdout
    );
    assert_eq!("superseded", routed.target(), "outputs were:\n{}", routed.outputs);
    assert!(
        routed.outputs.contains("reason=head_superseded"),
        "the gate and the job summary name the reason; outputs were:\n{}",
        routed.outputs
    );
    assert!(
        routed.outputs.contains("fallback_allowed=false"),
        "a superseded head must not reach the disk-full fallback either; outputs were:\n{}",
        routed.outputs
    );
    Ok(())
}

/// A head that still matches routes normally, which is the case that must stay
/// untouched: this is every run that will actually be merged.
#[test]
fn a_current_head_leaves_routing_to_the_runner_decision() -> Result<()> {
    let routed = check_freshness("16141", SNAPSHOT_HEAD, SNAPSHOT_HEAD)?;

    assert_eq!(0, routed.code, "{}", routed.stdout);
    assert!(
        routed.target().is_empty(),
        "a current head must leave the target to `{ROUTE_STEP}`; outputs were:\n{}",
        routed.outputs
    );
    assert!(routed.stdout.contains("head_freshness=current"), "stdout was:\n{}", routed.stdout);
    Ok(())
}

/// Every uncertainty routes normally.
///
/// This is the property that keeps the change from becoming the failure it
/// exists to remove. A wrongly-declared supersession skips analysis for the
/// head that merges and leaves `ripr+ New Gap Gate` unsatisfied on it; the cost
/// of failing open is one run of the waste that was there before.
#[test]
fn an_unanswered_read_routes_normally_rather_than_superseded() -> Result<()> {
    for (label, pr_number, snapshot, gh_mode) in [
        ("the live read failed", "16141", SNAPSHOT_HEAD, "fail"),
        ("the body is not a sha", "16141", SNAPSHOT_HEAD, "null"),
        ("the body is empty", "16141", SNAPSHOT_HEAD, ""),
        ("the body is truncated", "16141", SNAPSHOT_HEAD, "22222222"),
        ("the event carries no pull request", "", "", LIVE_HEAD),
    ] {
        let routed = check_freshness(pr_number, snapshot, gh_mode)?;

        assert_eq!(
            0, routed.code,
            "{label}: the step must not fail the router:\n{}",
            routed.stdout
        );
        assert!(
            routed.target().is_empty(),
            "{label}: an unanswered question must route normally, not `superseded`; outputs were:\n{}",
            routed.outputs
        );
    }
    Ok(())
}

/// The gate's half: no lane ran, so no proof exists for this SHA.
///
/// The falsifier this is aimed at is a later "fix" that turns the branch green
/// to keep the required context satisfied. It cannot be: a success published
/// for an unanalyzed commit becomes live the moment anyone force-pushes back to
/// it, and no later failure supersedes a success on a required context.
#[test]
fn the_gate_treats_a_superseded_head_as_not_proven() -> Result<()> {
    require_real_jq()?;

    let sandbox = tempfile::tempdir().context("creating the gate sandbox")?;
    let summary = sandbox.path().join("summary.md");
    fs::write(&summary, "")?;
    let deadline =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + 3600;

    let output = Command::new(bash_executable())
        .arg("-c")
        .arg(run_block("ripr", EVALUATE_STEP)?)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("GITHUB_REPOSITORY", "EffortlessMetrics/perl-lsp-swarm")
        .env("GH_TOKEN", "shim")
        .env("CHECK_NAME", "ripr+ New Gap Gate")
        .env("REQUIRED_CHECK_APP_ID", "15368")
        .env("GITHUB_RUN_ID", "35489448110")
        .env("PR_NUMBER", "16141")
        .env("HEAD_SHA", SNAPSHOT_HEAD)
        .env("IS_DRAFT_PR", "false")
        .env("ROUTE_RESULT", "success")
        .env("ROUTER_TARGET", "superseded")
        .env("ROUTER_REASON", "head_superseded")
        .env("SELFHOSTED_RESULT", "skipped")
        .env("GITHUB_RESULT", "skipped")
        .env("FALLBACK_RESULT", "skipped")
        .env("RIPR_GATE_TIMEOUT_SECONDS", "300")
        .env("RIPR_GATE_FINALIZATION_RESERVE_SECONDS", "60")
        .env("RIPR_GATE_DEADLINE_EPOCH", deadline.to_string())
        .output()
        .context("executing the gate run block")?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(
        Some(1),
        output.status.code(),
        "a superseded head produced no proof, so the gate blocks:\n{stdout}"
    );
    assert!(
        stdout.contains("RIPR_GATE_VERDICT=superseded-head-no-proof"),
        "the verdict must name head supersession rather than falling through to the \
         unknown-target branch, so a reader can tell this red from a real gap:\n{stdout}"
    );
    assert!(
        fs::read_to_string(&summary)?.contains("superseded-head-no-proof (NOT_PROVEN)"),
        "the job summary carries the verdict for whoever opens the run"
    );
    Ok(())
}

/// The wiring a behavioral test cannot see, because it lives in expressions
/// Actions evaluates rather than in shell.
#[test]
fn the_router_is_wired_to_the_freshness_decision() -> Result<()> {
    let route_ripr = job("route-ripr")?;

    // Without this the live read is a 403 on every run, which fails open —
    // silently, and forever.
    let permissions = route_ripr.get("permissions").ok_or_else(|| {
        anyhow!("route-ripr declares no permissions, so it cannot read the live head")
    })?;
    assert_eq!(
        Some("read"),
        permissions.get("pull-requests").and_then(Value::as_str),
        "reading the pull request's live head needs `pull-requests: read`"
    );

    // The freshness step has to precede the routing decision it short-circuits.
    let names: Vec<String> = route_ripr
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("route-ripr has no steps"))?
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let freshness_at = names.iter().position(|n| n == FRESHNESS_STEP);
    let route_at = names.iter().position(|n| n == ROUTE_STEP);
    assert!(
        matches!((freshness_at, route_at), (Some(f), Some(r)) if f < r),
        "`{FRESHNESS_STEP}` must run before `{ROUTE_STEP}`; steps are {names:?}"
    );

    // A superseded run must not also spend the router's capacity probe.
    assert_eq!(
        Some("steps.freshness.outputs.target == ''"),
        step("route-ripr", ROUTE_STEP)?.get("if").and_then(Value::as_str),
        "`{ROUTE_STEP}` runs only when the freshness step found nothing to report"
    );

    // And the job must publish the freshness verdict, not the skipped step's
    // empty output.
    let outputs =
        route_ripr.get("outputs").ok_or_else(|| anyhow!("route-ripr declares no outputs"))?;
    for key in ["target", "reason", "fallback_allowed"] {
        let expression = outputs.get(key).and_then(Value::as_str).unwrap_or_default();
        assert_eq!(
            format!("${{{{ steps.freshness.outputs.{key} || steps.route.outputs.{key} }}}}"),
            expression,
            "route-ripr output `{key}` must read the freshness verdict first: only one of the \
             two steps writes, and an unset output is empty, so the order is the selection"
        );
    }
    Ok(())
}

/// No lane may treat `superseded` as a target to run on.
///
/// The three analysis lanes gate on an equality against a named target, so a
/// third value skips all of them without touching their conditions. That holds
/// only while no condition names it — this is what would catch someone adding
/// one.
#[test]
fn no_lane_runs_on_a_superseded_target() -> Result<()> {
    let jobs = workflow_yaml()?
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("{WORKFLOW} has no jobs"))?
        .clone();

    for (name, definition) in jobs {
        let name = name.as_str().unwrap_or_default().to_owned();
        if name == "ripr" {
            // The aggregator reads every target by design; its own branch is
            // under proof above.
            continue;
        }
        let condition = definition.get("if").and_then(Value::as_str).unwrap_or_default();
        assert!(
            !condition.contains("'superseded'"),
            "job `{name}` names the superseded target in its condition, so a run for a head \
             nobody will merge would start a lane anyway: {condition}"
        );
    }
    Ok(())
}
