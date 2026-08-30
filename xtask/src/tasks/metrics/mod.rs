//! `cargo xtask metrics` subcommand tree.
//!
//! Each leaf module implements one user-facing subcommand.

pub mod diagnostics_stats;
pub mod hir_coverage;
#[path = "lsp_stats_guarded.rs"]
mod lsp_stats_guarded;
use self::lsp_stats_guarded as lsp_stats_admission;
#[path = "lsp_stats_timing_guard.rs"]
pub mod lsp_stats;
#[path = "lsp_stats.rs"]
mod lsp_stats_impl;
pub mod memory;
pub mod parser_accuracy;
pub mod parser_stats;
pub mod ratchet;
pub mod release_health;
pub mod stable_wins;
pub mod sweep_stats;
pub mod workspace_stats;
