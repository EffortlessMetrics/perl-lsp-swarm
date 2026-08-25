// The runner substrate in `tests/support/emacs_host_runner.rs` is shared by
// the integration tests and (via `emacs_host_run`) by this library, and it
// refers to the crate the way an external consumer would. Declaring the
// self-alias keeps one source of truth compilable in both contexts.
extern crate self as xtask;

pub mod actual_host_receipt;
pub mod client_compat_fixture;
pub mod clippy_repair_corpus;
pub mod close_proof;
pub mod contributor_topology;
pub mod editor_client_compat;
pub mod emacs_host_run;
pub mod emacs_subject_manifest;
pub mod file_identity;
pub mod git_ancestry;
pub mod publication_drift;
pub mod rust_hygiene;
pub mod vim_lsp_cell_catalog;
pub mod vim_lsp_specialized_driver;
pub mod worktree_cleanup;
