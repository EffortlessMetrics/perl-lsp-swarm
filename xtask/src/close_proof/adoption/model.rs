use super::super::{
    CloseProofError, DenominatorRow, IssueRef, NegativeControlRow, ProofLevel, SourceIdentity,
    content_digest_hex, wire,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Invocation precondition, not candidate-deserializable role authentication.
#[derive(Clone, Debug)]
pub struct CallerExpectedSelection {
    pub(super) repository: &'static str,
    pub(super) issue: u64,
    pub(super) comment_id: u64,
    pub(super) record_digest: &'static str,
    pub(super) issue_digest: &'static str,
    pub(super) policy_commit: &'static str,
    pub(super) template_blob: &'static str,
    pub(super) template_digest: &'static str,
    pub(super) selected_rulings: Vec<u64>,
    pub(super) conflicts_resolved: bool,
}

impl CallerExpectedSelection {
    /// Invoke only after an accountable root/issue integrator independently
    /// selects this exact reviewed mapping and resolves ruling conflicts.
    /// A shared account, candidate flag or software call does not prove that role.
    pub fn root_selected_15627() -> Self {
        Self {
            repository: "EffortlessMetrics/perl-lsp-swarm",
            issue: 15627,
            comment_id: 6001911471,
            record_digest: "18b36f2bb55b2db403bde80fddeb1d3cefe4846f087ce34fce60689e2dfae726",
            issue_digest: "f681cf7769d8da807e02ccff203dd1030a33952c548b6cb1315f875e0ca1ac7e",
            policy_commit: "6b774b54811572291a657067a6e1b4f9072f9ebe",
            template_blob: "764f0da5d2bbf40be0e5576c8a2b30a87bce0656",
            template_digest: "2b0e27b17a174164bf6c3eba0fa42e80792f2d315a60c961f8195d8b404e4215",
            selected_rulings: vec![6001911471],
            conflicts_resolved: true,
        }
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        if !self.conflicts_resolved || self.selected_rulings != [self.comment_id] {
            return Err("unresolved or competing caller-selected ruling set".into());
        }
        let fixed = Self::root_selected_15627();
        if self.binding() != fixed.binding() {
            return Err("unsupported caller selection; do not silently adopt another record".into());
        }
        Ok(())
    }
    pub(super) fn binding(&self) -> CandidateBinding {
        CandidateBinding {
            repository: self.repository.into(),
            issue: self.issue,
            comment_id: self.comment_id,
            record_digest: self.record_digest.into(),
            issue_digest: self.issue_digest.into(),
            policy_commit: self.policy_commit.into(),
            template_blob: self.template_blob.into(),
            template_digest: self.template_digest.into(),
        }
    }
}

/// Untrusted candidate claims. These fields never select the observed source.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateBinding {
    pub repository: String,
    pub issue: u64,
    pub comment_id: u64,
    pub record_digest: String,
    pub issue_digest: String,
    pub policy_commit: String,
    pub template_blob: String,
    pub template_digest: String,
}

/// Partial report view, reusing existing contract types; no issue kind/close modes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingMaterial {
    pub required_proof_level: ProofLevel,
    pub rows: Vec<DenominatorRow>,
    pub negative_controls: Vec<NegativeControlRow>,
    pub source_acceptance_quotes: Vec<String>,
    pub lexical_disposition: String,
    pub lexical_inputs: Vec<String>,
    pub retained_non_goals_and_context: String,
    pub declared_review_basis: SourceIdentity,
    pub declared_review_text: String,
    pub template: SourceIdentity,
    pub mandatory_children: Vec<IssueRef>,
    pub permitted_transferred_rows: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateMapping {
    pub schema_version: String,
    pub binding: CandidateBinding,
    pub material: MappingMaterial,
    pub mapping_digest: String,
}

impl CandidateMapping {
    pub fn from_json_str(raw: &str) -> Result<Self, CloseProofError> {
        wire::from_json_str(raw, "candidate_mapping")
    }
    pub fn from_material(
        binding: CandidateBinding,
        material: MappingMaterial,
    ) -> Result<Self, CloseProofError> {
        let mut value = Self {
            schema_version: "issue_contract_mapping_candidate.v1".into(),
            binding,
            material,
            mapping_digest: String::new(),
        };
        value.mapping_digest = value.compute_digest()?;
        Ok(value)
    }
    pub fn compute_digest(&self) -> Result<String, CloseProofError> {
        let canonical = self.canonicalized()?;
        let mut material = canonical;
        material.mapping_digest.clear();
        let bytes = serde_json::to_vec(&material).map_err(|error| CloseProofError::Schema {
            field: "candidate_mapping".into(),
            message: error.to_string(),
        })?;
        Ok(content_digest_hex(&bytes))
    }
    pub(super) fn validate(&self) -> Result<(), CloseProofError> {
        if self.schema_version != "issue_contract_mapping_candidate.v1"
            || self.compute_digest()? != self.mapping_digest
        {
            return Err(CloseProofError::Identity {
                message: "candidate shape/hash does not match its own declared material".into(),
            });
        }
        Ok(())
    }
    pub(super) fn canonicalized(&self) -> Result<Self, CloseProofError> {
        let mut result = self.clone();
        let unique = |items: Vec<String>| -> Result<(), CloseProofError> {
            let mut seen = BTreeSet::new();
            for value in items {
                if !seen.insert(value) {
                    return Err(CloseProofError::Coverage {
                        message: "duplicate semantic set member".into(),
                    });
                }
            }
            Ok(())
        };
        unique(result.material.rows.iter().map(|row| row.row_id.clone()).collect())?;
        unique(
            result.material.negative_controls.iter().map(|row| row.control_id.clone()).collect(),
        )?;
        unique(result.material.lexical_inputs.clone())?;
        unique(result.material.limitations.clone())?;
        unique(result.material.source_acceptance_quotes.clone())?;
        unique(result.material.permitted_transferred_rows.clone())?;
        unique(
            result
                .material
                .mandatory_children
                .iter()
                .map(|child| format!("{}#{}", child.repository, child.number))
                .collect(),
        )?;
        let row_ids: BTreeSet<&str> =
            result.material.rows.iter().map(|row| row.row_id.as_str()).collect();
        if result
            .material
            .negative_controls
            .iter()
            .any(|control| !row_ids.contains(control.guards_row_id.as_str()))
        {
            return Err(CloseProofError::Coverage { message: "dangling mandatory guard".into() });
        }
        result.material.rows.sort_by(|a, b| a.row_id.cmp(&b.row_id));
        result.material.negative_controls.sort_by(|a, b| a.control_id.cmp(&b.control_id));
        result.material.lexical_inputs.sort();
        result.material.limitations.sort();
        result.material.source_acceptance_quotes.sort();
        result.material.permitted_transferred_rows.sort();
        result
            .material
            .mandatory_children
            .sort_by(|a, b| (&a.repository, a.number).cmp(&(&b.repository, b.number)));
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Simulation,
    NotProven,
    AdoptedContractMapping,
}

/// Mapping/provenance report only. No domain evidence or terminal verdict.
#[derive(Clone, Debug, Serialize)]
pub struct MappingReport {
    pub status: ReportStatus,
    pub mapping: Option<CandidateMapping>,
    pub reason: Option<String>,
    pub caller_precondition: String,
    pub observation_limit: String,
    pub semantic_completion: String,
    pub evidence_admission: String,
    pub issue_close_authorized: bool,
}
