#![warn(missing_docs)]

//! `perl-evidence-envelope` — canonical `evidence_envelope.v1` core types and
//! deterministic envelope identity for the Perl toolchain's evidence
//! infrastructure.
//!
//! This crate defines the domain-neutral **outer** evidence record every
//! specific evidence payload (test receipts, coverage receipts, gate
//! receipts, and future kinds not yet designed) is carried inside, plus the
//! deterministic [`EnvelopeFingerprint`] that answers "is this the same
//! evidence?" independent of field or collection insertion order.
//!
//! # Internal infrastructure — no consumers yet
//!
//! This crate is `publish = false`: it is internal evidence infrastructure
//! being built ahead of the sibling crates (registry, validator, freshness
//! rules, JSON schema projection, CLI) that will actually mint and consume
//! `EvidenceEnvelope` values in production. It exists now so those slices can
//! consume real canonical types instead of ad hoc structs, without reopening
//! this contract per sibling.
//!
//! # Reuse, not reinvention
//!
//! This crate depends on [`perl_source_identity`] for
//! [`perl_source_identity::ContentDigest`] and
//! [`perl_source_identity::ProjectId`] rather than re-deriving
//! domain-separated content/project identity: that crate already owns
//! `source_identity.v1`'s SHA-256 digest and length-prefixed field encoding,
//! and the repository contract forbids a second, competing spelling of the
//! same authority. The only new hashing this crate adds is
//! [`EnvelopeFingerprint`]'s domain-separated hash, which genuinely needs its
//! own domain tag distinct from `ContentDigest`'s (see the domain-separation
//! test in `fingerprint.rs`).
//!
//! # What this crate must not depend on
//!
//! Per issue #15057 and the pattern set by `perl-source-identity` (#7652),
//! this crate must not import:
//!
//! - any parser implementation (AST/HIR/PIR);
//! - `perl-workspace` or the ProjectModel runtime;
//! - LSP/DAP/MCP/editor types;
//! - `tokio` or other async runtimes;
//! - Git, release workflows, repository receipts, or VS Code.
//!
//! This contract is asserted in `tests/dependency_contract.rs`.
//!
//! # What this crate deliberately does not own
//!
//! Explicitly deferred to sibling issues, so this slice stays independently
//! provable:
//!
//! - a receipt registry schema or loader;
//! - a validation library that enforces domain-specific payload shapes;
//! - a freshness rule registry;
//! - a JSON Schema projection of these types;
//! - CLI tooling;
//! - lineage **cycle detection** over [`InputReference`] edges — this crate
//!   carries lineage edges as data only.
//!
//! # Quick start
//!
//! ```
//! use perl_evidence_envelope::{
//!     ClaimBoundary, Completeness, EvidenceEnvelope, EvidenceEnvelopeSchemaVersion,
//!     EvidenceSubject, PayloadIdentity, ProducerIdentity, ReceiptId, RedactionClass,
//!     RetentionClass, RunIdentity, RunSource,
//! };
//! use perl_source_identity::{ContentDigest, ProjectId};
//!
//! // The receipt ID is namespaced by its producer, so a producer-local key
//! // such as "run-42" cannot collide with another producer's.
//! let producer = ProducerIdentity::new("perl-lsp-test-runner", "0.17.0", "abc123", "ci-42");
//!
//! let envelope = EvidenceEnvelope {
//!     schema_version: EvidenceEnvelopeSchemaVersion::V1,
//!     receipt_id: ReceiptId::from_producer_and_key(&producer, "run-42"),
//!     payload: PayloadIdentity::new(
//!         "test-receipt",
//!         1,
//!         ContentDigest::of_bytes(b"payload bytes"),
//!     ),
//!     producer,
//!     subject: EvidenceSubject::new(
//!         ProjectId::from_canonical_name("acme/widget"),
//!         Some("base-sha".to_string()),
//!         Some("head-sha".to_string()),
//!         None,
//!         None,
//!         RunIdentity::new(RunSource::Workflow, "run-1", 1),
//!     ),
//!     completeness: Completeness::Complete,
//!     redaction_class: RedactionClass::Public,
//!     retention_class: RetentionClass::Standard,
//!     claim_boundary: ClaimBoundary::empty(),
//!     limitations: Vec::new(),
//!     inputs: Vec::new(),
//! };
//!
//! assert!(envelope.is_schema_supported());
//! let fingerprint = envelope.fingerprint();
//! assert_eq!(fingerprint, envelope.fingerprint(), "fingerprint is deterministic");
//! ```
#![deny(clippy::map_err_ignore)]

/// Compiles the `README.md` quick start as a doctest.
///
/// The README is a consumer's first read and is not otherwise checked by any
/// build. Including it here means an API change that invalidates the example
/// fails `cargo test -p perl-evidence-envelope --doc` instead of shipping a
/// snippet that does not compile.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod claim;
mod classification;
mod completeness;
mod envelope;
mod fingerprint;
mod input;
mod payload;
mod producer;
mod receipt;
mod subject;
mod wire;

// ── Public re-exports ─────────────────────────────────────────────────────────

pub use claim::{ClaimBoundary, Limitation};
pub use classification::{RedactionClass, RetentionClass};
pub use completeness::Completeness;
pub use envelope::{
    EVIDENCE_ENVELOPE_SCHEMA_VERSION_V1, EvidenceEnvelope, EvidenceEnvelopeSchemaVersion,
};
pub use fingerprint::EnvelopeFingerprint;
pub use input::InputReference;
pub use payload::PayloadIdentity;
pub use producer::ProducerIdentity;
pub use receipt::ReceiptId;
pub use subject::{EvidenceSubject, RunIdentity, RunSource};
