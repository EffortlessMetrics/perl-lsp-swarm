//! Structural policy pins for the Post-Merge Corpus Ratchet cache cycle (#12823).
//!
//! The scheduled corpus lane was red every day since inception because the
//! checkpoint save required one complete cold install pass while an external
//! runner SIGTERM landed at ~24m29s of job wall-clock. These pins forbid the
//! self-sustaining cycle from returning: the warm leg must end below the
//! preemption envelope under an explicit budget, persist partial progress,
//! report completion truthfully, and the gate chain must stay byte-identical
//! and never enforce against partial state. Mutation controls are named per
//! `.spec/12823-corpus-cache-cycle/acceptance.md`.

use std::{collections::BTreeSet, fs, path::PathBuf};

use anyhow::{Result, anyhow, ensure};
use serde_yaml_ng::Value;

const WORKFLOW_FILE: &str = "post-merge-corpus-ratchet.yml";

/// Observed external preemption point (2026-08-10/25/26 runs, issue #12823):
/// SIGTERM after ~24m29s of job wall-clock, historically during Batch 9-10 of
/// the cold install. The budgeted install must end well below this envelope
/// so setup plus cache-save overhead still fits inside one safe pass.
const PREEMPTION_ENVELOPE_MINUTES: u64 = 24;
const INSTALL_BUDGET_MINUTES: u64 = 12;
const WARM_JOB_CEILING_MINUTES: u64 = 30;

const WARM_JOB: &str = "corpus-warm-full";
const RATCHET_JOB: &str = "corpus-ratchet-full";
const BOUNDED_JOB: &str = "corpus-ratchet-bounded";
const PR_WRITER_JOB: &str = "open-ratchet-pr";
const GOVERNED_JOBS: [&str; 4] = [BOUNDED_JOB, WARM_JOB, RATCHET_JOB, PR_WRITER_JOB];
const INSTALL_STEP: &str = "Install CPAN corpus checkpoint";
const CANONICAL_SAVE_STEP: &str = "Save CPAN corpus cache (canonical)";
const CHECKPOINT_SAVE_STEP: &str = "Save CPAN corpus checkpoint (partial progress)";

const FULL_JOB_GUARDS: &[(&str, &str)] = &[
    (
        WARM_JOB,
        r#"false &&
(github.event_name == 'schedule' ||
 (github.event_name == 'workflow_dispatch' &&
  github.event.inputs.mode == 'full' &&
  github.ref_name == github.event.repository.default_branch))"#,
    ),
    (
        RATCHET_JOB,
        r#"false &&
((github.event_name == 'schedule' ||
  (github.event_name == 'workflow_dispatch' &&
   github.event.inputs.mode == 'full' &&
   github.ref_name == github.event.repository.default_branch)) &&
 needs.corpus-warm-full.outputs.complete == 'true')"#,
    ),
    (
        PR_WRITER_JOB,
        r#"false &&
((github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') &&
 needs.corpus-ratchet-full.outputs.changed == 'true')"#,
    ),
];

const BOUNDED_ACTION_STEPS: &[(&str, &str, &str)] = &[
    (
        "Checkout bounded analysis tree",
        "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        r#"fetch-depth: 1
persist-credentials: false"#,
    ),
    (
        "Install Rust toolchain",
        "dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772",
        "toolchain: 1.95.0",
    ),
    (
        "Cache cargo dependencies",
        "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
        r#"cache-on-failure: true
shared-key: post-merge-corpus-ratchet-${{ hashFiles('Cargo.lock') }}"#,
    ),
    (
        "Install just",
        "taiki-e/install-action@1ed6d7be6168f6c9046541087ff549b6bc581fdf",
        "tool: just",
    ),
    (
        "Restore CPAN corpus cache (bounded)",
        "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
        r#"path: target/cpan-corpus-bounded
key: cpan-corpus-bounded-${{ runner.os }}-${{ hashFiles('.ci/cpan-top-50-distributions.txt') }}
restore-keys: |
  cpan-corpus-bounded-${{ runner.os }}-
"#,
    ),
    (
        "Save CPAN corpus cache (bounded)",
        "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
        r#"path: target/cpan-corpus-bounded
key: cpan-corpus-bounded-${{ runner.os }}-${{ hashFiles('.ci/cpan-top-50-distributions.txt') }}"#,
    ),
    (
        "Upload bounded corpus receipt",
        "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
        r#"name: cpan-corpus-bounded-receipt-${{ github.sha }}
path: target/corpus-receipts/bounded-sweep.json
retention-days: 14
if-no-files-found: warn"#,
    ),
];

const BOUNDED_RUN_STEPS: &[(&str, &str)] = &[
    (
        "Install CPAN corpus (bounded — top 50)",
        r#"cargo xtask cpan-corpus install \
  --dist-list .ci/cpan-top-50-distributions.txt \
  --install-dir target/cpan-corpus-bounded"#,
    ),
    (
        "Verify bounded corpus path",
        r#"test -d "$GITHUB_WORKSPACE/target/cpan-corpus-bounded/lib/perl5""#,
    ),
    (
        "Sweep bounded corpus and emit receipt",
        r#"set -euo pipefail
mkdir -p target/corpus-receipts
cargo xtask cpan-corpus sweep \
  --install-dir target/cpan-corpus-bounded \
  --output target/corpus-receipts/bounded-sweep.json
python3 - <<'PY'
import json
import os
from pathlib import Path

report = json.loads(
    Path('target/corpus-receipts/bounded-sweep.json').read_text(encoding='utf-8')
)
total = report.get('total_files', 0)
clean = report.get('clean_files', 0)
pct = (clean / total * 100) if total > 0 else 0
summary = Path(os.environ['GITHUB_STEP_SUMMARY'])
with summary.open('a', encoding='utf-8') as handle:
    handle.write('### CPAN Corpus (Bounded — Top 50)\n')
    handle.write('| Metric | Value |\n| --- | ---: |\n')
    handle.write(f'| Total .pm files | {total} |\n')
    handle.write(f'| Parse-clean | {clean} ({pct:.1f}%) |\n')
    handle.write(f'| Parse failures | {total - clean} |\n')
PY"#,
    ),
];

/// Per-job hard ceilings inherited downward-only from the base pin
/// `origin/main@d2f6f9bde`. Raising any of these would mask work behind a
/// longer rope instead of making each pass fit the runner's real preemption
/// behavior (CRW-006 mutation control). The gate leg keeps the legacy
/// full-lane 120-minute ceiling: the measured chronic-red leg was the cold
/// *install* (~24m29s external kill), not the sweep/ratchet/enforce chain, and
/// shrinking the gate rope below any duration receipt would manufacture a new
/// false-red instead of fixing one. Any reduction needs its own receipts.
const BASE_PIN_TIMEOUT_MAXIMA: &[(&str, i64)] =
    &[(BOUNDED_JOB, 30), (WARM_JOB, 30), (RATCHET_JOB, 120), (PR_WRITER_JOB, 5)];

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn workflow_raw() -> Result<String> {
    let path = project_root().join(".github/workflows").join(WORKFLOW_FILE);
    Ok(fs::read_to_string(&path)?)
}

fn workflow() -> Result<Value> {
    Ok(serde_yaml_ng::from_str(&workflow_raw()?)?)
}

fn job<'a>(workflow: &'a Value, name: &str) -> Result<&'a Value> {
    workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .and_then(|jobs| jobs.get(Value::String(name.into())))
        .ok_or_else(|| anyhow!("workflow must declare job `{name}`"))
}

fn steps_of<'a>(job: &'a Value, name: &str) -> Result<&'a Vec<Value>> {
    job.get("steps")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("job `{name}` must declare steps"))
}

fn named_step<'a>(steps: &'a [Value], name: &str) -> Result<&'a Value> {
    steps
        .iter()
        .find(|step| step.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| anyhow!("step `{name}` must exist"))
}

fn condition<'a>(step: &'a Value, step_name: &str) -> Result<Option<&'a str>> {
    let cond = match step.get("if") {
        Some(value) => value.as_str().ok_or_else(|| {
            anyhow!("step `{step_name}` condition must be a plain scalar for pinning")
        })?,
        None => return Ok(None),
    };
    Ok(Some(cond))
}

fn run_block<'a>(step: &'a Value, step_name: &str) -> Result<&'a str> {
    step.get("run").and_then(Value::as_str).ok_or_else(|| {
        anyhow!("step `{step_name}` must embed its execution inline so the policy can pin it")
    })
}

fn step_id<'a>(step: &'a Value, steps: &'a [Value]) -> Result<&'a str> {
    step.get("id").and_then(Value::as_str).ok_or_else(|| {
        anyhow!(
            "step `{}` must declare a stable id (found among {:?})",
            step.get("name").and_then(Value::as_str).unwrap_or("?"),
            steps.iter().filter_map(|s| s.get("name").and_then(Value::as_str)).collect::<Vec<_>>()
        )
    })
}

fn has_write_permissions(job: &Value) -> bool {
    match job.get("permissions") {
        None => false,
        Some(Value::String(permission)) => permission == "write-all",
        Some(Value::Mapping(permissions)) => {
            permissions.values().any(|permission| permission.as_str() == Some("write"))
        }
        Some(_) => true,
    }
}

fn ensure_full_chain_is_fail_closed(workflow: &Value) -> Result<()> {
    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("workflow must declare jobs"))?;

    let actual = jobs
        .keys()
        .map(|name| name.as_str().ok_or_else(|| anyhow!("workflow job names must be strings")))
        .collect::<Result<BTreeSet<_>>>()?;
    let governed = GOVERNED_JOBS.into_iter().collect::<BTreeSet<_>>();
    ensure!(
        actual == governed,
        "containment governs exactly {governed:?}; ungoverned or missing jobs: actual={actual:?}"
    );

    for (name, expected_guard) in FULL_JOB_GUARDS {
        let full_job = job(workflow, name)?;
        let guard = condition(full_job, name)?.ok_or_else(|| {
            anyhow!("contained full-corpus job `{name}` must carry an `if:` guard")
        })?;
        ensure!(
            guard.trim_end() == *expected_guard,
            "unsafe v1 full-corpus job `{name}` must remain fail-closed until #13004 adds identity, manifest, quiescence, atomicity, retention, and hosted proof; guard was: {guard}"
        );
    }

    Ok(())
}

fn ensure_bounded_top_50_is_safe_and_reachable(workflow: &Value) -> Result<()> {
    let bounded = job(workflow, BOUNDED_JOB)?;
    let guard = condition(bounded, BOUNDED_JOB)?
        .ok_or_else(|| anyhow!("bounded corpus job must carry an `if:` guard"))?;
    ensure!(
        !guard.trim_start().starts_with("false &&")
            && guard.contains("github.event_name == 'pull_request'"),
        "containment must preserve the bounded top-50 PR proof lane; guard was: {guard}"
    );

    ensure!(!has_write_permissions(bounded), "bounded proof must remain read-only");
    ensure!(bounded.get("uses").is_none(), "bounded proof must remain an inline job");
    let steps = steps_of(bounded, BOUNDED_JOB)?;
    ensure!(
        !steps.iter().any(|step| {
            step.get("uses").and_then(Value::as_str).is_some_and(|action| action.starts_with("./"))
        }),
        "bounded proof must not delegate to a repository-local action"
    );
    ensure!(
        !steps.iter().filter_map(|step| step.get("run").and_then(Value::as_str)).any(|run| run
            .lines()
            .any(|line| line.split_whitespace().any(|token| token == "just"))),
        "bounded proof must not delegate to a repository `just` alias"
    );
    let actual_step_names = steps
        .iter()
        .map(|step| {
            step.get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("bounded steps must carry explicit names"))
        })
        .collect::<Result<Vec<_>>>()?;
    let expected_step_names = [
        "Checkout bounded analysis tree",
        "Install Rust toolchain",
        "Cache cargo dependencies",
        "Install just",
        "Restore CPAN corpus cache (bounded)",
        "Install CPAN corpus (bounded — top 50)",
        "Save CPAN corpus cache (bounded)",
        "Verify bounded corpus path",
        "Sweep bounded corpus and emit receipt",
        "Upload bounded corpus receipt",
    ];
    ensure!(
        actual_step_names == expected_step_names,
        "bounded proof step inventory drifted: {actual_step_names:?}"
    );

    for (name, expected_action, expected_inputs) in BOUNDED_ACTION_STEPS {
        let step = named_step(steps, name)?;
        ensure!(
            step.get("run").is_none(),
            "bounded action step `{name}` must not add inline execution"
        );
        let actual = step.get("uses").and_then(Value::as_str);
        ensure!(
            actual == Some(*expected_action),
            "bounded action step `{name}` execution identity drifted: expected `{expected_action}`, found {actual:?}"
        );
        let expected_inputs = serde_yaml_ng::from_str::<Value>(expected_inputs)?;
        let actual_inputs = step.get("with");
        ensure!(
            actual_inputs == Some(&expected_inputs),
            "bounded action step `{name}` inputs drifted: expected {expected_inputs:?}, found {actual_inputs:?}"
        );
    }
    for (name, expected) in BOUNDED_RUN_STEPS {
        let step = named_step(steps, name)?;
        ensure!(
            step.get("uses").is_none(),
            "bounded inline step `{name}` must not delegate to an action"
        );
        let actual = run_block(step, name)?;
        ensure!(actual.trim_end() == *expected, "bounded inline step `{name}` execution drifted");
    }

    let rendered = serde_yaml_ng::to_string(bounded)?;
    ensure!(
        rendered.contains(".ci/cpan-top-50-distributions.txt")
            && rendered.contains("target/cpan-corpus-bounded"),
        "bounded job must keep both its top-50 list and isolated install path"
    );
    ensure!(
        !rendered.contains(".ci/cpan-top-1000-distributions.txt")
            && !rendered.contains("cpan-corpus-full-receipt")
            && !rendered.lines().any(|line| {
                let value =
                    line.trim().trim_matches(|character| character == '\'' || character == '"');
                value == "path: target/cpan-corpus" || value.starts_with("key: cpan-corpus-${{")
            }),
        "bounded job must not consume a full-corpus list, install path, cache key, or receipt"
    );
    ensure!(
        !rendered.contains("create-pull-request@"),
        "bounded proof must not acquire repository-writer behavior"
    );
    Ok(())
}

fn workflow_with_extra_job(name: &str, job_yaml: &str) -> Result<Value> {
    let mut candidate = workflow()?;
    let jobs = candidate
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("workflow must declare jobs"))?;
    jobs.insert(Value::String(name.into()), serde_yaml_ng::from_str(job_yaml)?);
    Ok(candidate)
}

// ---------------------------------------------------------------------------
// CRW-001 / CRW-007 / #13004: trusted-event anchoring and containment
// ---------------------------------------------------------------------------

#[test]
fn unsafe_full_checkpoint_chain_is_explicitly_disabled() -> Result<()> {
    let workflow = workflow()?;
    ensure_full_chain_is_fail_closed(&workflow)?;
    ensure_bounded_top_50_is_safe_and_reachable(&workflow)?;

    // The whole workflow keeps its narrow pull_request path triggers: only
    // distribution-list edits may spawn PR runs; no new PR surface may appear.
    let trigger_paths =
        ["'.ci/cpan-top-1000-distributions.txt'", "'.ci/cpan-top-50-distributions.txt'"];
    let raw = workflow_raw()?;
    let after_pull_request = raw
        .split("pull_request:")
        .nth(1)
        .ok_or_else(|| anyhow!("workflow must keep a pull_request trigger section"))?;
    let schedule_anchor = after_pull_request
        .find("schedule:")
        .ok_or_else(|| anyhow!("workflow must keep its schedule trigger"))?;
    let pull_section = &after_pull_request[..schedule_anchor];
    for expected in trigger_paths {
        assert!(
            pull_section.contains(expected),
            "pull_request path filter drifted: missing {expected}"
        );
    }
    Ok(())
}

#[test]
fn containment_rejects_unlisted_repository_alias_without_cpan_name() -> Result<()> {
    let candidate = workflow_with_extra_job(
        "unlisted-bank-refresh",
        r#"
if: github.event_name == 'schedule'
runs-on: ubuntu-24.04
steps:
  - name: Re-enable the full bank through an opaque repository alias
    run: just refresh-full-bank
"#,
    )?;

    let error = ensure_full_chain_is_fail_closed(&candidate)
        .err()
        .ok_or_else(|| anyhow!("an unlisted repository alias job must fail containment"))?;
    let message = error.to_string();
    ensure!(message.contains("unlisted-bank-refresh"), "unexpected refusal: {message}");
    ensure!(message.contains("actual="), "unexpected refusal: {message}");
    Ok(())
}

#[test]
fn containment_rejects_false_prefix_with_enabling_or_suffix() -> Result<()> {
    let mut candidate = workflow()?;
    let warm = candidate
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .and_then(|jobs| jobs.get_mut(Value::String(WARM_JOB.into())))
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("warm job must be mutable for the negative control"))?;
    warm.insert(
        Value::String("if".into()),
        Value::String("false && true || github.event_name == 'schedule'".into()),
    );

    let error = ensure_full_chain_is_fail_closed(&candidate)
        .err()
        .ok_or_else(|| anyhow!("an enabling suffix after a false prefix must fail containment"))?;
    ensure!(error.to_string().contains("guard was"), "unexpected refusal: {error}");
    Ok(())
}

#[test]
fn bounded_control_rejects_opaque_repository_alias_with_known_step_name() -> Result<()> {
    let mut candidate = workflow()?;
    let bounded = candidate
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .and_then(|jobs| jobs.get_mut(Value::String(BOUNDED_JOB.into())))
        .ok_or_else(|| anyhow!("bounded job must exist for the negative control"))?;
    let verify = bounded
        .get_mut("steps")
        .and_then(Value::as_sequence_mut)
        .and_then(|steps| {
            steps.iter_mut().find(|step| {
                step.get("name").and_then(Value::as_str) == Some("Verify bounded corpus path")
            })
        })
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("bounded verify step must exist"))?;
    verify.insert(Value::String("run".into()), Value::String("just refresh-full-bank".into()));

    let error = ensure_bounded_top_50_is_safe_and_reachable(&candidate)
        .err()
        .ok_or_else(|| anyhow!("opaque repository alias must fail containment"))?;
    ensure!(error.to_string().contains("`just` alias"), "unexpected refusal: {error}");
    Ok(())
}

#[test]
fn bounded_control_rejects_opaque_local_script_with_known_step_name() -> Result<()> {
    let mut candidate = workflow()?;
    let bounded = candidate
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .and_then(|jobs| jobs.get_mut(Value::String(BOUNDED_JOB.into())))
        .ok_or_else(|| anyhow!("bounded job must exist for the negative control"))?;
    let verify = bounded
        .get_mut("steps")
        .and_then(Value::as_sequence_mut)
        .and_then(|steps| {
            steps.iter_mut().find(|step| {
                step.get("name").and_then(Value::as_str) == Some("Verify bounded corpus path")
            })
        })
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("bounded verify step must exist"))?;
    verify
        .insert(Value::String("run".into()), Value::String("bash .ci/refresh-full-bank.sh".into()));

    let error = ensure_bounded_top_50_is_safe_and_reachable(&candidate)
        .err()
        .ok_or_else(|| anyhow!("opaque local script replacement must fail containment"))?;
    ensure!(error.to_string().contains("execution drifted"), "unexpected refusal: {error}");
    Ok(())
}

#[test]
fn bounded_control_rejects_different_pinned_external_action_with_known_name() -> Result<()> {
    let mut candidate = workflow()?;
    let bounded = candidate
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .and_then(|jobs| jobs.get_mut(Value::String(BOUNDED_JOB.into())))
        .ok_or_else(|| anyhow!("bounded job must exist for the negative control"))?;
    let checkout = bounded
        .get_mut("steps")
        .and_then(Value::as_sequence_mut)
        .and_then(|steps| steps.first_mut())
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("bounded checkout step must exist"))?;
    checkout.insert(
        Value::String("uses".into()),
        Value::String("actions/setup-python@8b4ff3e28c94af2b257e49916b3d0c03a1741b39".into()),
    );

    let error = ensure_bounded_top_50_is_safe_and_reachable(&candidate)
        .err()
        .ok_or_else(|| anyhow!("different pinned external action must fail containment"))?;
    ensure!(
        error.to_string().contains("execution identity drifted"),
        "unexpected refusal: {error}"
    );
    Ok(())
}

#[test]
fn bounded_control_rejects_full_bank_cache_restore_inputs() -> Result<()> {
    let mut candidate = workflow()?;
    let bounded = candidate
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .and_then(|jobs| jobs.get_mut(Value::String(BOUNDED_JOB.into())))
        .ok_or_else(|| anyhow!("bounded job must exist for the negative control"))?;
    let restore = bounded
        .get_mut("steps")
        .and_then(Value::as_sequence_mut)
        .and_then(|steps| {
            steps.iter_mut().find(|step| {
                step.get("name").and_then(Value::as_str)
                    == Some("Restore CPAN corpus cache (bounded)")
            })
        })
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("bounded restore step must exist"))?;
    restore.insert(
        Value::String("with".into()),
        serde_yaml_ng::from_str(
            r#"path: ./target/cpan-corpus
key: bounded-miss-${{ github.run_id }}
restore-keys: |
  cpan-corpus-${{ runner.os }}-"#,
        )?,
    );

    let error = ensure_bounded_top_50_is_safe_and_reachable(&candidate)
        .err()
        .ok_or_else(|| anyhow!("full-bank restore inputs must fail bounded containment"))?;
    ensure!(error.to_string().contains("inputs drifted"), "unexpected refusal: {error}");
    Ok(())
}

#[test]
fn bounded_control_rejects_scalar_write_all() -> Result<()> {
    let mut candidate = workflow()?;
    let bounded = candidate
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .and_then(|jobs| jobs.get_mut(Value::String(BOUNDED_JOB.into())))
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("bounded job must be mutable for the negative control"))?;
    bounded.insert(Value::String("permissions".into()), Value::String("write-all".into()));

    let error = ensure_bounded_top_50_is_safe_and_reachable(&candidate)
        .err()
        .ok_or_else(|| anyhow!("scalar `permissions: write-all` must fail containment"))?;
    ensure!(error.to_string().contains("read-only"), "unexpected refusal: {error}");
    Ok(())
}

#[test]
fn bounded_control_rejects_step_local_action_indirection() -> Result<()> {
    let mut candidate = workflow()?;
    let bounded = candidate
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .and_then(|jobs| jobs.get_mut(Value::String(BOUNDED_JOB.into())))
        .ok_or_else(|| anyhow!("bounded job must exist for the negative control"))?;
    let checkout = bounded
        .get_mut("steps")
        .and_then(Value::as_sequence_mut)
        .and_then(|steps| steps.first_mut())
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("bounded checkout step must exist"))?;
    checkout.insert(
        Value::String("uses".into()),
        Value::String("./.github/actions/full-corpus".into()),
    );

    let error = ensure_bounded_top_50_is_safe_and_reachable(&candidate)
        .err()
        .ok_or_else(|| anyhow!("step-level local action indirection must fail containment"))?;
    ensure!(error.to_string().contains("repository-local action"), "unexpected refusal: {error}");
    Ok(())
}

#[test]
fn ratchet_job_gates_on_warm_completion_output() -> Result<()> {
    let workflow = workflow()?;
    let ratchet = job(&workflow, RATCHET_JOB)?;

    let needs = ratchet.get("needs").ok_or_else(|| {
        anyhow!("ratchet job must take `needs: corpus-warm-full` so skip state composes")
    })?;
    let needs_warm = match needs {
        Value::String(one) => one.as_str() == WARM_JOB,
        Value::Sequence(many) => many.iter().filter_map(Value::as_str).any(|n| n == WARM_JOB),
        _ => false,
    };
    assert!(needs_warm, "ratchet job must depend on the warm job explicitly; found: {needs:?}");

    let ratchet_cond = condition(ratchet, RATCHET_JOB)?
        .ok_or_else(|| anyhow!("ratchet job must carry an `if:` guard"))?;
    assert!(
        ratchet_cond.contains(&format!("needs.{WARM_JOB}.outputs.complete == 'true'")),
        "gate chain must only enforce against a completed corpus (#12823 CRW-007); got: {ratchet_cond}"
    );

    let outputs_complete = format!("{WARM_JOB}.outputs.complete");
    assert!(
        !ratchet_cond.contains("result == 'skipped'"),
        "ratchet must not treat warm skips as neutral-success through result sniffing"
    );
    // Positive control: the completion expression appears under needs.* namespacing.
    assert!(
        ratchet_cond.contains(&format!("needs.{outputs_complete}")),
        "completion reference must be namespaced under needs"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// CRW-002 / CRW-003 / CRW-004: budgeted, unconditional, truth-reporting install
// ---------------------------------------------------------------------------

#[test]
fn install_step_is_unconditional_in_warm_job() -> Result<()> {
    let workflow = workflow()?;
    let warm = job(&workflow, WARM_JOB)?;
    let steps = steps_of(warm, WARM_JOB)?;
    let install = named_step(steps, INSTALL_STEP)?;
    let install_id = step_id(install, steps)?;

    assert_eq!(install_id, "cpan-install", "downstream gates pin this id");

    assert!(
        condition(install, INSTALL_STEP)?.is_none(),
        "`{INSTALL_STEP}` must run unconditionally — a `cache-hit != 'true'` skip is the \
         historical false-pass that left the frontier frozen behind stale caches (#12823)"
    );
    Ok(())
}

#[test]
fn budgeted_install_exists_only_in_warm_job_and_only_unconditioned() -> Result<()> {
    // CRW-002 mutation control, generalized safely: wherever a wall-clock
    // budgeted full-corpus install appears, it must be the warm job's
    // unconditional checkpoint pass. The bounded top-50 lane keeps its own
    // legitimate cache-hit skip because one complete pass fits easily in that
    // job's envelope.
    let workflow = workflow()?;
    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("workflow must declare jobs"))?;

    let mut budgeted_install_sites = Vec::new();
    for (job_name, job_value) in jobs {
        let name = job_name.as_str().unwrap_or("?").to_string();
        let steps = job_value.get("steps").and_then(Value::as_sequence).ok_or_else(|| {
            anyhow!("job `{name}` must declare steps so installs remain pinnable")
        })?;
        for step in steps {
            let Ok(run) = run_block(step, &format!("{name} step")) else { continue };
            if run.contains("cpan-corpus install") {
                budgeted_install_sites.push(name.clone());
                if run.contains("--time-budget-minutes") {
                    assert_eq!(
                        name, WARM_JOB,
                        "budgeted installs belong to the warm lane; found in `{name}`"
                    );
                    assert!(
                        condition(step, &name)?.is_none(),
                        "budgeted install in `{name}` must be unconditional"
                    );
                }
            }
        }
    }

    assert!(
        budgeted_install_sites.contains(&WARM_JOB.to_string()),
        "warm job must own the cpan-corpus install site; found sites: {budgeted_install_sites:?}"
    );
    Ok(())
}

#[test]
fn install_budget_pins_below_preemption_envelope() -> Result<()> {
    let workflow = workflow()?;
    let warm = job(&workflow, WARM_JOB)?;
    let steps = steps_of(warm, WARM_JOB)?;
    let install = named_step(steps, INSTALL_STEP)?;
    let run = run_block(install, INSTALL_STEP)?;

    let marker = "--time-budget-minutes";
    let start = run.find(marker).ok_or_else(|| {
        anyhow!(
            "install must carry {marker} so each pass ends below the runner preemption envelope"
        )
    })?;
    let digits: String =
        run[start + marker.len()..].trim_start().chars().take_while(char::is_ascii_digit).collect();
    let minutes: u64 = digits
        .parse()
        .map_err(|_| anyhow!("could not parse integer minutes after {marker}; found `{digits}`"))?;

    assert!(
        minutes < PREEMPTION_ENVELOPE_MINUTES,
        "install budget ({minutes}m) must stay strictly below the observed preemption \
         envelope ({PREEMPTION_ENVELOPE_MINUTES}m)"
    );
    assert_eq!(
        minutes, INSTALL_BUDGET_MINUTES,
        "budget drift requires re-reviewing setup+save headroom; update both sides deliberately"
    );
    Ok(())
}

#[test]
fn warm_job_ceiling_stays_below_legacy_rope() -> Result<()> {
    let workflow = workflow()?;
    let warm = job(&workflow, WARM_JOB)?;
    let ceiling = warm
        .get("timeout-minutes")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("warm job must keep an explicit numeric timeout"))?;

    assert_eq!(
        ceiling, WARM_JOB_CEILING_MINUTES as i64,
        "the old monolithic full job ran under a 120-minute rope that only proved how long \
         the platform lets a doomed pass hang; the warm job must stay compact"
    );
    Ok(())
}

#[test]
fn completeness_marker_is_captured_into_outputs() -> Result<()> {
    let workflow = workflow()?;
    let warm = job(&workflow, WARM_JOB)?;
    let steps = steps_of(warm, WARM_JOB)?;
    let install = named_step(steps, INSTALL_STEP)?;
    let run = run_block(install, INSTALL_STEP)?;

    assert!(
        run.contains("CPAN_CORPUS_INSTALL_COMPLETE=true"),
        "install must grep its own log for the truthful completion marker"
    );
    assert!(
        run.contains("complete=$complete") || run.contains("\"complete=$complete\""),
        "capture step must export `complete` into GITHUB_OUTPUT; got:\n{run}"
    );

    let outputs = warm.get("outputs").and_then(Value::as_mapping).ok_or_else(|| {
        anyhow!("warm job must publish `outputs.complete` for the gated ratchet leg")
    })?;
    let bound = format!("${{{{ steps.cpan-install.outputs.complete }}}}");
    assert!(
        outputs
            .iter()
            .any(|(key, value)| key.as_str() == Some("complete") && value.as_str() == Some(&bound)),
        "outputs.complete must bind to steps.cpan-install.outputs.complete verbatim"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// CRW-005: checkpoint persistence without false canonical saves
// ---------------------------------------------------------------------------

#[test]
fn canonical_save_gated_on_completion_and_checkpoint_save_exists_ungated_on_hit() -> Result<()> {
    let workflow = workflow()?;
    let warm = job(&workflow, WARM_JOB)?;
    let steps = steps_of(warm, WARM_JOB)?;

    let canonical = named_step(steps, CANONICAL_SAVE_STEP)?;
    let canonical_cond = condition(canonical, CANONICAL_SAVE_STEP)?.ok_or_else(|| {
        anyhow!("canonical save must gate on completion and a fresh-content pass")
    })?;
    assert!(
        canonical_cond.contains("steps.cpan-install.outcome == 'success'")
            && canonical_cond.contains("steps.cpan-install.outputs.complete == 'true'")
            && canonical_cond.contains("cache-hit != 'true'"),
        "canonical save fires only when a completed pass produced fresh content; got: {canonical_cond}"
    );

    let checkpoint = named_step(steps, CHECKPOINT_SAVE_STEP)?;
    let checkpoint_cond = condition(checkpoint, CHECKPOINT_SAVE_STEP)?
        .ok_or_else(|| anyhow!("checkpoint save must carry its own explicit guard for pinning"))?;
    assert!(
        !checkpoint_cond.contains("outputs.complete == 'true'"),
        "checkpoint exists precisely because passes stop early; it must not demand completion: {checkpoint_cond}"
    );
    assert!(
        checkpoint_cond.contains("outcome == 'success'"),
        "checkpoint persists only consistent, non-crashed states: {checkpoint_cond}"
    );
    let key = checkpoint
        .get("with")
        .ok_or_else(|| anyhow!("save steps must declare `with`"))?
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("checkpoint save must pin its cache key shape"))?
        .to_string();
    assert!(
        key.contains("-ckpt-"),
        "rolling checkpoint key must be unique per run (actions/cache keys are immutable); got: {key}"
    );

    // Both legs restore with the shared prefix so the newest banked state —
    // canonical or rolling checkpoint — wins regardless of which form it took.
    let restores: Vec<&str> = steps
        .iter()
        .chain(steps_of(job(&workflow, RATCHET_JOB)?, RATCHET_JOB)?.iter())
        .filter_map(|step| step.get("with"))
        .filter_map(|with| with.get("restore-keys"))
        .filter_map(Value::as_str)
        .collect();
    assert!(
        restores.iter().all(|prefix| prefix.contains("cpan-corpus-${{ runner.os }}-")),
        "every restore leg must fall back to the shared prefix so checkpoints are reachable: {restores:?}"
    );
    assert!(restores.len() >= 2, "warm and gate legs must both restore");
    Ok(())
}

// ---------------------------------------------------------------------------
// CRW-006: nothing weakened — commands byte-preserved, ceilings downward-only
// ---------------------------------------------------------------------------

#[test]
fn gate_chain_commands_are_byte_preserved() -> Result<()> {
    let workflow = workflow()?;
    let ratchet = job(&workflow, RATCHET_JOB)?;
    let steps = steps_of(ratchet, RATCHET_JOB)?;

    const PINNED_COMMANDS: [&str; 3] = [
        "cargo xtask cpan-corpus sweep --output .ci/cpan-corpus-baseline.json",
        "cargo xtask cpan-corpus ratchet",
        "cargo xtask cpan-corpus sweep --enforce",
    ];
    // Exact-line match on purpose: a suffix mutation like `--enforce-fast`
    // would slip through a substring check while genuinely weakening the gate.
    let command_lines: Vec<String> = steps
        .iter()
        .filter_map(|s| s.get("run"))
        .filter_map(Value::as_str)
        .flat_map(|run| run.lines())
        .map(|line| line.trim().to_string())
        .collect();
    for command in PINNED_COMMANDS {
        assert!(
            command_lines.iter().any(|line| line == command),
            "gate-chain regression: `{command}` drifted (#12823 forbids weakening the oracle)"
        );
    }

    let scope = named_step(steps, "Verify generated corpus scope")?;
    let scope_run = run_block(scope, "Verify generated corpus scope")?;
    for allowed in [".ci/cpan-corpus-baseline.json", ".ci/cpan-corpus-manifest.txt"] {
        assert!(
            scope_run.contains(allowed),
            "generated-output allowlist must stay exactly the two governed receipts: missing {allowed}"
        );
    }
    Ok(())
}

#[test]
fn no_timeout_minutes_exceeds_base_pin_maxima() -> Result<()> {
    let workflow = workflow()?;
    for (job_name, maximum) in BASE_PIN_TIMEOUT_MAXIMA {
        let actual = job(&workflow, job_name)?
            .get("timeout-minutes")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("job `{job_name}` must pin an explicit timeout"))?;
        assert!(
            actual <= *maximum,
            "job `{job_name}` raised its ceiling to {actual} beyond the base pin {maximum}; \
             longer ropes mask work instead of fitting the real preemption envelope"
        );
    }

    // Downward-only global guard vs the legacy monolith: nothing anywhere may
    // exceed the historic highest configured value in this workflow.
    fn walk(value: &Value) -> Vec<i64> {
        match value {
            Value::Mapping(map) => map
                .iter()
                .flat_map(|(key, inner)| {
                    let mut found = walk(inner);
                    if key.as_str() == Some("timeout-minutes") {
                        if let Some(n) = inner.as_i64() {
                            found.push(n);
                        }
                    }
                    found
                })
                .collect(),
            Value::Sequence(seq) => seq.iter().flat_map(walk).collect(),
            _ => Vec::new(),
        }
    }
    let all = walk(&workflow);
    assert!(
        all.iter().all(|minutes| *minutes <= 120),
        "no timeout in this workflow may exceed the legacy 120-minute maximum: {all:?}"
    );
    Ok(())
}
