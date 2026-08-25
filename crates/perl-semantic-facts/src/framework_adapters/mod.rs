//! Concrete registry-backed framework adapters built on the checked
//! [`crate::framework`] SDK surface.
//!
//! Generic registry dispatch and canonical shard publication are owned by the
//! framework registry/publication issues; the adapters here are
//! framework-specific and shadow-only until those land.

/// Dancer2 framework adapter (#8914).
pub mod dancer2;
