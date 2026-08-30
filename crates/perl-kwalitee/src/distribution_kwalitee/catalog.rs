//! Load and validate the frozen catalog.

use std::collections::{BTreeMap, BTreeSet};

use super::error::CatalogError;
use super::types::{
    CATALOG_KIND, CATALOG_SCHEMA_VERSION, CATALOG_VERSION, CatalogMetric,
    DistributionKwaliteeCatalog, MetricClass,
};

const CATALOG_TOML: &str = include_str!("../../distribution_kwalitee_catalog.v1.toml");
const CORE_UNVERIFIED_SEMANTICS: &str = "stays in the compatible-core denominator";
const NON_CORE_UNVERIFIED_SEMANTICS: &str = "never enters the compatible-core denominator";

/// Checked-in catalog TOML.
pub fn catalog_toml() -> &'static str {
    CATALOG_TOML
}

/// Decode and validate catalog TOML.
pub fn parse_catalog(toml: &str) -> Result<DistributionKwaliteeCatalog, CatalogError> {
    let catalog: DistributionKwaliteeCatalog =
        toml::from_str(toml).map_err(CatalogError::InvalidToml)?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

/// Load the frozen checked-in catalog.
pub fn load_distribution_kwalitee_catalog() -> Result<DistributionKwaliteeCatalog, CatalogError> {
    parse_catalog(CATALOG_TOML)
}

/// Order-independent identity over the metric-id set.
pub fn catalog_fingerprint(catalog: &DistributionKwaliteeCatalog) -> String {
    let mut ids = catalog.metric.iter().map(|metric| metric.id.as_str()).collect::<Vec<_>>();
    ids.sort_unstable();
    let ids = ids.join(",");
    format!("{}:{}:{}:{}", catalog.kind, catalog.catalog_version, catalog.metric.len(), ids)
}

/// Reject envelope, identity, dependency, and score-class drift.
pub fn validate_catalog(catalog: &DistributionKwaliteeCatalog) -> Result<(), CatalogError> {
    if catalog.schema_version != CATALOG_SCHEMA_VERSION {
        return Err(CatalogError::Metadata(format!(
            "schema_version: expected {CATALOG_SCHEMA_VERSION}, observed {}",
            catalog.schema_version
        )));
    }
    if catalog.kind != CATALOG_KIND {
        return Err(CatalogError::Metadata(format!(
            "kind: expected `{CATALOG_KIND}`, observed `{}`",
            catalog.kind
        )));
    }
    if catalog.catalog_version != CATALOG_VERSION {
        return Err(CatalogError::Metadata(format!(
            "catalog_version: expected `{CATALOG_VERSION}`, observed `{}`",
            catalog.catalog_version
        )));
    }
    if catalog.status != "frozen" {
        return Err(CatalogError::Metadata(format!(
            "status: expected `frozen`, observed `{}`",
            catalog.status
        )));
    }
    if catalog.production_runtime != "native_rust_offline" {
        return Err(CatalogError::Metadata(
            "production_runtime must be `native_rust_offline`".to_string(),
        ));
    }
    if catalog.oracle_role != "test_only_pinned_cpants" {
        return Err(CatalogError::Metadata(
            "oracle_role must be `test_only_pinned_cpants`".to_string(),
        ));
    }
    if catalog.metric.is_empty() {
        return Err(CatalogError::Metadata("catalog v1 must inventory at least one metric".into()));
    }

    let mut ids = BTreeSet::new();
    let mut aliases = BTreeSet::new();
    let mut by_id = BTreeMap::new();
    for metric in &catalog.metric {
        validate_metric_row(metric)?;
        if !ids.insert(metric.id.as_str()) {
            return Err(CatalogError::DuplicateIdentity(metric.id.clone()));
        }
        if !aliases.insert(metric.alias.as_str()) {
            return Err(CatalogError::DuplicateIdentity(metric.alias.clone()));
        }
        by_id.insert(metric.id.as_str(), metric);
    }

    for metric in &catalog.metric {
        for referenced in metric.depends_on.iter().chain(metric.permitted_cascades.iter()) {
            if !by_id.contains_key(referenced.as_str()) {
                return Err(CatalogError::UnknownReference {
                    id: metric.id.clone(),
                    referenced: referenced.clone(),
                });
            }
            if referenced == &metric.id {
                return Err(CatalogError::InvalidMetric {
                    id: metric.id.clone(),
                    reason: "depends_on/permitted_cascades must not include the row itself".into(),
                });
            }
        }
    }

    Ok(())
}

fn validate_metric_row(metric: &CatalogMetric) -> Result<(), CatalogError> {
    metric.validate_score_class()?;
    if metric.id.is_empty() || metric.alias.is_empty() {
        return Err(CatalogError::InvalidMetric {
            id: metric.id.clone(),
            reason: "id and alias are required".into(),
        });
    }
    let expected_id = match metric.class {
        MetricClass::NativeExtension => format!("native.{}", metric.alias),
        _ => format!("cpants.{}", metric.alias),
    };
    if metric.id != expected_id {
        return Err(CatalogError::InvalidMetric {
            id: metric.id.clone(),
            reason: format!("id must be `{expected_id}`"),
        });
    }
    let valid_unverified_semantics = if metric.participates_in_core_score {
        metric.unverified_semantics.contains(CORE_UNVERIFIED_SEMANTICS)
    } else {
        metric.unverified_semantics.contains(NON_CORE_UNVERIFIED_SEMANTICS)
            && !metric.unverified_semantics.contains(CORE_UNVERIFIED_SEMANTICS)
    };
    if !valid_unverified_semantics {
        let reason = if metric.participates_in_core_score {
            format!("unverified_semantics must contain `{CORE_UNVERIFIED_SEMANTICS}`")
        } else if metric.unverified_semantics.contains(CORE_UNVERIFIED_SEMANTICS) {
            format!("unverified_semantics must not contain `{CORE_UNVERIFIED_SEMANTICS}`")
        } else {
            format!("unverified_semantics must contain `{NON_CORE_UNVERIFIED_SEMANTICS}`")
        };
        return Err(CatalogError::ScoreClassContradiction { id: metric.id.clone(), reason });
    }
    if metric.title.is_empty()
        || metric.source_module.is_empty()
        || metric.source_version.is_empty()
        || metric.behavior_ref.is_empty()
        || metric.pass_semantics.is_empty()
        || metric.fail_semantics.is_empty()
        || metric.required_facts.is_empty()
        || metric.fixture_ids.is_empty()
        || metric.remediation_owner.is_empty()
        || metric.implementation_owner == 0
    {
        return Err(CatalogError::InvalidMetric {
            id: metric.id.clone(),
            reason: "title, source, facts, fixture_ids, remediation_owner, and implementation_owner are required"
                .into(),
        });
    }
    if metric.fixture_ids.iter().any(|fixture| fixture.is_empty()) {
        return Err(CatalogError::InvalidMetric {
            id: metric.id.clone(),
            reason: "fixture_ids must not contain empty identities".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]
    use super::*;
    use crate::distribution_kwalitee::types::{
        Applicability, CompatibilityRelationship, ScoringRule,
    };

    fn envelope() -> String {
        catalog_toml()
            .lines()
            .take_while(|line| !line.starts_with("[[metric]]"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn one_core_metric(id_alias: &str) -> String {
        format!(
            r#"
[[metric]]
id = "cpants.{id_alias}"
alias = "{id_alias}"
title = "Test metric"
class = "cpants_offline_core"
participates_in_core_score = true
relationship = "direct"
source_module = "Module::CPANTS::Kwalitee::Files"
source_version = "1.03"
behavior_ref = "https://example.test/{id_alias}"
required_facts = ["staged_files"]
applicability = "all_distributions"
pass_semantics = "pass"
fail_semantics = "fail"
not_applicable_semantics = "na"
unverified_semantics = "Required facts were not produced; this row stays in the compatible-core denominator and is reported as unverified."
depends_on = []
permitted_cascades = []
remediation_owner = "distribution_author"
implementation_owner = 7170
fixture_ids = ["minimal_valid"]
known_differences = []
limitations = []
"#
        )
    }

    #[test]
    fn checked_in_catalog_loads() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        assert_eq!(catalog.kind, CATALOG_KIND);
        assert_eq!(catalog.catalog_version, CATALOG_VERSION);
        assert_eq!(catalog.scoring_rule, ScoringRule::UnweightedApplicableOfflineCore);
        assert!(!catalog.metric.is_empty());
        assert!(catalog.metric.iter().any(|metric| metric.id == "cpants.has_manifest"));
        assert!(catalog.metric.iter().any(|metric| metric.id == "cpants.prereq_matches_use"));
        assert!(catalog_fingerprint(&catalog).contains("cpants.has_manifest"));
    }

    #[test]
    fn catalog_fingerprint_is_order_independent() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        let expected = catalog_fingerprint(&catalog);
        let mut reordered = catalog.clone();
        reordered.metric.reverse();
        assert_eq!(catalog_fingerprint(&reordered), expected);
    }

    #[test]
    fn site_analogues_and_extensions_cannot_enter_core_score() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        for metric in &catalog.metric {
            if matches!(
                metric.class,
                MetricClass::CpantsSiteAnalogue
                    | MetricClass::NativeExtension
                    | MetricClass::UnsupportedOrDeferred
                    | MetricClass::CpantsOfflineExtra
                    | MetricClass::CpantsOfflineExperimental
            ) {
                assert!(!metric.participates_in_core_score, "{} leaked into core score", metric.id);
            }
        }
        let prereq = catalog
            .metric
            .iter()
            .find(|metric| metric.alias == "prereq_matches_use")
            .expect("prereq_matches_use");
        assert_eq!(prereq.class, MetricClass::CpantsSiteAnalogue);
        assert!(!prereq.participates_in_core_score);
    }

    #[test]
    fn unknown_class_fails_decode() {
        let toml =
            format!("{}\n[[metric]]\nclass = \"not_a_class\"\nid = \"cpants.x\"\n", envelope());
        assert!(matches!(parse_catalog(&toml), Err(CatalogError::InvalidToml(_))));
    }

    #[test]
    fn unknown_field_fails_decode() {
        let toml =
            format!("{}future_authority = true\n{}", envelope(), one_core_metric("has_readme"));
        assert!(matches!(parse_catalog(&toml), Err(CatalogError::InvalidToml(_))));
    }

    #[test]
    fn duplicate_ids_fail() {
        let toml = format!(
            "{}{}{}",
            envelope(),
            one_core_metric("has_readme"),
            one_core_metric("has_readme")
        );
        assert!(matches!(parse_catalog(&toml), Err(CatalogError::DuplicateIdentity(_))));
    }

    #[test]
    fn unknown_dependency_fails() {
        let mut metric = one_core_metric("has_readme");
        metric = metric.replace("depends_on = []", r#"depends_on = ["cpants.missing"]"#);
        let toml = format!("{}{metric}", envelope());
        assert!(matches!(parse_catalog(&toml), Err(CatalogError::UnknownReference { .. })));
    }

    #[test]
    fn site_analogue_masquerading_as_core_fails() {
        let mut catalog =
            parse_catalog(&format!("{}{}", envelope(), one_core_metric("has_readme")))
                .expect("base");
        catalog.metric[0].class = MetricClass::CpantsSiteAnalogue;
        catalog.metric[0].relationship = CompatibilityRelationship::SiteAnalogue;
        catalog.metric[0].participates_in_core_score = true;
        assert!(matches!(
            validate_catalog(&catalog),
            Err(CatalogError::ScoreClassContradiction { .. })
        ));
    }

    #[test]
    fn native_extension_cannot_participate_in_core_score() {
        let mut catalog =
            parse_catalog(&format!("{}{}", envelope(), one_core_metric("has_readme")))
                .expect("base");
        catalog.metric[0].id = "native.has_readme".into();
        catalog.metric[0].class = MetricClass::NativeExtension;
        catalog.metric[0].relationship = CompatibilityRelationship::NativeExtension;
        catalog.metric[0].participates_in_core_score = true;
        assert!(matches!(
            validate_catalog(&catalog),
            Err(CatalogError::ScoreClassContradiction { .. })
        ));
    }

    #[test]
    fn extra_metric_marked_core_fails() {
        let mut catalog =
            parse_catalog(&format!("{}{}", envelope(), one_core_metric("has_readme")))
                .expect("base");
        catalog.metric[0].class = MetricClass::CpantsOfflineExtra;
        catalog.metric[0].participates_in_core_score = true;
        assert!(matches!(
            validate_catalog(&catalog),
            Err(CatalogError::ScoreClassContradiction { .. })
        ));
    }

    #[test]
    fn non_core_row_with_core_unverified_semantics_fails() {
        let mut catalog =
            parse_catalog(&format!("{}{}", envelope(), one_core_metric("has_readme")))
                .expect("base");
        catalog.metric[0].class = MetricClass::CpantsOfflineExtra;
        catalog.metric[0].participates_in_core_score = false;
        catalog.metric[0].unverified_semantics =
            "Required facts were not produced; this row stays in the compatible-core denominator and is reported as unverified, but it never enters the compatible-core denominator."
                .into();
        assert!(matches!(
            validate_catalog(&catalog),
            Err(CatalogError::ScoreClassContradiction { id, .. }) if id == "cpants.has_readme"
        ));
    }

    #[test]
    fn core_row_with_non_core_unverified_semantics_fails() {
        let mut catalog =
            parse_catalog(&format!("{}{}", envelope(), one_core_metric("has_readme")))
                .expect("base");
        catalog.metric[0].unverified_semantics =
            "Required facts were not produced; this row is reported as unverified and never enters the compatible-core denominator."
                .into();
        assert!(matches!(
            validate_catalog(&catalog),
            Err(CatalogError::ScoreClassContradiction { id, .. }) if id == "cpants.has_readme"
        ));
    }

    #[test]
    fn archive_metrics_are_archive_input() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        for id in ["cpants.extractable", "cpants.extracts_nicely", "cpants.no_pax_headers"] {
            let metric = catalog.metric.iter().find(|metric| metric.id == id).expect(id);
            assert_eq!(metric.applicability, Applicability::ArchiveInput);
            assert!(metric.participates_in_core_score);
        }
    }

    #[test]
    fn legacy_indicator_ids_do_not_appear_in_the_cpants_catalog() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        for metric in &catalog.metric {
            assert!(
                !metric.id.starts_with("manifest.")
                    && !metric.id.starts_with("release.")
                    && !metric.id.starts_with("product_surface."),
                "legacy readiness id leaked: {}",
                metric.id
            );
        }
    }
}
