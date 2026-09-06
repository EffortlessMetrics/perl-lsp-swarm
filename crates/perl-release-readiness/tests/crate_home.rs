//! Discriminating crate-home proof for the #8421 namespace move.
//!
//! These cases exist so a later edit cannot “fix” frozen `perl_kwalitee.v1`
//! indicators by pointing them at `perl-release-readiness`, or revive the
//! vacated package/library names.

#![deny(clippy::map_err_ignore)]

use std::path::PathBuf;

use perl_release_readiness::{
    KwaliteeOptions, KwaliteeProfile, RECEIPT_KIND, SCHEMA_VERSION, evaluate,
};

#[test]
fn live_package_and_library_names_are_release_readiness() {
    assert_eq!(env!("CARGO_PKG_NAME"), "perl-release-readiness");
    assert_eq!(env!("CARGO_CRATE_NAME"), "perl_release_readiness");
}

#[test]
fn historical_kwalitee_crate_directory_is_vacant() {
    let live = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        live.ends_with("crates/perl-release-readiness"),
        "live crate must live at crates/perl-release-readiness, got {}",
        live.display()
    );
    let vacated = live.join("..").join("perl-kwalitee");
    assert!(
        !vacated.exists(),
        "crates/perl-kwalitee must stay vacant for the native analyser, found {}",
        vacated.display()
    );
}

#[test]
fn receipt_kind_and_schema_stay_historical() {
    assert_eq!(RECEIPT_KIND, "perl_kwalitee");
    assert_eq!(SCHEMA_VERSION, 1);
    let receipt = evaluate(&KwaliteeOptions::new(".", KwaliteeProfile::Pr));
    assert_eq!(receipt.kind, "perl_kwalitee");
    assert_eq!(receipt.schema_version, 1);
}
