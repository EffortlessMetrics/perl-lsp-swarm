//! Concrete registry-backed framework adapters built on the checked
//! [`crate::framework`] SDK surface.
//!
//! Generic registry dispatch and canonical shard publication are owned by the
//! framework registry/publication issues; the adapters here are
//! framework-specific and shadow-only until those land.

/// Dancer2 framework adapter (#8914).
pub mod dancer2;
/// Registry-activated Dancer2 hook fact minting (#8924).
pub mod dancer2_hooks;
/// Registry-activated Dancer2 route fact minting (#8918).
pub mod dancer2_routes;
/// DBIx::Class result-class/result-source identity adapter (#9736).
pub mod dbix_class;

/// Mojo::Base framework adapter (#9681).
pub mod mojo_base;

/// Mojolicious application/controller identity adapter (#9688).
pub mod mojolicious;
