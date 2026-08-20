//! Version provenance for the post-publish smoke test (#9593, finding 1).
//!
//! `post-publish-smoke.yml` used to derive the version under test from the
//! upstream run's **branch name**:
//!
//! ```text
//! if [[ "$WORKFLOW_RUN_HEAD_BRANCH" =~ ^v([0-9]+\.[0-9]+\.[0-9]+.*)$ ]]; then
//! ```
//!
//! The upstream `conclusion == success` check proved that *something* ran, not
//! that anything was published. A branch named `v9.9.9` therefore produced a
//! green smoke run that reads as publication proof for a version nobody
//! published.
//!
//! The workflow now consumes a receipt the publish workflow writes only after
//! verifying the crates on the crates.io sparse index. These tests execute the
//! resolver's actual run block under Actions bash semantics rather than
//! asserting on the YAML text, because a text assertion would still pass if the
//! refusal branch were inverted.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, anyhow, bail};
use serde_yaml_ng::Value;

#[path = "support/workflow_bash.rs"]
mod workflow_bash;

use workflow_bash::bash_executable;

const SMOKE_WORKFLOW: &str = "post-publish-smoke.yml";
const PUBLISH_WORKFLOW: &str = "publish-crates.yml";
const RESOLVE_JOB: &str = "resolve-version";
const DOWNLOAD_STEP: &str = "Download publication receipt";
const RESOLVE_STEP: &str = "Determine version";

/// A version no publish ever produced, used as the branch name in the
/// reproduction of the original finding.
const FABRICATED: &str = "9.9.9";
/// A version a receipt legitimately attests to.
const PUBLISHED: &str = "0.18.0";
/// The repository's default branch, the authoritative fallback subject.
const DEFAULT_BRANCH: &str = "main";
/// The commit a publication receipt attests to.
const PUBLISHED_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn workflow(name: &str) -> Result<Value> {
    let path = project_root().join(".github/workflows").join(name);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml_ng::from_str(&content).with_context(|| format!("parsing {}", path.display()))
}

fn steps(workflow: &Value, job: &str) -> Result<Vec<Value>> {
    workflow
        .get("jobs")
        .and_then(|jobs| jobs.get(job))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .cloned()
        .ok_or_else(|| anyhow!("job `{job}` must declare steps"))
}

fn step_index(steps: &[Value], name: &str) -> Result<usize> {
    steps
        .iter()
        .position(|step| step.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| anyhow!("step `{name}` must exist"))
}

fn resolve_run_block() -> Result<String> {
    let workflow = workflow(SMOKE_WORKFLOW)?;
    let steps = steps(&workflow, RESOLVE_JOB)?;
    let index = step_index(&steps, RESOLVE_STEP)?;
    steps[index]
        .get("run")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("step `{RESOLVE_STEP}` must have a run block"))
}

/// What the resolver decided, read back from the `GITHUB_OUTPUT` file exactly
/// as a downstream job would consume it.
struct Resolution {
    output: Output,
    version: Option<String>,
    should_run: Option<String>,
    subject: Option<String>,
}

impl Resolution {
    fn combined(&self) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&self.output.stdout),
            String::from_utf8_lossy(&self.output.stderr)
        )
    }

    fn runs(&self) -> bool {
        self.should_run.as_deref() == Some("true")
    }

    /// Every refusal here is a deliberate `exit 0` with `should_run=false`, so
    /// the smoke job *skips*. Asserting only the outputs would let a resolver
    /// that writes them and then exits nonzero pass — turning an intended skip
    /// into a red workflow step, which reads as a broken release lane rather
    /// than an absent receipt.
    fn assert_step_succeeded(&self, context: &str) -> Result<()> {
        if !self.output.status.success() {
            bail!(
                "{context}: the resolver step must exit 0, got {:?}\n{}",
                self.output.status.code(),
                self.combined()
            );
        }
        Ok(())
    }
}

/// Execute the resolver with a controlled receipt.
///
/// `receipt` is the file content the download step would have produced;
/// `None` means the upstream run published no receipt.
fn resolve(event: &str, conclusion: &str, receipt: Option<&str>) -> Result<Resolution> {
    let dir = tempfile::tempdir().context("creating the resolver sandbox")?;
    let receipt_path = dir.path().join("publication-receipt.json");
    if let Some(body) = receipt {
        fs::write(&receipt_path, body).context("writing the fixture receipt")?;
    }
    let github_output = dir.path().join("github_output");
    fs::write(&github_output, "").context("creating GITHUB_OUTPUT")?;

    let run = resolve_run_block()?;
    let output = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-c", &run])
        .current_dir(dir.path())
        .env("EVENT_NAME", event)
        .env("WORKFLOW_RUN_CONCLUSION", conclusion)
        // The dispatch input is empty on every workflow_run path, exactly as
        // Actions renders an absent input.
        .env("DISPATCH_VERSION", "")
        // Set deliberately, and to the fabricated version. The resolver no
        // longer reads it, but the original defect *did*: without this the
        // refuse-path tests would pass against the old branch-regex resolver
        // simply because the variable was absent, and would prove nothing.
        .env("WORKFLOW_RUN_HEAD_BRANCH", format!("v{FABRICATED}"))
        .env("RECEIPT_PATH", &receipt_path)
        .env("DEFAULT_BRANCH", DEFAULT_BRANCH)
        .env("GITHUB_OUTPUT", &github_output)
        .output()
        .context("executing the resolver under Actions bash semantics")?;

    // Not `unwrap_or_default()`: an unreadable GITHUB_OUTPUT would read as
    // "no outputs", which is indistinguishable from a refusal — every
    // refuse-path test below would then pass without the resolver having
    // refused anything.
    let rendered = fs::read_to_string(&github_output)
        .with_context(|| format!("reading GITHUB_OUTPUT at {}", github_output.display()))?;
    let read = |key: &str| -> Option<String> {
        rendered
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter(|(name, _)| *name == key)
            .map(|(_, value)| value.to_owned())
            .next_back()
    };

    Ok(Resolution {
        version: read("version"),
        should_run: read("should_run"),
        subject: read("subject"),
        output,
    })
}

fn receipt_for(version: &str) -> String {
    format!(
        r#"{{"version":"{version}","subject_sha":"{PUBLISHED_SHA}","crate_count":34,"publish_run_id":"42"}}"#
    )
}

/// The finding itself. A ref that merely looks like a release must not produce
/// a smoke verdict, because the smoke result is read as publication proof.
#[test]
fn release_shaped_ref_without_a_receipt_yields_no_verdict() -> Result<()> {
    let resolved = resolve("workflow_run", "success", None)?;
    resolved.assert_step_succeeded("absent receipt")?;

    if resolved.runs() {
        bail!(
            "a run with no publication receipt must not smoke-test anything, got version={:?}\n{}",
            resolved.version,
            resolved.combined()
        );
    }
    // The fabricated version must not survive anywhere a later step could read
    // it — the original defect was a *claim*, not merely a wasted job.
    if resolved.version.as_deref() == Some(FABRICATED) {
        bail!("the resolver reported a version nobody published:\n{}", resolved.combined());
    }
    if !resolved.combined().contains("receipt") {
        bail!("the refusal must name the missing receipt:\n{}", resolved.combined());
    }
    Ok(())
}

/// The legitimate path: a receipt from a real publication is honoured.
#[test]
fn receipt_version_is_used() -> Result<()> {
    let resolved = resolve("workflow_run", "success", Some(&receipt_for(PUBLISHED)))?;
    resolved.assert_step_succeeded("receipted publication")?;

    if !resolved.runs() {
        bail!("a receipted publication must be smoke-tested:\n{}", resolved.combined());
    }
    if resolved.version.as_deref() != Some(PUBLISHED) {
        bail!(
            "expected the receipt's version {PUBLISHED}, got {:?}\n{}",
            resolved.version,
            resolved.combined()
        );
    }
    Ok(())
}

/// The receipt is the only authority. Even a perfectly release-shaped ref
/// cannot override what was actually published — this is the assertion that
/// fails if the ref-derivation is ever reintroduced as a fallback.
#[test]
fn the_ref_cannot_override_the_receipt() -> Result<()> {
    let resolved = resolve("workflow_run", "success", Some(&receipt_for(PUBLISHED)))?;
    resolved.assert_step_succeeded("receipt versus ref")?;

    if resolved.version.as_deref() == Some(FABRICATED) {
        bail!("the ref name outranked the receipt:\n{}", resolved.combined());
    }
    if resolved.version.as_deref() != Some(PUBLISHED) {
        bail!("expected {PUBLISHED}, got {:?}", resolved.version);
    }

    // The resolver must not consult the ref at all. A reader of this job should
    // not have to reason about which of two sources wins.
    let workflow = workflow(SMOKE_WORKFLOW)?;
    let resolve_job = workflow
        .get("jobs")
        .and_then(|jobs| jobs.get(RESOLVE_JOB))
        .ok_or_else(|| anyhow!("`{RESOLVE_JOB}` must exist"))?;
    let rendered = serde_yaml_ng::to_string(resolve_job)?;
    if rendered.contains("head_branch") {
        bail!("`{RESOLVE_JOB}` must not read the upstream ref name:\n{rendered}");
    }
    Ok(())
}

/// A receipt that carries no version is a broken instrument, not a licence to
/// guess.
#[test]
fn unusable_receipt_fails_closed() -> Result<()> {
    for body in [r#"{"subject_sha":"abc"}"#, "not json at all", "{}"] {
        let resolved = resolve("workflow_run", "success", Some(body))?;
        resolved.assert_step_succeeded(&format!("unusable receipt {body:?}"))?;
        if resolved.runs() {
            bail!(
                "receipt {body:?} must not produce a verdict, got version={:?}\n{}",
                resolved.version,
                resolved.combined()
            );
        }
    }
    Ok(())
}

/// Finding 2 of #9593: whoever can dispatch this workflow must not also supply
/// the code that issues the verdict. The smoke job executes
/// `scripts/post-publish-smoke.sh`, so the ref it checks out *is* the authority
/// deciding whether a published version is installable.
#[test]
fn the_proof_never_executes_the_dispatch_selection() -> Result<()> {
    let workflow = workflow(SMOKE_WORKFLOW)?;
    let steps = steps(&workflow, "smoke")?;
    let checkout = steps
        .iter()
        .find(|step| {
            step.get("uses").and_then(Value::as_str).is_some_and(|u| u.contains("actions/checkout"))
        })
        .ok_or_else(|| anyhow!("the smoke job must check out something"))?;

    // No `ref:` means "whatever the operator selected", which is the defect.
    let declared =
        checkout.get("with").and_then(|with| with.get("ref")).and_then(Value::as_str).ok_or_else(
            || anyhow!("the smoke checkout must pin a ref, not take the dispatch default"),
        )?;

    if !declared.contains("resolve-version.outputs.subject") {
        bail!("the smoke checkout must use the resolved subject, found `{declared}`");
    }
    Ok(())
}

/// The subject is taken from the same receipt as the version, so the proof runs
/// against the commit it is certifying rather than a ref chosen later.
#[test]
fn receipted_run_executes_the_published_subject() -> Result<()> {
    let resolved = resolve("workflow_run", "success", Some(&receipt_for(PUBLISHED)))?;
    resolved.assert_step_succeeded("receipted subject")?;

    if resolved.subject.as_deref() != Some(PUBLISHED_SHA) {
        bail!(
            "expected the receipt's subject {PUBLISHED_SHA}, got {:?}\n{}",
            resolved.subject,
            resolved.combined()
        );
    }
    Ok(())
}

/// A receipt predating the subject field must not strand a real publication,
/// but it must fall back to an authoritative ref rather than to the dispatch
/// selection.
#[test]
fn receipt_without_a_subject_falls_back_to_the_default_branch() -> Result<()> {
    let legacy = format!(r#"{{"version":"{PUBLISHED}","crate_count":34}}"#);
    let resolved = resolve("workflow_run", "success", Some(&legacy))?;
    resolved.assert_step_succeeded("receipt without a subject")?;

    if !resolved.runs() {
        bail!("a receipt with a version but no subject must still be verified");
    }
    if resolved.subject.as_deref() != Some(DEFAULT_BRANCH) {
        bail!("expected the default branch, got {:?}", resolved.subject);
    }
    Ok(())
}

/// A malformed subject is not a ref to trust. Anything that is not a full
/// commit SHA falls back rather than being handed to `actions/checkout`.
#[test]
fn a_malformed_subject_is_not_trusted() -> Result<()> {
    for bogus in ["refs/heads/attacker", "0123456", "../../etc", ""] {
        let receipt =
            format!(r#"{{"version":"{PUBLISHED}","subject_sha":"{bogus}","crate_count":34}}"#);
        let resolved = resolve("workflow_run", "success", Some(&receipt))?;
        // Falling back is a successful resolution, not a refusal — if it starts
        // exiting nonzero the publication silently stops being verified.
        resolved.assert_step_succeeded(&format!("malformed subject {bogus:?}"))?;
        if !resolved.runs() {
            bail!("a malformed subject must fall back, not skip the smoke test");
        }
        if resolved.subject.as_deref() != Some(DEFAULT_BRANCH) {
            bail!(
                "subject {bogus:?} must not be used as a checkout ref, got {:?}",
                resolved.subject
            );
        }
    }
    Ok(())
}

/// Dispatch supplies a version, not a ref — the proof still comes from the
/// default branch.
#[test]
fn dispatch_executes_the_default_branch() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let github_output = dir.path().join("github_output");
    fs::write(&github_output, "")?;

    let run = resolve_run_block()?;
    let output = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-c", &run])
        .current_dir(dir.path())
        .env("EVENT_NAME", "workflow_dispatch")
        .env("DISPATCH_VERSION", PUBLISHED)
        .env("WORKFLOW_RUN_CONCLUSION", "")
        .env("WORKFLOW_RUN_HEAD_BRANCH", format!("v{FABRICATED}"))
        .env("RECEIPT_PATH", dir.path().join("absent.json"))
        .env("DEFAULT_BRANCH", DEFAULT_BRANCH)
        .env("GITHUB_OUTPUT", &github_output)
        .output()?;

    if !output.status.success() {
        bail!(
            "dispatch must resolve successfully, got exit {:?}\n{}{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let rendered = fs::read_to_string(&github_output)?;
    if !rendered.contains(&format!("subject={DEFAULT_BRANCH}")) {
        bail!("dispatch must run the proof from the default branch, got:\n{rendered}");
    }
    Ok(())
}

/// A receipt whose version is not a version is a corrupt instrument, and the
/// only path here that must fail the step rather than skip it. Skipping would
/// hide a broken publish workflow behind a quiet "nothing to verify"; the
/// operator needs to see it.
#[test]
fn receipt_carrying_a_malformed_version_fails_the_step() -> Result<()> {
    let resolved = resolve("workflow_run", "success", Some(&receipt_for("banana")))?;

    if resolved.output.status.success() {
        bail!("a corrupt receipt version must fail the step:\n{}", resolved.combined());
    }
    if resolved.runs() {
        bail!("a corrupt receipt version must not be smoke-tested:\n{}", resolved.combined());
    }
    Ok(())
}

/// A failed publish must not be smoke-tested even if a receipt is somehow
/// present, since the receipt would predate the failure.
#[test]
fn unsuccessful_upstream_run_is_skipped() -> Result<()> {
    let resolved = resolve("workflow_run", "failure", Some(&receipt_for(PUBLISHED)))?;
    resolved.assert_step_succeeded("failed upstream run")?;
    if resolved.runs() {
        bail!("a failed publish must not be smoke-tested:\n{}", resolved.combined());
    }
    Ok(())
}

/// Manual dispatch still works — the fix must not remove the operator's
/// ability to re-verify a published version.
#[test]
fn manual_dispatch_still_resolves_its_input() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let github_output = dir.path().join("github_output");
    fs::write(&github_output, "")?;

    let run = resolve_run_block()?;
    let output = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-c", &run])
        .current_dir(dir.path())
        .env("EVENT_NAME", "workflow_dispatch")
        .env("DISPATCH_VERSION", PUBLISHED)
        .env("WORKFLOW_RUN_CONCLUSION", "")
        .env("WORKFLOW_RUN_HEAD_BRANCH", format!("v{FABRICATED}"))
        .env("RECEIPT_PATH", dir.path().join("absent.json"))
        .env("DEFAULT_BRANCH", DEFAULT_BRANCH)
        .env("GITHUB_OUTPUT", &github_output)
        .output()?;

    let rendered = fs::read_to_string(&github_output)?;
    if !rendered.contains(&format!("version={PUBLISHED}")) || !rendered.contains("should_run=true")
    {
        bail!(
            "dispatch must resolve its own input, got:\n{rendered}\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// The receipt is fetched from the run that published, not from this one, and
/// it has to arrive before anything reads it.
#[test]
fn receipt_is_downloaded_from_the_upstream_run_first() -> Result<()> {
    let workflow = workflow(SMOKE_WORKFLOW)?;
    let steps = steps(&workflow, RESOLVE_JOB)?;
    let download = step_index(&steps, DOWNLOAD_STEP)?;
    let resolve = step_index(&steps, RESOLVE_STEP)?;

    if download >= resolve {
        bail!("`{DOWNLOAD_STEP}` must run before `{RESOLVE_STEP}`");
    }

    let rendered = serde_yaml_ng::to_string(&steps[download])?;
    if !rendered.contains("workflow_run.id") {
        bail!("the receipt must be fetched from the upstream run's id:\n{rendered}");
    }
    // Cross-run artifact reads need this scope; without it the download step
    // fails and every publication silently stops being smoke-tested.
    let permissions = workflow.get("permissions").map(serde_yaml_ng::to_string).transpose()?;
    match permissions {
        Some(rendered) if rendered.contains("actions: read") => Ok(()),
        other => bail!("the workflow must grant `actions: read` to read the receipt: {other:?}"),
    }
}

/// The receipt is only evidence if the publish workflow writes it after proving
/// the crates are on the index, and never for a dry run.
#[test]
fn the_receipt_is_written_only_by_a_verified_publish() -> Result<()> {
    let workflow = workflow(PUBLISH_WORKFLOW)?;
    let verify = workflow
        .get("jobs")
        .and_then(|jobs| jobs.get("verify"))
        .ok_or_else(|| anyhow!("`verify` job must exist in {PUBLISH_WORKFLOW}"))?;

    // A dry run publishes nothing, so it must not be able to emit a receipt.
    let guard = verify.get("if").and_then(Value::as_str).unwrap_or_default();
    if !guard.contains("dry_run") {
        bail!("`verify` must stay excluded from dry runs, found if: {guard:?}");
    }

    let steps = steps(&workflow, "verify")?;
    let write = step_index(&steps, "Write publication receipt")?;
    let upload = step_index(&steps, "Upload publication receipt")?;
    let check = steps
        .iter()
        .position(|step| {
            step.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.contains("Verify all crates"))
        })
        .ok_or_else(|| anyhow!("`verify` must still check the sparse index"))?;

    if check >= write || write >= upload {
        bail!("the receipt must be written after verification and before upload");
    }
    for index in [write, upload] {
        let guard = steps[index].get("if").and_then(Value::as_str).unwrap_or_default();
        if !guard.contains("success()") {
            bail!("receipt step {index} must be guarded on success(), found {guard:?}");
        }
    }

    // The artifact name is the contract between the two workflows.
    let rendered = serde_yaml_ng::to_string(&steps[upload])?;
    if !rendered.contains("publication-receipt") {
        bail!("the uploaded artifact must be named `publication-receipt`:\n{rendered}");
    }
    Ok(())
}

/// The consumer and producer must agree on the artifact name, or the receipt
/// silently never arrives and every publication stops being verified.
#[test]
fn producer_and_consumer_agree_on_the_artifact_name() -> Result<()> {
    let produced = {
        let workflow = workflow(PUBLISH_WORKFLOW)?;
        let steps = steps(&workflow, "verify")?;
        let index = step_index(&steps, "Upload publication receipt")?;
        artifact_name(&steps[index])?
    };
    let consumed = {
        let workflow = workflow(SMOKE_WORKFLOW)?;
        let steps = steps(&workflow, RESOLVE_JOB)?;
        let index = step_index(&steps, DOWNLOAD_STEP)?;
        artifact_name(&steps[index])?
    };
    if produced != consumed {
        bail!("publish uploads `{produced}` but smoke downloads `{consumed}`");
    }
    Ok(())
}

fn artifact_name(step: &Value) -> Result<String> {
    step.get("with")
        .and_then(|with| with.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("artifact step must name its artifact"))
}

/// The path the consumer reads must be the path the download step writes to.
#[test]
fn receipt_path_matches_the_download_destination() -> Result<()> {
    let workflow = workflow(SMOKE_WORKFLOW)?;
    let steps = steps(&workflow, RESOLVE_JOB)?;

    let download = &steps[step_index(&steps, DOWNLOAD_STEP)?];
    let destination = download
        .get("with")
        .and_then(|with| with.get("path"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("the download step must declare a path"))?;

    let resolve = &steps[step_index(&steps, RESOLVE_STEP)?];
    let declared = resolve
        .get("env")
        .and_then(|env| env.get("RECEIPT_PATH"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("the resolver must declare RECEIPT_PATH"))?;

    let expected = Path::new(destination).join("publication-receipt.json");
    if Path::new(declared) != expected {
        bail!("RECEIPT_PATH is `{declared}` but the artifact lands in `{}`", expected.display());
    }
    Ok(())
}
