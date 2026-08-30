//! Compatible-core score contract. This is not an indicator implementation.

use std::collections::BTreeMap;

use super::catalog::validate_catalog;
use super::error::CatalogError;
use super::types::{
    CatalogMetric, CompatibleCoreScore, DistributionKwaliteeCatalog, InputRole, MetricClass,
    MetricObservation, ObservationStatus, ScoringRule,
};

/// Derive the catalog v1 compatible-core score from independently authored observations.
pub fn derive_compatible_core_score(
    catalog: &DistributionKwaliteeCatalog,
    input_role: InputRole,
    observations: &[MetricObservation],
) -> Result<CompatibleCoreScore, CatalogError> {
    validate_catalog(catalog)?;

    let mut seen = BTreeMap::new();
    let mut invalid_input_metric = None;
    for observation in observations {
        if catalog.metric.iter().all(|metric| metric.id != observation.id) {
            return Err(CatalogError::Observation(format!("unknown metric `{}`", observation.id)));
        }
        if seen.insert(observation.id.as_str(), observation.status).is_some() {
            return Err(CatalogError::Observation(format!(
                "duplicate observation for `{}`",
                observation.id
            )));
        }
        if observation.status == ObservationStatus::InvalidInput && invalid_input_metric.is_none() {
            invalid_input_metric = Some(observation.id.as_str());
        }
    }

    if input_role == InputRole::AuthoringTree {
        return Ok(CompatibleCoreScore::InvalidInput {
            reason: "authoring trees are not a staged distribution input".to_string(),
        });
    }
    if let Some(metric_id) = invalid_input_metric {
        return Ok(CompatibleCoreScore::InvalidInput {
            reason: format!("metric `{metric_id}` observed invalid input"),
        });
    }

    match catalog.scoring_rule {
        ScoringRule::UnweightedApplicableOfflineCore => {
            let mut passed = 0u32;
            let mut applicable = 0u32;
            let mut unverified = 0u32;

            for metric in catalog.metric.iter().filter(|metric| {
                metric.class == MetricClass::CpantsOfflineCore && metric.participates_in_core_score
            }) {
                match core_row_status(metric, input_role, seen.get(metric.id.as_str()).copied()) {
                    CoreRow::NotApplicable => {}
                    CoreRow::Pass => {
                        applicable = applicable.saturating_add(1);
                        passed = passed.saturating_add(1);
                    }
                    CoreRow::Fail => {
                        applicable = applicable.saturating_add(1);
                    }
                    CoreRow::Unverified => {
                        applicable = applicable.saturating_add(1);
                        unverified = unverified.saturating_add(1);
                    }
                }
            }

            if unverified > 0 {
                Ok(CompatibleCoreScore::Incomplete { passed, applicable, unverified })
            } else {
                Ok(CompatibleCoreScore::Complete { passed, applicable })
            }
        }
    }
}

enum CoreRow {
    NotApplicable,
    Pass,
    Fail,
    Unverified,
}

fn core_row_status(
    metric: &CatalogMetric,
    input_role: InputRole,
    observed: Option<ObservationStatus>,
) -> CoreRow {
    if !metric.applicability.applies_to(input_role) {
        return CoreRow::NotApplicable;
    }
    match observed {
        Some(ObservationStatus::Pass) => CoreRow::Pass,
        Some(ObservationStatus::Fail) => CoreRow::Fail,
        Some(
            ObservationStatus::NotApplicable
            | ObservationStatus::Unverified
            | ObservationStatus::Limitation
            | ObservationStatus::InvalidInput,
        )
        | None => CoreRow::Unverified,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]
    use super::*;
    use crate::distribution_kwalitee::catalog::load_distribution_kwalitee_catalog;
    use crate::distribution_kwalitee::types::{
        Applicability, CompatibilityRelationship, MetricClass,
    };

    fn catalog() -> DistributionKwaliteeCatalog {
        load_distribution_kwalitee_catalog().expect("catalog")
    }

    fn core_ids(catalog: &DistributionKwaliteeCatalog) -> Vec<String> {
        catalog
            .metric
            .iter()
            .filter(|metric| metric.class == MetricClass::CpantsOfflineCore)
            .map(|metric| metric.id.clone())
            .collect()
    }

    fn pass_all_directory(catalog: &DistributionKwaliteeCatalog) -> Vec<MetricObservation> {
        catalog
            .metric
            .iter()
            .filter(|metric| {
                metric.class == MetricClass::CpantsOfflineCore
                    && metric.applicability.applies_to(InputRole::StagedDirectory)
            })
            .map(|metric| MetricObservation::new(&metric.id, ObservationStatus::Pass))
            .collect()
    }

    fn directory_core_count(catalog: &DistributionKwaliteeCatalog) -> u32 {
        pass_all_directory(catalog).len() as u32
    }

    #[test]
    fn all_applicable_core_pass_is_complete_and_unweighted() {
        let catalog = catalog();
        let score = derive_compatible_core_score(
            &catalog,
            InputRole::StagedDirectory,
            &pass_all_directory(&catalog),
        )
        .expect("score");
        let directory_core = directory_core_count(&catalog);
        assert_eq!(
            score,
            CompatibleCoreScore::Complete { passed: directory_core, applicable: directory_core }
        );
        assert_eq!(score.ratio(), Some((directory_core, directory_core)));
        assert!(score.strict_complete());
    }

    #[test]
    fn invalid_catalog_fails_closed_before_scoring() {
        let mut catalog = catalog();
        let metric = catalog
            .metric
            .iter_mut()
            .find(|metric| metric.id == "cpants.has_readme")
            .expect("has_readme");
        metric.class = MetricClass::CpantsSiteAnalogue;
        metric.relationship = CompatibilityRelationship::SiteAnalogue;
        metric.participates_in_core_score = true;
        let error = derive_compatible_core_score(&catalog, InputRole::StagedDirectory, &[])
            .expect_err("invalid catalog");
        assert!(matches!(
            error,
            CatalogError::ScoreClassContradiction { id, .. } if id == "cpants.has_readme"
        ));
    }

    #[test]
    fn extra_and_site_analogue_results_do_not_change_core_score() {
        let catalog = catalog();
        let mut observations = pass_all_directory(&catalog);
        let before =
            derive_compatible_core_score(&catalog, InputRole::StagedDirectory, &observations)
                .expect("before");
        observations.push(MetricObservation::new("cpants.has_meta_json", ObservationStatus::Fail));
        observations
            .push(MetricObservation::new("cpants.prereq_matches_use", ObservationStatus::Fail));
        observations.push(MetricObservation::new("cpants.use_warnings", ObservationStatus::Pass));
        let after =
            derive_compatible_core_score(&catalog, InputRole::StagedDirectory, &observations)
                .expect("after");
        assert_eq!(before, after);
    }

    #[test]
    fn unverified_required_core_stays_in_the_denominator() {
        let catalog = catalog();
        let mut observations = pass_all_directory(&catalog);
        let manifest = observations
            .iter_mut()
            .find(|observation| observation.id == "cpants.has_manifest")
            .expect("manifest");
        manifest.status = ObservationStatus::Unverified;
        let directory_core = directory_core_count(&catalog);
        let score =
            derive_compatible_core_score(&catalog, InputRole::StagedDirectory, &observations)
                .expect("score");
        match score {
            CompatibleCoreScore::Incomplete { passed, applicable, unverified } => {
                assert_eq!(applicable, directory_core);
                assert_eq!(unverified, 1);
                assert_eq!(passed, directory_core.saturating_sub(1));
                assert!(!score.strict_complete());
            }
            other => panic!("expected incomplete, got {other:?}"),
        }
    }

    #[test]
    fn missing_core_observation_is_unverified_not_a_silent_shrink() {
        let catalog = catalog();
        let mut observations = pass_all_directory(&catalog);
        observations.retain(|observation| observation.id != "cpants.has_tests");
        let directory_core = directory_core_count(&catalog);
        let score =
            derive_compatible_core_score(&catalog, InputRole::StagedDirectory, &observations)
                .expect("score");
        match score {
            CompatibleCoreScore::Incomplete { passed, applicable, unverified } => {
                assert_eq!(applicable, directory_core);
                assert_eq!(unverified, 1);
                assert_eq!(passed, directory_core.saturating_sub(1));
            }
            other => panic!("expected incomplete, got {other:?}"),
        }
    }

    #[test]
    fn invalid_input_has_no_ordinary_score() {
        let catalog = catalog();
        let observations =
            vec![MetricObservation::new("cpants.extractable", ObservationStatus::InvalidInput)];
        let score = derive_compatible_core_score(&catalog, InputRole::Archive, &observations)
            .expect("score");
        assert!(matches!(score, CompatibleCoreScore::InvalidInput { .. }));
        assert_eq!(score.ratio(), None);
        assert!(!score.strict_complete());
    }

    #[test]
    fn invalid_input_does_not_hide_unknown_observations() {
        let catalog = catalog();
        let observations = vec![
            MetricObservation::new("cpants.extractable", ObservationStatus::InvalidInput),
            MetricObservation::new("cpants.not_a_metric", ObservationStatus::Pass),
        ];
        let error = derive_compatible_core_score(&catalog, InputRole::Archive, &observations)
            .expect_err("unknown observation");
        assert!(matches!(
            error,
            CatalogError::Observation(message) if message.contains("cpants.not_a_metric")
        ));
    }

    #[test]
    fn invalid_input_does_not_hide_duplicate_observations() {
        let catalog = catalog();
        let observations = vec![
            MetricObservation::new("cpants.extractable", ObservationStatus::InvalidInput),
            MetricObservation::new("cpants.extractable", ObservationStatus::Pass),
        ];
        let error = derive_compatible_core_score(&catalog, InputRole::Archive, &observations)
            .expect_err("duplicate observation");
        assert!(matches!(
            error,
            CatalogError::Observation(message) if message.contains("cpants.extractable")
        ));
    }

    #[test]
    fn archive_only_core_metrics_are_na_on_directory_input() {
        let catalog = catalog();
        let score = derive_compatible_core_score(
            &catalog,
            InputRole::StagedDirectory,
            &pass_all_directory(&catalog),
        )
        .expect("score");
        let all_core = core_ids(&catalog).len() as u32;
        let directory_core = directory_core_count(&catalog);
        let ratio = score.ratio().expect("ratio");
        assert_eq!(ratio, (directory_core, directory_core));
        assert!(ratio.1 < all_core, "archive metrics must drop out of the directory denominator");
    }

    #[test]
    fn one_core_fail_lowers_passed_but_keeps_denominator() {
        let catalog = catalog();
        let mut observations = pass_all_directory(&catalog);
        let readme = observations
            .iter_mut()
            .find(|observation| observation.id == "cpants.has_readme")
            .expect("readme");
        readme.status = ObservationStatus::Fail;
        let directory_core = directory_core_count(&catalog);
        let score =
            derive_compatible_core_score(&catalog, InputRole::StagedDirectory, &observations)
                .expect("score");
        match score {
            CompatibleCoreScore::Complete { passed, applicable } => {
                assert_eq!(applicable, directory_core);
                assert_eq!(passed, directory_core.saturating_sub(1));
            }
            other => panic!("expected complete, got {other:?}"),
        }
    }

    #[test]
    fn unknown_and_duplicate_observations_fail() {
        let catalog = catalog();
        let unknown = derive_compatible_core_score(
            &catalog,
            InputRole::StagedDirectory,
            &[MetricObservation::new("cpants.not_a_metric", ObservationStatus::Pass)],
        );
        assert!(matches!(unknown, Err(CatalogError::Observation(_))));
        let dup = derive_compatible_core_score(
            &catalog,
            InputRole::StagedDirectory,
            &[
                MetricObservation::new("cpants.has_readme", ObservationStatus::Pass),
                MetricObservation::new("cpants.has_readme", ObservationStatus::Fail),
            ],
        );
        assert!(matches!(dup, Err(CatalogError::Observation(_))));
    }

    #[test]
    fn limitation_on_core_is_unverified_not_a_pass() {
        let catalog = catalog();
        let mut observations = pass_all_directory(&catalog);
        let row = observations
            .iter_mut()
            .find(|observation| observation.id == "cpants.use_strict")
            .expect("use_strict");
        row.status = ObservationStatus::Limitation;
        let directory_core = directory_core_count(&catalog);
        let score =
            derive_compatible_core_score(&catalog, InputRole::StagedDirectory, &observations)
                .expect("score");
        match score {
            CompatibleCoreScore::Incomplete { passed, applicable, unverified } => {
                assert_eq!(applicable, directory_core);
                assert_eq!(unverified, 1);
                assert_eq!(passed, directory_core.saturating_sub(1));
            }
            other => panic!("expected incomplete, got {other:?}"),
        }
    }

    #[test]
    fn extra_invalid_input_still_has_no_ordinary_score() {
        let catalog = catalog();
        let observations =
            vec![MetricObservation::new("cpants.has_meta_json", ObservationStatus::InvalidInput)];
        let score =
            derive_compatible_core_score(&catalog, InputRole::StagedDirectory, &observations)
                .expect("score");
        assert!(matches!(score, CompatibleCoreScore::InvalidInput { .. }));
        assert_eq!(score.ratio(), None);
    }

    #[test]
    fn authoring_tree_has_no_ordinary_score() {
        let catalog = catalog();
        let score = derive_compatible_core_score(
            &catalog,
            InputRole::AuthoringTree,
            &pass_all_directory(&catalog),
        )
        .expect("score");
        assert!(matches!(score, CompatibleCoreScore::InvalidInput { .. }));
        assert_eq!(score.ratio(), None);
        assert!(!score.strict_complete());
        assert!(!Applicability::AllDistributions.applies_to(InputRole::AuthoringTree));
        assert!(!Applicability::ArchiveInput.applies_to(InputRole::AuthoringTree));
    }

    #[test]
    fn not_applicable_on_applicable_core_stays_in_the_denominator() {
        let catalog = catalog();
        let mut observations = pass_all_directory(&catalog);
        let readme = observations
            .iter_mut()
            .find(|observation| observation.id == "cpants.has_readme")
            .expect("readme");
        readme.status = ObservationStatus::NotApplicable;
        let score =
            derive_compatible_core_score(&catalog, InputRole::StagedDirectory, &observations)
                .expect("score");
        let directory_core = directory_core_count(&catalog);
        match score {
            CompatibleCoreScore::Incomplete { passed, applicable, unverified } => {
                assert_eq!(applicable, directory_core);
                assert_eq!(unverified, 1);
                assert_eq!(passed, directory_core.saturating_sub(1));
                assert!(!score.strict_complete());
            }
            other => panic!("expected incomplete, got {other:?}"),
        }
    }
}
