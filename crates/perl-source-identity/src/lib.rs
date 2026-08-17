#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! `perl-source-identity` — canonical `source_identity.v1` core types for the
//! Perl toolchain transport layer.
//!
//! This crate defines the smallest transport-neutral substrate for
//! `source_identity.v1` so downstream migrations can consume real canonical
//! types instead of opaque strings, path hashes, or locally invented
//! references.
//!
//! # Design
//!
//! All durable IDs use **SHA-256** with explicit domain separation, so:
//!
//! - fixed inputs always produce byte-identical IDs across machines and builds;
//! - no host path, URI, traversal-order counter, or process-local value becomes
//!   stable identity;
//! - IDs of different kinds never collide even when their material inputs match.
//!
//! # Identity hierarchy
//!
//! ```text
//! ProjectId                — stable across roots, machines, and sessions
//!   └── WorkspaceRootId      — one checkout of the project at a specific root
//!         └── LogicalSourceId  — one logical file within that root
//!               └── ContentRevision  — logical source + exact bytes
//! ```
//!
//! Freshness is tracked separately via [`SourceGeneration`].
//!
//! # What this crate must not depend on
//!
//! Per issue #7652 and PLSP-ADR-0006, this crate must not import:
//!
//! - any parser implementation (AST/HIR/PIR);
//! - `perl-workspace` or the ProjectModel runtime;
//! - LSP/DAP/MCP/editor types;
//! - `tokio` or other async runtimes;
//! - Git, release workflows, repository receipts, or VS Code.
//!
//! This contract is asserted in `tests/dependency_contract.rs`.
//!
//! # Quick start
//!
//! ```no_run
//! use perl_source_identity::{
//!     ContentDigest, ContentRevision, LogicalSourceId, ProjectId,
//!     SourceGeneration, SourceIdentityEnvelope, WorkspaceRootId,
//! };
//!
//! let project = ProjectId::from_canonical_name("https://github.com/acme/widget");
//! let root = WorkspaceRootId::from_project_and_root_key(&project, "abc123");
//! let src = LogicalSourceId::from_root_and_path(&root, "lib/Widget.pm");
//!
//! let content = b"package Widget;\n1;\n";
//! let digest = ContentDigest::of_bytes(content);
//! let revision = ContentRevision::new(src.clone(), digest);
//!
//! let envelope = SourceIdentityEnvelope::for_workspace_file(
//!     project,
//!     root,
//!     src,
//!     Some(revision),
//!     SourceGeneration::known("1"),
//! );
//!
//! assert!(envelope.is_schema_supported());
//! assert!(envelope.has_known_generation());
//! ```

mod digest;
mod envelope;
mod generation;
mod ids;
mod origin;

// ── Public re-exports ─────────────────────────────────────────────────────────

pub use digest::{CONTENT_DIGEST_SCHEMA_VERSION, ContentDigest};

pub use ids::{LogicalSourceId, ProjectId, WorkspaceRootId};

pub use generation::{ContentRevision, SourceGeneration};

pub use origin::{PhysicalSourceRole, SourceOrigin};

pub use envelope::{SCHEMA_VERSION_V1, SourceIdentityEnvelope, SourceIdentitySchemaVersion};
