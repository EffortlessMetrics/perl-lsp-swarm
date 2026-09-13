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
/// The receipt envelope this resolver understands. A receipt written to any
/// other schema is refused rather than partially read (#15332).
const SCHEMA: &str = "publication_receipt.v1";

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
        r#"{{"schema_version":"{SCHEMA}","release_version":"{version}","subject_sha":"{PUBLISHED_SHA}","crate_count":34,"publish_run_id":"42"}}"#
    )
}

/// The producer's own run block, executed rather than pattern-matched.
///
/// Reading the producer's `printf` as text and asserting it mentions the same
/// field names the consumer reads would pass while the two sides drift in any
/// way a string match cannot see — quoting, ordering, a field written but left
/// empty. Running it and feeding the bytes to the consumer cannot.
fn publish_receipt_run_block() -> Result<String> {
    let workflow = workflow(PUBLISH_WORKFLOW)?;
    let steps = steps(&workflow, "verify")?;
    let index = step_index(&steps, "Write publication receipt")?;
    steps[index]
        .get("run")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("step `Write publication receipt` must have a run block"))
}

/// Execute the publish workflow's receipt writer and return exactly what it
/// wrote to disk.
fn produced_receipt(version: &str, subject: &str) -> Result<String> {
    let dir = tempfile::tempdir().context("creating the producer sandbox")?;
    let run = publish_receipt_run_block()?;
    let output = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-c", &run])
        .current_dir(dir.path())
        .env("CRATES_JSON", r#"["perl-lsp","perl-parser"]"#)
        .env("PUBLISHED_VERSION", version)
        .env("SUBJECT_SHA", subject)
        .env("RUN_ID", "42")
        .output()
        .context("executing the receipt writer under Actions bash semantics")?;

    if !output.status.success() {
        bail!(
            "the receipt writer must succeed for version {version}, got exit {:?}\n{}{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let written = dir.path().join("receipt").join("publication-receipt.json");
    fs::read_to_string(&written)
        .with_context(|| format!("reading the produced receipt at {}", written.display()))
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

/// A receipt that exists but cannot be read is a broken instrument, and a
/// broken instrument has to be as visible as a failed one.
///
/// This previously asserted the opposite — that each of these skipped with
/// `exit 0`. That made the resolver green while the smoke test never ran and
/// nothing in the run said so (#15332). The skip is correct only for an
/// *absent* receipt, which means nothing was published; a receipt that is
/// present and unintelligible means something was published and this run
/// cannot say what.
#[test]
fn an_unreadable_receipt_fails_the_step() -> Result<()> {
    let cases = [
        // Corrupt bytes.
        ("not json at all", "non-JSON body"),
        // Valid JSON, no envelope, no fields.
        ("{}", "empty object"),
        // Valid JSON of the wrong kind entirely.
        (r#"[{"schema_version":"publication_receipt.v1"}]"#, "JSON array"),
        // Envelope present, but the payload is not a string.
        (
            r#"{"schema_version":"publication_receipt.v1","release_version":18,"subject_sha":"0123456789abcdef0123456789abcdef01234567"}"#,
            "non-string release_version",
        ),
        // Envelope present, release version absent.
        (
            r#"{"schema_version":"publication_receipt.v1","subject_sha":"0123456789abcdef0123456789abcdef01234567"}"#,
            "no release_version",
        ),
    ];

    for (body, description) in cases {
        let resolved = resolve("workflow_run", "success", Some(body))?;

        if resolved.output.status.success() {
            bail!(
                "a receipt with a {description} must fail the step, got exit 0\n{}",
                resolved.combined()
            );
        }
        if resolved.runs() {
            bail!(
                "a receipt with a {description} must not produce a verdict, got version={:?}\n{}",
                resolved.version,
                resolved.combined()
            );
        }
        if resolved.version.as_deref() == Some(FABRICATED) {
            bail!("a receipt with a {description} must not yield a version nobody published");
        }
    }
    Ok(())
}

/// A field cannot smuggle the next field's line.
///
/// The decoder hands the resolver one line per field. A `release_version`
/// carrying an embedded newline would otherwise supply the subject line itself,
/// and the real `subject_sha` would be read past and ignored — the checkout
/// would run a commit the receipt does not attest to, which is the exact
/// false-proof class this workflow exists to prevent.
#[test]
fn a_field_cannot_smuggle_the_next_line() -> Result<()> {
    let smuggled = "1111111111111111111111111111111111111111";
    let receipt = format!(
        r#"{{"schema_version":"{SCHEMA}","release_version":"{PUBLISHED}\n{smuggled}","subject_sha":"{PUBLISHED_SHA}"}}"#
    );
    let resolved = resolve("workflow_run", "success", Some(&receipt))?;

    if resolved.output.status.success() {
        bail!("a field containing a line break must fail the step:\n{}", resolved.combined());
    }
    if resolved.subject.as_deref() == Some(smuggled) {
        bail!("a smuggled line reached the checkout as the subject:\n{}", resolved.combined());
    }
    if resolved.runs() {
        bail!("a field containing a line break must not be certified:\n{}", resolved.combined());
    }
    Ok(())
}

/// The finding in #15332. A producer that moves to a new schema — renaming
/// `release_version`, adding a required field — must not be silently
/// unreadable to an older consumer.
///
/// Before the envelope existed the consumer read `.get("version", "")`,
/// received `""`, and turned that into `should_run=false` with `exit 0`: a
/// green run with no smoke test and no diagnostic. This is the case that
/// distinguishes an envelope-checking consumer from a field-guessing one.
#[test]
fn a_future_schema_version_fails_the_step() -> Result<()> {
    let v2 = format!(
        r#"{{"schema_version":"publication_receipt.v2","release":"{PUBLISHED}","subject_sha":"{PUBLISHED_SHA}"}}"#
    );
    let resolved = resolve("workflow_run", "success", Some(&v2))?;

    if resolved.output.status.success() {
        bail!("a v2 receipt must fail an unmigrated v1 consumer:\n{}", resolved.combined());
    }
    if resolved.runs() {
        bail!("a v2 receipt must not be smoke-tested:\n{}", resolved.combined());
    }
    // The operator has to be able to tell an unreadable receipt from an absent
    // one without reading the workflow source.
    if !resolved.combined().contains(SCHEMA) {
        bail!("the refusal must name the schema it expected:\n{}", resolved.combined());
    }
    Ok(())
}

/// The pre-#15332 receipt shape carries no envelope at all. It must be refused
/// rather than read on a best-effort basis, or the envelope check is decorative.
///
/// This is the one deliberate compatibility break in the change: a publish run
/// that started before this landed and completes after it fails this resolver
/// instead of being certified. That is the intended direction — the operator
/// re-certifies by dispatching this workflow with an explicit version, which
/// the test below still covers.
#[test]
fn a_legacy_receipt_without_an_envelope_is_refused() -> Result<()> {
    let legacy = format!(
        r#"{{"version":"{PUBLISHED}","subject_sha":"{PUBLISHED_SHA}","crate_count":34,"publish_run_id":"42"}}"#
    );
    let resolved = resolve("workflow_run", "success", Some(&legacy))?;

    if resolved.output.status.success() {
        bail!("an envelope-less receipt must fail the step:\n{}", resolved.combined());
    }
    if resolved.version.as_deref() == Some(PUBLISHED) {
        bail!("an envelope-less receipt must not be read for its version anyway");
    }
    Ok(())
}

/// The producer and the consumer agree on the envelope, proven by running the
/// producer and feeding the bytes it wrote to the consumer.
///
/// This is the cross-vend guard: renaming a field, changing the schema string,
/// or reordering the payload on either side alone fails here, in the PR that
/// does it, rather than during a release.
#[test]
fn a_produced_receipt_is_read_by_the_consumer() -> Result<()> {
    let receipt = produced_receipt(PUBLISHED, PUBLISHED_SHA)?;
    let resolved = resolve("workflow_run", "success", Some(&receipt))?;
    resolved.assert_step_succeeded("producer round trip")?;

    if !resolved.runs() {
        bail!(
            "the consumer refused the producer's own receipt {receipt:?}:\n{}",
            resolved.combined()
        );
    }
    if resolved.version.as_deref() != Some(PUBLISHED) {
        bail!(
            "expected {PUBLISHED} from the produced receipt {receipt:?}, got {:?}",
            resolved.version
        );
    }
    if resolved.subject.as_deref() != Some(PUBLISHED_SHA) {
        bail!(
            "expected subject {PUBLISHED_SHA} from the produced receipt {receipt:?}, got {:?}",
            resolved.subject
        );
    }
    Ok(())
}

/// The producer stamps the envelope, and does not reuse the overloaded
/// `version` key for the semver.
///
/// A receipt whose semver lives under `version` invites the next consumer to
/// read `version` as the envelope version — the overload that made the original
/// defect easy to miss.
#[test]
fn the_producer_stamps_the_envelope() -> Result<()> {
    let receipt = produced_receipt(PUBLISHED, PUBLISHED_SHA)?;
    let parsed: serde_json::Value =
        serde_json::from_str(&receipt).with_context(|| format!("parsing receipt {receipt:?}"))?;

    if parsed.get("schema_version").and_then(serde_json::Value::as_str) != Some(SCHEMA) {
        bail!("the producer must stamp schema_version={SCHEMA}, wrote {receipt:?}");
    }
    if parsed.get("release_version").and_then(serde_json::Value::as_str) != Some(PUBLISHED) {
        bail!("the producer must record the semver under release_version, wrote {receipt:?}");
    }
    if parsed.get("version").is_some() {
        bail!("the producer must not reuse the overloaded `version` key, wrote {receipt:?}");
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

/// A receipted run without a usable subject must not be certified at all.
///
/// Falling back to the default branch here would issue a publication verdict
/// using code that could have changed after the release — the same false-proof
/// class as the original finding, moved from "which version" to "which code".
/// No such receipt can exist in practice: the producer has written
/// `subject_sha` in the same `printf` as the version since the format was
/// introduced, so its absence means corruption, not age.
#[test]
fn a_receipted_run_without_a_subject_fails_closed() -> Result<()> {
    let no_subject = format!(
        r#"{{"schema_version":"{SCHEMA}","release_version":"{PUBLISHED}","crate_count":34}}"#
    );
    let resolved = resolve("workflow_run", "success", Some(&no_subject))?;

    if resolved.output.status.success() {
        bail!("a receipt with no subject must fail the step:\n{}", resolved.combined());
    }
    if resolved.runs() {
        bail!("a receipt with no subject must not be smoke-tested:\n{}", resolved.combined());
    }
    if resolved.subject.as_deref() == Some(DEFAULT_BRANCH) {
        bail!("the proof must not silently retarget the default branch");
    }
    Ok(())
}

/// A malformed subject is a corrupt instrument. It must neither be handed to
/// `actions/checkout` nor quietly replaced with a mutable branch.
#[test]
fn a_malformed_subject_fails_closed() -> Result<()> {
    for bogus in ["refs/heads/attacker", "0123456", "../../etc", "", "main"] {
        let receipt = format!(
            r#"{{"schema_version":"{SCHEMA}","release_version":"{PUBLISHED}","subject_sha":"{bogus}","crate_count":34}}"#
        );
        let resolved = resolve("workflow_run", "success", Some(&receipt))?;

        if resolved.output.status.success() {
            bail!("subject {bogus:?} must fail the step:\n{}", resolved.combined());
        }
        if resolved.runs() {
            bail!("subject {bogus:?} must not produce a verdict:\n{}", resolved.combined());
        }
        if resolved.subject.as_deref() == Some(bogus) {
            bail!("subject {bogus:?} must never reach the checkout");
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

    // A dispatch re-check is not publication proof for that release, and the
    // log has to say so — otherwise the two runs look identical in the UI.
    let spoken = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !spoken.contains("not publication proof") {
        bail!("a dispatch run must not present itself as publication proof:\n{spoken}");
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
