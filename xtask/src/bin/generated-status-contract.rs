//! Validate generated Markdown as typed, fail-closed status projections.
//!
//! Domain receipts remain authoritative. This checker prevents their rendered
//! projections from changing denominators, retaining pass beside unknown data,
//! or presenting missing attribution as complete evidence.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail, eyre};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Parser)]
#[command(name = "generated-status-contract")]
#[command(about = "Validate generated status arithmetic, completeness, and verdict truth")]
struct Args {
    /// Repository root. Defaults to the parent of the xtask crate.
    #[arg(long)]
    root: Option<PathBuf>,

    /// Projection policy.
    #[arg(long, default_value = "policy/generated-status-contract.toml")]
    policy: PathBuf,

    /// Optional deterministic JSON projection receipt.
    #[arg(long)]
    receipt: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize)]
struct Policy {
    schema_version: u32,
    policy: String,
    #[serde(default)]
    surface: Vec<Surface>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum SurfaceKind {
    RatioProjection,
    EvidenceVerdict,
    AttributionTable,
}

#[derive(Clone, Debug, Deserialize)]
struct Surface {
    id: String,
    kind: SurfaceKind,
    path: PathBuf,
    #[serde(default)]
    headline_label: Option<String>,
    #[serde(default)]
    table_label: Option<String>,
    #[serde(default)]
    row_label: Option<String>,
    #[serde(default)]
    begin_marker: Option<String>,
    #[serde(default)]
    end_marker: Option<String>,
    claim_boundary: String,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Completeness {
    Complete,
    Partial,
    Missing,
    NotProven,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Freshness {
    Current,
    Stale,
    NotProven,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Proven,
    Failed,
    Limited,
    NotProven,
    Stale,
    Contradictory,
    NotApplicable,
}

#[derive(Clone, Debug, Serialize)]
struct Subject {
    repository_sha: String,
    source_path: String,
    source_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
struct StatusValue {
    numerator: Option<u64>,
    denominator: Option<u64>,
    percent: Option<f64>,
    display: String,
}

#[derive(Clone, Debug, Serialize)]
struct Evidence {
    refs: Vec<String>,
    completeness: Completeness,
    freshness: Freshness,
}

#[derive(Clone, Debug, Serialize)]
struct StatusCell {
    schema_version: &'static str,
    id: String,
    subject: Subject,
    value: StatusValue,
    evidence: Evidence,
    verdict: Verdict,
    claim_boundary: String,
    limitations: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Finding {
    level: &'static str,
    code: &'static str,
    surface_id: String,
    path: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: &'static str,
    receipt_kind: &'static str,
    repository_sha: String,
    policy_path: String,
    passed: bool,
    cell_count: usize,
    error_count: usize,
    warning_count: usize,
    cells: Vec<StatusCell>,
    findings: Vec<Finding>,
}

#[derive(Clone, Copy, Debug)]
struct ParsedPercent {
    value: f64,
    decimals: u32,
}

#[derive(Clone, Copy, Debug)]
struct ParsedRatio {
    numerator: u64,
    denominator: u64,
    percent: ParsedPercent,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let root = args.root.unwrap_or_else(default_root);
    let policy_path = root.join(&args.policy);
    let content = fs::read_to_string(&policy_path)
        .with_context(|| format!("reading {}", policy_path.display()))?;
    let policy: Policy = toml::from_str(&content)
        .with_context(|| format!("parsing {}", policy_path.display()))?;
    let repository_sha = repository_sha(&root)?;
    let receipt = validate_policy(&root, &args.policy, &repository_sha, &policy)?;

    for finding in &receipt.findings {
        let command = if finding.level == "error" {
            "error"
        } else {
            "warning"
        };
        eprintln!(
            "::{command} file={}::[{}] {}: {}",
            finding.path, finding.code, finding.surface_id, finding.message
        );
    }

    if let Some(receipt_path) = args.receipt {
        let destination = if receipt_path.is_absolute() {
            receipt_path
        } else {
            root.join(receipt_path)
        };
        write_receipt(&destination, &receipt)?;
        println!("Generated-status receipt written: {}", destination.display());
    }

    if !receipt.passed {
        bail!(
            "generated-status contract failed with {} error(s)",
            receipt.error_count
        );
    }

    println!(
        "Generated-status contract passed ({} cell(s), {} warning(s))",
        receipt.cell_count, receipt.warning_count
    );
    Ok(())
}

fn default_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn validate_policy(
    root: &Path,
    policy_path: &Path,
    repository_sha: &str,
    policy: &Policy,
) -> Result<Receipt> {
    let mut findings = Vec::new();
    if policy.schema_version != 1 {
        findings.push(policy_error(
            "UNSUPPORTED_SCHEMA",
            format!("schema_version must be 1, got {}", policy.schema_version),
        ));
    }
    if policy.policy != "generated-status-contract" {
        findings.push(policy_error(
            "POLICY_ID_MISMATCH",
            format!("policy must be 'generated-status-contract', got '{}'", policy.policy),
        ));
    }
    if policy.surface.is_empty() {
        findings.push(policy_error(
            "NO_STATUS_SURFACES",
            "at least one status projection surface is required".to_string(),
        ));
    }

    let mut seen_ids = BTreeSet::new();
    let mut cells = Vec::new();
    for surface in &policy.surface {
        if surface.id.trim().is_empty() || !seen_ids.insert(surface.id.clone()) {
            findings.push(surface_error(
                surface,
                "INVALID_SURFACE_ID",
                "surface id must be non-empty and unique".to_string(),
            ));
            continue;
        }
        if surface.claim_boundary.trim().is_empty() {
            findings.push(surface_error(
                surface,
                "CLAIM_BOUNDARY_MISSING",
                "claim_boundary must not be empty".to_string(),
            ));
        }
        let source_path = root.join(&surface.path);
        let source = match fs::read_to_string(&source_path) {
            Ok(source) => source,
            Err(error) => {
                findings.push(surface_error(
                    surface,
                    "STATUS_SOURCE_UNREADABLE",
                    format!("unable to read {}: {error}", source_path.display()),
                ));
                cells.push(not_proven_cell(
                    surface,
                    repository_sha,
                    String::new(),
                    "status source is unreadable",
                ));
                continue;
            }
        };
        let subject = Subject {
            repository_sha: repository_sha.to_string(),
            source_path: normalize_path(&surface.path),
            source_sha256: sha256_text(&source),
        };
        let cell = match surface.kind {
            SurfaceKind::RatioProjection => {
                parse_ratio_projection(surface, &source, subject, &mut findings)?
            }
            SurfaceKind::EvidenceVerdict => {
                parse_evidence_verdict(surface, &source, subject, &mut findings)?
            }
            SurfaceKind::AttributionTable => {
                parse_attribution_table(surface, &source, subject, &mut findings)?
            }
        };
        validate_cell(&cell, surface, &mut findings);
        cells.push(cell);
    }

    findings.sort_by(|left, right| {
        (left.level, &left.surface_id, &left.path, left.code, &left.message).cmp(&(
            right.level,
            &right.surface_id,
            &right.path,
            right.code,
            &right.message,
        ))
    });
    let error_count = findings.iter().filter(|finding| finding.level == "error").count();
    let warning_count = findings
        .iter()
        .filter(|finding| finding.level == "warning")
        .count();

    Ok(Receipt {
        schema_version: "generated_status_projection.v1",
        receipt_kind: "generated_status_projection",
        repository_sha: repository_sha.to_string(),
        policy_path: normalize_path(policy_path),
        passed: error_count == 0,
        cell_count: cells.len(),
        error_count,
        warning_count,
        cells,
        findings,
    })
}

fn parse_ratio_projection(
    surface: &Surface,
    source: &str,
    subject: Subject,
    findings: &mut Vec<Finding>,
) -> Result<StatusCell> {
    let headline_label = required_option(surface, &surface.headline_label, "headline_label", findings);
    let table_label = required_option(surface, &surface.table_label, "table_label", findings);
    let Some(headline_label) = headline_label else {
        return Ok(not_proven_cell_from_subject(
            surface,
            subject,
            "ratio projection policy is incomplete",
        ));
    };
    let Some(table_label) = table_label else {
        return Ok(not_proven_cell_from_subject(
            surface,
            subject,
            "ratio projection policy is incomplete",
        ));
    };

    let headline = source.lines().find(|line| line.contains(headline_label));
    let table_line = find_markdown_row(source, table_label);
    let (Some(headline), Some(table_line)) = (headline, table_line) else {
        findings.push(surface_error(
            surface,
            "RATIO_PROJECTION_MISSING",
            format!(
                "could not find headline {headline_label:?} and table row {table_label:?}"
            ),
        ));
        return Ok(not_proven_cell_from_subject(
            surface,
            subject,
            "headline or aggregate row is missing",
        ));
    };

    let headline_ratio = parse_ratio_from_text(headline)?;
    let table_ratio = parse_ratio_from_row(&table_line)?;
    let (Some(headline_ratio), Some(table_ratio)) = (headline_ratio, table_ratio) else {
        findings.push(surface_error(
            surface,
            "RATIO_PROJECTION_UNPARSEABLE",
            "headline or aggregate row does not contain a complete numerator/denominator/percent"
                .to_string(),
        ));
        return Ok(not_proven_cell_from_subject(
            surface,
            subject,
            "ratio projection could not be parsed",
        ));
    };

    let mut contradictory = false;
    if headline_ratio.numerator != table_ratio.numerator
        || headline_ratio.denominator != table_ratio.denominator
        || !same_displayed_percent(headline_ratio.percent, table_ratio.percent)
    {
        contradictory = true;
        findings.push(surface_error(
            surface,
            "HEADLINE_TABLE_CONTRADICTION",
            format!(
                "headline reports {}/{} at {}%, table reports {}/{} at {}%",
                headline_ratio.numerator,
                headline_ratio.denominator,
                headline_ratio.percent.value,
                table_ratio.numerator,
                table_ratio.denominator,
                table_ratio.percent.value
            ),
        ));
    }
    if !ratio_percent_is_correct(headline_ratio) {
        contradictory = true;
        findings.push(surface_error(
            surface,
            "HEADLINE_PERCENT_MISMATCH",
            format!(
                "headline percent {} does not match {}/{} at its displayed precision",
                headline_ratio.percent.value,
                headline_ratio.numerator,
                headline_ratio.denominator
            ),
        ));
    }
    if !ratio_percent_is_correct(table_ratio) {
        contradictory = true;
        findings.push(surface_error(
            surface,
            "TABLE_PERCENT_MISMATCH",
            format!(
                "table percent {} does not match {}/{} at its displayed precision",
                table_ratio.percent.value,
                table_ratio.numerator,
                table_ratio.denominator
            ),
        ));
    }

    let ratio = table_ratio;
    let complete = ratio.numerator == ratio.denominator;
    let verdict = if contradictory {
        Verdict::Contradictory
    } else if complete {
        Verdict::Proven
    } else {
        Verdict::Limited
    };
    let limitations = if complete {
        Vec::new()
    } else {
        vec![format!(
            "{} declared rows are not implemented/proven in this projection",
            ratio.denominator.saturating_sub(ratio.numerator)
        )]
    };

    Ok(StatusCell {
        schema_version: "generated_status_cell.v1",
        id: surface.id.clone(),
        subject,
        value: StatusValue {
            numerator: Some(ratio.numerator),
            denominator: Some(ratio.denominator),
            percent: Some(ratio.percent.value),
            display: format!(
                "{}/{} ({}%)",
                ratio.numerator, ratio.denominator, ratio.percent.value
            ),
        },
        evidence: Evidence {
            refs: vec![normalize_path(&surface.path)],
            completeness: Completeness::Complete,
            freshness: Freshness::Current,
        },
        verdict,
        claim_boundary: surface.claim_boundary.clone(),
        limitations,
    })
}

fn parse_evidence_verdict(
    surface: &Surface,
    source: &str,
    subject: Subject,
    findings: &mut Vec<Finding>,
) -> Result<StatusCell> {
    let row_label = required_option(surface, &surface.row_label, "row_label", findings);
    let Some(row_label) = row_label else {
        return Ok(not_proven_cell_from_subject(
            surface,
            subject,
            "evidence-verdict policy is incomplete",
        ));
    };
    let Some(row) = find_markdown_row(source, row_label) else {
        findings.push(surface_error(
            surface,
            "EVIDENCE_ROW_MISSING",
            format!("could not find Markdown row {row_label:?}"),
        ));
        return Ok(not_proven_cell_from_subject(
            surface,
            subject,
            "required evidence row is missing",
        ));
    };

    let normalized = row.to_ascii_uppercase();
    let has_unknown = ["UNVERIFIED", "NOT_PROVEN", "NOT PROVEN", "UNKNOWN", "NO DATA"]
        .iter()
        .any(|token| normalized.contains(token));
    let has_pass = normalized.contains("PASS") || normalized.contains("100% PASS");
    let has_fail = normalized.contains("FAIL");
    let count = parse_named_count(&row, "lib tests")?;
    let percent = parse_first_percent(&row)?;

    let mut contradictory = false;
    if has_unknown && has_pass {
        contradictory = true;
        findings.push(surface_error(
            surface,
            "UNKNOWN_EVIDENCE_WITH_PASS",
            format!("row {row:?} contains unknown evidence and a passing verdict"),
        ));
    }
    if has_pass && count.is_none() {
        contradictory = true;
        findings.push(surface_error(
            surface,
            "PASS_WITHOUT_DENOMINATOR",
            "passing Tier A status requires a known test denominator".to_string(),
        ));
    }
    if has_pass && percent.is_none() {
        contradictory = true;
        findings.push(surface_error(
            surface,
            "PASS_WITHOUT_PERCENT",
            "passing Tier A status requires an explicit measured pass percentage".to_string(),
        ));
    }

    let (completeness, verdict, limitations) = if contradictory {
        (
            Completeness::NotProven,
            Verdict::Contradictory,
            vec!["unknown or denominator-free evidence cannot satisfy PASS".to_string()],
        )
    } else if has_unknown {
        (
            Completeness::NotProven,
            Verdict::NotProven,
            vec!["test discovery or execution denominator is not proven".to_string()],
        )
    } else if has_fail {
        (Completeness::Complete, Verdict::Failed, Vec::new())
    } else if has_pass {
        (Completeness::Complete, Verdict::Proven, Vec::new())
    } else {
        (
            Completeness::Partial,
            Verdict::Limited,
            vec!["row has no terminal PASS/FAIL verdict".to_string()],
        )
    };

    Ok(StatusCell {
        schema_version: "generated_status_cell.v1",
        id: surface.id.clone(),
        subject,
        value: StatusValue {
            numerator: count,
            denominator: count,
            percent: percent.map(|parsed| parsed.value),
            display: row.trim().to_string(),
        },
        evidence: Evidence {
            refs: vec![normalize_path(&surface.path)],
            completeness,
            freshness: Freshness::Current,
        },
        verdict,
        claim_boundary: surface.claim_boundary.clone(),
        limitations,
    })
}

fn parse_attribution_table(
    surface: &Surface,
    source: &str,
    subject: Subject,
    findings: &mut Vec<Finding>,
) -> Result<StatusCell> {
    let begin_marker = required_option(surface, &surface.begin_marker, "begin_marker", findings);
    let end_marker = required_option(surface, &surface.end_marker, "end_marker", findings);
    let (Some(begin_marker), Some(end_marker)) = (begin_marker, end_marker) else {
        return Ok(not_proven_cell_from_subject(
            surface,
            subject,
            "attribution-table policy is incomplete",
        ));
    };
    let Some(section) = section_between(source, begin_marker, end_marker) else {
        findings.push(surface_error(
            surface,
            "ATTRIBUTION_SECTION_MISSING",
            "could not locate the configured generated table markers".to_string(),
        ));
        return Ok(not_proven_cell_from_subject(
            surface,
            subject,
            "attribution table is missing",
        ));
    };

    let data_rows: Vec<&str> = section
        .lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter(|line| !line.contains("---"))
        .filter(|line| {
            parse_markdown_cells(line)
                .first()
                .is_some_and(|cell| normalize_markdown(cell) != "Crate")
        })
        .collect();
    if data_rows.is_empty() {
        findings.push(surface_error(
            surface,
            "ATTRIBUTION_TABLE_EMPTY",
            "per-crate attribution table has no data or explicit no-data row".to_string(),
        ));
        return Ok(not_proven_cell_from_subject(
            surface,
            subject,
            "per-crate attribution table is empty",
        ));
    }

    let no_data_rows = data_rows
        .iter()
        .filter(|row| row.to_ascii_lowercase().contains("no data yet"))
        .count();
    let attributed_rows = data_rows.len().saturating_sub(no_data_rows);
    let explicit_limitation = {
        let lower = source.to_ascii_lowercase();
        lower.contains("no crate attribution")
            || lower.contains("incomplete attribution")
            || lower.contains("excluded from the per-crate table")
    };

    let (completeness, verdict, limitations) = if no_data_rows == 0 {
        (Completeness::Complete, Verdict::Proven, Vec::new())
    } else if explicit_limitation {
        (
            Completeness::Partial,
            Verdict::Limited,
            vec![format!(
                "{no_data_rows} no-data row(s); {attributed_rows} attributed row(s)"
            )],
        )
    } else {
        findings.push(surface_error(
            surface,
            "UNQUALIFIED_ATTRIBUTION_GAP",
            "no-data per-crate output lacks an explicit incomplete-attribution limitation"
                .to_string(),
        ));
        (
            Completeness::NotProven,
            Verdict::Contradictory,
            vec!["missing per-crate attribution is presented without a limitation".to_string()],
        )
    };

    Ok(StatusCell {
        schema_version: "generated_status_cell.v1",
        id: surface.id.clone(),
        subject,
        value: StatusValue {
            numerator: Some(attributed_rows as u64),
            denominator: if no_data_rows == 0 {
                Some(data_rows.len() as u64)
            } else {
                None
            },
            percent: if no_data_rows == 0 {
                Some(100.0)
            } else {
                None
            },
            display: format!(
                "{attributed_rows} attributed row(s), {no_data_rows} no-data row(s)"
            ),
        },
        evidence: Evidence {
            refs: vec![normalize_path(&surface.path)],
            completeness,
            freshness: Freshness::Current,
        },
        verdict,
        claim_boundary: surface.claim_boundary.clone(),
        limitations,
    })
}

fn validate_cell(cell: &StatusCell, surface: &Surface, findings: &mut Vec<Finding>) {
    match (cell.value.numerator, cell.value.denominator) {
        (Some(numerator), Some(denominator)) => {
            if denominator == 0 {
                findings.push(surface_error(
                    surface,
                    "ZERO_DENOMINATOR",
                    "a measured ratio cannot use denominator zero".to_string(),
                ));
            }
            if numerator > denominator {
                findings.push(surface_error(
                    surface,
                    "NUMERATOR_EXCEEDS_DENOMINATOR",
                    format!("numerator {numerator} exceeds denominator {denominator}"),
                ));
            }
        }
        (None, None) => {}
        _ => findings.push(surface_error(
            surface,
            "PARTIAL_RATIO_IDENTITY",
            "numerator and denominator must both be present or both be absent".to_string(),
        )),
    }
    if cell.value.percent.is_some() && cell.value.denominator.is_none() {
        findings.push(surface_error(
            surface,
            "PERCENT_WITHOUT_DENOMINATOR",
            "percent cannot be projected without a denominator".to_string(),
        ));
    }
    if cell.verdict == Verdict::Proven
        && (cell.evidence.completeness != Completeness::Complete
            || cell.evidence.freshness != Freshness::Current)
    {
        findings.push(surface_error(
            surface,
            "PROVEN_WITH_INCOMPLETE_EVIDENCE",
            "proven verdict requires complete current evidence".to_string(),
        ));
    }
    if cell.claim_boundary.trim().is_empty() {
        findings.push(surface_error(
            surface,
            "CELL_CLAIM_BOUNDARY_MISSING",
            "status cell claim boundary must not be empty".to_string(),
        ));
    }
    if cell.evidence.refs.is_empty() {
        findings.push(surface_error(
            surface,
            "CELL_EVIDENCE_REFS_MISSING",
            "status cell must retain at least one evidence reference".to_string(),
        ));
    }
}

fn required_option<'a>(
    surface: &Surface,
    value: &'a Option<String>,
    field: &'static str,
    findings: &mut Vec<Finding>,
) -> Option<&'a str> {
    match value.as_deref().filter(|value| !value.trim().is_empty()) {
        Some(value) => Some(value),
        None => {
            findings.push(surface_error(
                surface,
                "SURFACE_FIELD_MISSING",
                format!("{} surface requires {field}", kind_name(surface.kind)),
            ));
            None
        }
    }
}

fn kind_name(kind: SurfaceKind) -> &'static str {
    match kind {
        SurfaceKind::RatioProjection => "ratio_projection",
        SurfaceKind::EvidenceVerdict => "evidence_verdict",
        SurfaceKind::AttributionTable => "attribution_table",
    }
}

fn parse_ratio_from_text(text: &str) -> Result<Option<ParsedRatio>> {
    let ratio_pattern = Regex::new(r"(?P<n>\d+)\s*/\s*(?P<d>\d+)")
        .context("compiling ratio pattern")?;
    let Some(captures) = ratio_pattern.captures(text) else {
        return Ok(None);
    };
    let numerator = captures
        .name("n")
        .ok_or_else(|| eyre!("ratio numerator capture missing"))?
        .as_str()
        .parse::<u64>()
        .context("parsing ratio numerator")?;
    let denominator = captures
        .name("d")
        .ok_or_else(|| eyre!("ratio denominator capture missing"))?
        .as_str()
        .parse::<u64>()
        .context("parsing ratio denominator")?;
    let Some(percent) = parse_first_percent(text)? else {
        return Ok(None);
    };
    Ok(Some(ParsedRatio { numerator, denominator, percent }))
}

fn parse_ratio_from_row(row: &str) -> Result<Option<ParsedRatio>> {
    let cells = parse_markdown_cells(row);
    if cells.len() < 4 {
        return Ok(None);
    }
    let numerator = normalize_markdown(&cells[1])
        .parse::<u64>()
        .context("parsing table numerator")?;
    let denominator = normalize_markdown(&cells[2])
        .parse::<u64>()
        .context("parsing table denominator")?;
    let Some(percent) = parse_first_percent(&cells[3])? else {
        return Ok(None);
    };
    Ok(Some(ParsedRatio { numerator, denominator, percent }))
}

fn parse_first_percent(text: &str) -> Result<Option<ParsedPercent>> {
    let pattern = Regex::new(r"(?P<p>\d+(?:\.\d+)?)%")
        .context("compiling percent pattern")?;
    let Some(captures) = pattern.captures(text) else {
        return Ok(None);
    };
    let raw = captures
        .name("p")
        .ok_or_else(|| eyre!("percent capture missing"))?
        .as_str();
    let decimals = raw.split_once('.').map_or(0, |(_, tail)| tail.len() as u32);
    Ok(Some(ParsedPercent {
        value: raw.parse::<f64>().context("parsing percent")?,
        decimals,
    }))
}

fn parse_named_count(text: &str, suffix: &str) -> Result<Option<u64>> {
    let escaped = regex::escape(suffix);
    let pattern = Regex::new(&format!(r"(?P<count>\d+)\s+{escaped}"))
        .context("compiling count pattern")?;
    let Some(captures) = pattern.captures(text) else {
        return Ok(None);
    };
    let count = captures
        .name("count")
        .ok_or_else(|| eyre!("count capture missing"))?
        .as_str()
        .parse::<u64>()
        .context("parsing count")?;
    Ok(Some(count))
}

fn ratio_percent_is_correct(ratio: ParsedRatio) -> bool {
    if ratio.denominator == 0 {
        return false;
    }
    let scale = 10_f64.powi(ratio.percent.decimals as i32);
    let exact = ratio.numerator as f64 * 100.0 / ratio.denominator as f64;
    let expected = (exact * scale).round() / scale;
    (expected - ratio.percent.value).abs() < 0.000_001
}

fn same_displayed_percent(left: ParsedPercent, right: ParsedPercent) -> bool {
    (left.value - right.value).abs() < 0.000_001
}

fn find_markdown_row<'a>(source: &'a str, label: &str) -> Option<&'a str> {
    source.lines().find(|line| {
        let cells = parse_markdown_cells(line);
        cells
            .first()
            .is_some_and(|cell| normalize_markdown(cell) == label)
    })
}

fn parse_markdown_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn normalize_markdown(value: &str) -> String {
    value
        .chars()
        .filter(|character| !matches!(character, '*' | '`' | '_'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn section_between<'a>(source: &'a str, begin: &str, end: &str) -> Option<&'a str> {
    let start = source.find(begin)? + begin.len();
    let remainder = source.get(start..)?;
    let finish = remainder.find(end)?;
    remainder.get(..finish)
}

fn repository_sha(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("running git rev-parse HEAD")?;
    if !output.status.success() {
        return Err(eyre!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let sha = String::from_utf8(output.stdout)
        .context("decoding repository SHA")?
        .trim()
        .to_string();
    let valid = sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid {
        return Err(eyre!("repository SHA is not a full 40-character hex commit: {sha:?}"));
    }
    Ok(sha.to_ascii_lowercase())
}

fn sha256_text(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn not_proven_cell(
    surface: &Surface,
    repository_sha: &str,
    source_sha256: String,
    limitation: &str,
) -> StatusCell {
    not_proven_cell_from_subject(
        surface,
        Subject {
            repository_sha: repository_sha.to_string(),
            source_path: normalize_path(&surface.path),
            source_sha256,
        },
        limitation,
    )
}

fn not_proven_cell_from_subject(
    surface: &Surface,
    subject: Subject,
    limitation: &str,
) -> StatusCell {
    StatusCell {
        schema_version: "generated_status_cell.v1",
        id: surface.id.clone(),
        subject,
        value: StatusValue {
            numerator: None,
            denominator: None,
            percent: None,
            display: "NOT_PROVEN".to_string(),
        },
        evidence: Evidence {
            refs: vec![normalize_path(&surface.path)],
            completeness: Completeness::NotProven,
            freshness: Freshness::NotProven,
        },
        verdict: Verdict::NotProven,
        claim_boundary: surface.claim_boundary.clone(),
        limitations: vec![limitation.to_string()],
    }
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let serialized = serde_json::to_string_pretty(receipt).context("serializing receipt")?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, format!("{serialized}\n"))
        .with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn policy_error(code: &'static str, message: String) -> Finding {
    Finding {
        level: "error",
        code,
        surface_id: "policy".to_string(),
        path: "policy/generated-status-contract.toml".to_string(),
        message,
    }
}

fn surface_error(surface: &Surface, code: &'static str, message: String) -> Finding {
    Finding {
        level: "error",
        code,
        surface_id: surface.id.clone(),
        path: normalize_path(&surface.path),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(path: &str) -> Subject {
        Subject {
            repository_sha: "1".repeat(40),
            source_path: path.to_string(),
            source_sha256: "2".repeat(64),
        }
    }

    fn ratio_surface() -> Surface {
        Surface {
            id: "lsp.protocol".to_string(),
            kind: SurfaceKind::RatioProjection,
            path: PathBuf::from("lsp.md"),
            headline_label: Some("Protocol Compliance".to_string()),
            table_label: Some("Overall".to_string()),
            row_label: None,
            begin_marker: None,
            end_marker: None,
            claim_boundary: "ratio projection".to_string(),
        }
    }

    fn evidence_surface() -> Surface {
        Surface {
            id: "tests.tier_a".to_string(),
            kind: SurfaceKind::EvidenceVerdict,
            path: PathBuf::from("tests.md"),
            headline_label: None,
            table_label: None,
            row_label: Some("Tier A Tests".to_string()),
            begin_marker: None,
            end_marker: None,
            claim_boundary: "test projection".to_string(),
        }
    }

    fn attribution_surface() -> Surface {
        Surface {
            id: "quality.attribution".to_string(),
            kind: SurfaceKind::AttributionTable,
            path: PathBuf::from("quality.md"),
            headline_label: None,
            table_label: None,
            row_label: None,
            begin_marker: Some("<!-- BEGIN -->".to_string()),
            end_marker: Some("<!-- END -->".to_string()),
            claim_boundary: "attribution projection".to_string(),
        }
    }

    #[test]
    fn matching_ratio_projection_passes() -> Result<()> {
        let source = "- **Protocol Compliance**: 100% overall support (125/125)\n\
                      | Area | Implemented | Total | Coverage |\n\
                      | **Overall** | **125** | **125** | **100%** |\n";
        let surface = ratio_surface();
        let mut findings = Vec::new();
        let cell = parse_ratio_projection(&surface, source, subject("lsp.md"), &mut findings)?;
        assert!(findings.is_empty());
        assert_eq!(cell.verdict, Verdict::Proven);
        Ok(())
    }

    #[test]
    fn headline_table_denominator_contradiction_fails() -> Result<()> {
        let source = "- **Protocol Compliance**: 100% overall support (123/123)\n\
                      | Area | Implemented | Total | Coverage |\n\
                      | **Overall** | **123** | **125** | **98%** |\n";
        let surface = ratio_surface();
        let mut findings = Vec::new();
        let cell = parse_ratio_projection(&surface, source, subject("lsp.md"), &mut findings)?;
        assert_eq!(cell.verdict, Verdict::Contradictory);
        assert!(
            findings
                .iter()
                .any(|finding| finding.code == "HEADLINE_TABLE_CONTRADICTION")
        );
        Ok(())
    }

    #[test]
    fn unverified_test_count_cannot_retain_pass() -> Result<()> {
        let source = "| **Tier A Tests** | UNVERIFIED lib tests (discovered) | 100% pass | PASS |";
        let surface = evidence_surface();
        let mut findings = Vec::new();
        let cell = parse_evidence_verdict(&surface, source, subject("tests.md"), &mut findings)?;
        assert_eq!(cell.verdict, Verdict::Contradictory);
        assert!(
            findings
                .iter()
                .any(|finding| finding.code == "UNKNOWN_EVIDENCE_WITH_PASS")
        );
        Ok(())
    }

    #[test]
    fn explicit_incomplete_attribution_is_limited_not_contradictory() -> Result<()> {
        let source = "<!-- BEGIN -->\n| Crate | Mutants | Tests |\n|---|---|---|\n| — | no data yet | no data yet |\n\n> Note: 9381 tests had no crate attribution and are excluded from the per-crate table.\n<!-- END -->";
        let surface = attribution_surface();
        let mut findings = Vec::new();
        let cell = parse_attribution_table(
            &surface,
            source,
            subject("quality.md"),
            &mut findings,
        )?;
        assert!(findings.is_empty());
        assert_eq!(cell.verdict, Verdict::Limited);
        assert_eq!(cell.evidence.completeness, Completeness::Partial);
        Ok(())
    }

    #[test]
    fn unqualified_no_data_attribution_is_contradictory() -> Result<()> {
        let source = "<!-- BEGIN -->\n| Crate | Mutants | Tests |\n|---|---|---|\n| — | no data yet | no data yet |\n<!-- END -->";
        let surface = attribution_surface();
        let mut findings = Vec::new();
        let cell = parse_attribution_table(
            &surface,
            source,
            subject("quality.md"),
            &mut findings,
        )?;
        assert_eq!(cell.verdict, Verdict::Contradictory);
        assert!(
            findings
                .iter()
                .any(|finding| finding.code == "UNQUALIFIED_ATTRIBUTION_GAP")
        );
        Ok(())
    }

    #[test]
    fn proven_cell_requires_complete_current_evidence() {
        let surface = evidence_surface();
        let cell = StatusCell {
            schema_version: "generated_status_cell.v1",
            id: surface.id.clone(),
            subject: subject("tests.md"),
            value: StatusValue {
                numerator: Some(1),
                denominator: Some(1),
                percent: Some(100.0),
                display: "1/1".to_string(),
            },
            evidence: Evidence {
                refs: vec!["tests.md".to_string()],
                completeness: Completeness::NotProven,
                freshness: Freshness::Current,
            },
            verdict: Verdict::Proven,
            claim_boundary: "test".to_string(),
            limitations: Vec::new(),
        };
        let mut findings = Vec::new();
        validate_cell(&cell, &surface, &mut findings);
        assert!(
            findings
                .iter()
                .any(|finding| finding.code == "PROVEN_WITH_INCOMPLETE_EVIDENCE")
        );
    }
}
