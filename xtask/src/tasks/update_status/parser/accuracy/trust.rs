//! The fail-closed trust check the status reader applies before rendering.
//!
//! Split from `accuracy.rs` for the 400-line gate in `update_status::mod_tests`,
//! and because this is where the reader must mirror the generator's
//! `validate_legacy_population_evidence`: a shape one accepts and the other
//! refuses is a contract split that only surfaces after publication.

use super::{FORBIDDEN_TRUST_FIELDS, ParserAccuracyArtifactSummary, ParserAccuracyMetricSummary};
use xtask::parser_accuracy_legacy_population::{
    LEGACY_QUARANTINED_METRICS, LEGACY_WHITESPACE_AGGREGATE_METRIC,
    is_canonical_population_identity,
};

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
    // Projected from the manifest the run scored, so the total is the run's
    // fixture count; anything else binds current metrics to stale evidence.
    if population.population_total_count != artifact.denominator.fixture_count {
        return false;
    }

    // The retained population is the whitespace one; any other quarantined row
    // declared as its aggregate would bind that row to the whitespace profile
    // and counts. Mirrors the generator's refusal.
    if population.aggregate_metric != LEGACY_WHITESPACE_AGGREGATE_METRIC {
        return false;
    }
    // The declared aggregate must appear in its own quarantine list, or the
    // two declarations disagree about what this population covers.
    if !population.quarantined_metrics.contains(&population.aggregate_metric) {
        return false;
    }
    if population.quarantined_metrics.iter().any(String::is_empty) {
        return false;
    }
    // The schema declares `uniqueItems`; mirror it so a repeated name is not
    // readable here while the schema rejects it.
    {
        let mut seen = std::collections::BTreeSet::new();
        if !population.quarantined_metrics.iter().all(|m| seen.insert(m.as_str())) {
            return false;
        }
    }
    // A *partial* declaration would otherwise obey the artifact into letting a
    // quarantined observation back through as `measured`. The declaration must
    // cover the contract's set; under-declaring is refused, not honoured.
    if !LEGACY_QUARANTINED_METRICS
        .iter()
        .all(|known| population.quarantined_metrics.iter().any(|m| m == known))
    {
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
                // A row typed investigation-only that the population does not
                // declare is a quarantine the artifact contradicts itself about.
                if !population.quarantined_metrics.contains(metric) {
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
            // Any quarantined metric, not only the declared aggregate: a
            // measured form of quarantined evidence is trusted accuracy by
            // another name.
            ParserAccuracyMetricSummary::Measured { metric, .. }
                if metric == &population.aggregate_metric
                    || population.quarantined_metrics.contains(metric) =>
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
