//! Draft pull-request feedback contract for issue #10006.
//!
//! Two layers keep the contract load-bearing:
//!
//! - compile-time byte assertions, restricted to strings inside YAML literal
//!   block scalars (`run: |`), which YAML reformatting cannot re-wrap or
//!   reorder, still fire under `cargo check --workspace --all-targets`;
//! - the runtime test parses `ci.yml` with `serde_yaml_ng` and asserts on the
//!   parsed job objects with whitespace-normalized expression matching, so
//!   folded-scalar re-flow, flow-sequence reordering, and indentation churn
//!   cannot fail the proof without a semantic change.

use std::fs;
use std::path::PathBuf;

use serde_yaml_ng::Value;

const CI_WORKFLOW: &[u8] = include_bytes!("../../.github/workflows/ci.yml");
// Both anchors live inside `run: |` literal block scalars, whose content is
// preserved verbatim by any YAML formatting pass; byte matching is stable there.
const DRAFT_SELECTOR: &[u8] = b"echo \"run_ci=false\" >> \"$GITHUB_OUTPUT\"";
const SKIPPED_IS_NOT_PROOF: &[u8] = b"A skipped job is not verification";

const fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }

    let mut start = 0;
    while start + needle.len() <= haystack.len() {
        let mut offset = 0;
        while offset < needle.len() && haystack[start + offset] == needle[offset] {
            offset += 1;
        }
        if offset == needle.len() {
            return true;
        }
        start += 1;
    }
    false
}

const _: () = assert!(contains(CI_WORKFLOW, DRAFT_SELECTOR));
const _: () = assert!(contains(CI_WORKFLOW, SKIPPED_IS_NOT_PROOF));

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

/// Collapse every whitespace run to one space so expression assertions match
/// the logical tokens regardless of YAML folded-scalar re-flow.
fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn mapping_value<'a>(
    value: &'a Value,
    key: &str,
    what: &str,
) -> Result<&'a Value, Box<dyn std::error::Error>> {
    value.get(key).ok_or_else(|| format!("ci.yml {what} is missing the `{key}` key").into())
}

fn scalar_str<'a>(value: &'a Value, what: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    value.as_str().ok_or_else(|| format!("ci.yml {what} must be a YAML string scalar").into())
}

fn job<'a>(workflow: &'a Value, name: &str) -> Result<&'a Value, Box<dyn std::error::Error>> {
    workflow
        .get("jobs")
        .ok_or("ci.yml is missing the `jobs` mapping")?
        .get(name)
        .ok_or_else(|| format!("ci.yml has no `{name}` job").into())
}

fn steps_of<'a>(job: &'a Value, name: &str) -> Result<&'a Vec<Value>, Box<dyn std::error::Error>> {
    mapping_value(job, "steps", &format!("`jobs.{name}`"))?
        .as_sequence()
        .ok_or_else(|| format!("`jobs.{name}.steps` must be a YAML sequence").into())
}

fn checkout_ref_of(job: &Value, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let steps = steps_of(job, name)?;
    let checkout = steps
        .iter()
        .find(|step| {
            step.get("uses")
                .and_then(Value::as_str)
                .is_some_and(|uses| uses.starts_with("actions/checkout@"))
        })
        .ok_or_else(|| format!("`jobs.{name}` has no `actions/checkout@` step"))?;
    let reference = mapping_value(
        mapping_value(checkout, "with", &format!("`jobs.{name}` checkout step"))?,
        "ref",
        &format!("`jobs.{name}` checkout step `with`"),
    )?;
    Ok(normalize_whitespace(scalar_str(reference, &format!("`jobs.{name}` checkout ref"))?))
}

/// Resolve the `on:` trigger mapping, accepting the boolean-key resolution a
/// YAML 1.1 parser could produce for the unquoted `on` key.
fn triggers_of(workflow: &Value) -> Result<&Value, Box<dyn std::error::Error>> {
    let mapping = workflow.as_mapping().ok_or("ci.yml top level must be a mapping")?;
    mapping
        .iter()
        .find(|(key, _)| key.as_str() == Some("on") || key.as_bool() == Some(true))
        .map(|(_, value)| value)
        .ok_or_else(|| "ci.yml has no `on:` trigger key".into())
}

/// The pull_request trigger set is asserted structurally, so re-ordering or
/// re-wrapping the flow sequence cannot fail the proof; only adding or
/// removing a trigger — a semantic change to what re-runs the draft guard —
/// must confront this contract.
fn assert_draft_event_triggers(workflow: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let pull_request = mapping_value(triggers_of(workflow)?, "pull_request", "`on`")?;
    let mut types: Vec<String> = mapping_value(pull_request, "types", "`on.pull_request`")?
        .as_sequence()
        .ok_or("`on.pull_request.types` must be a YAML sequence")?
        .iter()
        .map(|value| scalar_str(value, "`on.pull_request.types` entry").map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()?;
    types.sort();

    let expected = ["converted_to_draft", "opened", "ready_for_review", "reopened", "synchronize"];
    assert!(
        types == expected,
        "`on.pull_request.types` must keep exactly the draft-guard trigger set \
         {expected:?}: `ready_for_review` and `converted_to_draft` change the \
         draft-pr-check guard behavior and must keep triggering CI. Found {types:?}"
    );
    Ok(())
}

fn assert_draft_guard_defers_full_ci(workflow: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let guard_job = job(workflow, "draft-pr-check")?;

    // Every deferred job below keys its `if:` on this output, so the plumbing
    // from the guard step must stay intact.
    let run_ci_output = mapping_value(
        mapping_value(guard_job, "outputs", "`jobs.draft-pr-check`")?,
        "run_ci",
        "`jobs.draft-pr-check.outputs`",
    )?;
    assert_eq!(
        scalar_str(run_ci_output, "`jobs.draft-pr-check.outputs.run_ci`")?,
        "${{ steps.guard.outputs.run_ci }}",
        "`jobs.draft-pr-check.outputs.run_ci` must stay wired to the guard step"
    );

    let guard_step = steps_of(guard_job, "draft-pr-check")?
        .iter()
        .find(|step| step.get("id").and_then(Value::as_str) == Some("guard"))
        .ok_or_else(|| "`jobs.draft-pr-check` has no step with id `guard`")?;
    let guard_run = scalar_str(
        mapping_value(guard_step, "run", "`jobs.draft-pr-check` guard step")?,
        "`jobs.draft-pr-check` guard step `run`",
    )?;
    assert!(
        guard_run.contains("echo \"run_ci=false\" >> \"$GITHUB_OUTPUT\"")
            && guard_run.contains("Rust formatting")
            && guard_run.contains("Conflict marker check")
            && guard_run.contains("A skipped job is not verification"),
        "the draft guard must defer the expensive suite while identifying the real \
         exact-head checks and stating that skipped jobs are not proof. Guard run:\n{guard_run}"
    );
    Ok(())
}

/// Both `if:` branches are asserted on the whitespace-normalized expression:
/// GitHub reports a skipped need's outputs as empty strings, so on a draft the
/// needs branch evaluates `'false' == 'true' && '' == 'true'` and only the
/// dedicated draft branch can fire; merge_group and push entries have no draft
/// flag and rely entirely on the needs branch.
fn assert_conflict_markers_run_on_every_route(
    workflow: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let conflict_job = job(workflow, "conflict-markers")?;
    let route = normalize_whitespace(scalar_str(
        mapping_value(conflict_job, "if", "`jobs.conflict-markers`")?,
        "`jobs.conflict-markers.if`",
    )?);

    assert!(
        route.contains("always() &&")
            && route.contains(
                "(github.event_name == 'pull_request' && \
                 github.event.pull_request.draft == true)"
            ),
        "the conflict-marker job must keep its dedicated draft route: drafts skip \
         `preflight-latest-check`, so only `always()` plus the draft branch run this \
         exact-head check. Normalized `if`:\n{route}"
    );
    assert!(
        route.contains("||")
            && route.contains(
                "needs.draft-pr-check.outputs.run_ci == 'true' && \
                 needs.preflight-latest-check.outputs.is_latest == 'true'"
            ),
        "the conflict-marker job must keep the needs-output disjunction branch: it is \
         the only route for non-draft pull requests, merge_group entries, and pushes, \
         and the empty-string outputs of a skipped need can never satisfy it on a \
         draft. Normalized `if`:\n{route}"
    );

    let checkout_ref = checkout_ref_of(conflict_job, "conflict-markers")?;
    assert!(
        checkout_ref.contains("github.event.pull_request.head.sha"),
        "the conflict-marker checkout must pin the exact pull-request head SHA so \
         draft feedback describes this candidate. Normalized ref:\n{checkout_ref}"
    );
    Ok(())
}

/// Independence is asserted on the job's own `needs:`/`if:` fields, so a
/// comment that merely mentions the draft guard can never fail the contract
/// while real coupling to the draft/full-CI selector always does.
fn assert_rust_formatting_stays_independent(
    workflow: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let rust_formatting = job(workflow, "rust-formatting")?;
    assert!(
        rust_formatting.get("needs").is_none(),
        "`jobs.rust-formatting` must not gain a `needs:` edge: it has no upstream \
         dependency, which is exactly why drafts still receive exact-head rustfmt \
         feedback while the expensive merge suite stays deferred"
    );
    assert!(
        rust_formatting.get("if").is_none(),
        "`jobs.rust-formatting` must stay ungated: any `if:` consulting the \
         draft/full-CI selector would silently stop draft feedback"
    );

    let subject_sha = scalar_str(
        mapping_value(
            mapping_value(rust_formatting, "env", "`jobs.rust-formatting`")?,
            "SUBJECT_SHA",
            "`jobs.rust-formatting.env`",
        )?,
        "`jobs.rust-formatting.env.SUBJECT_SHA`",
    )?;
    assert!(
        normalize_whitespace(subject_sha).contains("github.event.pull_request.head.sha"),
        "the formatter subject must stay bound to the exact pull-request head SHA so \
         draft rustfmt feedback describes this candidate. Normalized value:\n{subject_sha}"
    );
    Ok(())
}

#[test]
fn drafts_keep_exact_head_cheap_feedback() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))?;
    let workflow: Value = serde_yaml_ng::from_str(&ci)
        .map_err(|error| format!("ci.yml is not structurally valid YAML: {error}"))?;

    assert_draft_event_triggers(&workflow)?;
    assert_draft_guard_defers_full_ci(&workflow)?;
    assert_conflict_markers_run_on_every_route(&workflow)?;
    assert_rust_formatting_stays_independent(&workflow)?;

    Ok(())
}
