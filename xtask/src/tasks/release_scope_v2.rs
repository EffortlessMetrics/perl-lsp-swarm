//! Structural model for the final v0.18 release-scope receipt.
//!
//! This module can represent a final freeze and serialize it deterministically.
//! It deliberately performs no GitHub observation, readiness evaluation,
//! preparation, synchronization, candidate selection, or publication.

#![allow(
    dead_code,
    reason = "FF01 registers the structural model before FF02 #13856 adds emit/verify consumers"
)]

use serde::{Deserialize, Serialize};

pub(crate) const RELEASE_SCOPE_V2_SCHEMA_PATH: &str = "schemas/release_scope.v2.schema.json";
pub(crate) const RELEASE_SCOPE_V2_SCHEMA_VERSION: &str = "perl_lsp.release_scope.v2";
const RELEASE_SCOPE_V2_CHECK: &str = "release-scope-v2";
const RELEASE: &str = "0.18.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReceiptEvent {
    Local,
    Push,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReceiptVerdict {
    Pass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ReleaseTrack {
    #[serde(rename = "public-beta")]
    PublicBeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ReleasePhase {
    #[serde(rename = "final-frozen")]
    FinalFrozen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceKind {
    GithubIssue,
    GithubIssueComment,
    GithubPull,
    GithubReview,
    GithubCheck,
    RepositoryBlob,
    RepositoryReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceRef {
    pub(crate) kind: EvidenceKind,
    #[serde(rename = "ref")]
    pub(crate) reference: String,
    pub(crate) digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PullRequestState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ReleaseDisposition {
    #[serde(rename = "0.18-blocker")]
    Blocker018,
    #[serde(rename = "0.18-candidate")]
    Candidate018,
    #[serde(rename = "post-0.18")]
    Post018,
    #[serde(rename = "superseded")]
    Superseded,
    #[serde(rename = "already-included")]
    AlreadyIncluded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservedPullRequest {
    pub(crate) number: u64,
    pub(crate) head_sha: String,
    pub(crate) state: PullRequestState,
    pub(crate) is_draft: bool,
    pub(crate) disposition: ReleaseDisposition,
    pub(crate) controlling_issue: Option<u64>,
    pub(crate) evidence: EvidenceRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BlockerStatus {
    Resolved,
    BoundedLimitation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlockerBinding {
    pub(crate) blocker_id: String,
    pub(crate) status: BlockerStatus,
    pub(crate) proof: EvidenceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KnownLimitation {
    pub(crate) id: String,
    pub(crate) statement: String,
    pub(crate) evidence: EvidenceRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClaimStrength {
    Full,
    Bounded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductClaim {
    pub(crate) id: String,
    pub(crate) strength: ClaimStrength,
    pub(crate) statement: String,
    pub(crate) evidence: EvidenceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubjectEvidence {
    pub(crate) subject_sha: String,
    pub(crate) evidence: EvidenceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TopologyBinding {
    pub(crate) subject_sha: String,
    pub(crate) digest: String,
    pub(crate) evidence: EvidenceRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum AllowedChange {
    #[serde(rename = "release-blocker")]
    ReleaseBlocker,
    #[serde(rename = "regression-fix")]
    RegressionFix,
    #[serde(rename = "release-proof")]
    ReleaseProof,
    #[serde(rename = "packaging")]
    Packaging,
    #[serde(rename = "version-notes-status")]
    VersionNotesStatus,
    #[serde(rename = "release-integrity")]
    ReleaseIntegrity,
    #[serde(rename = "release-lineage")]
    ReleaseLineage,
}

const EXPECTED_ALLOWED_CHANGES: &[AllowedChange] = &[
    AllowedChange::ReleaseBlocker,
    AllowedChange::RegressionFix,
    AllowedChange::ReleaseProof,
    AllowedChange::Packaging,
    AllowedChange::VersionNotesStatus,
    AllowedChange::ReleaseIntegrity,
    AllowedChange::ReleaseLineage,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FreezeRules {
    pub(crate) feature_intake_closed: bool,
    pub(crate) allowed_changes: Vec<AllowedChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum InvalidationKind {
    #[serde(rename = "product-change")]
    ProductChange,
    #[serde(rename = "release-metadata-change")]
    ReleaseMetadataChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum InvalidatedStage {
    #[serde(rename = "freeze")]
    Freeze,
    #[serde(rename = "preparation")]
    Preparation,
    #[serde(rename = "publication-sync")]
    PublicationSync,
    #[serde(rename = "candidate")]
    Candidate,
    #[serde(rename = "authorization")]
    Authorization,
}

const PRODUCT_INVALIDATIONS: &[InvalidatedStage] = &[
    InvalidatedStage::Freeze,
    InvalidatedStage::Preparation,
    InvalidatedStage::PublicationSync,
    InvalidatedStage::Candidate,
    InvalidatedStage::Authorization,
];

const RELEASE_METADATA_INVALIDATIONS: &[InvalidatedStage] = &[
    InvalidatedStage::Preparation,
    InvalidatedStage::PublicationSync,
    InvalidatedStage::Candidate,
    InvalidatedStage::Authorization,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvalidationClass {
    pub(crate) class: InvalidationKind,
    pub(crate) invalidates: Vec<InvalidatedStage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvalidationPolicy {
    pub(crate) product_change: InvalidationClass,
    pub(crate) release_metadata_change: InvalidationClass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReleaseScopeV2 {
    pub(crate) check: String,
    pub(crate) schema_version: String,
    pub(crate) event: ReceiptEvent,
    pub(crate) verdict: ReceiptVerdict,
    pub(crate) release: String,
    pub(crate) track: ReleaseTrack,
    pub(crate) phase: ReleasePhase,
    pub(crate) observation_sha: String,
    pub(crate) frozen_product_sha: String,
    pub(crate) prepared_swarm_sha: Option<String>,
    pub(crate) observed_pull_requests: Vec<ObservedPullRequest>,
    pub(crate) blockers: Vec<BlockerBinding>,
    pub(crate) known_limitations: Vec<KnownLimitation>,
    pub(crate) product_claims: Vec<ProductClaim>,
    pub(crate) topology: TopologyBinding,
    pub(crate) installed_acceptance: SubjectEvidence,
    pub(crate) release_integrity: SubjectEvidence,
    pub(crate) lineage: SubjectEvidence,
    pub(crate) freeze_rules: FreezeRules,
    pub(crate) invalidation: InvalidationPolicy,
}

impl ReleaseScopeV2 {
    pub(crate) fn validate(&self) -> Result<(), String> {
        require(self.check == RELEASE_SCOPE_V2_CHECK, "check must be release-scope-v2")?;
        require(
            self.schema_version == RELEASE_SCOPE_V2_SCHEMA_VERSION,
            "schema_version must be perl_lsp.release_scope.v2",
        )?;
        require(self.release == RELEASE, "release must be 0.18.0")?;
        validate_sha(&self.observation_sha, "observation_sha")?;
        validate_sha(&self.frozen_product_sha, "frozen_product_sha")?;
        if let Some(prepared) = &self.prepared_swarm_sha {
            validate_sha(prepared, "prepared_swarm_sha")?;
        }

        require(
            !self.observed_pull_requests.is_empty(),
            "observed_pull_requests must be non-empty",
        )?;
        validate_strictly_increasing_u64(
            self.observed_pull_requests.iter().map(|item| item.number),
            "observed_pull_requests.number",
        )?;
        for (index, item) in self.observed_pull_requests.iter().enumerate() {
            validate_sha(
                &item.head_sha,
                &format!("observed_pull_requests[{index}].head_sha"),
            )?;
            if let Some(issue) = item.controlling_issue {
                require(
                    issue > 0,
                    format!("observed_pull_requests[{index}].controlling_issue must be positive"),
                )?;
            }
            validate_evidence(
                &item.evidence,
                &format!("observed_pull_requests[{index}].evidence"),
            )?;
        }

        require(!self.blockers.is_empty(), "blockers must be non-empty")?;
        validate_sorted_identifiers(
            &self
                .blockers
                .iter()
                .map(|item| item.blocker_id.as_str())
                .collect::<Vec<_>>(),
            "blockers.blocker_id",
        )?;
        for (index, blocker) in self.blockers.iter().enumerate() {
            validate_evidence(&blocker.proof, &format!("blockers[{index}].proof"))?;
        }

        validate_sorted_identifiers(
            &self
                .known_limitations
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            "known_limitations.id",
        )?;
        for (index, limitation) in self.known_limitations.iter().enumerate() {
            validate_statement(
                &limitation.statement,
                &format!("known_limitations[{index}].statement"),
            )?;
            validate_evidence(
                &limitation.evidence,
                &format!("known_limitations[{index}].evidence"),
            )?;
        }

        require(!self.product_claims.is_empty(), "product_claims must be non-empty")?;
        validate_sorted_identifiers(
            &self
                .product_claims
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            "product_claims.id",
        )?;
        for (index, claim) in self.product_claims.iter().enumerate() {
            validate_statement(
                &claim.statement,
                &format!("product_claims[{index}].statement"),
            )?;
            validate_evidence(
                &claim.evidence,
                &format!("product_claims[{index}].evidence"),
            )?;
        }

        validate_sha(&self.topology.subject_sha, "topology.subject_sha")?;
        validate_digest(&self.topology.digest, "topology.digest")?;
        validate_evidence(&self.topology.evidence, "topology.evidence")?;
        validate_subject_evidence(&self.installed_acceptance, "installed_acceptance")?;
        validate_subject_evidence(&self.release_integrity, "release_integrity")?;
        validate_subject_evidence(&self.lineage, "lineage")?;

        require(
            self.freeze_rules.feature_intake_closed,
            "freeze_rules.feature_intake_closed must be true",
        )?;
        require(
            self.freeze_rules.allowed_changes.as_slice() == EXPECTED_ALLOWED_CHANGES,
            "freeze_rules.allowed_changes must equal the final-freeze allowlist",
        )?;

        validate_invalidation_class(
            &self.invalidation.product_change,
            InvalidationKind::ProductChange,
            PRODUCT_INVALIDATIONS,
            "invalidation.product_change",
        )?;
        validate_invalidation_class(
            &self.invalidation.release_metadata_change,
            InvalidationKind::ReleaseMetadataChange,
            RELEASE_METADATA_INVALIDATIONS,
            "invalidation.release_metadata_change",
        )?;
        require(
            self.invalidation.product_change != self.invalidation.release_metadata_change,
            "product and release-metadata invalidation classes must remain distinct",
        )?;

        Ok(())
    }
}

pub(crate) fn parse_release_scope_v2(input: &str) -> Result<ReleaseScopeV2, String> {
    let model: ReleaseScopeV2 = serde_json::from_str(input)
        .map_err(|error| format!("invalid release_scope.v2 JSON: {error}"))?;
    model.validate()?;
    Ok(model)
}

pub(crate) fn canonical_release_scope_v2(model: &ReleaseScopeV2) -> Result<String, String> {
    model.validate()?;
    let mut output = serde_json::to_string_pretty(model)
        .map_err(|error| format!("cannot serialize release_scope.v2: {error}"))?;
    output.push('\n');
    Ok(output)
}

fn validate_subject_evidence(value: &SubjectEvidence, name: &str) -> Result<(), String> {
    validate_sha(&value.subject_sha, &format!("{name}.subject_sha"))?;
    validate_evidence(&value.evidence, &format!("{name}.evidence"))
}

fn validate_invalidation_class(
    value: &InvalidationClass,
    expected_kind: InvalidationKind,
    expected_stages: &[InvalidatedStage],
    name: &str,
) -> Result<(), String> {
    require(value.class == expected_kind, format!("{name}.class is invalid"))?;
    require(
        value.invalidates.as_slice() == expected_stages,
        format!("{name}.invalidates must equal its closed ordered stage set"),
    )
}

fn validate_evidence(value: &EvidenceRef, name: &str) -> Result<(), String> {
    validate_digest(&value.digest, &format!("{name}.digest"))?;
    validate_statement(&value.reference, &format!("{name}.ref"))?;
    let valid = match value.kind {
        EvidenceKind::GithubIssue => valid_numbered_url(
            &value.reference,
            "https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/",
        ),
        EvidenceKind::GithubIssueComment => valid_anchored_numbered_url(
            &value.reference,
            "https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/",
            "#issuecomment-",
        ),
        EvidenceKind::GithubPull => valid_numbered_url(
            &value.reference,
            "https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/",
        ),
        EvidenceKind::GithubReview => valid_anchored_numbered_url(
            &value.reference,
            "https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/",
            "#pullrequestreview-",
        ),
        EvidenceKind::GithubCheck => valid_check_url(&value.reference),
        EvidenceKind::RepositoryBlob | EvidenceKind::RepositoryReceipt => {
            valid_repository_ref(&value.reference)
        }
    };
    require(valid, format!("{name}.ref does not match {:?}", value.kind))
}

fn valid_numbered_url(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(valid_positive_decimal)
}

fn valid_anchored_numbered_url(value: &str, prefix: &str, anchor: &str) -> bool {
    let Some(remainder) = value.strip_prefix(prefix) else {
        return false;
    };
    let Some((number, anchor_number)) = remainder.split_once(anchor) else {
        return false;
    };
    valid_positive_decimal(number) && valid_positive_decimal(anchor_number)
}

fn valid_check_url(value: &str) -> bool {
    let Some(remainder) = value
        .strip_prefix("https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/")
    else {
        return false;
    };
    let Some((run, job)) = remainder.split_once("/job/") else {
        return false;
    };
    valid_positive_decimal(run) && valid_positive_decimal(job)
}

fn valid_repository_ref(value: &str) -> bool {
    let Some(remainder) = value.strip_prefix("repo:") else {
        return false;
    };
    let Some((path, sha)) = remainder.rsplit_once('@') else {
        return false;
    };
    !path.is_empty()
        && !path.contains('@')
        && !path.chars().any(char::is_whitespace)
        && is_sha(sha)
}

fn valid_positive_decimal(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('0')
        && value.as_bytes().iter().all(u8::is_ascii_digit)
}

fn validate_sorted_identifiers(values: &[&str], name: &str) -> Result<(), String> {
    for (index, value) in values.iter().enumerate() {
        require(is_identifier(value), format!("{name}[{index}] is invalid"))?;
    }
    for pair in values.windows(2) {
        require(pair[0] < pair[1], format!("{name} must be sorted and unique"))?;
    }
    Ok(())
}

fn validate_strictly_increasing_u64(
    values: impl Iterator<Item = u64>,
    name: &str,
) -> Result<(), String> {
    let mut previous = None;
    for value in values {
        require(value > 0, format!("{name} must contain positive numbers"))?;
        if let Some(previous_value) = previous {
            require(
                previous_value < value,
                format!("{name} must be sorted and unique"),
            )?;
        }
        previous = Some(value);
    }
    Ok(())
}

fn validate_statement(value: &str, name: &str) -> Result<(), String> {
    require(
        !value.is_empty() && value.trim() == value,
        format!("{name} must be a non-empty trimmed string"),
    )
}

fn validate_sha(value: &str, name: &str) -> Result<(), String> {
    require(is_sha(value), format!("{name} must be a lowercase 40-character SHA"))
}

fn validate_digest(value: &str, name: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{name} must be sha256:<64 lowercase hex>"));
    };
    require(
        hex.len() == 64 && hex.as_bytes().iter().all(u8::is_ascii_hexdigit),
        format!("{name} must be sha256:<64 lowercase hex>"),
    )?;
    require(
        hex.as_bytes().iter().all(|byte| !byte.is_ascii_uppercase()),
        format!("{name} must use lowercase hex"),
    )
}

fn is_sha(value: &str) -> bool {
    value.len() == 40
        && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
        && value.as_bytes().iter().all(|byte| !byte.is_ascii_uppercase())
}

fn is_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit()
        || !bytes[bytes.len() - 1].is_ascii_lowercase()
            && !bytes[bytes.len() - 1].is_ascii_digit()
    {
        return false;
    }
    let mut previous_was_separator = false;
    for &byte in bytes {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            previous_was_separator = false;
        } else if matches!(byte, b'.' | b'_' | b'-') && !previous_was_separator {
            previous_was_separator = true;
        } else {
            return false;
        }
    }
    true
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

#[cfg(test)]
mod tests;
