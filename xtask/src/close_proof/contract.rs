//! `issue_contract.v1`: the versioned representation of the proposition an
//! issue owns, plus its structural validation rules.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::model::{
    CloseMode, ContractIdentity, DenominatorRow, EvidenceRef, ISSUE_CONTRACT_SCHEMA_V1, IssueKind,
    IssueRef, NegativeControlRow, ProofLevel, RulingIdentity, TransferPolicy,
};
use super::{CloseProofError, canonical_json, is_digest_hex, is_repository_id, is_stable_token};

/// Versioned issue contract (`issue_contract.v1`).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueContract {
    pub schema_version: String,
    pub repository: String,
    pub issue_number: u64,
    pub title: String,
    pub kind: IssueKind,
    pub required_proof_level: ProofLevel,
    pub allowed_close_modes: Vec<CloseMode>,
    pub denominator: Vec<DenominatorRow>,
    #[serde(default)]
    pub negative_controls: Vec<NegativeControlRow>,
    #[serde(default)]
    pub mandatory_children: Vec<IssueRef>,
    pub transfer_policy: TransferPolicy,
    #[serde(default)]
    pub domain_evidence_refs: Vec<String>,
    pub identity: ContractIdentity,
}

impl IssueContract {
    /// Parse a contract document. Structural validation is separate so that
    /// mis-typed documents and semantically invalid ones are distinguishable.
    pub fn from_json_str(json: &str) -> Result<Self, CloseProofError> {
        serde_json::from_str(json).map_err(|error| CloseProofError::Schema {
            field: "issue_contract".to_string(),
            message: error.to_string(),
        })
    }

    /// Deterministic serialization; a second generation produces no diff.
    pub fn to_canonical_json(&self) -> Result<String, CloseProofError> {
        canonical_json(self)
    }

    /// Smallest valid contract for an ordinary localized issue. The schema
    /// must not require a heavyweight permanent manifest for every one-line
    /// bug (#10380).
    pub fn minimal_leaf(
        repository: &str,
        issue_number: u64,
        title: &str,
        row_id: &str,
        statement: &str,
        required_proof_level: ProofLevel,
        issue_body_digest: &str,
    ) -> Result<Self, CloseProofError> {
        let denominator = vec![DenominatorRow {
            row_id: row_id.to_string(),
            statement: statement.to_string(),
            required_proof_level,
        }];
        let identity = ContractIdentity {
            issue_body_digest: issue_body_digest.to_string(),
            denominator_digest: compute_denominator_digest(&denominator)?,
            accepted_ruling: None,
        };
        let contract = Self {
            schema_version: ISSUE_CONTRACT_SCHEMA_V1.to_string(),
            repository: repository.to_string(),
            issue_number,
            title: title.to_string(),
            kind: IssueKind::Leaf,
            required_proof_level,
            allowed_close_modes: vec![
                CloseMode::Completed,
                CloseMode::PhaseCompleteIssueRemainsOpen,
                CloseMode::Superseded,
                CloseMode::TrueDuplicate,
                CloseMode::NotPlanned,
            ],
            denominator,
            negative_controls: Vec::new(),
            mandatory_children: Vec::new(),
            transfer_policy: TransferPolicy::default(),
            domain_evidence_refs: Vec::new(),
            identity,
        };
        contract.validate()?;
        Ok(contract)
    }

    /// Structural and referential validation of the contract in isolation.
    pub fn validate(&self) -> Result<(), CloseProofError> {
        if self.schema_version != ISSUE_CONTRACT_SCHEMA_V1 {
            return Err(CloseProofError::Schema {
                field: "schema_version".to_string(),
                message: format!(
                    "expected `{ISSUE_CONTRACT_SCHEMA_V1}`, found `{}`",
                    self.schema_version
                ),
            });
        }
        Self::validate_common_fields(
            &self.repository,
            self.issue_number,
            &self.title,
            &self.allowed_close_modes,
            &self.transfer_policy,
            &self.identity,
            &self.domain_evidence_refs,
        )?;

        let mut seen_rows = std::collections::BTreeSet::new();
        for row in &self.denominator {
            if !is_stable_token(&row.row_id) {
                return Err(CloseProofError::Schema {
                    field: "denominator.row_id".to_string(),
                    message: format!("`{}` is not a stable row token", row.row_id),
                });
            }
            if !seen_rows.insert(row.row_id.as_str()) {
                return Err(CloseProofError::Coverage {
                    message: format!("duplicate denominator row id `{}`", row.row_id),
                });
            }
            if row.statement.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "denominator.statement".to_string(),
                    message: format!("row `{}` has an empty statement", row.row_id),
                });
            }
        }
        if self.denominator.is_empty() {
            return Err(CloseProofError::Coverage {
                message: "denominator must contain at least one row".to_string(),
            });
        }

        let mut seen_controls = std::collections::BTreeSet::new();
        for control in &self.negative_controls {
            if !is_stable_token(&control.control_id) {
                return Err(CloseProofError::Schema {
                    field: "negative_controls.control_id".to_string(),
                    message: format!("`{}` is not a stable control token", control.control_id),
                });
            }
            if !seen_controls.insert(control.control_id.as_str()) {
                return Err(CloseProofError::Coverage {
                    message: format!("duplicate control id `{}`", control.control_id),
                });
            }
            if !seen_rows.contains(control.guards_row_id.as_str()) {
                return Err(CloseProofError::Coverage {
                    message: format!(
                        "control `{}` guards unknown row `{}`",
                        control.control_id, control.guards_row_id
                    ),
                });
            }
            if control.description.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "negative_controls.description".to_string(),
                    message: format!("control `{}` has an empty description", control.control_id),
                });
            }
        }

        let mut seen_children = std::collections::BTreeSet::new();
        for child in &self.mandatory_children {
            if !is_repository_id(&child.repository) {
                return Err(CloseProofError::Schema {
                    field: "mandatory_children.repository".to_string(),
                    message: format!("`{}` is not `owner/name`", child.repository),
                });
            }
            if !seen_children.insert((child.repository.as_str(), child.number)) {
                return Err(CloseProofError::Coverage {
                    message: format!(
                        "duplicate mandatory child `{}/{}`",
                        child.repository, child.number
                    ),
                });
            }
        }
        if self.kind == IssueKind::Controller && self.mandatory_children.is_empty() {
            return Err(CloseProofError::Coverage {
                message: "controller contracts require at least one mandatory child".to_string(),
            });
        }
        // The recorded denominator digest must describe the contract's own
        // rows; recompute so a hand-edited contract cannot carry a forged
        // identity.
        let recomputed = compute_denominator_digest(&self.denominator)?;
        if recomputed != self.identity.denominator_digest {
            return Err(CloseProofError::Digest {
                message: "identity.denominator_digest does not describe this contract's rows"
                    .to_string(),
            });
        }
        Ok(())
    }

    pub(crate) fn validate_common_fields(
        repository: &str,
        issue_number: u64,
        title: &str,
        allowed_close_modes: &[CloseMode],
        transfer_policy: &TransferPolicy,
        identity: &ContractIdentity,
        domain_evidence_refs: &[String],
    ) -> Result<(), CloseProofError> {
        if !is_repository_id(repository) {
            return Err(CloseProofError::Schema {
                field: "repository".to_string(),
                message: format!("`{repository}` is not `owner/name`"),
            });
        }
        if issue_number == 0 {
            return Err(CloseProofError::Schema {
                field: "issue_number".to_string(),
                message: "must be positive".to_string(),
            });
        }
        if title.trim().is_empty() {
            return Err(CloseProofError::Schema {
                field: "title".to_string(),
                message: "must not be empty".to_string(),
            });
        }
        if allowed_close_modes.is_empty() {
            return Err(CloseProofError::Coverage {
                message: "allowed_close_modes must not be empty".to_string(),
            });
        }
        let mut unique_modes = allowed_close_modes.to_vec();
        unique_modes.sort();
        unique_modes.dedup();
        if unique_modes.len() != allowed_close_modes.len() {
            return Err(CloseProofError::Coverage {
                message: "allowed_close_modes contains duplicates".to_string(),
            });
        }
        if transfer_policy.permitted && transfer_policy.conditions.is_empty() {
            return Err(CloseProofError::Coverage {
                message: "permitted transfer policy requires non-empty conditions".to_string(),
            });
        }
        for condition in &transfer_policy.conditions {
            if condition.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "transfer_policy.conditions".to_string(),
                    message: "conditions must not contain empty entries".to_string(),
                });
            }
        }
        validate_identity(identity)?;
        for reference in domain_evidence_refs {
            if reference.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "domain_evidence_refs".to_string(),
                    message: "references must not be empty".to_string(),
                });
            }
        }
        Ok(())
    }

    /// The exact set of stable denominator row IDs, in sorted order.
    pub fn row_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.denominator.iter().map(|row| row.row_id.as_str()).collect();
        ids.sort_unstable();
        ids
    }
}

pub(crate) fn validate_identity(identity: &ContractIdentity) -> Result<(), CloseProofError> {
    if !is_digest_hex(&identity.issue_body_digest) {
        return Err(CloseProofError::Digest {
            message: "identity.issue_body_digest is not 64 lowercase hex characters".to_string(),
        });
    }
    if !is_digest_hex(&identity.denominator_digest) {
        return Err(CloseProofError::Digest {
            message: "identity.denominator_digest is not 64 lowercase hex characters".to_string(),
        });
    }
    match &identity.accepted_ruling {
        Some(ruling) => validate_ruling(ruling),
        None => Ok(()),
    }
}

fn validate_ruling(ruling: &RulingIdentity) -> Result<(), CloseProofError> {
    if ruling.identity.trim().is_empty() {
        return Err(CloseProofError::Schema {
            field: "identity.accepted_ruling.identity".to_string(),
            message: "ruling identity must not be empty".to_string(),
        });
    }
    if !is_digest_hex(&ruling.digest) {
        return Err(CloseProofError::Digest {
            message: "identity.accepted_ruling.digest is not 64 lowercase hex characters"
                .to_string(),
        });
    }
    Ok(())
}

pub(crate) fn validate_evidence_ref(evidence: &EvidenceRef) -> Result<(), CloseProofError> {
    if evidence.producer.trim().is_empty() {
        return Err(CloseProofError::Schema {
            field: "evidence.producer".to_string(),
            message: "producer identity must not be empty".to_string(),
        });
    }
    if evidence.subject.trim().is_empty() {
        return Err(CloseProofError::Schema {
            field: "evidence.subject".to_string(),
            message: "evidence subject must not be empty".to_string(),
        });
    }
    if !is_digest_hex(&evidence.content_digest) {
        return Err(CloseProofError::Digest {
            message: "evidence.content_digest is not 64 lowercase hex characters".to_string(),
        });
    }
    if evidence.reference.trim().is_empty() {
        return Err(CloseProofError::Schema {
            field: "evidence.reference".to_string(),
            message: "evidence reference must not be empty".to_string(),
        });
    }
    Ok(())
}

/// Digest binding the exact denominator membership: sorted stable row IDs,
/// statements, and per-row required proof levels. Any membership or statement
/// change changes the digest, so packets cannot silently bind a moved
/// denominator.
pub fn compute_denominator_digest(rows: &[DenominatorRow]) -> Result<String, CloseProofError> {
    let mut sorted: Vec<&DenominatorRow> = rows.iter().collect();
    sorted.sort_by(|left, right| left.row_id.cmp(&right.row_id));
    let mut hasher = Sha256::new();
    for row in sorted {
        let encoded = canonical_json(row)?;
        hasher.update(encoded.as_bytes());
        hasher.update([0]);
    }
    Ok(hex_digest(&hasher.finalize()))
}

pub(crate) fn hex_digest(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
