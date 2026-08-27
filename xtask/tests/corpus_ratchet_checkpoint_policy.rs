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

use std::{fs, path::PathBuf};

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
const INSTALL_STEP: &str = "Install CPAN corpus checkpoint";
const CANONICAL_SAVE_STEP: &str = "Save CPAN corpus cache (canonical)";
const CHECKPOINT_SAVE_STEP: &str = "Save CPAN corpus checkpoint (partial progress)";

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
    job.get("permissions").and_then(Value::as_mapping).is_some_and(|permissions| {
        permissions.values().any(|permission| permission.as_str() == Some("write"))
    })
}

fn full_chain_capabilities(job: &Value) -> Result<Vec<&'static str>> {
    let rendered = serde_yaml_ng::to_string(job)?;
    let mut capabilities = Vec::new();

    // `cpan-corpus` covers direct xtask commands, repository-owned `just`
    // aliases, full cache/install paths, and full receipt names. Job-level
    // `uses` covers a reusable workflow that could hide the same operations.
    if rendered.contains("cpan-corpus") {
        capabilities.push("CPAN corpus command, alias, path, cache key, or receipt");
    }
    if job.get("uses").is_some() {
        capabilities.push("reusable workflow");
    }
    if rendered.contains("actions/cache/restore@") || rendered.contains("actions/cache/save@") {
        capabilities.push("cache reader or writer");
    }
    if rendered.contains("create-pull-request@") || has_write_permissions(job) {
        capabilities.push("repository writer");
    }

    Ok(capabilities)
}

fn ensure_full_chain_is_fail_closed(workflow: &Value) -> Result<()> {
    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("workflow must declare jobs"))?;

    for (job_name, job_value) in jobs {
        let name =
            job_name.as_str().ok_or_else(|| anyhow!("workflow job names must be strings"))?;
        if name == BOUNDED_JOB {
            continue;
        }

        let capabilities = full_chain_capabilities(job_value)?;
        if capabilities.is_empty() {
            continue;
        }

        let guard = condition(job_value, name)?.ok_or_else(|| {
            anyhow!(
                "full-chain-capable job `{name}` must carry an `if:` containment guard; capabilities: {}",
                capabilities.join(", ")
            )
        })?;
        ensure!(
            guard.trim_start().starts_with("false &&"),
            "unsafe v1 full-chain-capable job `{name}` must remain fail-closed until #13004 adds identity, manifest, quiescence, atomicity, retention, and hosted proof; capabilities: {}; guard was: {guard}",
            capabilities.join(", ")
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
        bounded.get("uses").is_none()
            && !has_write_permissions(bounded)
            && !rendered.contains("create-pull-request@"),
        "bounded proof must remain read-only and inline"
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
fn containment_rejects_unguarded_fourth_job_alias() -> Result<()> {
    let candidate = workflow_with_extra_job(
        "unsafe-full-alias",
        r#"
if: github.event_name == 'schedule'
runs-on: ubuntu-24.04
steps:
  - name: Re-enable the full install through a repository alias
    run: just cpan-corpus-install
"#,
    )?;

    let error = ensure_full_chain_is_fail_closed(&candidate)
        .err()
        .ok_or_else(|| anyhow!("an unguarded fourth-job alias must fail containment"))?;
    let message = error.to_string();
    ensure!(message.contains("unsafe-full-alias"), "unexpected refusal: {message}");
    ensure!(message.contains("CPAN corpus"), "unexpected refusal: {message}");
    Ok(())
}

#[test]
fn containment_rejects_hidden_reusable_cache_and_writer_paths() -> Result<()> {
    let controls = [
        (
            "unsafe-reusable",
            r#"
if: github.event_name == 'schedule'
uses: ./.github/workflows/full-corpus.yml
"#,
            "reusable workflow",
        ),
        (
            "unsafe-cache-writer",
            r#"
if: github.event_name == 'schedule'
runs-on: ubuntu-24.04
steps:
  - uses: actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9
    with:
      path: target/full-bank
      key: full-bank-${{ github.run_id }}
"#,
            "cache reader or writer",
        ),
        (
            "unsafe-repository-writer",
            r#"
if: github.event_name == 'schedule'
runs-on: ubuntu-24.04
permissions:
  contents: write
steps:
  - run: echo unsafe
"#,
            "repository writer",
        ),
    ];

    for (name, job_yaml, expected_capability) in controls {
        let candidate = workflow_with_extra_job(name, job_yaml)?;
        let error = ensure_full_chain_is_fail_closed(&candidate)
            .err()
            .ok_or_else(|| anyhow!("unguarded `{name}` control must fail containment"))?;
        let message = error.to_string();
        ensure!(message.contains(name), "unexpected refusal: {message}");
        ensure!(
            message.contains(expected_capability),
            "refusal for `{name}` did not identify `{expected_capability}`: {message}"
        );
    }
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
