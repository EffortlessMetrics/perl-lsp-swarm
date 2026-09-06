//! # perl-release-readiness
//!
//! Historical compatibility crate for the repository's first
//! `perl_kwalitee.v1` receipt. This is the one code home of that frozen mixed
//! evaluator; the `perl-kwalitee` / `perl_kwalitee` names stay vacant for the
//! native Rust CPANTS-compatible distribution analyser.
//!
//! The existing implementation predates the separation between:
//!
//! - CPAN distribution Kwalitee;
//! - native-product posture;
//! - engineering evidence;
//! - release integrity and governance;
//! - installed acceptance.
//!
//! Its current indicator catalog and weighted verdict are therefore frozen as
//! **mixed repository/product release-readiness history**. New indicators must
//! not be added here. [`legacy_migration_ledger`] records the exact destination
//! of every historical row, and [`read_legacy_receipt`] is the fail-closed
//! compatibility reader.
//!
//! Catalog v1 and the fixture-identity contract live beside the legacy
//! evaluator. They freeze metric class, scoring, and fixture identities
//! without implementing indicators, loading archives, or exposing a public
//! CLI.
//!
//! ## Existing evaluation API
//!
//! ```no_run
//! use perl_release_readiness::{evaluate, KwaliteeOptions, KwaliteeProfile};
//!
//! let options = KwaliteeOptions::new("/path/to/repo", KwaliteeProfile::Pr);
//! let receipt = evaluate(&options);
//! println!("verdict: {} score: {}", receipt.verdict.label(), receipt.score);
//! println!("{}", receipt.to_markdown());
//! ```

#![warn(missing_docs)]
// Production code stays under the workspace `unwrap_used`/`expect_used` deny;
// test code may use `expect()`/`unwrap()` for concise fixture construction.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

mod distribution_kwalitee;
mod evaluator;
mod evidence;
mod historical_home;
mod indicator;
mod legacy;
mod profile;
mod receipt;
mod score;

pub use distribution_kwalitee::{
    Applicability, CATALOG_KIND, CATALOG_SCHEMA_VERSION, CATALOG_VERSION, CatalogError,
    CatalogMetric, CompatibilityRelationship, CompatibleCoreScore, ContentStatus,
    CpantsComparability, DistributionKwaliteeCatalog, DistributionKwaliteeFixture,
    DistributionKwaliteeFixtureContract, ExpectationRule, FIXTURE_KIND, FIXTURE_SCHEMA_VERSION,
    FixtureError, FixtureKind, InputRole, MetricClass, MetricObservation, ObservationStatus,
    ScoringRule, catalog_fingerprint, catalog_toml, committed_fixture_root,
    derive_compatible_core_score, fixture_contract_toml, load_distribution_kwalitee_catalog,
    load_distribution_kwalitee_fixture_contract, parse_catalog, parse_fixture_contract,
    render_distribution_kwalitee_catalog_markdown, validate_catalog,
    validate_catalog_fixture_binding, validate_fixture_contract,
};
pub use evaluator::{EvidencePaths, ExternalResult, KwaliteeOptions, evaluate, is_known_indicator};
pub use indicator::{
    EvidenceRef, IndicatorExplanation, IndicatorStatus, KwaliteeIndicator, explain, indicator_ids,
};
pub use legacy::{
    LEGACY_DOMAIN, LEGACY_MIGRATION_SCHEMA_VERSION, LEGACY_REPLACEMENT, LEGACY_STATUS,
    LegacyCompatibilityError, LegacyDestinationRail, LegacyIndicatorMigration,
    LegacyIndicatorRecord, LegacyMigrationAction, LegacyMigrationLedger, legacy_indicator_records,
    legacy_migration_ledger, read_legacy_receipt, render_legacy_migration_markdown,
    validate_legacy_migration_ledger,
};
pub use profile::KwaliteeProfile;
pub use receipt::{KwaliteeReceipt, KwaliteeVerdict, RECEIPT_KIND, SCHEMA_VERSION};

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]
    use super::*;

    #[test]
    fn public_api_is_reachable() {
        // A smoke test that the re-exported surface composes.
        let opts = KwaliteeOptions::new(".", KwaliteeProfile::Pr);
        let receipt = evaluate(&opts);
        assert_eq!(receipt.kind, RECEIPT_KIND);
        assert_eq!(receipt.schema_version, SCHEMA_VERSION);
        assert!(!indicator_ids().is_empty());
        assert!(explain(indicator_ids()[0]).is_some());
        assert!(is_known_indicator("license.declared"));
        assert!(legacy_migration_ledger().is_ok());
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        assert_eq!(catalog.kind, CATALOG_KIND);
        assert!(catalog.metric.iter().any(|metric| metric.alias == "has_manifest"));
        let fixtures = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        validate_catalog_fixture_binding(&catalog, &fixtures, &committed_fixture_root())
            .expect("binding");
    }
}
