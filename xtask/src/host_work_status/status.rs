//! Aggregate host-work status: one subject's four dimension classifications,
//! deterministic aggregate observations, and descriptive cleanup-readiness
//! fields. This type owns no plan, verdict, or authorization surface.

use super::dimension::{
    CapacityFact, ComputeWorkObservation, HostWorkObservationSet, InitiatorReturn,
    MutationOwnership, ProcessTreeFact, UnknownVariantRecord,
};
use super::lifecycle::{
    CleanupReadiness, Dimension, DimensionEvidence, HostWorkClassification, HostWorkLifecycle,
    HostWorkObservationToken, HostWorkReason, merge_classifications,
};
use super::subject::{HOST_WORK_STATUS_SCHEMA_VERSION, HostWorkSubject, ProviderFamily};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusError {
    /// A row or set bound to a different subject was aggregated under this
    /// subject. Cross-subject satisfaction is rejected, never coerced.
    SubjectMismatch { expected: String, actual: String },
}

impl fmt::Display for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatusError::SubjectMismatch { expected, actual } => {
                write!(f, "subject mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for StatusError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostWorkStatus {
    pub schema_version: String,
    pub subject_key: String,
    /// Exactly one classification per [`Dimension`], fixed order.
    pub classifications: Vec<HostWorkClassification>,
    /// Deterministically ordered observation tokens (ascending severity;
    /// worst last). Never a dispatch verdict.
    pub aggregate: Vec<HostWorkObservationToken>,
    /// Descriptive handoffs only; no field authorizes a plan.
    pub cleanup_readiness: Vec<CleanupReadiness>,
    pub missing_providers: Vec<ProviderFamily>,
    pub unknown_provider_variants: Vec<UnknownVariantRecord>,
}

impl HostWorkStatus {
    /// Build the aggregate status for one subject over its observation set.
    /// `supplied_readiness` carries provider-owned readiness facts (e.g.
    /// worktree-cleanup ownership) verbatim; nothing here infers them.
    pub fn build(
        subject: &HostWorkSubject,
        set: &HostWorkObservationSet,
        supplied_readiness: &[CleanupReadiness],
    ) -> Result<HostWorkStatus, StatusError> {
        let expected = subject.subject_key();
        if set.subject_key() != expected.as_str() {
            return Err(StatusError::SubjectMismatch {
                expected: expected.clone(),
                actual: set.subject_key().to_string(),
            });
        }

        let check = |row_key: &str| -> Result<(), StatusError> {
            if row_key == expected.as_str() {
                Ok(())
            } else {
                Err(StatusError::SubjectMismatch {
                    expected: expected.clone(),
                    actual: row_key.to_string(),
                })
            }
        };
        for row in set.logical() {
            check(&row.subject_key)?;
        }
        for row in set.mutation() {
            check(&row.subject_key)?;
        }
        for row in set.compute() {
            check(&row.subject_key)?;
        }
        for row in set.storage() {
            check(&row.subject_key)?;
        }

        let logical = classify_dimension(
            Dimension::Logical,
            set.logical().iter().map(super::lifecycle::classify_logical).collect(),
        );
        let mutation = classify_dimension(
            Dimension::Mutation,
            set.mutation().iter().map(super::lifecycle::classify_mutation).collect(),
        );
        let compute = classify_dimension(
            Dimension::Compute,
            set.compute().iter().map(super::lifecycle::classify_compute).collect(),
        );
        let storage = classify_dimension(
            Dimension::Storage,
            set.storage().iter().map(super::lifecycle::classify_storage).collect(),
        );
        let classifications = vec![logical, mutation, compute, storage];

        let aggregate = Self::aggregate_observations(set, &classifications);
        let cleanup_readiness = Self::cleanup_readiness(set, &classifications, supplied_readiness);

        let mut missing_providers = set.missing_providers().to_vec();
        missing_providers.sort();
        missing_providers.dedup();

        let mut unknown_provider_variants = set.unknown_variants().to_vec();
        unknown_provider_variants.sort();
        unknown_provider_variants.dedup();

        Ok(HostWorkStatus {
            schema_version: HOST_WORK_STATUS_SCHEMA_VERSION.to_string(),
            subject_key: expected,
            classifications,
            aggregate,
            cleanup_readiness,
            missing_providers,
            unknown_provider_variants,
        })
    }

    fn aggregate_observations(
        set: &HostWorkObservationSet,
        classifications: &[HostWorkClassification],
    ) -> Vec<HostWorkObservationToken> {
        let mut tokens: Vec<HostWorkObservationToken> = Vec::new();

        if set.mutation().iter().any(|row| row.salvage_required) {
            tokens.push(HostWorkObservationToken::SalvageRequired);
        }
        if set.mutation().iter().any(|row| row.ownership == MutationOwnership::Contested) {
            tokens.push(HostWorkObservationToken::Collision);
        }
        for row in set.storage() {
            let below_floor = match row.free_capacity {
                CapacityFact::Measured { free_bytes } => {
                    row.configured_floor_bytes.is_some_and(|floor| free_bytes < floor)
                }
                _ => false,
            };
            if below_floor || row.below_configured_floor {
                tokens.push(HostWorkObservationToken::LowDisk);
            }
        }
        for row in set.compute() {
            let saturated = matches!(
                (row.capacity_units_in_use, row.capacity_units_total),
                (Some(in_use), Some(total)) if total > 0 && in_use >= total
            );
            if saturated {
                tokens.push(HostWorkObservationToken::Saturated);
            }
        }
        if classifications.iter().any(|c| c.lifecycle == HostWorkLifecycle::Ambiguous) {
            tokens.push(HostWorkObservationToken::Ambiguous);
        }
        let evidence_incomplete =
            classifications.iter().any(|c| c.evidence == DimensionEvidence::Incomplete);
        if evidence_incomplete
            || !set.missing_providers().is_empty()
            || !set.unknown_variants().is_empty()
        {
            tokens.push(HostWorkObservationToken::NotProven);
        }
        if !set.unknown_variants().is_empty() || !set.missing_providers().is_empty() {
            // Provider evidence is incomplete or contradictory: uncertainty
            // is contagious even when every classified dimension looks fine.
            tokens.push(HostWorkObservationToken::Ambiguous);
        }

        tokens.sort_by_key(|token| token.severity());
        tokens.dedup();
        if tokens.is_empty() {
            tokens.push(HostWorkObservationToken::Healthy);
        }
        tokens
    }

    fn cleanup_readiness(
        set: &HostWorkObservationSet,
        classifications: &[HostWorkClassification],
        supplied: &[CleanupReadiness],
    ) -> Vec<CleanupReadiness> {
        let mut readiness: Vec<CleanupReadiness> = supplied.to_vec();

        if set.mutation().iter().any(|row| row.salvage_required) {
            readiness.push(CleanupReadiness::RequiresSalvage);
        }
        if set
            .storage()
            .iter()
            .any(|row| matches!(row.reclaim_class, super::dimension::ReclaimClass::Approved { .. }))
        {
            readiness.push(CleanupReadiness::EligibleForCacheReclaimPlan);
        }
        if set.compute().iter().any(is_reap_candidate) {
            readiness.push(CleanupReadiness::EligibleForProcessReapPlan);
        }

        let evidence_incomplete =
            classifications.iter().any(|c| c.evidence == DimensionEvidence::Incomplete);
        if evidence_incomplete || !set.unknown_variants().is_empty() {
            readiness.push(CleanupReadiness::NotProven);
        }

        readiness.sort();
        readiness.dedup();
        readiness.sort_by_key(readiness_rank);
        if readiness.is_empty() && !evidence_incomplete {
            readiness.push(CleanupReadiness::ReadOnlyObservationComplete);
        }
        readiness
    }

    /// Deterministic single-line human projection.
    pub fn render_human(&self) -> String {
        let mut out = String::new();
        out.push_str("host work status\n");
        out.push_str("  subject: ");
        out.push_str(&self.subject_key.replace('\u{1f}', "|"));
        out.push('\n');
        for classification in &self.classifications {
            out.push_str(&format!(
                "  {:<8} {:<16} evidence={:<10} reasons=",
                classification.dimension.as_str(),
                classification.lifecycle.as_str(),
                match classification.evidence {
                    DimensionEvidence::Complete => "COMPLETE",
                    DimensionEvidence::Incomplete => "INCOMPLETE",
                }
            ));
            if classification.reasons.is_empty() {
                out.push('-');
            } else {
                let names: Vec<&'static str> =
                    classification.reasons.iter().map(|reason| reason.as_str()).collect();
                out.push_str(&names.join(","));
            }
            out.push('\n');
        }
        let aggregate: Vec<&'static str> =
            self.aggregate.iter().map(|token| token.as_str()).collect();
        out.push_str(&format!("  aggregate: {}\n", aggregate.join(",")));
        if !self.cleanup_readiness.is_empty() {
            let readiness: Vec<String> =
                self.cleanup_readiness.iter().map(render_readiness).collect();
            out.push_str(&format!("  readiness: {}\n", readiness.join(",")));
        }
        if !self.missing_providers.is_empty() {
            let families: Vec<&'static str> =
                self.missing_providers.iter().map(|family| family.as_str()).collect();
            out.push_str(&format!("  missing providers: {}\n", families.join(",")));
        }
        if !self.unknown_provider_variants.is_empty() {
            out.push_str("  unknown provider variants:\n");
            for record in &self.unknown_provider_variants {
                out.push_str(&format!(
                    "    {} {} {} {}\n",
                    record.family.as_str(),
                    record.schema_version,
                    record.source,
                    record.variant
                ));
            }
        }
        out
    }
}

fn is_reap_candidate(row: &ComputeWorkObservation) -> bool {
    matches!(
        &row.process_tree,
        ProcessTreeFact::Live {
            attribution: super::dimension::Attribution::ExactSubjectBinding,
            ..
        }
    ) && row.initiator_returned == InitiatorReturn::Returned
}

fn readiness_rank(readiness: &CleanupReadiness) -> u8 {
    match readiness {
        CleanupReadiness::NotTargetable => 0,
        CleanupReadiness::RequiresSalvage => 1,
        CleanupReadiness::ReadOnlyObservationComplete => 2,
        CleanupReadiness::EligibleForCacheReclaimPlan => 3,
        CleanupReadiness::EligibleForProcessReapPlan => 4,
        CleanupReadiness::WorktreeCleanupOwnedBy { .. } => 5,
        CleanupReadiness::NotProven => 6,
    }
}

fn render_readiness(readiness: &CleanupReadiness) -> String {
    match readiness {
        CleanupReadiness::NotTargetable => "NOT_TARGETABLE".to_string(),
        CleanupReadiness::RequiresSalvage => "REQUIRES_SALVAGE".to_string(),
        CleanupReadiness::ReadOnlyObservationComplete => {
            "READ_ONLY_OBSERVATION_COMPLETE".to_string()
        }
        CleanupReadiness::EligibleForProcessReapPlan => {
            "ELIGIBLE_FOR_PROCESS_REAP_PLAN".to_string()
        }
        CleanupReadiness::EligibleForCacheReclaimPlan => {
            "ELIGIBLE_FOR_CACHE_RECLAIM_PLAN".to_string()
        }
        CleanupReadiness::WorktreeCleanupOwnedBy { owner } => {
            format!("WORKTREE_CLEANUP_OWNED_BY({owner})")
        }
        CleanupReadiness::NotProven => "NOT_PROVEN".to_string(),
    }
}

fn classify_dimension(
    dimension: Dimension,
    rows: Vec<HostWorkClassification>,
) -> HostWorkClassification {
    match merge_classifications(&rows) {
        Some(merged) => merged,
        None => HostWorkClassification {
            dimension,
            lifecycle: HostWorkLifecycle::Ambiguous,
            reasons: vec![HostWorkReason::InstrumentUnavailable],
            evidence: DimensionEvidence::Incomplete,
        },
    }
}
