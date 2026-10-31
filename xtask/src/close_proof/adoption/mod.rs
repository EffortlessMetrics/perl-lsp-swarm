//! Bounded #15627 mapping report under an independently selected caller premise.
//!
//! Selection is outside candidate JSON. Native reads establish a bounded source
//! observation, never root-role authentication, evidence admission or completion.
//! Offline and simulated transport observations always remain Simulation.

mod model;
mod source;
#[cfg(test)]
mod tests;

pub use model::{
    CallerExpectedSelection, CandidateBinding, CandidateMapping, MappingMaterial, MappingReport,
    ReportStatus,
};
pub use source::{SimulationSources, report_live, report_simulation};

use super::{DenominatorRow, NegativeControlRow, ProofLevel, SourceIdentity, content_digest_hex};

const ROW_IDS: [&str; 4] = [
    "cp00.pr-asserted-proof-not-excluded",
    "cp00.pr-genuine-exclusion-detected",
    "cp00.pr-side-variation-isolated",
    "cp00.template-vocabulary-inert",
];

fn section<'a>(body: &'a str, heading: &str) -> Result<&'a str, String> {
    let marker = format!("### {heading}\n\n");
    let mut pieces = body.split(&marker);
    let _ = pieces.next();
    let tail = pieces.next().ok_or_else(|| format!("missing selected section {heading}"))?;
    if pieces.next().is_some() {
        return Err(format!("ambiguous selected section {heading}"));
    }
    Ok(tail.split("\n### ").next().unwrap_or(tail).trim_end_matches('\n'))
}

fn compile_mapping(
    selection: &CallerExpectedSelection,
    sources: &source::ObservedSources,
) -> Result<CandidateMapping, String> {
    let adopted = section(&sources.record, "Adopted acceptance rows")?;
    let acceptance = sources
        .issue
        .split("## Acceptance\n\n")
        .nth(1)
        .and_then(|tail| tail.split("\n## ").next())
        .ok_or_else(|| "missing current issue Acceptance".to_string())?;
    let quotes: Vec<String> = acceptance
        .lines()
        .filter_map(|line| line.strip_prefix("- [ ] ").map(str::to_string))
        .collect();
    if quotes.len() != 4 {
        return Err("pilot requires four current source rows".into());
    }
    let mut rows = Vec::new();
    for (index, row_id) in ROW_IDS.iter().enumerate() {
        let marker = format!("**A{} — `{row_id}`**\n\n", index + 1);
        let tail = adopted
            .split_once(&marker)
            .map(|(_, tail)| tail)
            .ok_or_else(|| format!("missing adopted row {row_id}"))?;
        let row_body = tail.split("\n**A").next().unwrap_or(tail);
        let meaning = row_body
            .split_once("Meaning: ")
            .map(|(_, tail)| tail)
            .ok_or_else(|| format!("missing meaning for {row_id}"))?;
        let meaning = meaning
            .split("\n\nRequired proof level")
            .next()
            .unwrap_or(meaning)
            .trim_end_matches('\n');
        rows.push(DenominatorRow {
            row_id: (*row_id).into(),
            statement: meaning.into(),
            required_proof_level: ProofLevel::Mechanism,
        });
    }
    if !adopted.contains("Required proof level for A1–A4: **mechanism**") {
        return Err("missing declared mechanism proof boundary".into());
    }
    let review =
        section(&sources.record, "Independent challenge and root disposition")?.to_string();
    if !review.contains("Independent LLM challenge") || !review.contains("The root resolves") {
        return Err("missing declared review/disposition basis".into());
    }
    let controls = [ROW_IDS[0], ROW_IDS[3]]
        .iter()
        .enumerate()
        .map(|(index, guard)| NegativeControlRow {
            control_id: format!("cp00.a2-opposing-control.{}", index + 1),
            guards_row_id: (*guard).into(),
            description: rows[1].statement.clone(),
        })
        .collect();
    let material = MappingMaterial {
        required_proof_level: ProofLevel::Mechanism, rows, negative_controls: controls,
        source_acceptance_quotes: quotes,
        lexical_disposition: section(&sources.record, "Explicit lexical disposition")?.into(),
        lexical_inputs: ["installed", "public", "packaged", "presentation", "release", "released", "actual host"].iter().map(|term| (*term).into()).collect(),
        retained_non_goals_and_context: section(&sources.record, "Non-goals and retained context")?.into(),
        declared_review_basis: SourceIdentity { identity: format!("issuecomment:{}/declared-review", selection.comment_id), digest: content_digest_hex(review.as_bytes()) },
        declared_review_text: review,
        template: SourceIdentity { identity: format!("{}:.github/PULL_REQUEST_TEMPLATE.md", selection.policy_commit), digest: selection.template_digest.into() },
        mandatory_children: Vec::new(), permitted_transferred_rows: Vec::new(),
        limitations: vec!["Partial mapping; no inferred issue kind or close modes.".into(), "Declared review basis; caller independently resolves role authority and ruling conflicts.".into(), "Untested literal forms are NOT_PROVEN, not excluded or complete.".into(), "No producer admission, semantic completion or issue-close authorization.".into()],
    };
    CandidateMapping::from_material(selection.binding(), material)
        .map_err(|error| error.to_string())
}

fn match_candidate(
    candidate: &CandidateMapping,
    compiled: &CandidateMapping,
) -> Result<(), String> {
    candidate.validate().map_err(|error| error.to_string())?;
    if candidate.canonicalized().map_err(|error| error.to_string())?
        != compiled.canonicalized().map_err(|error| error.to_string())?
    {
        return Err("Candidate mapping differs from independently selected source material.".into());
    }
    Ok(())
}

fn not_proven(reason: impl Into<String>) -> MappingReport {
    MappingReport { status: ReportStatus::NotProven, mapping: None, reason: Some(reason.into()),
        caller_precondition: "Trusted reviewed base and independently root-selected singleton; caller resolves authority/conflicts.".into(),
        observation_limit: "First/last matching reads are bounded observations, not an atomic snapshot or absence of ABA edits.".into(),
        semantic_completion: "not_evaluated".into(), evidence_admission: "not_evaluated".into(), issue_close_authorized: false }
}

fn report(
    selection: &CallerExpectedSelection,
    candidate: &CandidateMapping,
    sources: source::ObservedSources,
) -> MappingReport {
    let compiled = match compile_mapping(selection, &sources) {
        Ok(value) => value,
        Err(error) => return not_proven(error),
    };
    if let Err(error) = match_candidate(candidate, &compiled) {
        return not_proven(error);
    }
    MappingReport { status: if sources.native { ReportStatus::AdoptedContractMapping } else { ReportStatus::Simulation },
        mapping: Some(compiled), reason: None,
        caller_precondition: "Trusted reviewed base and independently root-selected singleton; caller resolves authority/conflicts. Software does not authenticate the root role.".into(),
        observation_limit: "First/last matching reads are bounded observations, not an atomic snapshot or absence of ABA edits.".into(),
        semantic_completion: "not_evaluated".into(), evidence_admission: "not_evaluated".into(), issue_close_authorized: false }
}
