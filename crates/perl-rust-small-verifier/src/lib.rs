//! Lightweight access to the repository's canonical Rust Small task.
//!
//! The task remains owned by xtask; sharing its source avoids a second receipt
//! contract and keeps canonical tests attached to both entrypoints.

// Match xtask/src/main.rs: task-runner diagnostics intentionally use stdout/stderr.
#[allow(clippy::print_stderr, clippy::print_stdout)]
// Canonical test assertion helpers panic on violated expectations. Match the
// workspace test-only policy (e.g. perl-parser-core); production keeps panic denied.
#[cfg_attr(test, allow(clippy::panic))]
#[path = "../../../xtask/src/tasks/rust_small_proof.rs"]
mod rust_small_proof;

pub use rust_small_proof::run;
