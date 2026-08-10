//! Policy guards for the UX regression gate's candidate-execution boundary (#6027).
//!
//! This workflow runs candidate-controlled code (`just ux-tests`, the extension's
//! npm scripts) against an exact PR head. Two properties keep that safe, and both
//! are the kind that read as correct while enforcing nothing, so they are pinned
//! here rather than left to review:
//!
//! 1. The tested-SHA binding must come from the immutable `github` context at
//!    step scope. A workflow- or job-level `TESTED_SHA` can be rebound by the
//!    candidate writing `TESTED_SHA=<any value>` to `$GITHUB_ENV`, after which the
//!    receipt emitter and the verifier agree on a candidate-chosen subject.
//! 2. Evidence publication must be gated on the subject check. Uploading on bare
//!    `always()` publishes a receipt the verifier rejected, under a name asserting
//!    the exact candidate SHA.

use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow};
use serde_yaml_ng::Value;

const IMMUTABLE_SUBJECT: &str = "github.event.pull_request.head.sha";

fn workflow() -> Result<(String, Value)> {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    let path = root.join(".github/workflows/ux-regression-gate.yml");
    let content = fs::read_to_string(&path)?;
    let parsed = serde_yaml_ng::from_str(&content)?;
    Ok((content, parsed))
}

fn jobs(workflow: &Value) -> Result<&serde_yaml_ng::Mapping> {
    workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("ux-regression-gate.yml must declare jobs"))
}

/// The candidate must not be able to rebind the tested subject through
/// `$GITHUB_ENV`. Every step that consumes `TESTED_SHA` declares it at step
/// scope from the `github` context; no wider scope defines it at all.
#[test]
fn tested_sha_cannot_be_rebound_by_candidate_code() -> Result<()> {
    let (_content, wf) = workflow()?;

    if wf.get("env").and_then(|env| env.get("TESTED_SHA")).is_some() {
        return Err(anyhow!(
            "workflow-level `TESTED_SHA` is rebindable through $GITHUB_ENV by \
             candidate code; declare it per step from the github context"
        ));
    }

    let mut checked = 0usize;
    for (job_name, job) in jobs(&wf)? {
        if job.get("env").and_then(|env| env.get("TESTED_SHA")).is_some() {
            return Err(anyhow!("job `{job_name:?}` declares a rebindable job-level `TESTED_SHA`"));
        }
        let Some(steps) = job.get("steps").and_then(Value::as_sequence) else {
            continue;
        };
        for step in steps {
            let run = step.get("run").and_then(Value::as_str).unwrap_or_default();
            let with = step.get("with").map(|w| format!("{w:?}")).unwrap_or_default();
            let step_env = step.get("env");

            if with.contains("env.TESTED_SHA") {
                return Err(anyhow!(
                    "job `{job_name:?}` reads `env.TESTED_SHA` in an action input; \
                     the env context reflects $GITHUB_ENV writes"
                ));
            }
            if !run.contains("TESTED_SHA") {
                continue;
            }

            let bound = step_env
                .and_then(|env| env.get("TESTED_SHA"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "job `{job_name:?}` has a step using TESTED_SHA with no \
                         step-level binding, so it inherits a rebindable value"
                    )
                })?;
            if !bound.contains(IMMUTABLE_SUBJECT) {
                return Err(anyhow!(
                    "job `{job_name:?}` binds TESTED_SHA to `{bound}`, which is not \
                     the immutable github-context subject"
                ));
            }
            checked += 1;
        }
    }

    assert!(checked >= 4, "expected the identity binding on at least 4 steps, found {checked}");
    Ok(())
}

/// A receipt whose subject the verifier rejected must not be published as
/// exact-candidate evidence.
#[test]
fn evidence_upload_is_gated_on_receipt_verification() -> Result<()> {
    let (_content, wf) = workflow()?;
    let gate = jobs(&wf)?
        .get(Value::String("ux-regression-gate".into()))
        .ok_or_else(|| anyhow!("ux-regression-gate job must exist"))?;
    let steps = gate
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("ux-regression-gate must declare steps"))?;

    let verifier_id = steps
        .iter()
        .find(|s| {
            s.get("run")
                .and_then(Value::as_str)
                .is_some_and(|run| run.contains("UX receipt subject"))
        })
        .and_then(|s| s.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("the receipt subject verifier must carry a step `id` to gate on"))?;

    let evidence = steps
        .iter()
        .find(|s| {
            s.get("with")
                .and_then(|w| w.get("name"))
                .and_then(Value::as_str)
                .is_some_and(|name| name.starts_with("ux-regression-gate-receipt-"))
        })
        .ok_or_else(|| anyhow!("the exact-candidate evidence upload must exist"))?;

    let condition = evidence
        .get("if")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("the evidence upload must be conditional"))?;

    assert!(
        condition.contains(&format!("steps.{verifier_id}.outcome == 'success'")),
        "the exact-candidate evidence upload must require the receipt verifier to \
         have succeeded; found `if: {condition}`"
    );

    let name = evidence
        .get("with")
        .and_then(|w| w.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        name.contains(IMMUTABLE_SUBJECT),
        "the evidence artifact name must be built from the immutable github-context \
         subject, not a rebindable env value; found `{name}`"
    );
    Ok(())
}
