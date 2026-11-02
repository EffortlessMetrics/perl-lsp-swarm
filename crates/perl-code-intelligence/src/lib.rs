#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! Transport-neutral code-intelligence session boundary (#6957 / #7218).
//!
//! Adapters (LSP, MCP, direct CLI) construct a [`CodeIntelligenceSession`] from
//! their own launch inputs. The session carries document source authority and
//! generation identity; it does not perform provider queries itself in this
//! first slice.
//!
//! # Dependency boundary
//!
//! This crate must not depend on LSP/MCP wire crates, editor clients, or the
//! `perl-lsp-rs` server runtime. The contract is asserted in
//! `tests/dependency_contract.rs`.

#![deny(clippy::map_err_ignore)]

mod document_source;
mod error;
mod session;

pub use document_source::{DocumentSourceAuthority, DocumentSourceKind, ResolvedDocumentAuthority};
pub use error::{DocumentAuthorityError, SessionConstructionError};
pub use session::{CodeIntelligenceSession, SessionIdentity, SessionScope};
