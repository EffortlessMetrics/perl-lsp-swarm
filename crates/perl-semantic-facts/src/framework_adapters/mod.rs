//! Shared framework contracts and reviewed reference data.
//!
//! This vocabulary retains stable types, identities, and tables alongside the
//! checked [`crate::framework`] SDK. Concrete detection, import interpretation,
//! and fact production are owned by `perl_semantic_analyzer::framework_adapters`.

/// Dancer2 activation contracts and reviewed DSL reference data (#8914).
pub mod dancer2;
/// Dancer2 hook declarations and reviewed hook-name contracts (#8924).
pub mod dancer2_hooks;
/// Dancer2 route declarations and shared route-family contracts (#8918).
pub mod dancer2_routes;
/// Dancer2 2.x activation contracts and core-DSL registry data (#13616).
pub mod dancer2_two_x;
/// DBIx::Class result-class/result-source identity contracts (#9736).
pub mod dbix_class;

/// Mojo::Base activation contracts (#9681).
pub mod mojo_base;
/// Mojo::Base object-fact contracts and stable identity construction (#9682).
pub mod mojo_base_facts;

/// Mojolicious application/controller identity contracts (#9688).
pub mod mojolicious;

/// Moose and Moose::Role activation contracts (#7788).
pub mod moose;

/// Version-bound DBIx::QuickORM API return and preserving-method contract (#13374).
pub mod quickorm_api;
