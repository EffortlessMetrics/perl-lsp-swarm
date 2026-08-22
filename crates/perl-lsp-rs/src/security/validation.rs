//! Input validation and sanitization utilities for production hardening.
//!
//! The concrete implementation lives in `perl-lsp-rs-core`.
pub use perl_lsp_rs_core::runtime::input_validation::{
    sanitize_string, validate_document_uri, validate_file_content, validate_file_path,
    validate_request_admission, validate_workspace_root,
};
