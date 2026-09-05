// The runner substrate in `tests/support/emacs_host_runner.rs` is shared by
// the integration tests and (via `emacs_host_run`) by this library, and it
// refers to the crate the way an external consumer would. Declaring the
// self-alias keeps one source of truth compilable in both contexts.
extern crate self as xtask;

pub mod actual_host_receipt;
pub mod branch_deletion_admission;
pub mod ci_route_plan;
pub mod client_compat_fixture;
pub mod clippy_repair_corpus;
pub mod close_proof;
pub mod compiler_lexical_cutline;
pub mod compiler_profile_contract;
pub mod compiler_profile_initial_rows;
pub mod compiler_profile_observation;
pub mod contributor_topology;
pub mod critic_rule_proof;
pub mod editor_client_compat;
pub mod editor_host;
pub mod emacs_eglot_upstream_patch;
pub mod emacs_host_journeys;
pub mod emacs_host_run;
pub mod emacs_stock_discovery;
pub mod emacs_subject_fan_in;
pub mod emacs_subject_manifest;
pub mod file_identity;
pub mod git_ancestry;
pub mod import_cleanup_train_manifest;
pub mod lsp_runtime_train_manifest;
pub mod native_helix_actions;
pub mod native_neovim_actions;
pub mod parser_accuracy_legacy_population;
pub mod publication_drift;
pub mod rust_hygiene;
pub mod vim_host_diagnostics_run;
pub mod vim_host_freshness_run;
pub mod vim_host_run;
pub mod vim_host_save_format_run;
pub mod vim_host_toolchain;
pub mod vim_lsp_cell_catalog;
pub mod vim_lsp_specialized_driver;
pub mod vim_lsp_subject_refresh;
pub mod worktree_cleanup;
pub mod worktree_forensic_recovery;
