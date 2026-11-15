//! Framework-specific interpretation over the shared semantic-fact contracts.
//!
//! These adapters detect reviewed framework profiles, interpret import evidence,
//! and produce activation and object facts. Contract types, stable identities,
//! provenance, validation, and reference tables remain in
//! [`perl_semantic_facts::framework_adapters`].

/// Dancer2 1.x detection, import interpretation, and activation.
pub mod dancer2;
/// Dancer2 hook interpretation and fact production.
pub mod dancer2_hooks;
/// Dancer2 route interpretation and fact production.
pub mod dancer2_routes;
/// Dancer2 2.x detection, import interpretation, and activation.
pub mod dancer2_two_x;
/// DBIx::Class result-class and result-source interpretation.
pub mod dbix_class;
/// Mojo::Base detection, import interpretation, and activation.
pub mod mojo_base;
/// Mojo::Base generated members, callable results, and parent facts.
pub mod mojo_base_facts;
/// Mojolicious Lite activation and application/controller role interpretation.
pub mod mojolicious;
/// Moose and Moose::Role activation detection.
pub mod moose;
