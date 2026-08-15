//! Release-identity binding for the VSIX release asset (#9597).
//!
//! `publish-extension.yml` derives `release_tag` from the dispatch **version
//! input** while building the VSIX from the dispatch-selected **ref**. Nothing
//! in the workflow made those the same subject, so dispatching an older version
//! rebuilt current source and `--clobber`ed that version's historical asset.
//!
//! The guard added for that finding is the only thing standing between the
//! dispatch inputs and a `contents: write` upload, so these tests execute its
//! actual run block under Actions bash semantics rather than asserting on the
//! YAML text. A text assertion would still pass if the comparison inverted.

use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Output},
};

use anyhow::{Context, Result, anyhow, bail};
use serde_yaml_ng::Value;

const WORKFLOW: &str = "publish-extension.yml";
const JOB: &str = "github-release-asset";
const GUARD_STEP: &str = "Verify release tag resolves to the built subject";
const UPLOAD_STEP: &str = "Create GitHub Release asset";

/// The commit the VSIX was actually built from.
const BUILT_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
/// A different commit — what an older release tag points at.
const HISTORICAL_SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn workflow() -> Result<Value> {
    let path = project_root().join(".github/workflows").join(WORKFLOW);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml_ng::from_str(&content).with_context(|| format!("parsing {}", path.display()))
}

fn steps(workflow: &Value, job_name: &str) -> Result<Vec<Value>> {
    workflow
        .get("jobs")
        .and_then(|jobs| jobs.get(Value::String(job_name.into())))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .cloned()
        .ok_or_else(|| anyhow!("job `{job_name}` must declare steps"))
}

fn step_index(steps: &[Value], name: &str) -> Result<usize> {
    steps
        .iter()
        .position(|step| step.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| anyhow!("step `{name}` must exist in job `{JOB}`"))
}

fn guard_run_block() -> Result<String> {
    let workflow = workflow()?;
    let steps = steps(&workflow, JOB)?;
    let index = step_index(&steps, GUARD_STEP)?;
    steps[index]
        .get("run")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("step `{GUARD_STEP}` must have a run block"))
}

fn bash_executable() -> PathBuf {
    if let Some(path) = env::var_os("BASH") {
        return path.into();
    }
    #[cfg(windows)]
    {
        let git_bash = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
        if git_bash.is_file() {
            return git_bash;
        }
    }
    PathBuf::from("bash")
}

/// Execute the guard with a stubbed `gh` so the tag resolution is controlled.
///
/// `ref_json` is what `git/ref/tags/<tag>` returns (empty string => the API call
/// fails, i.e. the tag does not exist). `tag_json` is what dereferencing an
/// annotated tag object returns. `sleep` is neutered so the retry loop in the
/// not-yet-available path does not cost the suite five minutes.
fn run_guard(build_sha: &str, ref_json: &str, tag_json: &str) -> Result<Output> {
    let run = guard_run_block()?;
    let script = format!(
        r#"
sleep() {{ :; }}
gh() {{
  local url="$2"
  shift 2
  local filter=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --jq) filter="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  local body=""
  case "$url" in
    */git/ref/tags/*) body="$FAKE_REF_JSON" ;;
    */git/tags/*) body="$FAKE_TAG_JSON" ;;
  esac
  if [ -z "$body" ]; then
    return 1
  fi
  if [ -n "$filter" ]; then
    printf '%s' "$body" | jq -r "$filter"
  else
    printf '%s' "$body"
  fi
}}
{run}
"#
    );

    Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-c", &script])
        .env("BUILD_SHA", build_sha)
        .env("RELEASE_TAG", "v0.14.0")
        .env("GH_REPO", "EffortlessMetrics/perl-lsp-swarm")
        .env("GH_TOKEN", "stub-token")
        .env("FAKE_REF_JSON", ref_json)
        .env("FAKE_TAG_JSON", tag_json)
        .output()
        .context("executing the release-identity guard under Actions bash semantics")
}

fn lightweight_tag(sha: &str) -> String {
    format!(r#"{{"object":{{"sha":"{sha}","type":"commit"}}}}"#)
}

fn annotated_tag(tag_object_sha: &str) -> String {
    format!(r#"{{"object":{{"sha":"{tag_object_sha}","type":"tag"}}}}"#)
}

fn assert_exit(output: &Output, expected: i32, context: &str) -> Result<()> {
    if output.status.code() != Some(expected) {
        bail!(
            "{context}: expected exit {expected}, got {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The finding itself: a dispatch naming an older version must not reach the
/// upload. Without the guard this case silently replaced a shipped asset.
#[test]
fn mismatched_release_tag_is_refused() -> Result<()> {
    let output = run_guard(BUILT_SHA, &lightweight_tag(HISTORICAL_SHA), "")?;
    assert_exit(&output, 1, "historical tag with rebuilt source")?;

    let text = combined(&output);
    if !text.contains("Release identity mismatch") {
        bail!("expected an identity-mismatch diagnostic, got:\n{text}");
    }
    // The operator has to be told how to publish legitimately, or the guard
    // just reads as a broken workflow.
    if !text.contains("release tag") {
        bail!("mismatch diagnostic must state the supported dispatch, got:\n{text}");
    }
    Ok(())
}

/// The legitimate path: dispatching from the release tag itself.
#[test]
fn tag_pointing_at_the_built_commit_is_accepted() -> Result<()> {
    let output = run_guard(BUILT_SHA, &lightweight_tag(BUILT_SHA), "")?;
    assert_exit(&output, 0, "tag matching the built subject")?;
    Ok(())
}

/// Release tags are commonly annotated, which resolves to a tag object rather
/// than a commit. Failing to dereference would reject every legitimate publish.
#[test]
fn annotated_tag_is_dereferenced_to_its_commit() -> Result<()> {
    let tag_object = "ffffffffffffffffffffffffffffffffffffffff";
    let accepted = run_guard(BUILT_SHA, &annotated_tag(tag_object), &lightweight_tag(BUILT_SHA))?;
    assert_exit(&accepted, 0, "annotated tag matching the built subject")?;

    // Dereferencing must not become a way to pass: an annotated tag over a
    // different commit is still a mismatch.
    let refused =
        run_guard(BUILT_SHA, &annotated_tag(tag_object), &lightweight_tag(HISTORICAL_SHA))?;
    assert_exit(&refused, 1, "annotated tag over a historical commit")?;
    Ok(())
}

/// An absent tag must fail closed rather than fall through to the upload.
#[test]
fn unresolvable_tag_fails_closed() -> Result<()> {
    let output = run_guard(BUILT_SHA, "", "")?;
    assert_exit(&output, 1, "tag that never appears")?;
    Ok(())
}

/// A tag whose object cannot be reduced to a commit SHA must be reported as
/// unresolvable. The mismatch branch would also refuse it, but only after
/// telling the operator the tag "points at ", which reads as a workflow bug
/// rather than a missing tag object.
#[test]
fn empty_tag_object_is_reported_as_unresolvable() -> Result<()> {
    let output = run_guard(BUILT_SHA, r#"{"object":{}}"#, "")?;
    assert_exit(&output, 1, "tag object without a sha")?;

    let text = combined(&output);
    if text.contains("share subject") {
        bail!("an unresolvable tag object must not be reported as a match:\n{text}");
    }
    if !text.contains("Could not resolve") {
        bail!("an unresolvable tag object must say so, not report an empty mismatch:\n{text}");
    }
    Ok(())
}

/// If the built subject is not a real commit SHA the guard has nothing to bind
/// against, so it must refuse rather than compare two placeholder values.
#[test]
fn missing_build_subject_fails_closed() -> Result<()> {
    let output = run_guard("", &lightweight_tag(BUILT_SHA), "")?;
    assert_exit(&output, 1, "absent build subject")?;
    Ok(())
}

/// The guard is only meaningful if it runs before the credentialed upload.
#[test]
fn guard_precedes_the_release_upload() -> Result<()> {
    let workflow = workflow()?;
    let steps = steps(&workflow, JOB)?;
    let guard = step_index(&steps, GUARD_STEP)?;
    let upload = step_index(&steps, UPLOAD_STEP)?;

    if guard >= upload {
        bail!("`{GUARD_STEP}` must run before `{UPLOAD_STEP}`");
    }

    // The upload is the step that spends `contents: write`; if it ever grows an
    // `if:` that can skip the guard, or the guard gains one, re-verify by hand.
    if steps[guard].get("if").is_some() {
        bail!("`{GUARD_STEP}` must not be conditional");
    }
    Ok(())
}
