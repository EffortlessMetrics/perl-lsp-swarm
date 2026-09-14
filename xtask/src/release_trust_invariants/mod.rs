//! Versioned release trust-invariant registry, checker, and status (#9392).
//!
//! This module defines machine-readable `release_trust_invariants.v1` rows
//! and fails closed on unknown, duplicate, ownerless, or superseded producer
//! authority. It does not consume live candidate receipts, execute falsifiers,
//! or change release controllers.

mod error;
mod model;
mod status;
mod validate;

pub use error::RegistryError;
pub use model::{REGISTRY_PATH, SCHEMA_PATH, SCHEMA_VERSION, STATUS_PATH, TrustInvariantRegistry};
pub use status::{check_status, render_status, write_status};
pub use validate::{load_and_validate, validate_registry_value};

use std::path::Path;

/// Schema, authority, and generated-status check. No live receipts.
pub fn check(root: &Path) -> Result<TrustInvariantRegistry, RegistryError> {
    let registry = load_and_validate(root)?;
    check_status(root, &registry)?;
    Ok(registry)
}

/// Validate, then rewrite the generated Markdown projection.
pub fn check_and_write_status(root: &Path) -> Result<TrustInvariantRegistry, RegistryError> {
    let registry = load_and_validate(root)?;
    write_status(root, &registry)?;
    Ok(registry)
}

/// Stable invariant IDs in registry order.
#[must_use]
pub fn list_invariant_ids(registry: &TrustInvariantRegistry) -> Vec<&str> {
    registry.invariants.iter().map(|row| row.invariant_id.as_str()).collect()
}
