#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Public Cargo facade for the `perllsp` language server.
//!
//! Install the server with:
//!
//! ```bash
//! cargo install perllsp
//! ```
//!
//! That installs the `perllsp` binary while delegating the implementation to
//! the `perl-lsp-rs` crate.

#![deny(unsafe_code)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

/// Claude plugin/server compatibility contracts consumed by setup and support surfaces.
pub mod claude_compat;

pub use perl_lsp::*;
