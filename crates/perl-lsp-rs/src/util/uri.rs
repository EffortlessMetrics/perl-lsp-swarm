//! URI utilities for LSP.
//!
//! This module re-exports helpers from the `perl-lsp-uri` microcrate.

// Wave G3 (#4535): perl-lsp-uri absorbed into perl-lsp-rs-core::uri
pub use perl_lsp_rs_core::uri::{parse_uri, parse_uri_or_err, try_parse_uri};
