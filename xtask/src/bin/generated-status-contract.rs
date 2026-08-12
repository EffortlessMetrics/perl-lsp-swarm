//! Build sibling Markdown and JSON status projections from typed source evidence.
#![allow(clippy::print_stderr, clippy::print_stdout)]
use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(name = "generated-status-contract")]
struct Args {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long, default_value = "policy/generated-status-evidence.json")]
    evidence: PathBuf,
    #[arg(long, default_value = "docs/project/status/generated-status-cells.json")]
    json: PathBuf,
    #[arg(long, default_value = "docs/project/status/generated-status-cells.md")]
    markdown: PathBuf,
    #[arg(long, conflicts_with = "check")]
    write: bool,
    #[arg(long)]
    check: bool,

    /// Reject evidence older than this many hours.
    #[arg(long, default_value_t = 168)]
    max_age_hours: i64,
}
#[derive(Debug, Clone, Deserialize)]
struct EvidenceBundle {
    schema_version: String,
    subject_sha: String,
    run_id: String,
    observed_at: String,
    cells: Vec<SourceCell>,
}
#[derive(Debug, Clone, Deserialize)]
struct SourceCell {
    id: String,
    claim_boundary: String,
    expected_population: Vec<String>,
    results: Vec<ResultRow>,
}
#[derive(Debug, Clone, Deserialize)]
struct ResultRow {
    identity: String,
    subject_sha: String,
    run_id: String,
    observed_at: String,
    outcome: Outcome,
}
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Pass,
    Fail,
    Skip,
    NotRun,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Projection {
    schema_version: String,
    source: SourceIdentity,
    cells: Vec<StatusCell>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SourceIdentity {
    subject_sha: String,
    run_id: String,
    observed_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StatusCell {
    id: String,
    subject_sha: String,
    run_id: String,
    observed_at: String,
    expected: u64,
    passed: u64,
    failed: u64,
    skipped: u64,
    not_run: u64,
    completeness: String,
    verdict: String,
    claim_boundary: String,
    limitations: Vec<String>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let a = Args::parse();
    let root = a.root.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."));
    let raw =
        fs::read_to_string(root.join(&a.evidence)).context("reading typed source evidence")?;
    let evidence: EvidenceBundle =
        serde_json::from_str(&raw).context("parsing typed source evidence")?;
    validate_freshness(&evidence.observed_at, a.max_age_hours, chrono::Utc::now())?;
    let projection = construct(&evidence)?;
    let json = format!("{}\n", serde_json::to_string_pretty(&projection)?);
    let md = render_markdown(&projection);
    if a.write {
        atomic_write(&root.join(&a.json), &json)?;
        atomic_write(&root.join(&a.markdown), &md)?;
    } else {
        check_file(&root.join(&a.json), &json)?;
        check_file(&root.join(&a.markdown), &md)?;
    }
    println!("generated status projections are current ({} cells)", projection.cells.len());
    Ok(())
}

fn construct(e: &EvidenceBundle) -> Result<Projection> {
    if e.schema_version != "generated_status_evidence.v1" {
        bail!("unsupported evidence schema")
    }
    validate_identity(&e.subject_sha, &e.run_id, &e.observed_at)?;
    let mut ids = BTreeSet::new();
    let mut cells = Vec::new();
    for c in &e.cells {
        if !ids.insert(&c.id) {
            bail!("duplicate cell id {}", c.id)
        }
        if c.claim_boundary.trim().is_empty() {
            bail!("{}: empty claim boundary", c.id)
        }
        let expected: BTreeSet<_> = c.expected_population.iter().collect();
        if expected.len() != c.expected_population.len() || expected.is_empty() {
            bail!("{}: expected population must be nonempty and unique", c.id)
        }
        let mut rows = BTreeMap::new();
        for r in &c.results {
            validate_identity(&r.subject_sha, &r.run_id, &r.observed_at)?;
            if r.subject_sha != e.subject_sha {
                bail!("{}: cross-head evidence for {}", c.id, r.identity)
            }
            if r.run_id != e.run_id || r.observed_at != e.observed_at {
                bail!("{}: mixed-run evidence for {}", c.id, r.identity)
            }
            if !expected.contains(&r.identity) {
                bail!("{}: unknown population member {}", c.id, r.identity)
            }
            if rows.insert(&r.identity, r.outcome).is_some() {
                bail!("{}: duplicate result {}", c.id, r.identity)
            }
        }
        let missing = expected.len() - rows.len();
        let mut pass = 0;
        let mut fail = 0;
        let mut skip = 0;
        let mut not_run = missing as u64;
        for o in rows.values() {
            match o {
                Outcome::Pass => pass += 1,
                Outcome::Fail => fail += 1,
                Outcome::Skip => skip += 1,
                Outcome::NotRun => not_run += 1,
            }
        }
        let total = pass + fail + skip + not_run;
        if total != expected.len() as u64 {
            bail!("{}: result counts do not reconcile", c.id)
        }
        let complete = missing == 0 && not_run == 0;
        let (completeness, verdict) = if !complete {
            ("limited", "limited")
        } else if fail > 0 {
            ("complete", "failed")
        } else {
            ("complete", "proven")
        };
        let limitations = if complete {
            vec![]
        } else {
            vec![format!("{} expected population member(s) have incomplete attribution", not_run)]
        };
        cells.push(StatusCell {
            id: c.id.clone(),
            subject_sha: e.subject_sha.clone(),
            run_id: e.run_id.clone(),
            observed_at: e.observed_at.clone(),
            expected: expected.len() as u64,
            passed: pass,
            failed: fail,
            skipped: skip,
            not_run,
            completeness: completeness.into(),
            verdict: verdict.into(),
            claim_boundary: c.claim_boundary.clone(),
            limitations,
        });
    }
    Ok(Projection {
        schema_version: "generated_status_projection.v1".into(),
        source: SourceIdentity {
            subject_sha: e.subject_sha.clone(),
            run_id: e.run_id.clone(),
            observed_at: e.observed_at.clone(),
        },
        cells,
    })
}
fn validate_freshness(
    at: &str,
    max_age_hours: i64,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let observed = chrono::DateTime::parse_from_rfc3339(at)
        .context("invalid observed-at time")?
        .with_timezone(&chrono::Utc);
    let age = now.signed_duration_since(observed);
    if age < chrono::Duration::zero() || age > chrono::Duration::hours(max_age_hours) {
        bail!(
            "stale evidence: observed-at {at} is outside the {max_age_hours}-hour freshness window"
        )
    }
    Ok(())
}

fn validate_identity(sha: &str, run: &str, at: &str) -> Result<()> {
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid subject SHA")
    };
    if run.trim().is_empty() {
        bail!("missing run identity")
    };
    chrono::DateTime::parse_from_rfc3339(at).context("invalid observed-at time")?;
    Ok(())
}
fn render_markdown(p: &Projection) -> String {
    let mut s = format!(
        "<!-- generated from typed evidence; do not edit -->\n# Generated status cells\n\nSubject: `{}` · Run: `{}` · Observed: `{}`\n\n| Cell | Passed | Failed | Skipped | Not run | Expected | Completeness | Verdict |\n|---|---:|---:|---:|---:|---:|---|---|\n",
        p.source.subject_sha, p.source.run_id, p.source.observed_at
    );
    for c in &p.cells {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            c.id, c.passed, c.failed, c.skipped, c.not_run, c.expected, c.completeness, c.verdict
        ));
        if !c.limitations.is_empty() {
            s.push_str(&format!("\n> **{} limitation:** {}\n", c.id, c.limitations.join("; ")));
        }
    }
    s
}
fn check_file(path: &Path, want: &str) -> Result<()> {
    let got = fs::read_to_string(path)
        .with_context(|| format!("reading projection {}; run with --write", path.display()))?;
    if got != want {
        bail!("stale or copied projection {}; regenerate with --write", path.display())
    }
    Ok(())
}
fn atomic_write(path: &Path, text: &str) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bundle() -> EvidenceBundle {
        let sha = "a".repeat(40);
        EvidenceBundle {
            schema_version: "generated_status_evidence.v1".into(),
            subject_sha: sha.clone(),
            run_id: "run-1".into(),
            observed_at: "2026-08-12T00:00:00Z".into(),
            cells: vec![SourceCell {
                id: "tier_a".into(),
                claim_boundary: "test execution only".into(),
                expected_population: vec!["a".into(), "b".into()],
                results: vec![
                    ResultRow {
                        identity: "a".into(),
                        subject_sha: sha.clone(),
                        run_id: "run-1".into(),
                        observed_at: "2026-08-12T00:00:00Z".into(),
                        outcome: Outcome::Pass,
                    },
                    ResultRow {
                        identity: "b".into(),
                        subject_sha: sha,
                        run_id: "run-1".into(),
                        observed_at: "2026-08-12T00:00:00Z".into(),
                        outcome: Outcome::Skip,
                    },
                ],
            }],
        }
    }
    #[test]
    fn exact_populations_reconcile() {
        let p = construct(&bundle()).unwrap();
        assert_eq!((p.cells[0].passed, p.cells[0].skipped, p.cells[0].expected), (1, 1, 2));
        assert_eq!(p.cells[0].verdict, "proven")
    }
    #[test]
    fn deleted_child_is_limited() {
        let mut e = bundle();
        e.cells[0].results.pop();
        let p = construct(&e).unwrap();
        assert_eq!(p.cells[0].verdict, "limited");
        assert_eq!(p.cells[0].not_run, 1)
    }
    #[test]
    fn another_head_is_rejected() {
        let mut e = bundle();
        e.cells[0].results[0].subject_sha = "b".repeat(40);
        assert!(construct(&e).unwrap_err().to_string().contains("cross-head"))
    }
    #[test]
    fn mixed_run_is_rejected() {
        let mut e = bundle();
        e.cells[0].results[0].run_id = "copied-run".into();
        assert!(construct(&e).unwrap_err().to_string().contains("mixed-run"))
    }
    #[test]
    fn count_verdict_disagreement_cannot_be_manufactured() {
        let mut e = bundle();
        e.cells[0].expected_population.pop();
        assert!(construct(&e).is_err())
    }
    #[test]
    fn stale_source_time_is_rejected() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-12T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(
            validate_freshness("2026-08-01T00:00:00Z", 24, now)
                .unwrap_err()
                .to_string()
                .contains("stale evidence")
        );
    }

    #[test]
    fn copied_stale_markdown_fails_check() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.md");
        fs::write(&p, "old projection").unwrap();
        assert!(
            check_file(&p, "new projection").unwrap_err().to_string().contains("stale or copied")
        )
    }
}
