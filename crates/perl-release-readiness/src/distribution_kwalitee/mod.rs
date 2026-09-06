//! Frozen native CPANTS-compatible catalog and fixture contract (#7170).
//!
//! This module is independent of the historical `perl_kwalitee.v1` evaluator.
//! It freezes metric identity, class, scoring, and fixture identities so later
//! PRs can implement indicators without reconstructing CPANTS semantics.

mod catalog;
mod error;
mod fixtures;
mod render;
mod score;
mod types;

pub use catalog::{
    catalog_fingerprint, catalog_toml, load_distribution_kwalitee_catalog, parse_catalog,
    validate_catalog,
};
pub use error::{CatalogError, FixtureError};
pub use fixtures::{
    committed_fixture_root, fixture_contract_toml, load_distribution_kwalitee_fixture_contract,
    parse_fixture_contract, validate_catalog_fixture_binding, validate_fixture_contract,
};
pub use render::render_distribution_kwalitee_catalog_markdown;
pub use score::derive_compatible_core_score;
pub use types::{
    Applicability, CATALOG_KIND, CATALOG_SCHEMA_VERSION, CATALOG_VERSION, CatalogMetric,
    CompatibilityRelationship, CompatibleCoreScore, ContentStatus, CpantsComparability,
    DistributionKwaliteeCatalog, DistributionKwaliteeFixture, DistributionKwaliteeFixtureContract,
    ExpectationRule, FIXTURE_KIND, FIXTURE_SCHEMA_VERSION, FixtureKind, InputRole, MetricClass,
    MetricObservation, ObservationStatus, ScoringRule,
};
