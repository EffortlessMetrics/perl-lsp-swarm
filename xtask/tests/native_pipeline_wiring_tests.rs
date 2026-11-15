//! Repository enrollment and evidence ratchets for the extracted native formatter.
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// NPC-007 — nightly enrollment of the new bench target
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn nightly_workflow() -> Result<String, Box<dyn std::error::Error>> {
    let path = repo_root().join(".github/workflows/ci-nightly.yml");
    Ok(std::fs::read_to_string(path)?)
}

#[test]
fn bench_target_enrolls_native_pipeline_benchmark() -> Result<(), Box<dyn std::error::Error>> {
    let workflow = nightly_workflow()?;
    assert!(
        workflow.contains("\"perl-tidy:native_pipeline_benchmark:\""),
        "BENCH_TARGETS must enroll perl-tidy:native_pipeline_benchmark (declared-superset contract)"
    );
    assert!(
        workflow.contains(&format!(
            "--expect-id \"{}\"",
            "native_pipeline_document/delimited_n32_lf_tabs"
        )),
        "the extract step must require the representative native pipeline bench id"
    );

    let manifest = std::fs::read_to_string(repo_root().join("crates/perl-tidy/Cargo.toml"))?;
    let bench_section =
        manifest.split("[[bench]]").nth(1).ok_or("perl-tidy must declare a [[bench]] target")?;
    assert!(
        bench_section.contains("native_pipeline_benchmark")
            && bench_section.contains("harness = false"),
        "the [[bench]] target must be the criterion harness for native_pipeline_benchmark"
    );
    assert!(
        manifest.contains("criterion = { workspace = true }"),
        "criterion must be a dev-dependency of perl-tidy"
    );
    let benchmark = std::fs::read_to_string(
        repo_root().join("crates/perl-tidy/benches/native_pipeline_benchmark.rs"),
    )?;
    assert!(benchmark.contains("format_document_typed_with_counters"));
    assert!(benchmark.contains("identity_row_with_counters"));
    assert!(benchmark.contains("native-pipeline-measurements.v1.json"));
    let timing_body =
        benchmark.split("b.iter(|| {").nth(1).ok_or("benchmark timing closure missing")?;
    assert!(
        timing_body.contains("format_document_typed(")
            && !timing_body.contains("format_document_typed_with_counters("),
        "Criterion timing must use the plain typed entry point so counter collection stays in the dedicated receipt pass"
    );
    assert!(workflow.contains("NATIVE_PIPELINE_RUN_ID"));
    assert!(
        workflow.contains("CARGO_TARGET_DIR: ${{ github.workspace }}/target"),
        "Cargo, Criterion, native sidecar and the nightly readers must share one explicit artifact root"
    );
    assert!(workflow.contains("target/criterion/native-pipeline-measurements.v1.json"));
    Ok(())
}

#[test]
fn criterion_timing_invokes_native_formatter_production_path()
-> Result<(), Box<dyn std::error::Error>> {
    let benchmark = std::fs::read_to_string(
        repo_root().join("crates/perl-tidy/benches/native_pipeline_benchmark.rs"),
    )?;
    let iter_start = benchmark.find("b.iter(|| {").ok_or("benchmark timing closure missing")?;
    let iter_body = &benchmark[iter_start..];
    let iter_end = iter_body.find("});").ok_or("benchmark timing closure is unterminated")?;
    let iter_body = &iter_body[..iter_end];
    let production_invocation = "NativeFormatter::new().format_document_typed(";

    assert_eq!(
        iter_body.matches(production_invocation).count(),
        1,
        "Criterion must time exactly one native production invocation per iteration"
    );
    assert!(
        iter_body.contains("black_box(&source)"),
        "the timed production invocation must consume the enrolled subject source"
    );
    assert!(
        !iter_body.contains("format_document_typed_with_counters("),
        "counter collection belongs to the dedicated receipt pass, not Criterion timing"
    );
    Ok(())
}

#[test]
fn sidecar_run_id_uses_single_derivation_for_envelope_and_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let benchmark = std::fs::read_to_string(
        repo_root().join("crates/perl-tidy/benches/native_pipeline_benchmark.rs"),
    )?;
    let invariant = "the envelope and every row must carry an identical run id, which the nightly validator requires";
    assert_eq!(
        benchmark.matches("std::env::var(\"NATIVE_PIPELINE_RUN_ID\")").count(),
        1,
        "{invariant}; derive NATIVE_PIPELINE_RUN_ID exactly once"
    );
    assert!(
        benchmark.contains("fn build_subject_identities(toolchain: &str, run_id: &str)"),
        "{invariant}; build_subject_identities must receive the shared run id"
    );
    assert!(
        benchmark
            .contains("identity_row_with_counters(spec, &typed, toolchain, run_id, &counters)"),
        "{invariant}; every row must use the shared run id parameter"
    );
    assert!(
        benchmark.contains("\"run_id\": run_id"),
        "{invariant}; the sidecar envelope must use the shared run id parameter"
    );
    Ok(())
}

#[test]
fn elapsed_measurement_wraps_classification_for_document_and_range()
-> Result<(), Box<dyn std::error::Error>> {
    let source =
        std::fs::read_to_string(repo_root().join("crates/perl-tidy/src/native/outcome.rs"))?;
    let start =
        source.find("pub fn format_with_counters").ok_or("canonical counted entry missing")?;
    let body = &source[start..];
    let invocation = body
        .find("self.format(source, request, config, context)")
        .ok_or("canonical invocation missing")?;
    let elapsed = body.find("observe_elapsed(counters").ok_or("elapsed observation missing")?;
    assert!(
        invocation < elapsed,
        "the counted canonical call must include classification and plan verification in total elapsed"
    );
    Ok(())
}

#[test]
fn native_package_keeps_performance_evidence_limits_explicit()
-> Result<(), Box<dyn std::error::Error>> {
    let readme = std::fs::read_to_string(repo_root().join("crates/perl-tidy/README.md"))?;
    assert!(readme.contains("Allocation bounds and algorithmic complexity are not proven"));
    assert!(readme.contains("#10302 remains open"));
    Ok(())
}

// ---------------------------------------------------------------------------
// NPC-009 — timing stays evidence, never a required gate
// ---------------------------------------------------------------------------

fn step_retains_advisory_posture(workflow: &str, step_name: &str) -> bool {
    let mut in_step = false;
    for line in workflow.lines() {
        if line.contains("- name:") {
            in_step = line.contains(step_name);
            continue;
        }
        if in_step && line.contains("continue-on-error:") {
            return line.contains("true");
        }
    }
    false
}

#[test]
fn timing_stays_advisory_in_nightly_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let workflow = nightly_workflow()?;
    assert!(
        step_retains_advisory_posture(&workflow, "Compare against baseline"),
        "the Compare against baseline step must keep continue-on-error: true (#3979/#5282)"
    );
    assert!(
        step_retains_advisory_posture(&workflow, "Generate performance alerts"),
        "the Generate performance alerts step must keep continue-on-error: true (#3979/#5282)"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// NPC-010 — anti-masking downward-only ratchet
// ---------------------------------------------------------------------------

/// Base-pin maxima (origin/main @ a9664af79, 2026-08-27). Any timeout above
/// these maxima fails this canary; lowering is always allowed.
fn base_pin_max_timeout_minutes(job: &str) -> Option<u64> {
    match job {
        "mutation" => Some(60),
        // Raised from 45 by 159d80c9b2 (#18350): at the 45-minute pin the
        // nightly deadline landed mid-`native_pipeline_benchmark` (~46 minutes
        // in) and cancelled the job before extraction, comparison, and
        // receipts could run. The pin tracks that owned headroom raise rather
        // than absorbing a regression: the bump paid for the full benchmark
        // target finishing, not for lost speed.
        "benchmark" => Some(75),
        "real-repo-latency" => Some(30),
        "corpus-differential" => Some(20),
        "lsp-memory-plateau" => Some(35),
        "test-coverage" => Some(45),
        // Raised from 10 on `main` by 85d810dcd (#14061), which widened this
        // job from a spot check to a repository-wide tautological-assertion
        // sweep. The pin tracks that owned change rather than absorbing a
        // regression: the ratchet exists to stop a slow job being hidden by a
        // timeout bump, and this bump paid for new work, not for lost speed.
        "tautology-check" => Some(25),
        "semver-check" => Some(20),
        "public-api-check" => Some(20),
        "scorecard-ratchet-check" => Some(15),
        "clippy-strict" => Some(20),
        "perl-kwalitee" => Some(20),
        // Raised from 15 by 1ae41f302b (#17028, merged #17821): the 600s fuzz
        // budget plus cold toolchain/cargo-fuzz build headroom put the job
        // deadline inside the crash-artifact upload window at 15 minutes.
        // The pin tracks that owned headroom raise rather than absorbing a
        // regression: the bump paid for crash artifacts surviving
        // cancellation, not for lost speed.
        "fuzz" => Some(30),
        _ => None,
    }
}

fn job_timeouts(workflow: &str) -> Vec<(String, u64)> {
    let mut jobs = Vec::new();
    let mut current_job = String::new();
    for line in workflow.lines() {
        if let Some(job) = line.strip_prefix("  ")
            && let Some((name, rest)) = job.split_once(':')
            && rest.trim().is_empty()
            && !name.contains(' ')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            current_job = name.to_string();
        }
        if let Some((key, value)) = line.trim().split_once(':')
            && key.trim() == "timeout-minutes"
            && let Ok(minutes) = value.trim().parse::<u64>()
        {
            jobs.push((current_job.clone(), minutes));
        }
    }
    jobs
}

#[test]
fn no_timeout_or_budget_constant_exceeds_base_pin_maxima() -> Result<(), Box<dyn std::error::Error>>
{
    let workflow = nightly_workflow()?;
    let timeouts = job_timeouts(&workflow);
    assert!(timeouts.len() >= 13, "the ratchet must see every job timeout in ci-nightly.yml");
    for (job, minutes) in timeouts {
        let max = base_pin_max_timeout_minutes(&job);
        assert!(
            max.is_some(),
            "job {job} declares timeout-minutes {minutes} but has no pinned maximum; \
             lower the timeout or update the base-pin ratchet consciously"
        );
        let Some(max) = max else { continue };
        assert!(
            minutes <= max,
            "job {job} timeout {minutes} exceeds the base-pin maximum {max} — \
             anti-masking forbids absorbing regressions via timeout bumps"
        );
    }
    Ok(())
}
