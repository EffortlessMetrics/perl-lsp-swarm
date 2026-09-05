//! The fail-closed trust check the status reader applies before rendering.
//!
//! Split from `accuracy.rs` for the 400-line gate in `update_status::mod_tests`,
//! and because this is where the reader must mirror the generator's
//! `validate_legacy_population_evidence`: a shape one accepts and the other
//! refuses is a contract split that only surfaces after publication.

use super::{FORBIDDEN_TRUST_FIELDS, ParserAccuracyArtifactSummary, ParserAccuracyMetricSummary};
use xtask::parser_accuracy_legacy_population::is_canonical_population_identity;

/// Fail-closed consumption of the typed trust and disposition contract.
///
/// Unknown trust or disposition values, contradictory shapes, non-canonical
/// identities, populations whose counts do not close, and investigation rows
/// claiming floor eligibility or packet emission all reject the artifact rather
/// than silently render trusted accuracy.
///
/// The one case that is *not* a rejection is a population with zero applied
/// rows: it has nothing to observe, so an `insufficient_data` aggregate is
/// honest, and refusing it would fail a valid no-observation run.
pub(super) fn trust_disposition_is_fail_closed(artifact: &ParserAccuracyArtifactSummary) -> bool {
    let population = &artifact.legacy_population;
    if !is_canonical_population_identity(&population.population_identity) {
        return false;
    }
    if population.transformation_profile.is_empty()
        || population.aggregate_metric.is_empty()
        || population.manifest_schema_version == 0
        || population.population_total_count == 0
    {
        return false;
    }
    // Checked: these are migration-supplied `u64`s, a wrapping sum can forge a
    // closing population, and the plain `+` aborts a debug build outright.
    let Some(closed) =
        population.population_applied_count.checked_add(population.population_unclassified_count)
    else {
        return false;
    };
    if closed != population.population_total_count {
        return false;
    }

    let expects_observation = population.population_applied_count > 0;
    let mut aggregate_investigation_rows = 0_usize;
    let mut aggregate_insufficient_rows = 0_usize;
    for metric in &artifact.metrics {
        // A projected row carrying a trust field the schema allows only on an
        // investigation row is a contradictory claim, and would otherwise be
        // rendered as trusted accuracy.
        if let ParserAccuracyMetricSummary::Measured { unmodeled_fields, .. }
        | ParserAccuracyMetricSummary::InsufficientData { unmodeled_fields, .. } = metric
            && FORBIDDEN_TRUST_FIELDS.iter().any(|field| unmodeled_fields.contains_key(*field))
        {
            return false;
        }
        match metric {
            ParserAccuracyMetricSummary::InvestigationOnly {
                metric,
                value: _,
                sample_count,
                transformation_profile,
                evidence_class,
                terminal_disposition,
                reason,
                packet_policy,
                floor_eligible,
                unknown_fields,
            } => {
                // The schema forbids extra properties on this variant, so an
                // artifact carrying any is one the schema rejects.
                if !unknown_fields.is_empty() {
                    return false;
                }
                if evidence_class != "investigation_only"
                    || terminal_disposition != "not_proven"
                    || packet_policy != "none"
                    || *floor_eligible
                    || reason.is_empty()
                    || transformation_profile.is_empty()
                    || *sample_count == 0
                {
                    return false;
                }
                if metric == &population.aggregate_metric {
                    if transformation_profile != &population.transformation_profile {
                        return false;
                    }
                    if *sample_count != population.population_applied_count {
                        return false;
                    }
                    aggregate_investigation_rows += 1;
                }
            }
            // Only a population that applied to nothing may report an untyped
            // aggregate; otherwise this is the conflation the contract rejects.
            ParserAccuracyMetricSummary::InsufficientData { metric, .. }
                if metric == &population.aggregate_metric =>
            {
                if expects_observation {
                    return false;
                }
                aggregate_insufficient_rows += 1;
            }
            // A measured aggregate is trusted accuracy by another name.
            ParserAccuracyMetricSummary::Measured { metric, .. }
                if metric == &population.aggregate_metric =>
            {
                return false;
            }
            ParserAccuracyMetricSummary::Measured { .. }
            | ParserAccuracyMetricSummary::InsufficientData { .. } => {}
        }
    }

    // Exactly one row carries the aggregate: without uniqueness two otherwise
    // valid rows with different values both pass and array order decides.
    // Mirrors the generator's rule: an investigation row declares
    // `packet_policy: none`, so a failure packet naming one reports an active
    // parser defect against evidence that emits no packets.
    let investigation_metrics: Vec<&str> = artifact
        .metrics
        .iter()
        .filter_map(|row| match row {
            ParserAccuracyMetricSummary::InvestigationOnly { metric, .. } => Some(metric.as_str()),
            _ => None,
        })
        .collect();
    if artifact.failure_packets.iter().any(|packet| {
        packet.metric.as_deref().is_some_and(|metric| investigation_metrics.contains(&metric))
    }) {
        return false;
    }

    if expects_observation {
        aggregate_investigation_rows == 1
    } else {
        aggregate_investigation_rows == 0 && aggregate_insufficient_rows == 1
    }
}
