//! `issue_close_proof.v1`: the versioned close packet and its validation
//! against a current `issue_contract.v1`.

use std::collections::BTreeSet;

use super::contract::validate_evidence_ref;
use super::model::{
    CLOSE_PACKET_SCHEMA_V1, ChildDispositionRecord, ChildState, ClaimStatement, CloseMode,
    ClosePacket, ControlOutcome, PacketBinding, RowDispositionValue,
};
use super::{CloseProofError, is_digest_hex, is_repository_id, is_stable_token};

impl ClosePacket {
    /// Parse a close packet document. Structural validation is separate so
    /// mis-typed documents and semantically invalid ones stay distinguishable.
    pub fn from_json_str(json: &str) -> Result<Self, CloseProofError> {
        serde_json::from_str(json).map_err(|error| CloseProofError::Schema {
            field: "close_packet".to_string(),
            message: error.to_string(),
        })
    }

    /// Deterministic serialization; a second generation produces no diff.
    pub fn to_canonical_json(&self) -> Result<String, CloseProofError> {
        super::canonical_json(self)
    }

    /// Self-consistency checks that do not need the contract.
    pub fn validate_shape(&self) -> Result<(), CloseProofError> {
        if self.schema_version != CLOSE_PACKET_SCHEMA_V1 {
            return Err(CloseProofError::Schema {
                field: "schema_version".to_string(),
                message: format!(
                    "expected `{CLOSE_PACKET_SCHEMA_V1}`, found `{}`",
                    self.schema_version
                ),
            });
        }
        if !is_repository_id(&self.repository) {
            return Err(CloseProofError::Schema {
                field: "repository".to_string(),
                message: format!("`{}` is not `owner/name`", self.repository),
            });
        }
        if self.issue_number == 0 {
            return Err(CloseProofError::Schema {
                field: "issue_number".to_string(),
                message: "must be positive".to_string(),
            });
        }
        validate_binding(&self.contract_binding)?;
        for evidence in &self.landing_content_proof {
            validate_evidence_ref(evidence)?;
        }
        for subject in &self.landed_subjects {
            if subject.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "landed_subjects".to_string(),
                    message: "landed subjects must not be empty".to_string(),
                });
            }
        }
        for claim in self.established_claims.iter().chain(&self.explicitly_not_established_claims) {
            validate_claim(claim)?;
        }
        for (row_id, disposition) in &self.row_dispositions {
            if !is_stable_token(row_id) {
                return Err(CloseProofError::Schema {
                    field: "row_dispositions.key".to_string(),
                    message: format!("`{row_id}` is not a stable row token"),
                });
            }
            validate_disposition(disposition)?;
        }
        for (control_id, outcome) in &self.negative_control_dispositions {
            if !is_stable_token(control_id) {
                return Err(CloseProofError::Schema {
                    field: "negative_control_dispositions.key".to_string(),
                    message: format!("`{control_id}` is not a stable control token"),
                });
            }
            validate_control_outcome(outcome)?;
        }
        for record in &self.child_dispositions {
            validate_child_record(record)?;
        }
        if let Some(duplicate) = &self.duplicate_of {
            if !is_repository_id(&duplicate.repository) {
                return Err(CloseProofError::Schema {
                    field: "duplicate_of.repository".to_string(),
                    message: format!("`{}` is not `owner/name`", duplicate.repository),
                });
            }
            if duplicate.number == 0 {
                return Err(CloseProofError::Schema {
                    field: "duplicate_of.number".to_string(),
                    message: "must be positive".to_string(),
                });
            }
        }
        if self.requested_close_mode == CloseMode::TrueDuplicate && self.duplicate_of.is_none() {
            return Err(CloseProofError::Coverage {
                message: "true_duplicate close requests must name their duplicate target"
                    .to_string(),
            });
        }
        validate_verdict(&self.verdict)
    }
}

fn validate_binding(binding: &PacketBinding) -> Result<(), CloseProofError> {
    if !is_digest_hex(&binding.contract_issue_body_digest) {
        return Err(CloseProofError::Digest {
            message: "contract_binding.contract_issue_body_digest is not 64 lowercase hex"
                .to_string(),
        });
    }
    if !is_digest_hex(&binding.contract_denominator_digest) {
        return Err(CloseProofError::Digest {
            message: "contract_binding.contract_denominator_digest is not 64 lowercase hex"
                .to_string(),
        });
    }
    if let Some(identity) = &binding.accepted_ruling_identity
        && identity.trim().is_empty()
    {
        return Err(CloseProofError::Schema {
            field: "contract_binding.accepted_ruling_identity".to_string(),
            message: "accepted ruling identity must not be empty".to_string(),
        });
    }
    match &binding.accepted_ruling_digest {
        Some(digest) if !is_digest_hex(digest) => Err(CloseProofError::Digest {
            message: "contract_binding.accepted_ruling_digest is not 64 lowercase hex".to_string(),
        }),
        _ => Ok(()),
    }
}

fn validate_claim(claim: &ClaimStatement) -> Result<(), CloseProofError> {
    if claim.statement.trim().is_empty() {
        return Err(CloseProofError::Schema {
            field: "claims.statement".to_string(),
            message: "claim statements must not be empty".to_string(),
        });
    }
    for row_id in &claim.covers_rows {
        if !is_stable_token(row_id) {
            return Err(CloseProofError::Schema {
                field: "claims.covers_rows".to_string(),
                message: format!("`{row_id}` is not a stable row token"),
            });
        }
    }
    Ok(())
}

fn validate_disposition(disposition: &RowDispositionValue) -> Result<(), CloseProofError> {
    match disposition {
        RowDispositionValue::ProvenCurrentMain { evidence }
        | RowDispositionValue::RemovedSurfaceWithProof { proof: evidence } => {
            validate_evidence_ref(evidence)
        }
        RowDispositionValue::NotApplicableByReviewedRuling { ruling_ref } => {
            if ruling_ref.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "row_dispositions.ruling_ref".to_string(),
                    message: "not_applicable_by_reviewed_ruling requires a ruling reference"
                        .to_string(),
                });
            }
            Ok(())
        }
        RowDispositionValue::TransferredToOpenOwner {
            proposition,
            destination_repository,
            destination_issue,
            destination_contract_identity,
            rationale,
        } => validate_transfer(
            proposition,
            destination_repository,
            *destination_issue,
            destination_contract_identity,
            rationale,
        ),
        RowDispositionValue::NotProven { reason }
        | RowDispositionValue::Contradicted { reason }
        | RowDispositionValue::Stale { reason } => {
            if reason.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "row_dispositions.reason".to_string(),
                    message: "non-satisfying dispositions require an exact reason".to_string(),
                });
            }
            Ok(())
        }
    }
}

pub(crate) fn validate_transfer(
    proposition: &str,
    destination_repository: &str,
    destination_issue: u64,
    destination_contract_identity: &str,
    rationale: &str,
) -> Result<(), CloseProofError> {
    if proposition.trim().is_empty() {
        return Err(CloseProofError::Schema {
            field: "transferred_to_open_owner.proposition".to_string(),
            message: "transfer must name the exact proposition".to_string(),
        });
    }
    if !is_repository_id(destination_repository) {
        return Err(CloseProofError::Schema {
            field: "transferred_to_open_owner.destination_repository".to_string(),
            message: format!("`{destination_repository}` is not `owner/name`"),
        });
    }
    if destination_issue == 0 {
        return Err(CloseProofError::Schema {
            field: "transferred_to_open_owner.destination_issue".to_string(),
            message: "destination issue must be positive".to_string(),
        });
    }
    if !is_digest_hex(destination_contract_identity) {
        return Err(CloseProofError::Digest {
            message: "destination_contract_identity is not 64 lowercase hex characters".to_string(),
        });
    }
    if rationale.trim().is_empty() {
        return Err(CloseProofError::Schema {
            field: "transferred_to_open_owner.rationale".to_string(),
            message: "transfer must state why transfer is permitted".to_string(),
        });
    }
    Ok(())
}

fn validate_control_outcome(outcome: &ControlOutcome) -> Result<(), CloseProofError> {
    match outcome {
        ControlOutcome::Verified => Ok(()),
        ControlOutcome::Failed { reason } | ControlOutcome::NotProven { reason } => {
            if reason.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "negative_control_dispositions.reason".to_string(),
                    message: "failed or unproven controls require an exact reason".to_string(),
                });
            }
            Ok(())
        }
    }
}

fn validate_child_record(record: &ChildDispositionRecord) -> Result<(), CloseProofError> {
    if !is_repository_id(&record.child.repository) {
        return Err(CloseProofError::Schema {
            field: "child_dispositions.child.repository".to_string(),
            message: format!("`{}` is not `owner/name`", record.child.repository),
        });
    }
    if record.child.number == 0 {
        return Err(CloseProofError::Schema {
            field: "child_dispositions.child.number".to_string(),
            message: "must be positive".to_string(),
        });
    }
    match &record.state {
        ChildState::ClosedByPacket { packet_subject } => {
            if packet_subject.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "child_dispositions.packet_subject".to_string(),
                    message: "closed children must name their closing packet subject".to_string(),
                });
            }
            Ok(())
        }
        ChildState::StillOpen => Ok(()),
        ChildState::TransferredToOpenOwner {
            proposition,
            destination_repository,
            destination_issue,
            destination_contract_identity,
            rationale,
        } => validate_transfer(
            proposition,
            destination_repository,
            *destination_issue,
            destination_contract_identity,
            rationale,
        ),
    }
}

fn validate_verdict(verdict: &super::model::CloseVerdict) -> Result<(), CloseProofError> {
    if verdict.reasons.is_empty() || verdict.reasons.iter().any(|reason| reason.trim().is_empty()) {
        return Err(CloseProofError::Schema {
            field: "verdict.reasons".to_string(),
            message: "verdict requires exact, non-empty reasons".to_string(),
        });
    }
    Ok(())
}

/// Validate a packet against its current contract.
///
/// Enforces, at the schema layer (#10380):
/// - the packet binds the current issue body, denominator, and accepted-ruling
///   identity; any movement makes the packet stale;
/// - every denominator row carries exactly one disposition (no silent drops),
///   and no unknown rows are dispositioned;
/// - every negative control and mandatory child is covered exactly;
/// - claim statements only cover rows the contract owns.
///
/// Whether those dispositions satisfy the requested close mode remains CP03's
/// evaluation decision (#10382).
pub fn validate_packet_against_contract(
    packet: &ClosePacket,
    contract: &super::contract::IssueContract,
) -> Result<(), CloseProofError> {
    contract.validate()?;
    packet.validate_shape()?;

    if !contract.allowed_close_modes.contains(&packet.requested_close_mode) {
        return Err(CloseProofError::Coverage {
            message: format!(
                "requested close mode {:?} is not allowed by contract",
                packet.requested_close_mode
            ),
        });
    }

    if packet.repository != contract.repository || packet.issue_number != contract.issue_number {
        return Err(CloseProofError::Identity {
            message: format!(
                "packet targets `{}/{}`, but the contract owns `{}/{}`",
                packet.repository, packet.issue_number, contract.repository, contract.issue_number
            ),
        });
    }

    let identity = &contract.identity;
    if packet.contract_binding.contract_issue_body_digest != identity.issue_body_digest {
        return Err(CloseProofError::Identity {
            message: "the issue body moved after this packet was generated; the packet is stale"
                .to_string(),
        });
    }
    if packet.contract_binding.contract_denominator_digest != identity.denominator_digest {
        return Err(CloseProofError::Identity {
            message: "the denominator moved after this packet was generated; the packet is stale"
                .to_string(),
        });
    }
    match (
        &packet.contract_binding.accepted_ruling_identity,
        &packet.contract_binding.accepted_ruling_digest,
        &identity.accepted_ruling,
    ) {
        (Some(packet_identity), Some(packet_digest), Some(ruling))
            if packet_identity == &ruling.identity && packet_digest == &ruling.digest => {}
        (None, None, None) => {}
        _ => {
            return Err(CloseProofError::Identity {
                message:
                    "the accepted ruling moved after this packet was generated; the packet is stale"
                        .to_string(),
            });
        }
    }

    let expected_rows = contract.row_ids();
    let mut missing: Vec<&str> = expected_rows
        .iter()
        .copied()
        .filter(|id| !packet.row_dispositions.contains_key(*id))
        .collect();
    let unknown: Vec<&String> =
        packet.row_dispositions.keys().filter(|id| !expected_rows.contains(&id.as_str())).collect();
    if !missing.is_empty() {
        missing.sort_unstable();
        return Err(CloseProofError::Coverage {
            message: format!(
                "denominator rows were silently dropped from the packet: {}",
                missing.join(", ")
            ),
        });
    }
    if !unknown.is_empty() {
        let names = unknown.into_iter().map(String::as_str).collect::<Vec<_>>();
        return Err(CloseProofError::Coverage {
            message: format!("packet disposes unknown rows: {}", names.join(", ")),
        });
    }

    let expected_controls: BTreeSet<&str> =
        contract.negative_controls.iter().map(|control| control.control_id.as_str()).collect();
    let dispositioned_controls: BTreeSet<&str> =
        packet.negative_control_dispositions.keys().map(String::as_str).collect();
    let missing_controls: Vec<&str> =
        expected_controls.difference(&dispositioned_controls).copied().collect();
    if !missing_controls.is_empty() {
        return Err(CloseProofError::Coverage {
            message: format!(
                "negative controls were silently dropped from the packet: {}",
                missing_controls.join(", ")
            ),
        });
    }
    let unknown_controls: Vec<&str> =
        dispositioned_controls.difference(&expected_controls).copied().collect();
    if !unknown_controls.is_empty() {
        return Err(CloseProofError::Coverage {
            message: format!(
                "packet disposes unknown negative controls: {}",
                unknown_controls.join(", ")
            ),
        });
    }

    let mut seen_child_dispositions = BTreeSet::new();
    for record in &packet.child_dispositions {
        let identity = (record.child.repository.as_str(), record.child.number);
        if !seen_child_dispositions.insert(identity) {
            return Err(CloseProofError::Coverage {
                message: format!(
                    "duplicate mandatory child disposition `{}/#{}`",
                    record.child.repository, record.child.number
                ),
            });
        }
    }
    let expected_children: BTreeSet<(&str, u64)> =
        contract.mandatory_children.iter().map(|c| (c.repository.as_str(), c.number)).collect();
    let packet_children: BTreeSet<(&str, u64)> = packet
        .child_dispositions
        .iter()
        .map(|record| (record.child.repository.as_str(), record.child.number))
        .collect();
    let missing_children: Vec<String> = expected_children
        .difference(&packet_children)
        .map(|(repo, number)| format!("{repo}/#{number}"))
        .collect();
    if !missing_children.is_empty() {
        return Err(CloseProofError::Coverage {
            message: format!(
                "mandatory child dispositions are missing: {}",
                missing_children.join(", ")
            ),
        });
    }
    let extra_children: Vec<String> = packet_children
        .difference(&expected_children)
        .map(|(repo, number)| format!("{repo}/#{number}"))
        .collect();
    if !extra_children.is_empty() {
        return Err(CloseProofError::Coverage {
            message: format!(
                "packet disposes non-mandatory children: {}",
                extra_children.join(", ")
            ),
        });
    }

    for claim in packet.established_claims.iter().chain(&packet.explicitly_not_established_claims) {
        for row_id in &claim.covers_rows {
            if !expected_rows.contains(&row_id.as_str()) {
                return Err(CloseProofError::Coverage {
                    message: format!("claim statement covers unknown row `{row_id}`"),
                });
            }
        }
    }
    Ok(())
}
