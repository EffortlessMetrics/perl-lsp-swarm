//! Parsed-workflow contract for `ci-nightly.yml` cache writer authority (#13923).
//!
//! This proves the current hybrid nightly denominator, explicit save authority
//! on every candidate-reachable Rust cache, retained key identities, restore-
//! before-work ordering, and the two statically PR-excluded exceptions. It is
//! a static YAML oracle executed by the existing Workflow Policy lane.
//!
//! It does not prove runtime restore/save provenance, GitHub expression
//! evaluation, live eviction, trigger pruning (#10070), inventory schema
//! (#9177), or class-wide recurrence (#13927). Those remain NOT_PROVEN here.

use std::{collections::BTreeSet, fs, path::PathBuf};

use anyhow::{Result, anyhow, bail, ensure};

use serde_yaml_ng::Value;

const WORKFLOW: &str = ".github/workflows/ci-nightly.yml";
const POLICY: &str = ".github/workflows/workflow-policy.yml";
const CACHE_ACTION: &str = "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6";
const TRUSTED_SAVE_IF: &str =
    "${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}";
const CONTRACT_TEST: &str = "xtask/tests/ci_nightly_cache_contract.rs";
const CONTRACT_COMMAND: &str = "cargo test -p xtask --test ci_nightly_cache_contract --locked";

#[derive(Clone, Copy)]
struct CacheFamily {
    job: &'static str,
    key: Option<&'static str>,
    shared_key: Option<&'static str>,
    cache_all_crates: Option<bool>,
    cache_targets: Option<bool>,
    candidate_reachable: bool,
}

const FAMILIES: &[CacheFamily] = &[
    CacheFamily {
        job: "mutation",
        key: Some("${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "benchmark",
        key: Some("${{ runner.os }}-bench-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "real-repo-latency",
        key: Some("${{ runner.os }}-real-repo-latency-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "corpus-differential",
        key: Some("${{ runner.os }}-corpus-differential-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "lsp-memory-plateau",
        key: None,
        shared_key: Some("nightly-lsp-memory-${{ hashFiles('Cargo.lock') }}"),
        cache_all_crates: Some(true),
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "tautology-check",
        key: Some("tautology-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "semver-check",
        key: Some("semver-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "public-api-check",
        key: Some("public-api-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "scorecard-ratchet-check",
        key: Some("scorecard-ratchet-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "clippy-strict",
        key: Some("${{ runner.os }}-clippy-strict-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "perl-kwalitee",
        key: Some("${{ runner.os }}-kwalitee-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: true,
    },
    CacheFamily {
        job: "test-coverage",
        key: Some("${{ runner.os }}-coverage-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: Some(false),
        candidate_reachable: false,
    },
    CacheFamily {
        job: "fuzz",
        key: Some("${{ runner.os }}-fuzz-${{ matrix.target }}-${{ hashFiles('Cargo.lock') }}"),
        shared_key: None,
        cache_all_crates: None,
        cache_targets: None,
        candidate_reachable: false,
    },
];

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn read_repo_file(relative: &str) -> Result<String> {
    let path = project_root().join(relative);
    Ok(fs::read_to_string(&path)?)
}

/// YAML 1.1 parsers fold a bare `on:` key into boolean `true`.
fn workflow_on(workflow: &Value) -> Option<&Value> {
    workflow.as_mapping()?.iter().find_map(|(key, value)| match key {
        Value::String(name) if name == "on" => Some(value),
        Value::Bool(true) => Some(value),
        _ => None,
    })
}

fn mapping_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_mapping()?.get(Value::String(key.to_owned()))
}

fn job_if(job: &Value) -> &str {
    mapping_get(job, "if").and_then(Value::as_str).unwrap_or("")
}

fn candidate_reachable_from_if(if_expr: &str) -> bool {
    if_expr.contains("pull_request")
}

fn uses_rust_cache(step: &Value) -> bool {
    mapping_get(step, "uses")
        .and_then(Value::as_str)
        .is_some_and(|uses| uses.starts_with("Swatinem/rust-cache@"))
}

fn warms_rust_work(run: &str) -> bool {
    run.split([' ', '\n', '\t', '|', '&', ';', '`', '(', ')', '\\'])
        .any(|token| token == "cargo" || token == "just")
}

fn expected_writers() -> BTreeSet<&'static str> {
    FAMILIES.iter().filter(|family| family.candidate_reachable).map(|family| family.job).collect()
}

fn expected_excluded() -> BTreeSet<&'static str> {
    FAMILIES.iter().filter(|family| !family.candidate_reachable).map(|family| family.job).collect()
}

fn validate_triggers(workflow: &Value) -> Result<()> {
    let triggers = workflow_on(workflow)
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("ci-nightly.yml must declare mapping-valued triggers"))?;
    let names: BTreeSet<_> = triggers.keys().filter_map(Value::as_str).collect();
    ensure!(
        names == BTreeSet::from(["pull_request", "schedule", "workflow_dispatch"]),
        "unapproved nightly trigger set {names:?}; pull_request_target, merge_group, and extra events can make a canonical-looking ref a candidate writer"
    );
    let pull_request = triggers
        .get(Value::String("pull_request".into()))
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("pull_request must be a mapping"))?;
    let branches: Vec<_> = pull_request
        .get(Value::String("branches".into()))
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("pull_request must declare branches"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    ensure!(
        branches == ["main", "master"],
        "pull_request branches must remain the canonical pair: {branches:?}"
    );
    let types: BTreeSet<_> = pull_request
        .get(Value::String("types".into()))
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("pull_request must declare activity types"))?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    ensure!(
        types.contains("labeled"),
        "pull_request must retain labeled so label-gated jobs stay PR-reachable without becoming cache writers"
    );
    ensure!(
        types
            == BTreeSet::from(["opened", "synchronize", "reopened", "ready_for_review", "labeled"]),
        "pull_request activity types changed: {types:?}"
    );
    Ok(())
}

fn validate_family(job_name: &str, job: &Value, family: &CacheFamily) -> Result<()> {
    let reachable = candidate_reachable_from_if(job_if(job));
    ensure!(
        reachable == family.candidate_reachable,
        "job `{job_name}` reachability drifted: parsed_candidate={reachable}, expected={}",
        family.candidate_reachable
    );
    let steps = mapping_get(job, "steps")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("job `{job_name}` must declare steps"))?;
    let cache_indexes: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| uses_rust_cache(step))
        .map(|(index, _)| index)
        .collect();
    ensure!(
        cache_indexes.len() == 1,
        "job `{job_name}` must have exactly one rust-cache step, found {}",
        cache_indexes.len()
    );
    let cache_index = cache_indexes[0];
    let cache = &steps[cache_index];
    let uses = mapping_get(cache, "uses").and_then(Value::as_str).unwrap_or("");
    ensure!(uses == CACHE_ACTION, "job `{job_name}` rust-cache pin drifted: {uses}");
    ensure!(
        mapping_get(cache, "if").is_none(),
        "job `{job_name}` must not disable cache restore with a step if"
    );
    let cache_with = mapping_get(cache, "with")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("job `{job_name}` rust-cache must declare inputs"))?;
    ensure!(
        cache_with.get(Value::String("cache-on-failure".into())) == Some(&Value::Bool(true)),
        "job `{job_name}` must keep cache-on-failure: true"
    );
    ensure!(
        cache_with.get(Value::String("lookup-only".into())).is_none(),
        "job `{job_name}` must not replace save-if with lookup-only"
    );
    match (family.key, cache_with.get(Value::String("key".into())).and_then(Value::as_str)) {
        (Some(expected), Some(actual)) if expected == actual => {}
        (None, None) => {}
        (expected, actual) => {
            bail!(
                "job `{job_name}` cache key identity changed: expected {expected:?}, found {actual:?}"
            )
        }
    }
    match (
        family.shared_key,
        cache_with.get(Value::String("shared-key".into())).and_then(Value::as_str),
    ) {
        (Some(expected), Some(actual)) if expected == actual => {}
        (None, None) => {}
        (expected, actual) => {
            bail!(
                "job `{job_name}` shared-key identity changed: expected {expected:?}, found {actual:?}"
            )
        }
    }
    match family.cache_all_crates {
        Some(expected) => ensure!(
            cache_with.get(Value::String("cache-all-crates".into()))
                == Some(&Value::Bool(expected)),
            "job `{job_name}` cache-all-crates drifted"
        ),
        None => ensure!(
            cache_with.get(Value::String("cache-all-crates".into())).is_none(),
            "job `{job_name}` must not add cache-all-crates under this writer-authority claim"
        ),
    }
    match family.cache_targets {
        Some(expected) => ensure!(
            cache_with.get(Value::String("cache-targets".into())) == Some(&Value::Bool(expected)),
            "job `{job_name}` cache-targets drifted"
        ),
        None => ensure!(
            cache_with.get(Value::String("cache-targets".into())).is_none(),
            "job `{job_name}` must not add cache-targets under this writer-authority claim"
        ),
    }
    validate_save_if(
        job_name,
        family.candidate_reachable,
        cache_with.get(Value::String("save-if".into())),
    )?;

    let first_work = steps.iter().position(|step| {
        mapping_get(step, "run").and_then(Value::as_str).is_some_and(warms_rust_work)
    });
    if let Some(work_index) = first_work {
        ensure!(
            cache_index < work_index,
            "cache restore must precede cargo/just work in `{job_name}`"
        );
    }
    Ok(())
}

fn validate_save_if(
    job_name: &str,
    candidate_reachable: bool,
    save_if: Option<&Value>,
) -> Result<()> {
    if !candidate_reachable {
        ensure!(
            save_if.is_none(),
            "statically PR-excluded cache `{job_name}` must not grow a textual save-if"
        );
        return Ok(());
    }
    // YAML 1.1 folds unquoted `true` into a boolean, which is also Swatinem's
    // default write-enabled posture. Inspect the raw node, not only strings.
    match save_if {
        None => bail!("job `{job_name}` is candidate-reachable and missing trusted save-if"),
        Some(Value::Bool(true)) => {
            bail!("job `{job_name}` save-if must not be unconditionally true")
        }
        Some(Value::Bool(false)) => {
            bail!("job `{job_name}` save-if must retain trusted default-branch save authority")
        }
        Some(Value::String(value)) if value == "true" || value == "${{ true }}" => {
            bail!("job `{job_name}` save-if must not be unconditionally true")
        }
        Some(Value::String(value))
            if value.contains("pull_request") || value.contains("head_ref") =>
        {
            bail!(
                "job `{job_name}` save-if must not be authorized by a candidate event, label, or head-branch string"
            )
        }
        Some(Value::String(value)) if value == TRUSTED_SAVE_IF => Ok(()),
        Some(other) => bail!(
            "job `{job_name}` save-if must be the canonical default-branch expression, found {other:?}"
        ),
    }
}

fn validate_nightly_workflow(source: &str) -> Result<()> {
    let workflow: Value = serde_yaml_ng::from_str(source)?;
    validate_triggers(&workflow)?;
    let jobs = mapping_get(&workflow, "jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("ci-nightly.yml must declare jobs"))?;

    let mut seen = BTreeSet::new();
    let mut cache_jobs = BTreeSet::new();
    for (job_name, job) in jobs {
        let name = job_name.as_str().ok_or_else(|| anyhow!("job names must be strings"))?;
        seen.insert(name.to_owned());
        let Some(steps) = mapping_get(job, "steps").and_then(Value::as_sequence) else {
            continue;
        };
        let cache_count = steps.iter().filter(|step| uses_rust_cache(step)).count();
        if cache_count == 0 {
            ensure!(
                steps.iter().all(|step| {
                    mapping_get(step, "run").and_then(Value::as_str).is_none_or(|run| {
                        !run.contains("rust-cache") && !run.contains("actions/cache")
                    })
                }),
                "job `{name}` hides an alternate cache writer in a shell body"
            );
            continue;
        }
        cache_jobs.insert(name.to_owned());
        let family = FAMILIES.iter().find(|family| family.job == name).ok_or_else(|| {
            anyhow!("unexpected rust-cache consumer `{name}` is outside the nightly denominator")
        })?;
        validate_family(name, job, family)?;
    }

    let expected: BTreeSet<_> = FAMILIES.iter().map(|family| family.job.to_owned()).collect();
    ensure!(
        cache_jobs == expected,
        "nightly rust-cache denominator drifted: expected {expected:?}, found {cache_jobs:?}"
    );
    for family in FAMILIES {
        ensure!(seen.contains(family.job), "expected nightly job `{}` is missing", family.job);
    }
    ensure!(
        expected_writers().len() == 11,
        "candidate-reachable nightly writer count must stay the corrected eleven-row denominator"
    );
    ensure!(
        expected_excluded() == BTreeSet::from(["test-coverage", "fuzz"]),
        "statically PR-excluded nightly caches must remain test-coverage and fuzz"
    );
    ensure!(
        seen.contains("perl-kwalitee"),
        "current #8421 source still uses job id perl-kwalitee; this leaf must not rename it"
    );
    Ok(())
}

fn replace_once(source: &str, from: &str, to: &str) -> Result<String> {
    ensure!(source.matches(from).count() == 1, "fixture anchor must occur exactly once: {from}");
    Ok(source.replacen(from, to, 1))
}

fn reject_mutation(source: &str, from: &str, to: &str, needle: &str) -> Result<()> {
    let mutated = replace_once(source, from, to)?;
    let error = validate_nightly_workflow(&mutated)
        .err()
        .ok_or_else(|| anyhow!("mutation was accepted: {from}"))?;
    let message = error.to_string();
    ensure!(
        message.contains(needle),
        "mutation rejected for the wrong reason; expected `{needle}` in {message}"
    );
    Ok(())
}

#[test]
fn nightly_rust_caches_use_explicit_writer_authority() -> Result<()> {
    validate_nightly_workflow(&read_repo_file(WORKFLOW)?)
}

#[test]
fn workflow_policy_lane_executes_the_nightly_cache_contract() -> Result<()> {
    let policy = read_repo_file(POLICY)?;
    ensure!(
        policy.matches(CONTRACT_TEST).count() >= 2,
        "Workflow Policy path filters must cover {CONTRACT_TEST} on pull_request and push"
    );
    ensure!(
        policy.contains(&format!("run: {CONTRACT_COMMAND}")),
        "Workflow Policy must execute {CONTRACT_COMMAND}"
    );
    Ok(())
}

#[test]
fn mutated_nightly_writer_postures_are_rejected() -> Result<()> {
    let source = read_repo_file(WORKFLOW)?;
    validate_nightly_workflow(&source)?;

    reject_mutation(
        &source,
        "          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n",
        "missing trusted save-if",
    )?;
    reject_mutation(
        &source,
        "          key: tautology-${{ hashFiles('Cargo.lock') }}\n          cache-on-failure: true\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "          key: tautology-${{ hashFiles('Cargo.lock') }}\n          cache-on-failure: true\n",
        "missing trusted save-if",
    )?;
    reject_mutation(
        &source,
        "          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: true\n",
        "unconditionally true",
    )?;
    reject_mutation(
        &source,
        "          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: false\n",
        "retain trusted default-branch save authority",
    )?;
    reject_mutation(
        &source,
        "          key: ${{ runner.os }}-kwalitee-${{ hashFiles('Cargo.lock') }}\n          cache-on-failure: true\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "          key: ${{ runner.os }}-kwalitee-${{ hashFiles('Cargo.lock') }}\n          cache-on-failure: true\n          save-if: ${{ contains(github.event.pull_request.labels.*.name, 'ci:kwalitee') }}\n",
        "candidate event, label, or head-branch",
    )?;
    reject_mutation(
        &source,
        "          key: ${{ runner.os }}-clippy-strict-${{ hashFiles('Cargo.lock') }}\n          cache-on-failure: true\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "          key: ${{ runner.os }}-clippy-strict-${{ hashFiles('Cargo.lock') }}\n          cache-on-failure: true\n          save-if: ${{ github.head_ref == 'main' }}\n",
        "candidate event, label, or head-branch",
    )?;
    reject_mutation(
        &source,
        "          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n",
        "          key: ${{ runner.os }}-mutation-v2-${{ hashFiles('Cargo.lock') }}\n",
        "cache key identity changed",
    )?;
    reject_mutation(
        &source,
        "          shared-key: nightly-lsp-memory-${{ hashFiles('Cargo.lock') }}\n",
        "          shared-key: nightly-lsp-memory-v2-${{ hashFiles('Cargo.lock') }}\n",
        "shared-key identity changed",
    )?;
    reject_mutation(
        &source,
        "          shared-key: nightly-lsp-memory-${{ hashFiles('Cargo.lock') }}\n",
        "          key: nightly-lsp-memory-${{ hashFiles('Cargo.lock') }}\n",
        "cache key identity changed",
    )?;
    reject_mutation(
        &source,
        "          key: ${{ runner.os }}-coverage-${{ hashFiles('Cargo.lock') }}\n          cache-targets: false\n          cache-on-failure: true\n",
        "          key: ${{ runner.os }}-coverage-${{ hashFiles('Cargo.lock') }}\n          cache-targets: false\n          cache-on-failure: true\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "must not grow a textual save-if",
    )?;
    reject_mutation(
        &source,
        "          key: ${{ runner.os }}-coverage-${{ hashFiles('Cargo.lock') }}\n          cache-targets: false\n          cache-on-failure: true\n",
        "          key: ${{ runner.os }}-coverage-${{ hashFiles('Cargo.lock') }}\n          cache-targets: false\n          cache-on-failure: true\n          save-if: true\n",
        "must not grow a textual save-if",
    )?;
    reject_mutation(
        &source,
        "      github.event_name == 'schedule' ||\n      (github.event_name == 'workflow_dispatch' && inputs.run_coverage)\n",
        "      github.event_name == 'schedule' ||\n      (github.event_name == 'workflow_dispatch' && inputs.run_coverage) ||\n      github.event_name == 'pull_request'\n",
        "reachability drifted",
    )?;
    reject_mutation(
        &source,
        "on:\n  pull_request:\n",
        "on:\n  pull_request_target:\n  pull_request:\n",
        "unapproved nightly trigger set",
    )?;
    reject_mutation(
        &source,
        "    types: [opened, synchronize, reopened, ready_for_review, labeled]\n",
        "    types: [opened, synchronize, reopened, ready_for_review]\n",
        "must retain labeled",
    )?;
    reject_mutation(
        &source,
        "  perl-kwalitee:\n",
        "  release-readiness:\n",
        "unexpected rust-cache consumer",
    )?;
    reject_mutation(
        &source,
        "      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6  # v2.9.2\n        with:\n          cache-on-failure: true\n          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n      - run: cargo install cargo-mutants --locked || true\n",
        "      - run: cargo install cargo-mutants --locked || true\n      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6  # v2.9.2\n        with:\n          cache-on-failure: true\n          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "cache restore must precede cargo/just work",
    )?;
    reject_mutation(
        &source,
        "      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6  # v2.9.2\n        with:\n          cache-on-failure: true\n          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6  # v2.9.2\n        with:\n          cache-on-failure: true\n          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6  # v2.9.2\n        with:\n          key: extra-unguarded-${{ hashFiles('Cargo.lock') }}\n",
        "exactly one rust-cache step",
    )?;
    reject_mutation(
        &source,
        "      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6  # v2.9.2\n        with:\n          cache-on-failure: true\n          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "      - if: false\n        uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6  # v2.9.2\n        with:\n          cache-on-failure: true\n          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "must not disable cache restore",
    )?;
    reject_mutation(
        &source,
        "          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "          key: ${{ runner.os }}-mutation-${{ hashFiles('Cargo.lock') }}\n          lookup-only: true\n          save-if: ${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}\n",
        "must not replace save-if with lookup-only",
    )?;
    Ok(())
}
