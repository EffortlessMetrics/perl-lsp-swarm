//! Catalog-evidence consistency tests (#6731).
//!
//! The former `lsp_3_17_compliance_tests.rs` contained a zero-assertion
//! "partial result streaming contract" (the server has never implemented
//! `partialResultToken`) and a "full compliance" test that printed claims it
//! never verified and asserted only a method-list length. Both are deleted;
//! this file replaces them with executable checks over the evidence model:
//!
//! 1. the authority catalog validates cleanly against the GA evidence
//!    policy — every citation names an existing file and an existing
//!    assertion-bearing test function, and no row claims more than its class
//!    allows;
//! 2. the evidence-backed cohort cannot silently empty out: at least one row
//!    must remain `proven`, or claims and proof have drifted apart;
//! 3. the initialize response really advertises a non-empty capability set
//!    with completion present (a behavioral anchor, not a printed claim).

#![allow(clippy::print_stdout)]

mod support;

use std::path::PathBuf;

use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn repo_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .ok_or("cannot locate repository root from CARGO_MANIFEST_DIR")?;
    if !root.join("features.toml").is_file() {
        return Err(format!("authority catalog missing at {}", root.display()).into());
    }
    Ok(root.to_path_buf())
}

#[test]
fn authority_catalog_claims_never_exceed_their_evidence() -> TestResult {
    let root = repo_root()?;
    let catalog = perl_lsp_rs_core::feature_catalog::read_catalog(&root.join("features.toml"))?;
    let policy = perl_lsp_rs_core::feature_evidence::GaEvidencePolicy::load(
        &root.join("policy/ga-evidence-policy.toml"),
    )?;

    if let Err(violations) =
        perl_lsp_rs_core::feature_evidence::validate_catalog_evidence(&root, &catalog, &policy)
    {
        let rendered: Vec<String> =
            violations.iter().map(|v| format!("{}: {}", v.feature_id, v.detail)).collect();
        panic!(
            "catalog claims exceed their evidence:\n{}",
            rendered.join("\n")
        );
    }

    let proven_rows: Vec<&str> = catalog
        .features()
        .iter()
        .filter(|f| f.claim == Some(perl_lsp_rs_core::feature_evidence::DeclaredClaim::Proven))
        .map(|f| f.id.as_str())
        .collect();
    assert!(
        !proven_rows.is_empty(),
        "no catalog row carries claim = \"proven\"; an empty cohort means \
         public claims and proof have drifted apart (#6731)"
    );
    Ok(())
}

/// Behavioral anchor replacing the deleted method-count "full compliance"
/// theater: a live initialize exchange must return a non-empty capabilities
/// object with completion advertised.
#[test]
fn initialize_advertises_a_non_empty_capability_set() -> TestResult {
    let mut harness = LspHarness::new();
    let init_response = harness.initialize(None)?;
    let caps = &init_response["capabilities"];
    assert!(
        caps.is_object() && caps.as_object().is_some_and(|m| !m.is_empty()),
        "initialize returned no server capabilities: {init_response}"
    );
    assert!(
        caps.get("completionProvider").is_some_and(|v| !v.is_null()),
        "completion capability missing from advertise set: {init_response}"
    );
    Ok(())
}
