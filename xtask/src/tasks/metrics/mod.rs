//! `cargo xtask metrics` subcommand tree.
//!
//! Each leaf module implements one user-facing subcommand.

pub mod diagnostics_stats;
pub mod hir_coverage;
#[path = "lsp_stats_timing_guard.rs"]
pub mod lsp_stats;
#[path = "lsp_stats_guarded.rs"]
mod lsp_stats_guarded;
#[path = "lsp_stats.rs"]
mod lsp_stats_impl;
pub mod memory;
pub mod parser_accuracy;
// The safe-point/region registry is consumed in production by the
// parser-accuracy integrity consult today; its full typed surface (admission
// decisions, outcome accessors, per-fixture evaluation) becomes bin-reachable
// as the typed plane comparator (#13662) and single-run control plane (#13664)
// land. The applicability suites exercise the complete surface.
#[allow(dead_code)]
pub mod parser_accuracy_metamorphic_registry;
pub mod parser_accuracy_metamorphic_transform;
pub mod parser_stats;
pub mod ratchet;
pub mod release_health;
pub mod stable_wins;
pub mod sweep_stats;
pub mod workspace_stats;
