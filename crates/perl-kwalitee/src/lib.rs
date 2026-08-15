//! # perl-kwalitee
//!
//! Historical compatibility crate for the repository's first
//! `perl_kwalitee.v1` receipt.
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
//! The implementation remains available temporarily so existing automation can
//! migrate without evidence loss. The canonical code home moves to
//! `perl-release-readiness`; the reclaimed `perl-kwalitee` name is reserved for
//! the native Rust CPANTS-compatible distribution analyser.
//!
//! ## Existing evaluation API
//!
//! ```no_run
//! use perl_kwalitee::{evaluate, KwaliteeOptions, KwaliteeProfile};
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

mod evaluator;
mod evidence;
mod indicator;
mod legacy;
mod profile;
mod receipt;
mod score;

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
    }
}
