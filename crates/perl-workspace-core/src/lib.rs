//! `perl-workspace-core` — the LSP-free substrate of deterministic project-level
//! Perl facts.
//!
//! # What this crate is
//!
//! This crate sits **below** LSP, DAP, editor transport, and any shipped-product
//! runtime. It consumes parser / semantic / symbol / module / range primitives
//! and produces stable *project facts*: files, packages, symbols, modules,
//! imports, exports, POD, tests, dist metadata — each with a source range,
//! provenance, confidence, and deterministic identity where applicable. Other
//! lanes (DAP, critic, tidy, RIPR, Test2, Kwalitee, tree-sitter-compatible
//! output) *consume* these facts; they do not define the substrate.
//!
//! # Invariants
//!
//! - **Native ships; external tools compare.** Nothing here shells out to or
//!   depends on editor/tool runtimes.
//! - **No editor/runtime dependencies.** The crate must never depend on
//!   `perl-lsp-rs`, `perl-lsp-rs-core`, `perllsp`, `perl-dap`, `lsp-types`,
//!   `tokio`, `tower-lsp`, or perltidy/perlcritic adapters. This is enforced
//!   mechanically by `tests/dependency_contract.rs`.
//! - **Byte/UTF-8 ranges only.** UTF-16 conversion is an LSP-boundary concern;
//!   core ranges are [`SourceRange`] byte offsets.
//! - **Repo-relative paths only.** Host absolute paths are rejected at the
//!   [`RepoRelativePath`] boundary — no machine-specific state leaks into facts.
//! - **No faked certainty.** Where Perl behaviour is runtime/dynamic, the
//!   substrate emits a [`DynamicBoundary`] or [`ModelLimitation`] and lowers
//!   [`Confidence`] rather than pretending the answer is statically known.
//! - **Deterministic identity.** [`file_id_for`] and [`SymbolId::derive`] derive
//!   IDs from stable coordinates so facts are reproducible across runs.
//!
//! # Shared vocabulary
//!
//! Provenance, confidence, and the strongly-typed fact IDs come from
//! [`perl_semantic_facts`] and are re-exported here so the whole ecosystem
//! shares one identity space. This crate does not re-define them.
//!
//! # Roadmap (follow-up PRs)
//!
//! This is the skeleton PR: primitives + the enforced dependency contract. The
//! fact producers and the query API ([`packages_in_file`], `symbols_in_file`,
//! `imports_in_file`, `resolve_module`, `owner_at`, ...) land in follow-ups,
//! layered by fact class and gated by [`FactClasses`].
//!
//! [`packages_in_file`]: crate#roadmap-follow-up-prs

#![cfg_attr(docsrs, feature(doc_cfg))]

mod digest;
mod fact_class;
mod file;
mod ids;
mod limitation;
mod path;
mod range;

pub use digest::SourceDigest;
pub use fact_class::FactClasses;
pub use file::{FileRole, ParseStatus, classify_role};
pub use ids::{FileId, SymbolId, file_id_for};
pub use limitation::{DynamicBoundary, DynamicBoundaryKind, ModelLimitation};
pub use path::{PathError, RepoRelativePath};
pub use range::SourceRange;

// Re-export the neutral fact vocabulary so downstream consumers depend on one
// identity space rather than re-declaring provenance/confidence.
pub use perl_semantic_facts::{Confidence, Provenance};
