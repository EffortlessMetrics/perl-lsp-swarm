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

    /// Registry of generated surfaces whose internal consistency this tool
    /// verifies (the surfaces are the subject; the projections are derived).
    #[arg(long, default_value = "policy/generated-status-contract.toml")]
    contract: PathBuf,
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
    /// `bootstrap` evidence is checked-in structural seed data: it proves the
    /// projection wiring, not any observation, so the fatal freshness window
    /// does not apply. `live_run` evidence comes from a real producer run and
    /// must be fresh.
    evidence_class: String,
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
    enforce_evidence_class_freshness(
        &evidence.evidence_class,
        &evidence.observed_at,
        a.max_age_hours,
        chrono::Utc::now(),
    )?;
    let checked_surfaces = check_surfaces(&root, &root.join(&a.contract))?;
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
    println!(
        "generated status projections are current ({} cells); {} registered surfaces consistent",
        projection.cells.len(),
        checked_surfaces.len()
    );
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
fn enforce_evidence_class_freshness(
    evidence_class: &str,
    observed_at: &str,
    max_age_hours: i64,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    match evidence_class {
        "live_run" => validate_freshness(observed_at, max_age_hours, now),
        // Checked-in structural seed data proves the projection wiring, not an
        // observation; the fatal freshness window exists for real producers.
        "bootstrap" => Ok(()),
        other => bail!(
            "unknown evidence_class {other:?}; expected bootstrap or live_run — an undeclared class never skips the freshness window"
        ),
    }
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
// ---------------------------------------------------------------------------
// Registered-surface consistency checks (#6909).
//
// The projection above derives documents from typed evidence. These checks are
// the other half of the contract: the registry names the generated surfaces
// this repository presents, and each surface must not contradict itself — a
// headline must not disagree with its own table, unknown evidence must not sit
// beside a passing verdict, and a missing attribution table must not render as
// ordinary status. The surfaces are the subject; the registry has a reader
// again.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SurfaceContract {
    schema_version: u32,
    policy: String,
    surface: Vec<SurfaceRow>,
}

#[derive(Debug, Deserialize)]
struct SurfaceRow {
    id: String,
    kind: String,
    path: String,
    headline_label: Option<String>,
    table_label: Option<String>,
    row_label: Option<String>,
    begin_marker: Option<String>,
    end_marker: Option<String>,
    claim_boundary: String,
}

fn check_surfaces(root: &Path, contract_path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(contract_path)
        .with_context(|| format!("reading surface registry {}", contract_path.display()))?;
    let contract: SurfaceContract = toml::from_str(&raw)
        .with_context(|| format!("parsing surface registry {}", contract_path.display()))?;
    if contract.schema_version != 1 {
        bail!("unsupported surface registry schema_version {}", contract.schema_version);
    }
    if contract.policy.trim().is_empty() || contract.surface.is_empty() {
        bail!("surface registry must declare a policy and at least one surface");
    }
    let mut checked = Vec::new();
    for surface in &contract.surface {
        if surface.claim_boundary.trim().is_empty() {
            bail!("surface {}: registry entry must declare a claim boundary", surface.id);
        }
        let text = fs::read_to_string(root.join(&surface.path))
            .with_context(|| format!("surface {}: cannot read {}", surface.id, surface.path))?;
        match surface.kind.as_str() {
            "ratio_projection" => check_ratio_projection(surface, &text)?,
            "evidence_verdict" => check_evidence_verdict(surface, &text)?,
            "attribution_table" => check_attribution_table(surface, &text)?,
            other => bail!("surface {}: unknown kind {other:?}", surface.id),
        }
        checked.push(surface.id.clone());
    }
    Ok(checked)
}

fn find_line<'a>(text: &'a str, label: &str, surface: &SurfaceRow, role: &str) -> Result<&'a str> {
    text.lines()
        .find(|line| line.contains(label))
        .ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "surface {}: no {role} line containing {label:?} — the generated surface drifted from its registry entry",
                surface.id
            )
        })
}

/// Extract `(percent, numerator, denominator)` from a headline or table row.
/// The headline writes `98% ... (123/125 ...)` (slash ratio after the
/// percent); the table row writes cells `123 | 125 | 98%` (the two digit
/// groups before the percent). URLs are excluded from slash matching.
fn ratio_parts(line: &str) -> Option<(u64, u64, u64)> {
    let bytes = line.as_bytes();
    let read_back = |end: usize| -> Option<(u64, usize)> {
        let mut i = end;
        while i > 0 && bytes[i - 1].is_ascii_digit() {
            i -= 1;
        }
        if i == end {
            return None;
        }
        line.get(i..end)?.parse::<u64>().ok().map(|v| (v, i))
    };
    let read_fwd = |start_at: usize| -> Option<u64> {
        let mut i = start_at;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start_at {
            return None;
        }
        line.get(start_at..i)?.parse::<u64>().ok()
    };
    let percent_at = bytes.iter().position(|&b| b == b'%')?;
    let (percent, percent_digits_start) = read_back(percent_at)?;

    // Prefer an explicit a/b ratio of digits.
    for (at, &byte) in bytes.iter().enumerate() {
        if byte != b'/' {
            continue;
        }
        let prev_is_digit = at > 0 && bytes[at - 1].is_ascii_digit();
        let next_is_digit = at + 1 < bytes.len() && bytes[at + 1].is_ascii_digit();
        if prev_is_digit && next_is_digit {
            let (n, _) = read_back(at)?;
            let d = read_fwd(at + 1)?;
            return Some((percent, n, d));
        }
    }

    // Otherwise the last two digit groups before the percent are
    // (implemented, total) — the table-row shape `123 | 125 | 98%`.
    let mut groups = Vec::new();
    let mut i = 0;
    while i < percent_digits_start {
        if bytes[i].is_ascii_digit() {
            let mut value = String::new();
            while i < percent_digits_start && bytes[i].is_ascii_digit() {
                value.push(bytes[i] as char);
                i += 1;
            }
            groups.push(value.parse::<u64>().ok()?);
        } else {
            i += 1;
        }
    }
    if groups.len() < 2 {
        return None;
    }
    let denominator = groups[groups.len() - 1];
    let numerator = groups[groups.len() - 2];
    Some((percent, numerator, denominator))
}

fn check_ratio_projection(surface: &SurfaceRow, text: &str) -> Result<()> {
    let headline_label = surface.headline_label.as_deref().ok_or_else(|| {
        color_eyre::eyre::eyre!("surface {}: ratio_projection needs headline_label", surface.id)
    })?;
    let table_label = surface.table_label.as_deref().ok_or_else(|| {
        color_eyre::eyre::eyre!("surface {}: ratio_projection needs table_label", surface.id)
    })?;
    let headline = find_line(text, headline_label, surface, "headline")?;
    let table = find_line(text, table_label, surface, "table row")?;
    let headline_parts = ratio_parts(headline).ok_or_else(|| {
        color_eyre::eyre::eyre!("surface {}: headline carries no ratio", surface.id)
    })?;
    let table_parts = ratio_parts(table).ok_or_else(|| {
        color_eyre::eyre::eyre!("surface {}: table row carries no ratio", surface.id)
    })?;
    if headline_parts != table_parts {
        bail!(
            "surface {}: CONTRADICTORY — headline {}/{} = {}% disagrees with its own table row {}/{} = {}%",
            surface.id,
            headline_parts.1,
            headline_parts.2,
            headline_parts.0,
            table_parts.1,
            table_parts.2,
            table_parts.0
        );
    }
    let (percent, numerator, denominator) = headline_parts;
    if denominator == 0 {
        bail!(
            "surface {}: NOT_PROVEN — a percentage is rendered against a zero denominator",
            surface.id
        );
    }
    if numerator > denominator {
        bail!(
            "surface {}: CONTRADICTORY — numerator {} exceeds the declared denominator {}",
            surface.id,
            numerator,
            denominator
        );
    }
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let computed = (100.0 * numerator as f64 / denominator as f64).round() as u64;
    if computed != percent {
        bail!(
            "surface {}: CONTRADICTORY — {}/{} computes to {}% but {}% is rendered; a hand-adjusted percentage cannot stand beside its own ratio",
            surface.id,
            numerator,
            denominator,
            computed,
            percent
        );
    }
    Ok(())
}

/// Percent values (`N%`) and digit ratios (`a/b`) carried by a status row.
fn row_quantities(line: &str) -> (Vec<u64>, Vec<(u64, u64)>) {
    let bytes = line.as_bytes();
    let mut percents = Vec::new();
    let mut ratios = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let Some(value) = line.get(start..i).and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        if i < bytes.len() && bytes[i] == b'%' {
            percents.push(value);
        } else if i < bytes.len()
            && bytes[i] == b'/'
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_digit()
        {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if let Some(denominator) = line.get(i + 1..j).and_then(|s| s.parse::<u64>().ok()) {
                ratios.push((value, denominator));
            }
        }
    }
    (percents, ratios)
}

fn check_evidence_verdict(surface: &SurfaceRow, text: &str) -> Result<()> {
    let row_label = surface.row_label.as_deref().ok_or_else(|| {
        color_eyre::eyre::eyre!("surface {}: evidence_verdict needs row_label", surface.id)
    })?;
    let row = find_line(text, row_label, surface, "evidence row")?;
    if row.contains("BYPASS") {
        bail!(
            "surface {}: BYPASS in the {row_label} row — bypassed evidence is not a verdict and must never render as status",
            surface.id
        );
    }
    let has_pass = row.contains("PASS");
    if has_pass && row.contains("FAIL") {
        bail!("surface {}: CONTRADICTORY — PASS and FAIL in the same {row_label} row", surface.id);
    }
    let unknown_evidence =
        row.contains("UNVERIFIED") || row.contains("unknown") || row.contains("TBD");
    let passing_verdict = has_pass || row.contains("100%");
    if unknown_evidence && passing_verdict {
        bail!(
            "surface {}: CONTRADICTORY — unknown evidence (UNVERIFIED/unknown/TBD) beside a passing verdict in the {row_label} row",
            surface.id
        );
    }
    if has_pass {
        // A passing verdict requires a known, non-zero, fully-passing
        // population: a partial or empty pass is LIMITED, never PASS.
        let (percents, ratios) = row_quantities(row);
        if let Some(percent) = percents.iter().find(|&&p| p < 100) {
            bail!(
                "surface {}: CONTRADICTORY — {percent}% beside PASS in the {row_label} row; a partial pass is not a passing verdict",
                surface.id
            );
        }
        if let Some((numerator, denominator)) = ratios.iter().find(|&&(n, d)| d == 0 || n < d) {
            bail!(
                "surface {}: CONTRADICTORY — {numerator}/{denominator} beside PASS in the {row_label} row; a partial pass is not a passing verdict",
                surface.id
            );
        }
        if percents.is_empty() && ratios.is_empty() {
            bail!(
                "surface {}: NOT_PROVEN — PASS in the {row_label} row carries no count, ratio, or percentage; a passing verdict requires a known non-zero denominator",
                surface.id
            );
        }
    }
    Ok(())
}

fn check_attribution_table(surface: &SurfaceRow, text: &str) -> Result<()> {
    let begin = surface.begin_marker.as_deref().ok_or_else(|| {
        color_eyre::eyre::eyre!("surface {}: attribution_table needs begin_marker", surface.id)
    })?;
    let end = surface.end_marker.as_deref().ok_or_else(|| {
        color_eyre::eyre::eyre!("surface {}: attribution_table needs end_marker", surface.id)
    })?;
    let start = text.find(begin).ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "surface {}: attribution markers are gone — aggregate discovery must not stand in for per-crate evidence",
            surface.id
        )
    })?;
    let region = text[start..].split(end).next().unwrap_or(&text[start..]);
    let has_data_row = region
        .lines()
        .any(|line| line.starts_with('|') && !line.contains("---") && !line.contains("Crate"));
    if !has_data_row || region.contains("no data yet") {
        bail!(
            "surface {}: NOT_PROVEN — the per-crate attribution table is missing or empty; incomplete attribution must not render as ordinary status",
            surface.id
        );
    }
    Ok(())
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
            evidence_class: "bootstrap".into(),
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

    fn surface_fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let contract = r#"
schema_version = 1
policy = "generated-status-contract"

[[surface]]
id = "lsp.protocol_compliance"
kind = "ratio_projection"
path = "status/lsp.md"
headline_label = "Protocol Compliance"
table_label = "Overall"
claim_boundary = "fixture"

[[surface]]
id = "tests.tier_a"
kind = "evidence_verdict"
path = "status/tests.md"
row_label = "Tier A Tests"
claim_boundary = "fixture"

[[surface]]
id = "quality.per_crate_attribution"
kind = "attribution_table"
path = "status/quality.md"
begin_marker = "<!-- BEGIN: QUALITY_CRATE_TABLE -->"
end_marker = "<!-- END: QUALITY_CRATE_TABLE -->"
claim_boundary = "fixture"
"#;
        fs::write(dir.path().join("contract.toml"), contract).unwrap();
        for (name, text) in files {
            let target = dir.path().join(name);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, text).unwrap();
        }
        let contract_path = dir.path().join("contract.toml");
        (dir, contract_path)
    }

    const LSP_OK: &str =
        "- **Protocol Compliance**: 98% overall LSP protocol support (123/125 including plumbing)

| **Overall** | **123** | **125** | **98%** |
";
    const LSP_BAD: &str =
        "- **Protocol Compliance**: 100% overall LSP protocol support (123/123 including plumbing)

| **Overall** | **123** | **125** | **98%** |
";
    const TESTS_OK: &str =
        "| **Tier A Tests** | 10161 lib tests (discovered), 15 ignores (tracked) | 100% pass | PASS |
";
    const TESTS_BAD: &str = "| **Tier A Tests** | UNVERIFIED | 100% pass | PASS |
";
    const QUALITY_OK: &str = "<!-- BEGIN: QUALITY_CRATE_TABLE -->
| Crate | Tests (lib) |
|---|---|
| perl-ast | 38 |
<!-- END: QUALITY_CRATE_TABLE -->
";
    const QUALITY_BAD: &str = "<!-- BEGIN: QUALITY_CRATE_TABLE -->
no data yet
<!-- END: QUALITY_CRATE_TABLE -->
";

    #[test]
    fn consistent_surfaces_pass() {
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_OK),
            ("status/tests.md", TESTS_OK),
            ("status/quality.md", QUALITY_OK),
        ]);
        let checked = check_surfaces(dir.path(), &contract).unwrap();
        assert_eq!(checked.len(), 3);
    }

    #[test]
    fn contradictory_headline_and_table_fail_closed() {
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_BAD),
            ("status/tests.md", TESTS_OK),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("CONTRADICTORY"));
    }

    #[test]
    fn internally_consistent_but_wrong_arithmetic_fails_closed() {
        // Headline and table agree with each other — and both miscompute the
        // percentage. Agreement between projections is not arithmetic truth.
        const LSP_WRONG_MATH: &str =
            "- **Protocol Compliance**: 97% overall LSP protocol support (123/125 including plumbing)

| **Overall** | **123** | **125** | **97%** |
";
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_WRONG_MATH),
            ("status/tests.md", TESTS_OK),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("computes to 98% but 97% is rendered"));
    }

    #[test]
    fn denominator_rewriting_to_preserve_100_fails_closed() {
        // The #6648 defect: numerator silently replaces the denominator.
        const LSP_REWRITTEN: &str =
            "- **Protocol Compliance**: 100% overall LSP protocol support (123/123 including plumbing)

| **Overall** | **123** | **123** | **100%** |
";
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_REWRITTEN),
            ("status/tests.md", TESTS_OK),
            ("status/quality.md", QUALITY_OK),
        ]);
        let checked = check_surfaces(dir.path(), &contract).unwrap();
        assert_eq!(checked.len(), 3, "123/123 is internally consistent arithmetic");
        // …but it must still disagree with the real table. The guard for the
        // #6648 case is the headline/table comparison itself:
        const LSP_REWRITTEN_HEADLINE_ONLY: &str =
            "- **Protocol Compliance**: 100% overall LSP protocol support (123/123 including plumbing)

| **Overall** | **123** | **125** | **98%** |
";
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_REWRITTEN_HEADLINE_ONLY),
            ("status/tests.md", TESTS_OK),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("CONTRADICTORY"));
    }

    #[test]
    fn zero_percent_beside_pass_fails_closed() {
        const TESTS_ZERO: &str = "| **Tier A Tests** | 0 lib tests (discovered) | 0% pass | PASS |
";
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_OK),
            ("status/tests.md", TESTS_ZERO),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("0% beside PASS"));
    }

    #[test]
    fn partial_pass_fails_closed() {
        const TESTS_PARTIAL: &str =
            "| **Tier A Tests** | 9000 lib tests (discovered) | 88% pass | PASS |
";
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_OK),
            ("status/tests.md", TESTS_PARTIAL),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("88% beside PASS"));
        const TESTS_PARTIAL_RATIO: &str = "| **Tier A Tests** | 9000/10161 lib tests pass | PASS |
";
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_OK),
            ("status/tests.md", TESTS_PARTIAL_RATIO),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("9000/10161 beside PASS"));
    }

    #[test]
    fn bypass_fails_closed() {
        const TESTS_BYPASS: &str =
            "| **Tier A Tests** | 10161 lib tests (discovered) | BYPASS | PASS |
";
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_OK),
            ("status/tests.md", TESTS_BYPASS),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("BYPASS"));
    }

    #[test]
    fn contradictory_pass_and_fail_fails_closed() {
        const TESTS_BOTH: &str =
            "| **Tier A Tests** | 10161 lib tests (discovered) | 100% pass | PASS (was FAIL) |
";
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_OK),
            ("status/tests.md", TESTS_BOTH),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("PASS and FAIL"));
    }

    #[test]
    fn denominator_free_pass_fails_closed() {
        const TESTS_NO_EVIDENCE: &str = "| **Tier A Tests** | suite green | PASS |
";
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_OK),
            ("status/tests.md", TESTS_NO_EVIDENCE),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("no count, ratio, or percentage"));
    }

    #[test]
    fn workflow_checker_binary_target_is_registered() {
        // The workflow invokes `cargo run -p xtask --bin generated-status-contract`;
        // an unregistered target makes the gate unrunnable (the original defect).
        #[derive(Deserialize)]
        struct Manifest {
            #[serde(default)]
            bin: Vec<BinTarget>,
        }
        #[derive(Deserialize)]
        struct BinTarget {
            name: String,
            path: String,
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
        let raw = fs::read_to_string(root.join("xtask/Cargo.toml")).unwrap();
        let manifest: Manifest = toml::from_str(&raw).unwrap();
        let target = manifest
            .bin
            .iter()
            .find(|b| b.name == "generated-status-contract")
            .expect("xtask must register a generated-status-contract bin target");
        assert!(root.join("xtask").join(&target.path).is_file());
    }

    #[test]
    fn unknown_evidence_beside_pass_fails_closed() {
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_OK),
            ("status/tests.md", TESTS_BAD),
            ("status/quality.md", QUALITY_OK),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("CONTRADICTORY"));
    }

    #[test]
    fn missing_attribution_table_fails_closed() {
        let (dir, contract) = surface_fixture(&[
            ("status/lsp.md", LSP_OK),
            ("status/tests.md", TESTS_OK),
            ("status/quality.md", QUALITY_BAD),
        ]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("NOT_PROVEN"));
    }

    #[test]
    fn a_missing_registered_surface_fails_closed() {
        let (dir, contract) =
            surface_fixture(&[("status/lsp.md", LSP_OK), ("status/tests.md", TESTS_OK)]);
        let error = check_surfaces(dir.path(), &contract).unwrap_err();
        assert!(format!("{error:#}").contains("cannot read"));
    }

    #[test]
    fn committed_registry_surfaces_are_consistent() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
        let checked =
            check_surfaces(&root, &root.join("policy/generated-status-contract.toml")).unwrap();
        assert_eq!(checked.len(), 3);
    }

    #[test]
    fn bootstrap_evidence_never_hits_the_freshness_window() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-19T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(
            enforce_evidence_class_freshness("bootstrap", "2026-08-12T00:00:00Z", 168, now).is_ok()
        );
        assert!(
            enforce_evidence_class_freshness("live_run", "2026-08-12T00:00:00Z", 168, now)
                .unwrap_err()
                .to_string()
                .contains("stale evidence")
        );
        assert!(
            enforce_evidence_class_freshness("copied", "2026-08-19T09:00:00Z", 168, now)
                .unwrap_err()
                .to_string()
                .contains("unknown evidence_class")
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
