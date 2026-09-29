//! Versioned native critic rule-proof manifest, checker, and status (#6973).
//! Checker residuals beyond the four-rule pilot: #14560.

mod digest;
mod error;
mod execute;
mod mapping;
mod model;
mod status;
mod validate;

pub use digest::file_digest;
pub use error::ProofError;
pub use execute::{EXECUTE_LIVE_OWNER_PATHS, execute_manifest};
pub use model::{
    EvidenceClass, FIXTURE_ROOT, MANIFEST_PATH, PILOT_RULES, RuleProofManifest, SCHEMA_PATH,
    SCHEMA_VERSION, STATUS_PATH, resolve_fixture_path,
};
pub use status::{check_status, render_status, write_status};
pub use validate::{load_and_validate, validate_manifest_value};

use std::path::Path;

/// Full check: schema, authorities, fixture digests, live critic, status freshness.
pub fn check(root: &Path) -> Result<RuleProofManifest, ProofError> {
    let manifest = load_and_validate(root)?;
    execute_manifest(root, &manifest)?;
    check_status(root, &manifest)?;
    Ok(manifest)
}

/// Validate and execute, then rewrite the generated status page.
pub fn check_and_write_status(root: &Path) -> Result<RuleProofManifest, ProofError> {
    let manifest = load_and_validate(root)?;
    execute_manifest(root, &manifest)?;
    write_status(root, &manifest)?;
    Ok(manifest)
}

/// Stable case IDs in manifest order.
#[must_use]
pub fn list_case_ids(manifest: &RuleProofManifest) -> Vec<&str> {
    manifest.cases.iter().map(|case| case.case_id.as_str()).collect()
}
