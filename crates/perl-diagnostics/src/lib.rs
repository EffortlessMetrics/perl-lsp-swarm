#![deny(unsafe_code)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]

//! Unified diagnostic codes, transport-neutral byte spans, types, and catalog for Perl LSP.
//!
//! This crate consolidates three previously separate diagnostic crates:
//! - `perl-diagnostics-codes` — stable diagnostic codes, severity, and tags (now [`codes`])
//! - `perl-lsp-diagnostic-types` — diagnostic model types (now [`types`])
//! - `perl-lsp-diagnostic-catalog` — metadata builders for codes (now [`catalog`])
//!
//! # Modules
//!
//! - [`codes`] — canonical `DiagnosticCode`, `DiagnosticCategory`, `DiagnosticSeverity`, and
//!   `DiagnosticTag`
//! - [`types`] — validated [`ByteSpan`], [`Diagnostic`], and [`RelatedInformation`]
//! - [`catalog`] — diagnostic metadata catalog functions
//! - [`anchor`] — [`ParseDiagnosticAnchor`] for stale-source detection and
//!   once-per-batch freshness checking
//!
//! # Location contract
//!
//! Diagnostic locations are half-open UTF-8 byte spans. The type validates interval ordering;
//! source length, UTF-8 scalar boundaries, line/column projection, URI identity, and negotiated
//! position encoding remain responsibilities of the exact source-snapshot and transport layers.
//!
//! # Re-exports
//!
//! The crate root re-exports all public items via [`api`].
//!
//! [`ByteSpan`]: crate::types::ByteSpan
//! [`Diagnostic`]: crate::types::Diagnostic
//! [`RelatedInformation`]: crate::types::RelatedInformation
//! [`ParseDiagnosticAnchor`]: crate::anchor::ParseDiagnosticAnchor

/// Parser diagnostic anchors for stale-source detection and once-per-batch freshness.
pub mod anchor;
/// Diagnostic metadata catalog and LSP-facing helpers.
pub mod catalog;
/// Canonical diagnostic codes, categories, severities, and tags.
pub mod codes;
/// Transport-neutral diagnostic payload and byte-span types.
pub mod types;

mod api;
pub use api::*;
