use chrono::{DateTime, Duration as ChronoDuration, Utc};
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::utils::project_root;

const COST_PER_MINUTE: f64 = 0.008;
const MONTHLY_BUDGET_TARGET: f64 = 60.0;
const ANNUAL_BUDGET_TARGET: f64 = 720.0;

#[derive(Deserialize)]
struct RepoInfo {
    owner: RepoOwner,
    name: String,
}

#[derive(Deserialize)]
struct RepoOwner {
    login: String,
}

#[derive(Serialize)]
struct CiCostWorkflow {
    name: String,
    runs: u64,
    total_minutes: u64,
    average_minutes: f64,
    cost: f64,
}

#[derive(Serialize)]
struct CostProjection {
    minutes: u64,
    cost: f64,
    budget_target: f64,
    budget_percentage: f64,
}

#[derive(Serialize)]
struct AnnualProjection {
    cost: f64,
    budget_target: f64,
}

#[derive(Serialize)]
pub struct CiCostReport {
    period_days: u64,
    start_date: String,
    repository: String,
    total_runs: u64,
    successful_runs: u64,
    failed_runs: u64,
    total_minutes: u64,
    total_cost: f64,
    monthly_projection: CostProjection,
    annual_projection: AnnualProjection,
    workflows: Vec<CiCostWorkflow>,
}

#[derive(Default)]
struct CostCounters {
    runs: u64,
    minutes: u64,
    successful_runs: u64,
    failed_runs: u64,
}

#[derive(Default)]
struct BaselineCounters {
    total_runs: u64,
    success_count: u64,
    failure_count: u64,
    skipped_count: u64,
    durations: Vec<u64>,
    billable_minutes: u64,
    name: String,
}

#[derive(Serialize)]
struct BaselineWorkflow {
    name: String,
    total_runs: u64,
    completed_runs: u64,
    success_count: u64,
    failure_count: u64,
    skipped_count: u64,
    success_rate_percent: f64,
    median_duration_seconds: u64,
    p95_duration_seconds: u64,
    avg_duration_seconds: u64,
    billable_minutes: u64,
    unique_failures: u64,
    unique_catch_rate_percent: f64,
    signal_per_dollar: f64,
}

#[derive(Serialize)]
struct BaselineSummary {
    total_runs: u64,
    total_billable_minutes: u64,
    overall_success_rate_percent: f64,
    total_unique_failures: u64,
    overall_signal_per_dollar: f64,
}

/// Envelope version stamped on every `ci_baseline.json` this module writes.
///
/// Umbrella #15990 Lane 0 ruling: `schema_version` is a `u32` integer and
/// consumers assert it fail-closed. The release-health consumer refuses any
/// other value (#15367), so bumping this constant requires updating every
/// consumer in the same change.
pub(crate) const SCHEMA_VERSION: u32 = 1;

/// `pub(crate)` so the release-health consumer's producer-conformance test
/// can serialize the producer's real type (#15369). Field privacy is
/// unchanged; consumers only ever see the serialized artifact.
#[derive(Serialize)]
pub(crate) struct BaselineReport {
    /// Envelope contract for on-disk consumers (#15367).
    schema_version: u32,
    generated_at: String,
    branch: String,
    days_analyzed: u64,
    /// Whether the fetched sample covers the whole requested window.
    ///
    /// `gh run list --limit N` returns the N most recent runs. When the
    /// fetch hits that cap while the requested window extends further back,
    /// the retained rows are a `partial_sample` of the period, not a
    /// complete baseline (#15377). Downstream consumers (release-health,
    /// policy thresholds) must not treat a partial sample as a full period.
    sample_completeness: SampleCompleteness,
    /// Raw rows returned by the fetch, before date filtering.
    fetched_runs: u64,
    /// Oldest/newest `createdAt` actually fetched; `None` when no row
    /// carried a parseable timestamp. Together they show the window the
    /// sample really covers.
    oldest_fetched_at: Option<String>,
    newest_fetched_at: Option<String>,
    workflows: BTreeMap<String, BaselineWorkflow>,
    summary: BaselineSummary,
}

/// Producer-conformance fixture (#15369): a fully populated
/// [`BaselineReport`] carrying every field of the on-disk `ci_baseline.json`
/// envelope, built from the producer's real types. The release-health
/// consumer serializes this and proves it still accepts the producer's whole
/// envelope, so a producer field rename or removal is caught by that test
/// instead of drifting away undetected.
#[cfg(test)]
pub(crate) fn baseline_report_fixture() -> BaselineReport {
    let mut workflows = BTreeMap::new();
    workflows.insert(
        "ci".to_string(),
        BaselineWorkflow {
            name: "ci".to_string(),
            total_runs: 42,
            completed_runs: 42,
            success_count: 40,
            failure_count: 2,
            skipped_count: 0,
            success_rate_percent: 95.2,
            median_duration_seconds: 310,
            p95_duration_seconds: 900,
            avg_duration_seconds: 420,
            billable_minutes: 137,
            unique_failures: 2,
            unique_catch_rate_percent: 100.0,
            signal_per_dollar: 1.5,
        },
    );
    BaselineReport {
        schema_version: SCHEMA_VERSION,
        generated_at: "2026-09-18T00:00:00+00:00".to_string(),
        branch: "main".to_string(),
        days_analyzed: 30,
        sample_completeness: SampleCompleteness::Complete,
        fetched_runs: 42,
        oldest_fetched_at: Some("2026-08-19T00:00:00+00:00".to_string()),
        newest_fetched_at: Some("2026-09-18T00:00:00+00:00".to_string()),
        workflows,
        summary: BaselineSummary {
            total_runs: 42,
            total_billable_minutes: 137,
            overall_success_rate_percent: 95.5,
            total_unique_failures: 2,
            overall_signal_per_dollar: 1.5,
        },
    }
}

/// Completeness of a baseline sample relative to its requested window.
///
/// Serialized as `complete` / `partial_sample` so JSON consumers can match
/// without tracking Rust variant renames.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum SampleCompleteness {
    Complete,
    PartialSample,
}

struct BaselineRun {
    workflow_key: String,
    conclusion: String,
    head_sha: Option<String>,
}

pub fn run_cost_monitor(days: u64, json_output: bool) -> Result<()> {
    let root = project_root()?;
    if days == 0 {
        bail!("--days must be greater than zero");
    }

    run_gh_auth_check(&root)?;
    let repo_info = parse_repo_info(&root)?;
    let repository = format!("{}/{}", repo_info.owner.login, repo_info.name);

    let now = Utc::now();
    let start_time = now - ChronoDuration::days(days as i64);
    let start_date = start_time.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let runs_json = run_gh_command(
        &root,
        "querying action runs",
        vec![
            "api".to_string(),
            format!("repos/{repository}/actions/runs"),
            "--paginate".to_string(),
            "-X".to_string(),
            "GET".to_string(),
            "-F".to_string(),
            format!("created=>{start_date}"),
            "-F".to_string(),
            "per_page=100".to_string(),
        ],
    )?;

    let mut workflow_stats: HashMap<String, CostCounters> = HashMap::new();
    let mut total_runs = 0_u64;
    let mut successful_runs = 0_u64;
    let mut failed_runs = 0_u64;
    let mut total_minutes = 0_u64;

    for page in serde_json::Deserializer::from_str(&runs_json).into_iter::<Value>() {
        let page = page.context("failed to parse gh api response page")?;

        let runs = if let Some(runs) = page.get("workflow_runs").and_then(Value::as_array) {
            runs
        } else if let Value::Array(values) = &page {
            values
        } else {
            continue;
        };

        for run in runs {
            let created = match read_timestamp(run, &["created_at", "createdAt"]) {
                Some(value) => value,
                None => continue,
            };
            if created < start_time {
                continue;
            }

            let start = read_timestamp(run, &["run_started_at", "runStartedAt"]).unwrap_or(created);
            let end = read_timestamp(run, &["updated_at", "updatedAt"])
                .or_else(|| read_timestamp(run, &["completed_at", "completedAt"]))
                .or_else(|| read_timestamp(run, &["finished_at", "finishedAt"]));

            let elapsed_seconds = end.and_then(|end_ts| {
                let elapsed = (end_ts - start).num_seconds();
                if elapsed > 0 { u64::try_from(elapsed).ok() } else { None }
            });

            let elapsed_seconds = elapsed_seconds.unwrap_or(0);
            let elapsed_minutes = elapsed_seconds.div_ceil(60);

            let workflow_name = run
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| run.get("workflow_name").and_then(Value::as_str))
                .or_else(|| run.get("workflowName").and_then(Value::as_str))
                .unwrap_or("(unknown workflow)")
                .to_string();

            let conclusion = run.get("conclusion").and_then(Value::as_str).unwrap_or("");
            if conclusion.is_empty() {
                continue;
            }
            if conclusion == "skipped" {
                continue;
            }

            let entry = workflow_stats.entry(workflow_name).or_default();
            entry.runs += 1;
            entry.minutes += elapsed_minutes;

            total_runs += 1;
            total_minutes += elapsed_minutes;

            if conclusion == "success" {
                entry.successful_runs += 1;
                successful_runs += 1;
            } else {
                entry.failed_runs += 1;
                failed_runs += 1;
            }
        }
    }

    if total_runs == 0 {
        if json_output {
            println!("{{\"error\": \"No workflow runs found\", \"period_days\": {days}}}");
        } else {
            println!("No workflow runs found in the last {days} days");
        }
        return Ok(());
    }

    let mut sorted_workflows: Vec<(String, CostCounters)> = workflow_stats.into_iter().collect();

    sorted_workflows.sort_by(|(name_a, counters_a), (name_b, counters_b)| {
        let cost_a = counters_a.minutes as f64 * COST_PER_MINUTE;
        let cost_b = counters_b.minutes as f64 * COST_PER_MINUTE;
        cost_b
            .partial_cmp(&cost_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| name_a.cmp(name_b))
    });

    let mut workflow_payloads = Vec::with_capacity(sorted_workflows.len());
    for (name, counters) in sorted_workflows {
        let average_minutes =
            if counters.runs > 0 { counters.minutes as f64 / counters.runs as f64 } else { 0.0 };

        workflow_payloads.push(CiCostWorkflow {
            name,
            runs: counters.runs,
            total_minutes: counters.minutes,
            average_minutes,
            cost: round_two_decimals(counters.minutes as f64 * COST_PER_MINUTE),
        });
    }

    let monthly_minutes = (total_minutes as f64) * 30.0 / (days as f64);
    let monthly_minutes =
        if monthly_minutes.is_sign_negative() { 0 } else { monthly_minutes.round() as u64 };
    let monthly_cost = round_two_decimals(monthly_minutes as f64 * COST_PER_MINUTE);
    let annual_cost = round_two_decimals(monthly_cost * 12.0);
    let total_cost = round_two_decimals(total_minutes as f64 * COST_PER_MINUTE);
    let budget_percentage = if MONTHLY_BUDGET_TARGET == 0.0 {
        0.0
    } else {
        (monthly_cost / MONTHLY_BUDGET_TARGET) * 100.0
    };

    let report = CiCostReport {
        period_days: days,
        start_date,
        repository,
        total_runs,
        successful_runs,
        failed_runs,
        total_minutes,
        total_cost,
        monthly_projection: CostProjection {
            minutes: monthly_minutes,
            cost: monthly_cost,
            budget_target: MONTHLY_BUDGET_TARGET,
            budget_percentage: round_one_decimal(budget_percentage),
        },
        annual_projection: AnnualProjection {
            cost: annual_cost,
            budget_target: ANNUAL_BUDGET_TARGET,
        },
        workflows: workflow_payloads,
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).context("failed to serialize cost report")?
        );
        return Ok(());
    }

    println!("===============================================================================");
    println!("                    CI Cost Analysis Report");
    println!("===============================================================================");
    println!("Period: Last {days} days (since {})", report.start_date);
    println!("Repository: {}", report.repository);
    println!("===============================================================================");
    println!("                             Summary");
    println!("===============================================================================");
    println!("{:<30} {:>10}", "Total workflow runs:", report.total_runs);
    println!(
        "{:<30} {:>10} ({:.1}%)",
        "  Successful:",
        report.successful_runs,
        percent(report.successful_runs, report.total_runs)
    );
    println!(
        "{:<30} {:>10} ({:.1}%)",
        "  Failed:",
        report.failed_runs,
        percent(report.failed_runs, report.total_runs)
    );
    println!("{:<30} {:>10} minutes", "Total CI time:", report.total_minutes);
    println!("{:<30} ${:.2}", "Total cost:", report.total_cost);

    println!("===============================================================================");
    println!("                     Monthly Projection");
    println!("===============================================================================");
    println!("{:<30} {:>10} minutes", "Estimated monthly usage:", monthly_minutes);
    println!("{:<30} ${:.2}", "Estimated monthly cost:", monthly_cost);
    println!("{:<30} ${:.0}", "Monthly budget target:", MONTHLY_BUDGET_TARGET);
    println!("{:<30} {:.1}%", "Budget utilization:", budget_percentage);
    if monthly_cost <= MONTHLY_BUDGET_TARGET {
        println!("Budget utilization: within budget");
    } else {
        println!("Budget utilization: over budget");
    }

    println!("===============================================================================");
    println!("                      Annual Projection");
    println!("===============================================================================");
    println!("{:<30} ${:.2}", "Estimated annual cost:", annual_cost);
    println!("{:<30} ${:.0}", "Annual budget target:", ANNUAL_BUDGET_TARGET);
    if annual_cost <= ANNUAL_BUDGET_TARGET {
        println!("Annual projection within budget");
    } else {
        let needed = round_two_decimals(annual_cost - ANNUAL_BUDGET_TARGET);
        println!("Annual projection exceeds budget by ${needed}");
    }

    println!("===============================================================================");
    println!("                   Per-Workflow Breakdown");
    println!("===============================================================================");
    println!(
        "{:<35} {:>8} {:>12} {:>12} {:>10}",
        "Workflow", "Runs", "Total Min", "Avg Min", "Cost"
    );
    println!("-------------------------------------------------------------------------------");
    for workflow in &report.workflows {
        println!(
            "{:<35} {:>8} {:>12} {:>12.1} ${:>9.2}",
            workflow.name,
            workflow.runs,
            workflow.total_minutes,
            workflow.average_minutes,
            workflow.cost
        );
    }

    println!("===============================================================================");
    println!("                        Recommendations");
    println!("===============================================================================");
    println!();

    if let Some(most_expensive) = report
        .workflows
        .iter()
        .max_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap_or(std::cmp::Ordering::Equal))
    {
        let share = if total_cost > 0.0 {
            round_one_decimal(most_expensive.cost * 100.0 / total_cost)
        } else {
            0.0
        };
        println!(
            "1. Most Expensive Workflow: '{}' costs ${:.2} ({share:.1}% of total)",
            most_expensive.name, most_expensive.cost
        );
    }

    if failed_runs > 0 {
        println!(
            "2. Failed Runs: {} failed runs ({:.1}% failure rate)",
            failed_runs,
            percent(failed_runs, report.total_runs)
        );
    }

    println!("3. Concurrency Cancellation:");
    println!("   Use cancel-in-progress for all scheduled workflows.");
    println!("4. Caching Strategy:");
    println!("   Add or tune caching for expensive build/test steps.");
    println!("5. Gating:");
    println!("   Gate expensive workflows behind labels or workflow_dispatch where possible.");

    Ok(())
}

pub fn run_ci_baseline(branch: String, days: u64, limit: usize, output_dir: PathBuf) -> Result<()> {
    let root = project_root()?;
    if days == 0 {
        bail!("--days must be greater than zero");
    }

    run_gh_auth_check(&root)?;

    // Resolve the branch: an empty CLI default is "use whatever the repo's
    // default branch is right now", so the tool doesn't silently emit a
    // baseline for a branch the repository does not have. This guards
    // against the long-standing `master` default that returned zero rows on
    // the `main` branch without recording why.
    let branch = if branch.is_empty() { resolve_default_branch(&root)? } else { branch };

    let runs_json = run_gh_command(
        &root,
        "listing workflow runs",
        vec![
            "run".to_string(),
            "list".to_string(),
            "--limit".to_string(),
            limit.to_string(),
            "--branch".to_string(),
            branch.clone(),
            "--json".to_string(),
            "name,conclusion,createdAt,updatedAt,databaseId,workflowName,status,startedAt,headSha"
                .to_string(),
        ],
    )?;

    let runs: Vec<Value> =
        serde_json::from_str(&runs_json).context("failed to parse JSON output from gh run list")?;

    let generated_at = Utc::now();
    let cutoff = generated_at - ChronoDuration::days(days as i64);
    let report = match build_baseline_report(&branch, days, generated_at, cutoff, limit, &runs) {
        Some(report) => report,
        None => {
            // Distinguish "no runs in the requested window" from
            // "branch does not exist in this repository" so a default
            // invocation that silently returns zero rows cannot leave a
            // stale baseline behind without explanation.
            if branch_exists(&root, &branch)? {
                // Legitimate no-data: the branch exists but has no runs in
                // the window. A previous successful run may have left
                // `ci_baseline.{json,md}` behind; retaining them would let a
                // stale baseline look current to file consumers
                // (release-health degrades an absent file to null, so
                // removal is the honest signal). The branch-missing arm
                // below stays untouched: it bails, and a failed invocation
                // must not delete evidence.
                let removed = clear_stale_baseline_outputs(&root, &output_dir)?;
                println!("No workflow runs found in requested period");
                for path in &removed {
                    println!("Removed stale baseline output: {}", path.display());
                }
            } else {
                bail!(
                    "branch '{branch}' was not found in this repository; \
                     supply --branch with a branch that exists or run without \
                     --branch to use the repository default"
                );
            }
            return Ok(());
        }
    };

    let output_dir = root.join(output_dir);
    fs::create_dir_all(&output_dir).context("failed to create output directory")?;

    let json_path = output_dir.join("ci_baseline.json");
    fs::write(
        &json_path,
        serde_json::to_string_pretty(&report).context("failed to serialize baseline report")?,
    )
    .with_context(|| format!("failed to write {}", json_path.display()))?;

    let md_path = output_dir.join("ci_baseline.md");
    let markdown = build_baseline_markdown(&report)?;
    fs::write(&md_path, markdown)
        .with_context(|| format!("failed to write {}", md_path.display()))?;

    println!();
    println!("======================================");
    println!("CI Baseline Summary");
    println!("======================================");
    println!("Branch:              {}", report.branch);
    println!("Analysis period:     Last {} days", report.days_analyzed);
    println!(
        "Sample:              {} (fetched {}, window {}..{})",
        match report.sample_completeness {
            SampleCompleteness::Complete => "complete",
            SampleCompleteness::PartialSample =>
                "PARTIAL SAMPLE - fetch cap hit before the window was covered",
        },
        report.fetched_runs,
        report.oldest_fetched_at.as_deref().unwrap_or("?"),
        report.newest_fetched_at.as_deref().unwrap_or("?"),
    );
    println!("Total runs:          {}", report.summary.total_runs);
    println!("Total billable:      {}m", report.summary.total_billable_minutes);
    println!("Overall success:     {:.1}%", report.summary.overall_success_rate_percent);
    println!("Output JSON:         {}", json_path.display());
    println!("Output markdown:     {}", md_path.display());
    println!("======================================");

    Ok(())
}

/// Remove stale `ci_baseline.{json,md}` outputs after a successful no-data
/// invocation, returning what was removed.
///
/// A no-data run writes nothing; without this, a previous successful run's
/// files would remain on disk and file consumers would present them as the
/// current baseline (#15377). Only the two exact baseline filenames are ever
/// removed, and only when they are plain files: anything else in the output
/// directory (including an absent directory itself) is left alone.
fn clear_stale_baseline_outputs(root: &Path, output_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for file in ["ci_baseline.json", "ci_baseline.md"] {
        let path = root.join(output_dir).join(file);
        if path.is_file() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove stale {}", path.display()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

fn build_baseline_report(
    branch: &str,
    days: u64,
    generated_at: DateTime<Utc>,
    cutoff: DateTime<Utc>,
    limit: usize,
    runs: &[Value],
) -> Option<BaselineReport> {
    let mut workflow_counters: BTreeMap<String, BaselineCounters> = BTreeMap::new();
    let mut baseline_runs: Vec<BaselineRun> = Vec::new();

    // The fetch returns most-recent-first up to `limit` rows. Record the
    // fetched span before date filtering: when the cap is hit while the
    // requested window reaches further back, the retained rows cannot stand
    // in for the whole period (#15377).
    let fetched_runs = runs.len();
    let mut oldest_fetched: Option<DateTime<Utc>> = None;
    let mut newest_fetched: Option<DateTime<Utc>> = None;
    for run in runs {
        if let Some(created) = read_timestamp(run, &["createdAt", "created_at"]) {
            oldest_fetched = Some(oldest_fetched.map_or(created, |oldest| oldest.min(created)));
            newest_fetched = Some(newest_fetched.map_or(created, |newest| newest.max(created)));
        }
    }
    let cap_hit = limit > 0 && fetched_runs >= limit;
    let sample_completeness = match (cap_hit, oldest_fetched) {
        // Fewer rows than the cap: the API returned everything available,
        // so whatever the date filter keeps is the complete picture.
        (false, _) => SampleCompleteness::Complete,
        // Cap hit but the fetched span already reaches past the window edge:
        // the cap did not cut off any in-window row.
        (true, Some(oldest)) if oldest < cutoff => SampleCompleteness::Complete,
        // Cap hit and every fetched row is inside the window: older
        // in-window rows exist that we never saw.
        (true, _) => SampleCompleteness::PartialSample,
    };

    for run in runs {
        let created = match read_timestamp(run, &["createdAt", "created_at"]) {
            Some(value) => value,
            None => continue,
        };
        if created < cutoff {
            continue;
        }

        let workflow_name = run
            .get("workflowName")
            .and_then(Value::as_str)
            .or_else(|| run.get("name").and_then(Value::as_str))
            .unwrap_or("(unknown workflow)");

        let conclusion = run.get("conclusion").and_then(Value::as_str).unwrap_or("");
        if conclusion.is_empty() {
            continue;
        }

        let mut duration_seconds = 0_u64;
        let start = read_timestamp(run, &["startedAt", "runStartedAt", "run_started_at"])
            .or_else(|| read_timestamp(run, &["createdAt", "created_at"]));
        let end = read_timestamp(run, &["updatedAt", "updated_at"]);
        if let (Some(start_ts), Some(end_ts)) = (start, end) {
            let elapsed = (end_ts - start_ts).num_seconds();
            if elapsed > 0 {
                duration_seconds = u64::try_from(elapsed).unwrap_or_default();
            }
        }

        let key = workflow_key(workflow_name);
        let counters = workflow_counters.entry(key.clone()).or_default();
        counters.name = workflow_name.to_string();
        counters.total_runs += 1;

        let head_sha = run.get("headSha").and_then(Value::as_str).map(str::to_string);
        baseline_runs.push(BaselineRun {
            workflow_key: key.clone(),
            conclusion: conclusion.to_string(),
            head_sha,
        });

        match conclusion {
            "success" => counters.success_count += 1,
            "skipped" => counters.skipped_count += 1,
            _ => counters.failure_count += 1,
        }

        if conclusion == "skipped" {
            continue;
        }

        if duration_seconds > 0 {
            counters.durations.push(duration_seconds);
            counters.billable_minutes += duration_seconds.div_ceil(60);
        }
    }

    if workflow_counters.is_empty() {
        return None;
    }

    // Unique-catch credit requires the complete sibling set for each head
    // SHA (#15377): on a truncated fetch a workflow can look like the only
    // failing lane only because its siblings were never fetched ("missing
    // sibling creates false unique catch"). A partial sample therefore
    // emits no unique-catch credit at all; the existing complete-sample
    // test below pins that the credit survives when the window is covered.
    let unique_failure_counts = if sample_completeness == SampleCompleteness::Complete {
        compute_unique_failures(&baseline_runs)
    } else {
        BTreeMap::new()
    };

    let mut workflow_reports = BTreeMap::new();
    for (key, counters) in workflow_counters {
        let mut durations = counters.durations.clone();
        durations.sort_unstable();

        let median_seconds = percentile(&durations, 50.0);
        let p95_seconds = percentile(&durations, 95.0);
        let avg_seconds = if durations.is_empty() {
            0
        } else {
            let sum: u64 = durations.iter().sum();
            sum / u64::try_from(durations.len()).unwrap_or(1)
        };

        let completed_runs = counters.success_count + counters.failure_count;
        let success_rate_percent = if completed_runs > 0 {
            (counters.success_count as f64 * 100.0) / (completed_runs as f64)
        } else {
            0.0
        };
        let unique_failures = unique_failure_counts.get(&key).copied().unwrap_or(0);
        let unique_catch_rate_percent = if counters.failure_count > 0 {
            (unique_failures as f64 * 100.0) / (counters.failure_count as f64)
        } else {
            0.0
        };
        let estimated_cost = counters.billable_minutes as f64 * COST_PER_MINUTE;
        let signal_per_dollar =
            if estimated_cost > 0.0 { unique_failures as f64 / estimated_cost } else { 0.0 };

        workflow_reports.insert(
            key,
            BaselineWorkflow {
                name: counters.name,
                total_runs: counters.total_runs,
                completed_runs,
                success_count: counters.success_count,
                failure_count: counters.failure_count,
                skipped_count: counters.skipped_count,
                success_rate_percent,
                median_duration_seconds: median_seconds,
                p95_duration_seconds: p95_seconds,
                avg_duration_seconds: avg_seconds,
                billable_minutes: counters.billable_minutes,
                unique_failures,
                unique_catch_rate_percent,
                signal_per_dollar,
            },
        );
    }

    let total_runs: u64 = workflow_reports.values().map(|workflow| workflow.total_runs).sum();

    let total_billable: u64 =
        workflow_reports.values().map(|workflow| workflow.billable_minutes).sum();

    let total_success: u64 = workflow_reports.values().map(|workflow| workflow.success_count).sum();
    let total_completed: u64 = workflow_reports
        .values()
        .map(|workflow| workflow.success_count + workflow.failure_count)
        .sum();

    let overall_success_rate_percent = if total_completed > 0 {
        (total_success as f64 * 100.0) / (total_completed as f64)
    } else {
        0.0
    };
    let total_unique_failures: u64 =
        workflow_reports.values().map(|workflow| workflow.unique_failures).sum();
    let total_cost = total_billable as f64 * COST_PER_MINUTE;
    let overall_signal_per_dollar =
        if total_cost > 0.0 { total_unique_failures as f64 / total_cost } else { 0.0 };

    Some(BaselineReport {
        schema_version: SCHEMA_VERSION,
        generated_at: generated_at.to_rfc3339(),
        branch: branch.to_string(),
        days_analyzed: days,
        sample_completeness,
        fetched_runs: u64::try_from(fetched_runs).unwrap_or(u64::MAX),
        oldest_fetched_at: oldest_fetched.map(|ts| ts.to_rfc3339()),
        newest_fetched_at: newest_fetched.map(|ts| ts.to_rfc3339()),
        workflows: workflow_reports,
        summary: BaselineSummary {
            total_runs,
            total_billable_minutes: total_billable,
            overall_success_rate_percent,
            total_unique_failures,
            overall_signal_per_dollar,
        },
    })
}

fn compute_unique_failures(runs: &[BaselineRun]) -> BTreeMap<String, u64> {
    let mut failing_lanes_by_sha: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for run in runs {
        if run.conclusion == "failure"
            && let Some(sha) = run.head_sha.as_deref()
        {
            failing_lanes_by_sha.entry(sha).or_default().push(run.workflow_key.as_str());
        }
    }

    let mut unique_counts: BTreeMap<String, u64> = BTreeMap::new();
    for failures in failing_lanes_by_sha.values() {
        if failures.len() == 1 {
            let lane = failures[0].to_string();
            *unique_counts.entry(lane).or_default() += 1;
        }
    }
    unique_counts
}

fn run_gh_auth_check(root: &Path) -> Result<()> {
    let status = Command::new("gh")
        .current_dir(root)
        .args(["auth", "status"])
        .status()
        .context("failed to run gh auth status")?;

    if !status.success() {
        bail!("gh CLI is not authenticated. Run 'gh auth login'.");
    }

    Ok(())
}

fn parse_repo_info(root: &Path) -> Result<RepoInfo> {
    let repo_json = run_gh_command(
        root,
        "reading repository info",
        vec![
            "repo".to_string(),
            "view".to_string(),
            "--json".to_string(),
            "owner,name".to_string(),
        ],
    )?;

    serde_json::from_str(&repo_json).context("failed to parse gh repo view JSON")
}

fn run_gh_command(root: &Path, action: &str, args: Vec<String>) -> Result<String> {
    let output = Command::new("gh")
        .current_dir(root)
        .args(&args)
        .output()
        .with_context(|| format!("failed to execute gh command while {action}"))?;

    if !output.status.success() {
        bail!("gh command failed while {action}: {}", String::from_utf8_lossy(&output.stderr));
    }

    String::from_utf8(output.stdout).context("gh output was not valid UTF-8")
}

/// Resolve the repository's current default branch via `gh repo view`.
///
/// We avoid hardcoding any branch name (the previous default of `master`
/// silently returned zero rows once the repository moved to `main`).
/// Errors from `gh` are surfaced verbatim so the caller sees why the
/// resolution failed.
fn resolve_default_branch(root: &Path) -> Result<String> {
    let raw = run_gh_command(
        root,
        "resolving repository default branch",
        vec![
            "repo".to_string(),
            "view".to_string(),
            "--json".to_string(),
            "defaultBranchRef".to_string(),
        ],
    )?;

    parse_default_branch(&raw)
}

/// Parse the JSON envelope returned by `gh repo view --json defaultBranchRef`.
///
/// Extracted as a pure helper so the parsing path can be exercised without
/// invoking the `gh` CLI. Returns an error when `defaultBranchRef` is absent
/// (forks, archived repositories, or older `gh` versions may omit it).
fn parse_default_branch(raw: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct DefaultBranchRef {
        name: String,
    }
    #[derive(Deserialize)]
    struct DefaultBranchEnvelope {
        #[serde(rename = "defaultBranchRef")]
        default_branch_ref: Option<DefaultBranchRef>,
    }

    let envelope: DefaultBranchEnvelope =
        serde_json::from_str(raw).context("failed to parse gh repo view defaultBranchRef")?;

    envelope
        .default_branch_ref
        .map(|r| r.name)
        .ok_or_else(|| color_eyre::eyre::eyre!("gh repo view did not return a defaultBranchRef"))
}

/// Test whether a branch ref exists in the current repository.
///
/// Uses the GitHub `GET /repos/{owner}/{repo}/branches/{branch}` API via
/// `gh api`. A non-existent branch returns HTTP 404, which we report as
/// `Ok(false)` rather than a `gh` error so the caller can produce a clean
/// "branch not found" diagnostic.
fn branch_exists(root: &Path, branch: &str) -> Result<bool> {
    let repo = parse_repo_info(root)?;
    let endpoint = format!("repos/{}/{}/branches/{}", repo.owner.login, repo.name, branch,);

    let status = Command::new("gh")
        .current_dir(root)
        .args(["api", "--method", "GET", &endpoint])
        .output()
        .with_context(|| format!("failed to query branch existence for {branch}"))?;

    Ok(status.status.success())
}

fn read_timestamp(run: &Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    for key in keys {
        let value = match run.get(*key).and_then(Value::as_str) {
            Some(value) => value,
            None => continue,
        };
        if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
            return Some(timestamp.with_timezone(&Utc));
        }
    }
    None
}

fn round_two_decimals(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn round_one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn percent(part: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { round_one_decimal((part as f64 * 100.0) / (total as f64)) }
}

fn percentile(values: &[u64], percentile: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }

    let index = (((values.len() - 1) as f64) * (percentile / 100.0)).floor() as usize;
    values[index]
}

fn workflow_key(name: &str) -> String {
    let mut key = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            key.push(ch);
        } else if ch == ' ' {
            key.push('_');
        }
    }

    if key.is_empty() { "workflow".to_string() } else { key }
}

fn build_baseline_markdown(report: &BaselineReport) -> Result<String> {
    let mut out = String::new();
    out.push_str("# CI Baseline Metrics Report\n\n");
    out.push_str(&format!("**Generated:** {}\n", report.generated_at.replace('T', " ")));
    out.push_str(&format!("**Branch:** {}\n", report.branch));
    out.push_str(&format!("**Analysis Period:** Last {} days\n", report.days_analyzed));
    out.push_str(&format!(
        "**Sample:** {} (fetched {}, window {}..{})\n\n",
        match report.sample_completeness {
            SampleCompleteness::Complete => "complete",
            SampleCompleteness::PartialSample =>
                "PARTIAL SAMPLE - fetch cap hit before the window was covered; do not use as a full-period baseline",
        },
        report.fetched_runs,
        report.oldest_fetched_at.as_deref().unwrap_or("?"),
        report.newest_fetched_at.as_deref().unwrap_or("?"),
    ));

    out.push_str("## Summary\n\n");
    out.push_str("| Metric | Value |\n|--------|-------|\n");
    out.push_str(&format!("| Total Runs | {} |\n", report.summary.total_runs));
    out.push_str(&format!(
        "| Overall Success Rate | {:.1}% |\n",
        report.summary.overall_success_rate_percent
    ));
    out.push_str(&format!(
        "| Total Billable Minutes | {}m |\n\n",
        report.summary.total_billable_minutes
    ));
    out.push_str(&format!(
        "| Total Unique Failures | {} |\n",
        report.summary.total_unique_failures
    ));
    out.push_str(&format!(
        "| Overall Signal per $ | {:.2} unique catches/$ |\n\n",
        report.summary.overall_signal_per_dollar
    ));

    out.push_str("## Workflow Details\n\n");
    out.push_str(
        "| Workflow | Runs | Success Rate | Median | P95 | Billable | Unique Catches | Unique Catch Rate | Signal/$ |\n",
    );
    out.push_str(
        "|----------|------|--------------|--------|-----|----------|----------------|-------------------|----------|\n",
    );

    for workflow in report.workflows.values() {
        out.push_str(&format!(
            "| {} | {} | {:.1}% | {}s | {}s | {}m | {} | {:.1}% | {:.2} |\n",
            workflow.name,
            workflow.total_runs,
            workflow.success_rate_percent,
            workflow.median_duration_seconds,
            workflow.p95_duration_seconds,
            workflow.billable_minutes,
            workflow.unique_failures,
            workflow.unique_catch_rate_percent,
            workflow.signal_per_dollar
        ));
    }

    out.push_str("\n## Notes\n\n");
    out.push_str("- Median Duration: 50th percentile of run duration (in seconds)\n");
    out.push_str("- P95 Duration: 95th percentile of run duration (in seconds)\n");
    out.push_str(
        "- Billable Minutes: Estimated billable time (each run rounded up to nearest minute)\n",
    );
    out.push_str("- Success Rate: Calculated excluding skipped runs\n\n");
    out.push_str("- Unique Catches: Failures where this workflow was the only failing lane on a commit SHA\n");
    out.push_str(
        "- Signal/$: Unique catches divided by estimated workflow cost (minutes × $0.008)\n\n",
    );

    out.push_str("## Recommendations\n\n");
    out.push_str("1. Monitor P95 durations for workflow variance.\n");
    out.push_str("2. Track unique catch rate by lane; demote lanes with sustained near-zero unique catches.\n");
    out.push_str("3. Prioritize lanes with highest signal-per-dollar and trim low-yield expensive lanes.\n\n");
    out.push_str("---\nGenerated by cargo xtask ci-baseline\n");

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::eyre;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn read_timestamp_uses_fallback_keys() -> Result<()> {
        let run = json!({
            "createdAt": "2026-03-25T12:00:00Z"
        });

        let timestamp = read_timestamp(&run, &["created_at", "createdAt"])
            .ok_or_else(|| eyre!("expected timestamp"))?;

        assert_eq!(
            timestamp,
            DateTime::parse_from_rfc3339("2026-03-25T12:00:00Z")?.with_timezone(&Utc)
        );
        Ok(())
    }

    #[test]
    fn baseline_report_keeps_zero_minute_runs_and_excludes_skips_from_success_rate() -> Result<()> {
        let generated_at =
            DateTime::parse_from_rfc3339("2026-03-25T12:00:00Z")?.with_timezone(&Utc);
        let cutoff = DateTime::parse_from_rfc3339("2026-03-24T12:00:00Z")?.with_timezone(&Utc);
        let runs = vec![
            json!({
                "workflowName": "CI",
                "conclusion": "success",
                "createdAt": "2026-03-25T11:00:00Z",
                "startedAt": "2026-03-25T11:00:00Z",
                "updatedAt": "2026-03-25T11:01:30Z"
            }),
            json!({
                "workflowName": "CI",
                "conclusion": "skipped",
                "createdAt": "2026-03-25T10:00:00Z",
                "startedAt": "2026-03-25T10:00:00Z",
                "updatedAt": "2026-03-25T10:00:30Z"
            }),
            json!({
                "workflowName": "CI",
                "conclusion": "failure",
                "createdAt": "2026-03-25T09:00:00Z",
                "startedAt": "2026-03-25T09:00:00Z"
            }),
        ];

        let report = build_baseline_report("master", 1, generated_at, cutoff, 200, &runs)
            .ok_or_else(|| eyre!("expected baseline report"))?;
        let workflow =
            report.workflows.get("CI").ok_or_else(|| eyre!("expected workflow report"))?;

        assert_eq!(workflow.total_runs, 3);
        assert_eq!(workflow.completed_runs, 2);
        assert_eq!(workflow.success_count, 1);
        assert_eq!(workflow.failure_count, 1);
        assert_eq!(workflow.skipped_count, 1);
        assert_eq!(workflow.billable_minutes, 2);
        assert_eq!(workflow.unique_failures, 0);
        assert_eq!(report.summary.total_runs, 3);
        assert_eq!(report.summary.total_billable_minutes, 2);
        assert_eq!(report.summary.overall_success_rate_percent, 50.0);

        let markdown = build_baseline_markdown(&report)?;
        assert!(markdown.contains("| CI | 3 | 50.0% | 90s | 90s | 2m | 0 | 0.0% | 0.00 |"));

        Ok(())
    }

    #[test]
    fn baseline_report_tracks_unique_catches_per_sha() -> Result<()> {
        let generated_at =
            DateTime::parse_from_rfc3339("2026-03-25T12:00:00Z")?.with_timezone(&Utc);
        let cutoff = DateTime::parse_from_rfc3339("2026-03-24T12:00:00Z")?.with_timezone(&Utc);
        let runs = vec![
            json!({
                "workflowName": "CI",
                "conclusion": "failure",
                "createdAt": "2026-03-25T11:00:00Z",
                "headSha": "sha-a",
                "startedAt": "2026-03-25T11:00:00Z",
                "updatedAt": "2026-03-25T11:01:00Z"
            }),
            json!({
                "workflowName": "Lint",
                "conclusion": "success",
                "createdAt": "2026-03-25T11:00:00Z",
                "headSha": "sha-a",
                "startedAt": "2026-03-25T11:00:00Z",
                "updatedAt": "2026-03-25T11:01:00Z"
            }),
            json!({
                "workflowName": "CI",
                "conclusion": "failure",
                "createdAt": "2026-03-25T10:00:00Z",
                "headSha": "sha-b",
                "startedAt": "2026-03-25T10:00:00Z",
                "updatedAt": "2026-03-25T10:01:00Z"
            }),
            json!({
                "workflowName": "Lint",
                "conclusion": "failure",
                "createdAt": "2026-03-25T10:00:00Z",
                "headSha": "sha-b",
                "startedAt": "2026-03-25T10:00:00Z",
                "updatedAt": "2026-03-25T10:01:00Z"
            }),
        ];

        let report = build_baseline_report("master", 1, generated_at, cutoff, 200, &runs)
            .ok_or_else(|| eyre!("expected baseline report"))?;
        let ci = report.workflows.get("CI").ok_or_else(|| eyre!("expected CI workflow"))?;
        let lint = report.workflows.get("Lint").ok_or_else(|| eyre!("expected Lint workflow"))?;

        assert_eq!(ci.failure_count, 2);
        assert_eq!(ci.unique_failures, 1);
        assert_eq!(ci.unique_catch_rate_percent, 50.0);
        assert_eq!(lint.failure_count, 1);
        assert_eq!(lint.unique_failures, 0);
        assert_eq!(report.summary.total_unique_failures, 1);

        Ok(())
    }

    /// `parse_default_branch` must extract the `name` field from a real
    /// `gh repo view --json defaultBranchRef` payload, including the
    /// `main` shape that the previous hard-coded `master` default
    /// silently missed.
    #[test]
    fn parse_default_branch_extracts_main() -> Result<()> {
        let raw = r#"{"defaultBranchRef":{"name":"main"}}"#;
        assert_eq!(parse_default_branch(raw)?, "main");
        Ok(())
    }

    /// A payload that does not carry `defaultBranchRef` (archived forks,
    /// older `gh` versions) must surface a clear error rather than
    /// returning an empty string that the caller would forward as a
    /// branch filter.
    #[test]
    fn parse_default_branch_errors_when_default_branch_ref_absent() {
        let raw = r#"{}"#;
        let result = parse_default_branch(raw);
        assert!(result.is_err(), "expected an error when defaultBranchRef is missing");
    }

    /// A malformed payload (the kind a transient `gh` failure produces)
    /// must surface a parse error rather than silently substituting an
    /// empty branch.
    #[test]
    fn parse_default_branch_errors_on_garbage_input() {
        let raw = "this is not json";
        let result = parse_default_branch(raw);
        assert!(result.is_err(), "expected an error when payload is not valid JSON");
    }

    /// `build_baseline_report` must accept an empty run slice without
    /// panicking: the wrong-branch diagnostic is delivered by the caller
    /// after this returns `None`. A panic here would mask the
    /// `gh run list --branch foo` zero-row case that the issue cites.
    #[test]
    fn build_baseline_report_returns_none_for_empty_runs() -> Result<()> {
        let generated_at =
            DateTime::parse_from_rfc3339("2026-03-25T12:00:00Z")?.with_timezone(&Utc);
        let cutoff = DateTime::parse_from_rfc3339("2026-03-24T12:00:00Z")?.with_timezone(&Utc);
        let runs: Vec<Value> = Vec::new();

        let report = build_baseline_report("main", 1, generated_at, cutoff, 200, &runs);
        assert!(report.is_none(), "expected no report when zero rows are fetched");

        Ok(())
    }

    fn truncated_window_runs() -> Vec<Value> {
        // Two rows, both inside the requested window: with `--limit 2` the
        // fetch hit the cap while the window reaches further back, so older
        // in-window rows exist that were never fetched.
        vec![
            json!({
                "workflowName": "CI",
                "conclusion": "success",
                "createdAt": "2026-03-25T11:00:00Z",
                "startedAt": "2026-03-25T11:00:00Z",
                "updatedAt": "2026-03-25T11:01:00Z"
            }),
            json!({
                "workflowName": "CI",
                "conclusion": "success",
                "createdAt": "2026-03-25T10:00:00Z",
                "startedAt": "2026-03-25T10:00:00Z",
                "updatedAt": "2026-03-25T10:01:00Z"
            }),
        ]
    }

    /// 201+ runs inside the period cannot appear as a complete 30-day
    /// 200-run baseline (#15377): when the fetch hits `--limit` while the
    /// window extends past the oldest fetched row, the report must say
    /// `partial_sample` everywhere the sample is presented.
    #[test]
    fn baseline_report_marks_truncated_fetch_as_partial_sample() -> Result<()> {
        let generated_at =
            DateTime::parse_from_rfc3339("2026-03-25T12:00:00Z")?.with_timezone(&Utc);
        let cutoff = DateTime::parse_from_rfc3339("2026-03-24T12:00:00Z")?.with_timezone(&Utc);

        let report =
            build_baseline_report("main", 1, generated_at, cutoff, 2, &truncated_window_runs())
                .ok_or_else(|| eyre!("expected baseline report"))?;
        assert_eq!(report.sample_completeness, SampleCompleteness::PartialSample);
        assert_eq!(report.fetched_runs, 2);
        assert_eq!(report.oldest_fetched_at.as_deref(), Some("2026-03-25T10:00:00+00:00"));
        assert_eq!(report.newest_fetched_at.as_deref(), Some("2026-03-25T11:00:00+00:00"));

        let markdown = build_baseline_markdown(&report)?;
        assert!(
            markdown.contains("PARTIAL SAMPLE"),
            "markdown must carry the partial-sample warning, got:\n{markdown}"
        );
        assert!(
            markdown.contains("do not use as a full-period baseline"),
            "markdown must state the consumption limit, got:\n{markdown}"
        );

        Ok(())
    }

    /// Hitting the cap is not itself truncation: when the fetched span
    /// already reaches past the window edge, no in-window row was cut off.
    /// (The 10:00 row falls outside the window so only the 11:00 row is
    /// retained, but the fetched span proves the window is covered.)
    #[test]
    fn baseline_report_marks_cap_covered_window_complete() -> Result<()> {
        let generated_at =
            DateTime::parse_from_rfc3339("2026-03-25T12:00:00Z")?.with_timezone(&Utc);
        let cutoff = DateTime::parse_from_rfc3339("2026-03-25T10:30:00Z")?.with_timezone(&Utc);

        let report =
            build_baseline_report("main", 1, generated_at, cutoff, 2, &truncated_window_runs())
                .ok_or_else(|| eyre!("expected baseline report"))?;
        assert_eq!(report.sample_completeness, SampleCompleteness::Complete);

        Ok(())
    }

    /// The producer stamps the envelope (#15367): serialized
    /// `ci_baseline.json` carries `schema_version: 1`, which release-health
    /// asserts fail-closed. Keep the stamp, the constant, and the consumer
    /// assert in lockstep.
    #[test]
    fn baseline_report_stamps_schema_version_for_consumers() -> Result<()> {
        let generated_at =
            DateTime::parse_from_rfc3339("2026-03-25T12:00:00Z")?.with_timezone(&Utc);
        let cutoff = DateTime::parse_from_rfc3339("2026-03-25T10:30:00Z")?.with_timezone(&Utc);

        let report =
            build_baseline_report("main", 1, generated_at, cutoff, 2, &truncated_window_runs())
                .ok_or_else(|| eyre!("expected baseline report"))?;
        assert_eq!(report.schema_version, SCHEMA_VERSION);

        let json = serde_json::to_string(&report)?;
        let parsed: serde_json::Value = serde_json::from_str(&json)?;
        assert_eq!(parsed["schema_version"], 1);
        Ok(())
    }

    /// Fewer rows than the cap means the API returned everything available:
    /// the sample is complete even though the same rows would be partial
    /// under a tighter limit.
    #[test]
    fn baseline_report_marks_under_cap_fetch_complete() -> Result<()> {
        let generated_at =
            DateTime::parse_from_rfc3339("2026-03-25T12:00:00Z")?.with_timezone(&Utc);
        let cutoff = DateTime::parse_from_rfc3339("2026-03-24T12:00:00Z")?.with_timezone(&Utc);

        let report =
            build_baseline_report("main", 1, generated_at, cutoff, 200, &truncated_window_runs())
                .ok_or_else(|| eyre!("expected baseline report"))?;
        assert_eq!(report.sample_completeness, SampleCompleteness::Complete);

        Ok(())
    }

    /// A successful no-data run must not leave an older baseline looking
    /// current (#15377): both baseline outputs are removed when present, an
    /// unrelated file in the same directory survives, and the removed paths
    /// are reported back.
    #[test]
    fn clear_stale_baseline_outputs_removes_only_baseline_files() -> Result<()> {
        let tmp = TempDir::new()?;
        let out = tmp.path().join("target").join("metrics");
        fs::create_dir_all(&out)?;
        fs::write(out.join("ci_baseline.json"), r#"{"stale": true}"#)?;
        fs::write(out.join("ci_baseline.md"), "# stale")?;
        fs::write(out.join("other-artifact.json"), "{}")?;

        let removed = clear_stale_baseline_outputs(tmp.path(), Path::new("target/metrics"))?;
        assert_eq!(removed.len(), 2);
        assert!(!out.join("ci_baseline.json").exists());
        assert!(!out.join("ci_baseline.md").exists());
        assert!(
            out.join("other-artifact.json").exists(),
            "unrelated files in the output directory must survive"
        );

        Ok(())
    }

    /// Clearing a directory that holds no baseline files (or no directory
    /// at all, e.g. a fresh clone) is a no-op success, not an error.
    #[test]
    fn clear_stale_baseline_outputs_tolerates_absence() -> Result<()> {
        let tmp = TempDir::new()?;
        let removed = clear_stale_baseline_outputs(tmp.path(), Path::new("target/metrics"))?;
        assert!(removed.is_empty());

        let out = tmp.path().join("target").join("metrics");
        fs::create_dir_all(&out)?;
        fs::write(out.join("other-artifact.json"), "{}")?;
        let removed = clear_stale_baseline_outputs(tmp.path(), Path::new("target/metrics"))?;
        assert!(removed.is_empty());

        Ok(())
    }

    /// Cost-per-unique-catch is emitted only when both numerator and
    /// denominator are complete (#15377): on a truncated fetch the sibling
    /// set per head SHA is incomplete, so a workflow that looks like the
    /// only failing lane may simply have lost its siblings to the cap.
    /// The same shape under a covering limit keeps its credit
    /// (`baseline_report_tracks_unique_catches_per_sha`).
    #[test]
    fn partial_sample_suppresses_unique_catch_credit() -> Result<()> {
        let generated_at =
            DateTime::parse_from_rfc3339("2026-03-25T12:00:00Z")?.with_timezone(&Utc);
        let cutoff = DateTime::parse_from_rfc3339("2026-03-24T12:00:00Z")?.with_timezone(&Utc);
        // Same failure shape as the complete-sample unique-catch test, but
        // fetched at the cap with the window reaching further back.
        let runs = vec![
            json!({
                "workflowName": "CI",
                "conclusion": "failure",
                "createdAt": "2026-03-25T11:00:00Z",
                "headSha": "sha-a",
                "startedAt": "2026-03-25T11:00:00Z",
                "updatedAt": "2026-03-25T11:01:00Z"
            }),
            json!({
                "workflowName": "Lint",
                "conclusion": "success",
                "createdAt": "2026-03-25T11:00:00Z",
                "headSha": "sha-a",
                "startedAt": "2026-03-25T11:00:00Z",
                "updatedAt": "2026-03-25T11:01:00Z"
            }),
        ];

        let report = build_baseline_report("main", 1, generated_at, cutoff, 2, &runs)
            .ok_or_else(|| eyre!("expected baseline report"))?;
        assert_eq!(report.sample_completeness, SampleCompleteness::PartialSample);
        let ci = report.workflows.get("CI").ok_or_else(|| eyre!("expected CI workflow"))?;
        assert_eq!(
            ci.unique_failures, 0,
            "a lone failure on a truncated fetch must not earn unique-catch credit"
        );
        assert_eq!(ci.signal_per_dollar, 0.0);
        assert_eq!(report.summary.total_unique_failures, 0);
        assert_eq!(report.summary.overall_signal_per_dollar, 0.0);

        Ok(())
    }
}
