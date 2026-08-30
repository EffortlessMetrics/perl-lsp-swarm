//! Parsed writer-authority contract for the hybrid nightly workflow (#13923).
//!
//! `ci-nightly.yml` mixes schedule, manual, and label-gated pull-request jobs.
//! Candidate jobs may restore Rust state but must not publish it. Two jobs are
//! deliberately outside that writer-repair denominator because their job guards
//! are statically unreachable from pull requests.

use std::{collections::BTreeSet, fs, path::PathBuf};

use anyhow::{Result, anyhow, ensure};
use serde_yaml_ng::Value;

const WORKFLOW_FILE: &str = "ci-nightly.yml";
const RUST_CACHE_ACTION: &str =
    "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6";
const TRUSTED_SAVE_IF: &str =
    "${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}";

struct CacheContract {
    job: &'static str,
    cache_step_index: usize,
    inputs: &'static str,
}

const CANDIDATE_WRITERS: &[CacheContract] = &[
    CacheContract {
        job: "mutation",
        cache_step_index: 2,
        inputs: r#"cache-on-failure: true
key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
    CacheContract {
        job: "benchmark",
        cache_step_index: 2,
        inputs: r#"key: ${{ runner.os }}-bench-${{ hashFiles('Cargo.lock') }}
cache-on-failure: true
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
    CacheContract {
        job: "real-repo-latency",
        cache_step_index: 2,
        inputs: r#"key: ${{ runner.os }}-real-repo-latency-${{ hashFiles('Cargo.lock') }}
cache-on-failure: true
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
    CacheContract {
        job: "corpus-differential",
        cache_step_index: 2,
        inputs: r#"key: ${{ runner.os }}-corpus-differential-${{ hashFiles('Cargo.lock') }}
cache-on-failure: true
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
    CacheContract {
        job: "lsp-memory-plateau",
        cache_step_index: 2,
        inputs: r#"cache-on-failure: true
cache-all-crates: true
shared-key: nightly-lsp-memory-${{ hashFiles('Cargo.lock') }}
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
    CacheContract {
        job: "semver-check",
        cache_step_index: 2,
        inputs: r#"key: semver-${{ hashFiles('Cargo.lock') }}
cache-on-failure: true
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
    CacheContract {
        job: "public-api-check",
        cache_step_index: 4,
        inputs: r#"key: public-api-${{ hashFiles('Cargo.lock') }}
cache-on-failure: true
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
    CacheContract {
        job: "scorecard-ratchet-check",
        cache_step_index: 2,
        inputs: r#"key: scorecard-ratchet-${{ hashFiles('Cargo.lock') }}
cache-on-failure: true
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
    CacheContract {
        job: "clippy-strict",
        cache_step_index: 2,
        inputs: r#"key: ${{ runner.os }}-clippy-strict-${{ hashFiles('Cargo.lock') }}
cache-on-failure: true
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
    CacheContract {
        job: "perl-kwalitee",
        cache_step_index: 2,
        inputs: r#"key: ${{ runner.os }}-kwalitee-${{ hashFiles('Cargo.lock') }}
cache-on-failure: true
save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"#,
    },
];

const TRUSTED_ONLY_CONSUMERS: &[CacheContract] = &[
    CacheContract {
        job: "test-coverage",
        cache_step_index: 3,
        inputs: r#"key: ${{ runner.os }}-coverage-${{ hashFiles('Cargo.lock') }}
cache-targets: false
cache-on-failure: true"#,
    },
    CacheContract {
        job: "fuzz",
        cache_step_index: 2,
        inputs: r#"key: ${{ runner.os }}-fuzz-${{ matrix.target }}-${{ hashFiles('Cargo.lock') }}
cache-on-failure: true"#,
    },
];

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn workflow() -> Result<Value> {
    let path = project_root().join(".github/workflows").join(WORKFLOW_FILE);
    Ok(serde_yaml_ng::from_str(&fs::read_to_string(path)?)?)
}

fn jobs(workflow: &Value) -> Result<&serde_yaml_ng::Mapping> {
    workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("{WORKFLOW_FILE} must declare jobs"))
}

fn job<'a>(workflow: &'a Value, name: &str) -> Result<&'a Value> {
    jobs(workflow)?
        .get(Value::String(name.into()))
        .ok_or_else(|| anyhow!("{WORKFLOW_FILE} must declare job `{name}`"))
}

fn steps<'a>(workflow: &'a Value, name: &str) -> Result<&'a Vec<Value>> {
    job(workflow, name)?
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("job `{name}` must declare inline steps"))
}

fn cache_step<'a>(workflow: &'a Value, contract: &CacheContract) -> Result<&'a Value> {
    let job_steps = steps(workflow, contract.job)?;
    let matches = job_steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.get("uses").and_then(Value::as_str) == Some(RUST_CACHE_ACTION))
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "job `{}` must contain exactly one reviewed Rust cache consumer; found {}",
        contract.job,
        matches.len()
    );
    let (index, step) = matches[0];
    ensure!(
        index == contract.cache_step_index,
        "job `{}` cache moved from reviewed step index {} to {index}",
        contract.job,
        contract.cache_step_index
    );
    Ok(step)
}

fn condition<'a>(workflow: &'a Value, name: &str) -> Result<&'a str> {
    job(workflow, name)?
        .get("if")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("job `{name}` must carry a plain-scalar condition"))
}

fn expected_inputs(contract: &CacheContract) -> Result<Value> {
    Ok(serde_yaml_ng::from_str(contract.inputs)?)
}

fn ensure_contract(workflow: &Value) -> Result<()> {
    let discovered = jobs(workflow)?
        .iter()
        .filter_map(|(job_name, job)| {
            let has_cache = job
                .get("steps")
                .and_then(Value::as_sequence)
                .is_some_and(|steps| {
                    steps.iter().any(|step| {
                        step.get("uses").and_then(Value::as_str) == Some(RUST_CACHE_ACTION)
                    })
                });
            has_cache.then(|| job_name.as_str().unwrap_or("<non-string-job>"))
        })
        .collect::<BTreeSet<_>>();
    let expected = CANDIDATE_WRITERS
        .iter()
        .chain(TRUSTED_ONLY_CONSUMERS)
        .map(|contract| contract.job)
        .collect::<BTreeSet<_>>();
    ensure!(
        discovered == expected,
        "nightly Rust cache denominator drifted: expected {expected:?}, found {discovered:?}"
    );

    for contract in CANDIDATE_WRITERS {
        let guard = condition(workflow, contract.job)?;
        ensure!(
            guard.contains("pull_request"),
            "candidate writer job `{}` no longer proves its PR reachability: {guard}",
            contract.job
        );
        let cache = cache_step(workflow, contract)?;
        ensure!(
            cache.get("with") == Some(&expected_inputs(contract)?),
            "candidate writer job `{}` cache inputs drifted",
            contract.job
        );
        let save_if = cache
            .get("with")
            .and_then(|inputs| inputs.get("save-if"))
            .and_then(Value::as_str);
        ensure!(
            save_if == Some(TRUSTED_SAVE_IF),
            "candidate writer job `{}` must keep exact canonical-ref save authority; found {save_if:?}",
            contract.job
        );
    }

    for contract in TRUSTED_ONLY_CONSUMERS {
        let guard = condition(workflow, contract.job)?;
        ensure!(
            !guard.contains("pull_request")
                && guard.contains("workflow_dispatch")
                && guard.contains("schedule"),
            "trusted-only job `{}` must remain statically PR-excluding: {guard}",
            contract.job
        );
        let cache = cache_step(workflow, contract)?;
        ensure!(
            cache.get("with") == Some(&expected_inputs(contract)?),
            "trusted-only job `{}` cache inputs changed under the writer-only repair",
            contract.job
        );
        ensure!(
            cache
                .get("with")
                .and_then(|inputs| inputs.get("save-if"))
                .is_none(),
            "trusted-only job `{}` must remain byte-equivalent rather than receiving a cosmetic guard",
            contract.job
        );
    }

    Ok(())
}

fn cache_step_mut<'a>(workflow: &'a mut Value, job_name: &str) -> Result<&'a mut Value> {
    workflow
        .get_mut("jobs")
        .and_then(Value::as_mapping_mut)
        .and_then(|jobs| jobs.get_mut(Value::String(job_name.into())))
        .and_then(|job| job.get_mut("steps"))
        .and_then(Value::as_sequence_mut)
        .and_then(|steps| {
            steps.iter_mut().find(|step| {
                step.get("uses").and_then(Value::as_str) == Some(RUST_CACHE_ACTION)
            })
        })
        .ok_or_else(|| anyhow!("job `{job_name}` must contain a mutable Rust cache step"))
}

#[test]
fn hybrid_nightly_cache_writers_have_exact_authority() -> Result<()> {
    ensure_contract(&workflow()?)
}

#[test]
fn missing_candidate_save_guard_is_rejected() -> Result<()> {
    let mut candidate = workflow()?;
    cache_step_mut(&mut candidate, "mutation")?
        .get_mut("with")
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("mutation cache inputs must be mutable"))?
        .remove(Value::String("save-if".into()));

    let error = ensure_contract(&candidate)
        .err()
        .ok_or_else(|| anyhow!("an implicit candidate writer must fail the contract"))?;
    ensure!(error.to_string().contains("inputs drifted"));
    Ok(())
}

#[test]
fn label_or_candidate_output_cannot_authorize_a_save() -> Result<()> {
    let mut candidate = workflow()?;
    cache_step_mut(&mut candidate, "benchmark")?
        .get_mut("with")
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("benchmark cache inputs must be mutable"))?
        .insert(
            Value::String("save-if".into()),
            Value::String(
                "${{ github.event.action == 'labeled' || steps.compare.outputs.REGRESSION == '0' }}"
                    .into(),
            ),
        );

    let error = ensure_contract(&candidate)
        .err()
        .ok_or_else(|| anyhow!("candidate-controlled save authority must fail the contract"))?;
    ensure!(error.to_string().contains("inputs drifted"));
    Ok(())
}

#[test]
fn cache_identity_drift_is_rejected() -> Result<()> {
    let mut candidate = workflow()?;
    cache_step_mut(&mut candidate, "semver-check")?
        .get_mut("with")
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("semver cache inputs must be mutable"))?
        .insert(Value::String("key".into()), Value::String("semver-wide".into()));

    let error = ensure_contract(&candidate)
        .err()
        .ok_or_else(|| anyhow!("cache-key drift must fail the writer-only contract"))?;
    ensure!(error.to_string().contains("inputs drifted"));
    Ok(())
}

#[test]
fn trusted_only_cache_is_not_rewritten_for_uniformity() -> Result<()> {
    let mut candidate = workflow()?;
    cache_step_mut(&mut candidate, "test-coverage")?
        .get_mut("with")
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| anyhow!("coverage cache inputs must be mutable"))?
        .insert(Value::String("save-if".into()), Value::String(TRUSTED_SAVE_IF.into()));

    let error = ensure_contract(&candidate)
        .err()
        .ok_or_else(|| anyhow!("cosmetic trusted-only cache changes must fail"))?;
    ensure!(error.to_string().contains("writer-only repair"));
    Ok(())
}
