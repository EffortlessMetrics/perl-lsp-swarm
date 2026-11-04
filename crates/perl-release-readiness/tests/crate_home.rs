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
    // Integration-test binaries have their own CARGO_CRATE_NAME. The library
    // name is `perl_release_readiness` because this file compiles against it.
    let _library: fn(
        &perl_release_readiness::KwaliteeOptions,
    ) -> perl_release_readiness::KwaliteeReceipt = perl_release_readiness::evaluate;
    let _ = _library;
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

/// Receipt fixtures need no parser. Keep focused evaluator tests from compiling
/// the product stack through an otherwise unused test helper, including aliases
/// and transitive/build dependencies. Cargo inspection never compiles a target.
#[test]
fn focused_tests_do_not_compile_the_parser_stack() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "tree",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
            "-p",
            "perl-release-readiness",
            "--edges",
            "normal,dev,build",
            "--prefix",
            "none",
            "--locked",
            "--offline",
        ])
        .output()?;
    assert!(
        output.status.success(),
        "dependency graph is unproven: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8(output.stdout)?;
    let names: Vec<_> = tree.lines().filter_map(|line| line.split_whitespace().next()).collect();
    assert!(names.contains(&"perl-release-readiness"), "missing evaluator root: {tree}");
    for forbidden in ["perl-parser-core", "perl-lexer"] {
        assert!(!names.contains(&forbidden), "focused evaluator tests pull {forbidden}:\n{tree}");
    }
    Ok(())
}
