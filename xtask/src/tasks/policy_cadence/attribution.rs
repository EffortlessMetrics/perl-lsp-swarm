//! Candidate-bound attribution for registered time-triggered policy results (#7061).
//!
//! Domain validators remain authoritative for pass/fail. This module evaluates
//! the accepted base and candidate under one explicit observation date and
//! records who owns a governing failure without relabeling it as integration-ready.

use super::{
    CadenceState, SCENARIO_67_MANIFEST, ScenarioWaiver, classify_obligation,
    parse_scenario_waivers, raw_from_scenario_waiver, scenario_waivers_from_text,
};
use crate::tasks::staged::{StagedPathText, read_staged_path_text};
use chrono::NaiveDate;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SCHEMA_VERSION: &str = "policy_attribution.v1";
const OWNER_DISPOSITION_ISSUE: &str = "#6193";

/// Domain governing verdict at the receipt's explicit `as_of` date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoverningResult {
    Pass,
    Failed,
    NotProven,
}

/// Who owns the failure relative to the evaluated candidate (#7061 vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateAttribution {
    CandidateOwned,
    BaseOwned,
    IntegrationInteraction,
    InstrumentOrClockInteraction,
    NotProven,
}

/// CLI configuration for the read-only attribution check.
pub struct AttributionArgs {
    pub base: String,
    pub as_of: String,
    /// When set and different from `as_of`, every row is classified as an
    /// instrument/clock interaction rather than product attribution.
    pub candidate_as_of: Option<String>,
    pub json: PathBuf,
    pub markdown: Option<PathBuf>,
}

/// One immutable attribution row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttributionRow {
    pub record_id: String,
    pub source_kind: String,
    pub source_path: String,
    pub owner: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_issue: Option<String>,
    pub as_of: String,
    pub clock_source: &'static str,
    pub base_cadence_state: CadenceState,
    pub candidate_cadence_state: CadenceState,
    pub governing_result: GoverningResult,
    pub candidate_attribution: CandidateAttribution,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disposition_route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_proven_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limitation: Option<String>,
}

#[derive(Debug, Serialize)]
struct AttributionReceipt {
    schema_version: &'static str,
    base: String,
    candidate_source: &'static str,
    as_of: String,
    candidate_as_of: String,
    clock_source: &'static str,
    rows: Vec<AttributionRow>,
}

/// Compare Scenario 67 waiver records between base and candidate at one clock.
pub fn run(root: &Path, args: AttributionArgs) -> Result<()> {
    let as_of = parse_date(&args.as_of, "--as-of")?;
    let candidate_as_of = match &args.candidate_as_of {
        Some(raw) => parse_date(raw, "--candidate-as-of")?,
        None => as_of,
    };
    let clock_mismatch = candidate_as_of != as_of;
    let receipt = build_receipt(root, &args.base, as_of, candidate_as_of, clock_mismatch)?;
    let json = serde_json::to_string_pretty(&receipt)? + "\n";
    write_output(root, &args.json, &json)?;
    if let Some(markdown) = &args.markdown {
        write_output(root, markdown, &render_markdown(&receipt))?;
    }
    for row in receipt.rows.iter().filter(|row| row.governing_result == GoverningResult::Failed) {
        println!(
            "ATTRIBUTION {}/{} governing={:?} candidate={:?}",
            row.source_kind, row.record_id, row.governing_result, row.candidate_attribution
        );
    }
    println!(
        "Policy attribution: {} Scenario 67 row(s) at as_of {}; wrote {}",
        receipt.rows.len(),
        receipt.as_of,
        args.json.display()
    );
    Ok(())
}

fn build_receipt(
    root: &Path,
    base: &str,
    as_of: NaiveDate,
    candidate_as_of: NaiveDate,
    clock_mismatch: bool,
) -> Result<AttributionReceipt> {
    let base_tree = resolve_base_tree(root, base)?;
    let base_text = read_base(root, SCENARIO_67_MANIFEST, &base_tree)?;
    let candidate_text = read_candidate(root, SCENARIO_67_MANIFEST)?;
    let base_side = parse_waiver_side(base_text.as_deref())?;
    let candidate_side = parse_waiver_side(candidate_text.as_deref())?;
    let mut keys: BTreeSet<String> = BTreeSet::new();
    keys.extend(base_side.unique.keys().cloned());
    keys.extend(candidate_side.unique.keys().cloned());
    keys.extend(base_side.ambiguous.iter().cloned());
    keys.extend(candidate_side.ambiguous.iter().cloned());
    let mut rows = Vec::new();
    for record_id in keys {
        if base_side.ambiguous.contains(&record_id) || candidate_side.ambiguous.contains(&record_id)
        {
            rows.push(ambiguous_waiver_row(&record_id, as_of));
            continue;
        }
        rows.push(attribute_row(
            &record_id,
            base_side.unique.get(&record_id),
            candidate_side.unique.get(&record_id),
            as_of,
            candidate_as_of,
            clock_mismatch,
        ));
    }
    rows.sort_by(|left, right| {
        (&left.source_kind, &left.record_id).cmp(&(&right.source_kind, &right.record_id))
    });
    Ok(AttributionReceipt {
        schema_version: SCHEMA_VERSION,
        base: base.to_string(),
        candidate_source: "working_tree",
        as_of: as_of.to_string(),
        candidate_as_of: candidate_as_of.to_string(),
        clock_source: "explicit_cli",
        rows,
    })
}

type WaiverMap = BTreeMap<String, ScenarioWaiver>;

struct ParsedWaivers {
    unique: WaiverMap,
    ambiguous: BTreeSet<String>,
}

fn parse_waiver_side(text: Option<&str>) -> Result<ParsedWaivers> {
    let mut unique = WaiverMap::new();
    let mut ambiguous = BTreeSet::new();
    let Some(text) = text else {
        return Ok(ParsedWaivers { unique, ambiguous });
    };
    for entry in parse_scenario_waivers(text)? {
        let record_id = format!("{}:{}", entry.project, entry.journey);
        if unique.contains_key(&record_id) || ambiguous.contains(&record_id) {
            unique.remove(&record_id);
            ambiguous.insert(record_id);
        } else {
            unique.insert(record_id, entry);
        }
    }
    Ok(ParsedWaivers { unique, ambiguous })
}

fn scenario_domain_governing(waiver: &ScenarioWaiver, as_of: NaiveDate) -> GoverningResult {
    if waiver.issue == 0 {
        return GoverningResult::NotProven;
    }
    let Ok(expiry) = parse_date(&waiver.expires_after, "expires_after") else {
        return GoverningResult::NotProven;
    };
    if as_of > expiry { GoverningResult::Failed } else { GoverningResult::Pass }
}

fn attribute_row(
    record_id: &str,
    base: Option<&ScenarioWaiver>,
    candidate: Option<&ScenarioWaiver>,
    as_of: NaiveDate,
    candidate_as_of: NaiveDate,
    clock_mismatch: bool,
) -> AttributionRow {
    let source_path = SCENARIO_67_MANIFEST.to_string();
    let source_kind = "scenario_67_error_waiver".to_string();
    if clock_mismatch {
        return AttributionRow {
            record_id: record_id.to_string(),
            source_kind,
            source_path,
            owner: owner_from(candidate.or(base)),
            owner_issue: owner_issue_from(candidate.or(base)),
            as_of: as_of.to_string(),
            clock_source: "explicit_cli",
            base_cadence_state: CadenceState::NotProven,
            candidate_cadence_state: CadenceState::NotProven,
            governing_result: GoverningResult::NotProven,
            candidate_attribution: CandidateAttribution::InstrumentOrClockInteraction,
            disposition_route: None,
            not_proven_reason: Some(
                "base and candidate were evaluated under different explicit as_of dates"
                    .to_string(),
            ),
            limitation: None,
        };
    }
    let (Some(base_waiver), Some(candidate_waiver)) = (base, candidate) else {
        return AttributionRow {
            record_id: record_id.to_string(),
            source_kind,
            source_path,
            owner: owner_from(candidate.or(base)),
            owner_issue: owner_issue_from(candidate.or(base)),
            as_of: as_of.to_string(),
            clock_source: "explicit_cli",
            base_cadence_state: CadenceState::NotProven,
            candidate_cadence_state: CadenceState::NotProven,
            governing_result: GoverningResult::NotProven,
            candidate_attribution: CandidateAttribution::NotProven,
            disposition_route: None,
            not_proven_reason: Some("accepted base or candidate record is unavailable".to_string()),
            limitation: None,
        };
    };
    let base_ob =
        classify_obligation(raw_from_scenario_waiver(base_waiver, SCENARIO_67_MANIFEST), as_of);
    let candidate_ob = classify_obligation(
        raw_from_scenario_waiver(candidate_waiver, SCENARIO_67_MANIFEST),
        candidate_as_of,
    );
    let governing = scenario_domain_governing(candidate_waiver, candidate_as_of);
    let semantics_equal = waiver_semantics_equal(base_waiver, candidate_waiver);
    let attribution = if !semantics_equal {
        if candidate_changes_governed_fields(base_waiver, candidate_waiver)
            || candidate_worsens(base_ob.state, candidate_ob.state)
        {
            CandidateAttribution::CandidateOwned
        } else {
            CandidateAttribution::IntegrationInteraction
        }
    } else {
        CandidateAttribution::BaseOwned
    };
    let (not_proven_reason, limitation) = match attribution {
        CandidateAttribution::NotProven if semantics_equal => {
            (Some("attribution ambiguous under current Scenario 67 projection".to_string()), None)
        }
        CandidateAttribution::IntegrationInteraction => (
            None,
            Some(
                "combined-tree product observation may differ while manifest rows match"
                    .to_string(),
            ),
        ),
        _ => (None, None),
    };
    let disposition_route = (attribution == CandidateAttribution::BaseOwned
        && governing == GoverningResult::Failed)
        .then(|| format!("{OWNER_DISPOSITION_ISSUE} base-owned repair; candidate must not widen unrelated files"));
    AttributionRow {
        record_id: record_id.to_string(),
        source_kind,
        source_path,
        owner: format!("#{}", base_waiver.issue),
        owner_issue: Some(format!("#{}", base_waiver.issue)),
        as_of: as_of.to_string(),
        clock_source: "explicit_cli",
        base_cadence_state: base_ob.state,
        candidate_cadence_state: candidate_ob.state,
        governing_result: governing,
        candidate_attribution: attribution,
        disposition_route,
        not_proven_reason,
        limitation,
    }
}

fn ambiguous_waiver_row(record_id: &str, as_of: NaiveDate) -> AttributionRow {
    AttributionRow {
        record_id: record_id.to_string(),
        source_kind: "scenario_67_error_waiver".to_string(),
        source_path: SCENARIO_67_MANIFEST.to_string(),
        owner: "—".to_string(),
        owner_issue: None,
        as_of: as_of.to_string(),
        clock_source: "explicit_cli",
        base_cadence_state: CadenceState::NotProven,
        candidate_cadence_state: CadenceState::NotProven,
        governing_result: GoverningResult::NotProven,
        candidate_attribution: CandidateAttribution::NotProven,
        disposition_route: None,
        not_proven_reason: Some(
            "duplicate Scenario 67 waiver identity in one manifest side".to_string(),
        ),
        limitation: None,
    }
}

fn waiver_semantics_equal(left: &ScenarioWaiver, right: &ScenarioWaiver) -> bool {
    left.project == right.project
        && left.journey == right.journey
        && left.expected_error_class == right.expected_error_class
        && left.issue == right.issue
        && left.expires_after == right.expires_after
}

fn candidate_changes_governed_fields(base: &ScenarioWaiver, candidate: &ScenarioWaiver) -> bool {
    base.expires_after != candidate.expires_after
        || base.expected_error_class != candidate.expected_error_class
        || base.issue != candidate.issue
}

fn candidate_worsens(base: CadenceState, candidate: CadenceState) -> bool {
    use CadenceState::*;
    matches!(
        (base, candidate),
        (Current, Expired | ReviewOverdue | Invalid | Expiring | NotProven)
            | (ReviewDueSoon, Expired | ReviewOverdue | Invalid)
            | (Expiring, Expired | Invalid)
    )
}

fn owner_from(waiver: Option<&ScenarioWaiver>) -> String {
    waiver.map(|entry| format!("#{}", entry.issue)).unwrap_or_else(|| "—".to_string())
}

fn owner_issue_from(waiver: Option<&ScenarioWaiver>) -> Option<String> {
    waiver.map(|entry| format!("#{}", entry.issue))
}

fn parse_date(raw: &str, field: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| eyre!("{field} must be a real YYYY-MM-DD date: {raw}"))
}

fn resolve_base_tree(root: &Path, base: &str) -> Result<String> {
    let spec = format!("{base}^{{tree}}");
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--verify", "--quiet", &spec])
        .output()
        .with_context(|| format!("failed to resolve base revision {base}"))?;
    let oid = String::from_utf8(output.stdout)
        .map_err(|_| eyre!("git printed a non-UTF-8 object id for base revision {base}"))?
        .trim()
        .to_string();
    if !output.status.success() || oid.is_empty() {
        bail!("base revision `{base}` does not resolve to a tree in this repository");
    }
    Ok(oid)
}

fn read_base(root: &Path, path: &str, base: &str) -> Result<Option<String>> {
    match read_staged_path_text(root, path, Some(base))
        .with_context(|| format!("reading {path} at base {base}"))?
    {
        StagedPathText::Present(text) => Ok(Some(text)),
        StagedPathText::Absent => Ok(None),
        StagedPathText::Binary => {
            Err(eyre!("registered attribution source {path} is not text at base {base}"))
        }
    }
}

fn read_candidate(root: &Path, path: &str) -> Result<Option<String>> {
    let full = root.join(path);
    if !full.exists() {
        return Ok(None);
    }
    fs::read_to_string(&full)
        .map(Some)
        .with_context(|| format!("reading candidate {}", full.display()))
}

fn write_output(root: &Path, path: &Path, contents: &str) -> Result<()> {
    let path = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
}

fn render_markdown(receipt: &AttributionReceipt) -> String {
    let mut output = format!(
        "# Policy candidate attribution\n\nBase `{}`. Candidate `{}`. As of `{}` (candidate clock `{}`).\n\n",
        receipt.base, receipt.candidate_source, receipt.as_of, receipt.candidate_as_of
    );
    output.push_str(
        "| Governing | Attribution | Kind | Record | Owner | Base cadence | Candidate cadence | Route |\n",
    );
    output.push_str("|---|---|---|---|---|---|---|---|\n");
    for row in &receipt.rows {
        output.push_str(&format!(
            "| `{:?}` | `{:?}` | `{}` | `{}` | `{}` | `{:?}` | `{:?}` | {} |\n",
            row.governing_result,
            row.candidate_attribution,
            row.source_kind,
            row.record_id,
            row.owner,
            row.base_cadence_state,
            row.candidate_cadence_state,
            row.disposition_route.as_deref().unwrap_or("—").replace('|', "\\|"),
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluate_fixture_pair(
        base_waiver: &ScenarioWaiver,
        candidate_waiver: &ScenarioWaiver,
        as_of: NaiveDate,
    ) -> AttributionRow {
        let record_id = format!("{}:{}", base_waiver.project, base_waiver.journey);
        attribute_row(&record_id, Some(base_waiver), Some(candidate_waiver), as_of, as_of, false)
    }

    fn waiver(expires_after: &str) -> ScenarioWaiver {
        ScenarioWaiver {
            project: "mojolicious".to_string(),
            journey: "edit_burst_completion".to_string(),
            expected_error_class: "request_superseded".to_string(),
            issue: 4050,
            expires_after: expires_after.to_string(),
        }
    }

    #[test]
    fn current_waiver_before_expiry_governs_pass() {
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 4).expect("valid date");
        let row = evaluate_fixture_pair(&waiver("2026-08-11"), &waiver("2026-08-11"), as_of);
        assert_eq!(row.governing_result, GoverningResult::Pass);
        assert_eq!(row.candidate_attribution, CandidateAttribution::BaseOwned);
    }

    #[test]
    fn invalid_issue_zero_is_not_proven() {
        let mut bad = waiver("2026-08-11");
        bad.issue = 0;
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 4).expect("valid date");
        let row = evaluate_fixture_pair(&bad, &bad, as_of);
        assert_eq!(row.governing_result, GoverningResult::NotProven);
    }

    #[test]
    fn candidate_issue_change_is_candidate_owned() {
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 4).expect("valid date");
        let mut changed = waiver("2026-08-11");
        changed.issue = 0;
        let row = evaluate_fixture_pair(&waiver("2026-08-11"), &changed, as_of);
        assert_eq!(row.candidate_attribution, CandidateAttribution::CandidateOwned);
    }

    #[test]
    fn duplicate_waiver_identity_is_not_proven() -> Result<()> {
        let text = r#"{
          "error_waivers": [
            {"project":"mojolicious","journey":"edit_burst_completion","expected_error_class":"request_superseded","issue":4050,"expires_after":"2026-08-04"},
            {"project":"mojolicious","journey":"edit_burst_completion","expected_error_class":"request_superseded","issue":4050,"expires_after":"2026-08-11"}
          ]
        }"#;
        let side = parse_waiver_side(Some(text))?;
        assert!(side.ambiguous.contains("mojolicious:edit_burst_completion"));
        Ok(())
    }

    #[test]
    fn identical_waiver_expired_after_midnight_is_base_owned() {
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 12).expect("valid date");
        let row = evaluate_fixture_pair(&waiver("2026-08-11"), &waiver("2026-08-11"), as_of);
        assert_eq!(row.governing_result, GoverningResult::Failed);
        assert_eq!(row.candidate_attribution, CandidateAttribution::BaseOwned);
        assert!(row.disposition_route.is_some());
    }

    #[test]
    fn candidate_extending_expiry_without_other_change_is_candidate_owned() {
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 4).expect("valid date");
        let row = evaluate_fixture_pair(&waiver("2026-08-04"), &waiver("2026-08-11"), as_of);
        assert_eq!(row.candidate_attribution, CandidateAttribution::CandidateOwned);
    }

    #[test]
    fn mismatched_clocks_are_instrument_interaction() {
        let record_id = "mojolicious:edit_burst_completion";
        let waiver = waiver("2026-08-11");
        let row = attribute_row(
            record_id,
            Some(&waiver),
            Some(&waiver),
            NaiveDate::from_ymd_opt(2026, 8, 11).expect("valid"),
            NaiveDate::from_ymd_opt(2026, 8, 12).expect("valid"),
            true,
        );
        assert_eq!(row.candidate_attribution, CandidateAttribution::InstrumentOrClockInteraction);
    }

    #[test]
    fn missing_base_record_is_not_proven() {
        let row = attribute_row(
            "mojolicious:edit_burst_completion",
            None,
            Some(&waiver("2026-08-11")),
            NaiveDate::from_ymd_opt(2026, 8, 12).expect("valid"),
            NaiveDate::from_ymd_opt(2026, 8, 12).expect("valid"),
            false,
        );
        assert_eq!(row.candidate_attribution, CandidateAttribution::NotProven);
    }

    #[test]
    fn pr6867_fixture_manifest_projects_base_owned_rows() -> Result<()> {
        let root = crate::utils::project_root()?;
        let fixture =
            root.join("fixtures/policy-attribution/scenario_67_pr6867/golden_editor_workload.json");
        let text = fs::read_to_string(&fixture)?;
        let waivers = scenario_waivers_from_text(&text, SCENARIO_67_MANIFEST)?;
        assert_eq!(waivers.len(), 4, "PR #6867 shape carries four Scenario 67 waivers");
        let as_of = NaiveDate::from_ymd_opt(2026, 8, 12).expect("valid");
        for raw in waivers {
            let parts: Vec<_> = raw.record_id.split(':').collect();
            let entry = ScenarioWaiver {
                project: parts[0].to_string(),
                journey: parts[1].to_string(),
                expected_error_class: raw.expected_debt_class.unwrap_or_default(),
                issue: 4050,
                expires_after: raw.expires.unwrap_or_default(),
            };
            let row = evaluate_fixture_pair(&entry, &entry, as_of);
            assert_eq!(
                row.candidate_attribution,
                CandidateAttribution::BaseOwned,
                "{}",
                raw.record_id
            );
            assert_eq!(row.governing_result, GoverningResult::Failed);
        }
        Ok(())
    }
}
