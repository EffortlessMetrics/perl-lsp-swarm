//! Semantic analysis, symbol extraction, and type inference.

/// Pure callsite materialization for canonical callable-result relations.
pub mod call_result_materializer;
/// Bounded syntax-level callable exit summaries.
pub mod callable_exit;
/// Callable-local semantic summary assembly (#12674, I02): joins existing
/// canonical HIR/PIR facts by identity into validated per-callable packets.
pub mod callable_semantic_summary;
/// Class model for Moose/Moo/Mouse intelligence.
pub mod class_model;
/// Registry-backed Dancer2 activation-site extraction (#8914).
pub mod dancer2_activation;
pub mod dancer2_handler_targets;
pub mod dancer2_hooks;
/// Dancer2 route-declaration extraction (#8918).
pub mod dancer2_routes;
/// Static DBIx::Class result-source extraction (#9736).
pub mod dbix_class_result;
/// Go-to-declaration support and parent map construction.
#[cfg(not(target_arch = "wasm32"))]
pub mod declaration;
/// Export symbol extraction for Exporter-based Perl modules.
#[cfg(not(target_arch = "wasm32"))]
pub mod export_analyzer;
/// Generated member extraction from Moo/Moose `has` declarations.
#[cfg(not(target_arch = "wasm32"))]
pub mod generated_member_extractor;
/// Import specification extraction for static `use` statements.
#[cfg(not(target_arch = "wasm32"))]
pub mod import_extractor;
/// Lightweight workspace symbol index.
#[cfg(not(target_arch = "wasm32"))]
pub mod index;
/// Registry-backed Mojo::Base activation-site extraction (#9681).
pub mod mojo_base_activation;
/// Mojo::Base `has` attribute-declaration extraction (#9682).
pub mod mojo_base_attributes;
/// Registry-backed Mojolicious::Lite activation-site extraction (#9688).
pub mod mojolicious_activation;
/// Package graph edge extraction from inheritance and role-composition patterns.
#[cfg(not(target_arch = "wasm32"))]
pub mod package_graph_extractor;
/// Receiver facts for trust-bounded method completion.
pub mod receiver_facts;
/// Scope analysis for variable and subroutine resolution.
#[allow(missing_docs)]
pub mod scope_analyzer;
/// Semantic analyzer and token classification.
pub mod semantic;
/// Symbol extraction and symbol table construction.
pub mod symbol;
/// Rich type facts for expression and receiver inference.
pub mod type_facts;
/// Type inference engine for Perl variable analysis.
pub mod type_inference;
/// Lightweight value-shape inference from constructor calls, bless, and `$self`.
#[cfg(not(target_arch = "wasm32"))]
pub mod value_shape_inferrer;
