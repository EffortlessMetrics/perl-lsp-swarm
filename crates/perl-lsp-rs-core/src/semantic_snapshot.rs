//! Ticket-bound immutable file semantic snapshot envelope (#12150).
//!
//! This module defines, without constructing anything, what one *completed*
//! semantic operation result for one accepted parser ticket is:
//!
//! 1. one closed versioned identity vocabulary (document instance, accepted
//!    parser ticket, semantic schema/implementation/profile, contribution
//!    set, materialized query view, work receipt, predecessor, project-fact
//!    projection);
//! 2. one immutable transport-neutral envelope [`FileSemanticSnapshotV1`]
//!    binding those identities to one exact analysis subject;
//! 3. one checked constructor and typed validator
//!    ([`FileSemanticSnapshotValidationError`]) refusing mixed, stale,
//!    incomplete and contradictory subjects;
//! 4. one bounded read-only access boundary that performs no semantic work
//!    and cannot flatten partial, stale, unavailable, failed or
//!    instrument-incomplete state into empty success.
//!
//! # Claim ceiling (representation only)
//!
//! Contract only. No semantic construction cell, no analysis execution, no
//! concurrent scheduling, no accepted attachment or currentness publication,
//! no ProjectModel publication, no compiler-contribution embedding, no
//! provider cutover, no release behavior. #12151 owns construction, #8575
//! owns acceptance/currentness, #9284/#8669 own compiler-side joins, #4772
//! owns project-fact projection.
//!
//! # Object boundaries (from the controlling issue)
//!
//! ```text
//! FileSemanticContributionSet  (owned by #12135) — facts for one exact subject
//! FileSemanticSnapshot         (this module)      — one completed typed result
//! Accepted semantic attachment (owned by #8575)   — proof this became current
//! FilePirLexicalContribution   (owned by #9284)   — sibling compiler result
//! ```
//!
//! A valid snapshot is not current merely because its fields validate.
//! Currentness is a later proof owned by #8575. A compiler contribution is
//! joined by identity, never embedded as semantic facts (#8669).
//!
//! # Identity authority
//!
//! Logical source, content revision and source-generation identities reuse
//! the canonical `source_identity.v1` types from `perl-source-identity`.
//! Parser-side identities (accepted parse generation, snapshot disposition
//! and production strategy) are represented as closed wire vocabularies
//! value-aligned with `perl_parser::incremental::{ParseGeneration,
//! ParseTerminalDisposition, ParseSnapshotStrategy}`; converting live parser
//! values into this envelope belongs to the constructor owner (#12151), so
//! this module performs no parser-crate conversion and imports no parser or
//! analyzer crate.
//!
//! # Determinism
//!
//! The snapshot fingerprint is a domain-separated digest over canonical
//! length-prefixed wire parts. No wall-clock time, process id, host path,
//! map iteration order, or other nondeterministic input enters any identity
//! or fingerprint. Materialized views and limitation entries are canonically
//! ordered at construction; unordered wire input is refused rather than
//! silently normalized.

use std::fmt::Write as _;

use perl_source_identity::{ContentDigest, ContentRevision, LogicalSourceId, SourceGeneration};
use serde::{Deserialize, Deserializer, Serialize};

// ---------------------------------------------------------------------------
// Schema version
// ---------------------------------------------------------------------------

/// Current schema version for the `file_semantic_snapshot.v1` envelope.
pub const FILE_SEMANTIC_SNAPSHOT_SCHEMA_VERSION_V1: u32 = 1;

/// A versioned schema marker for the `file_semantic_snapshot.v1` format.
///
/// # Fail-closed deserialization
///
/// Deserialization rejects any version this build does not support. An
/// unrecognized version is an error at the serde boundary rather than a
/// value that flows onward and is only caught if a consumer remembers to
/// check support. This mirrors the `source_identity.v1` envelope precedent:
/// the failure worth preventing is a future-schema snapshot being read as
/// though it were v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct FileSemanticSnapshotSchemaVersion(pub u32);

impl<'de> Deserialize<'de> for FileSemanticSnapshotSchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = u32::deserialize(deserializer)?;
        let version = Self(raw);
        if version.is_supported() {
            Ok(version)
        } else {
            Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Unsigned(u64::from(raw)),
                &"a supported file_semantic_snapshot schema version (currently 1)",
            ))
        }
    }
}

impl FileSemanticSnapshotSchemaVersion {
    /// The current `file_semantic_snapshot.v1` schema version.
    pub const V1: Self = Self(FILE_SEMANTIC_SNAPSHOT_SCHEMA_VERSION_V1);

    /// Returns `true` if this version is one the current runtime recognizes.
    #[must_use]
    pub const fn is_supported(&self) -> bool {
        self.0 == FILE_SEMANTIC_SNAPSHOT_SCHEMA_VERSION_V1
    }

    /// Unwrap the raw integer version.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for FileSemanticSnapshotSchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "file_semantic_snapshot.v{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Domain digest + wire-id machinery
// ---------------------------------------------------------------------------

/// Compute a domain-separated digest over canonical length-prefixed parts.
///
/// The canonical form is `domain || NUL || (u32_be(len) || bytes)*`. Parts
/// must already be in wire/canonical form. The result is a
/// [`ContentDigest`], which applies its own `content-digest.v1` domain and
/// length framing on top, so every id below is doubly domain-separated.
fn semantic_domain_digest(domain: &[u8], parts: &[&[u8]]) -> ContentDigest {
    let mut canonical = Vec::with_capacity(64);
    canonical.extend_from_slice(domain);
    canonical.push(0);
    for part in parts {
        let len = u32::try_from(part.len()).unwrap_or(u32::MAX);
        canonical.extend_from_slice(&len.to_be_bytes());
        canonical.extend_from_slice(part);
    }
    ContentDigest::of_bytes(&canonical)
}

/// Wire-strict lowercase hex digits (SHA-256 rendering) accepted by every
/// semantic wire id.
fn is_wire_hex(bytes: &[u8]) -> bool {
    bytes.len() == 64 && bytes.iter().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}

fn wire_digest_suffix(digest: &ContentDigest) -> &str {
    &digest.as_wire()["sha256:".len()..]
}

/// Declare a hashed wire id type: `<prefix><64 lowercase hex digits>`.
///
/// Mirrors the `wire_id!` discipline of `perl-source-identity`: one id has
/// exactly one spelling, uppercase hex is rejected rather than normalized,
/// and deserialization is validating so an ill-formed id can never exist as
/// a value of the type.
macro_rules! semantic_wire_id {
    ($ty:ident, $prefix:literal, $expected:literal) => {
        impl $ty {
            /// Parse this id from its wire representation.
            ///
            /// Returns `None` unless the string is exactly
            #[doc = concat!("`", $prefix, "<64 lowercase hex digits>`.")]
            /// Uppercase hex is rejected rather than normalized, because
            /// equality and hashing are defined over the wire string.
            #[must_use]
            pub fn from_wire(s: &str) -> Option<Self> {
                let rest = s.strip_prefix($prefix)?;
                is_wire_hex(rest.as_bytes()).then(|| Self(s.to_owned()))
            }

            /// The wire representation.
            #[must_use]
            pub fn as_wire(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Self::from_wire(&raw).ok_or_else(|| {
                    serde::de::Error::invalid_value(serde::de::Unexpected::Str(&raw), &$expected)
                })
            }
        }

        impl std::fmt::Display for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

const DOC_INSTANCE_DOMAIN: &[u8] = b"perl-lsp:semantic-document-instance-id:v1";
const PARSE_TICKET_DOMAIN: &[u8] = b"perl-lsp:accepted-parser-ticket-id:v1";
const SEMANTIC_SCHEMA_DOMAIN: &[u8] = b"perl-lsp:semantic-schema-id:v1";
const SEMANTIC_IMPL_DOMAIN: &[u8] = b"perl-lsp:semantic-implementation-id:v1";
const SEMANTIC_PROFILE_DOMAIN: &[u8] = b"perl-lsp:semantic-profile-id:v1";
const SEMANTIC_PROFILE_FP_DOMAIN: &[u8] = b"perl-lsp:semantic-profile-fingerprint:v1";
const SEMANTIC_SET_ID_DOMAIN: &[u8] = b"perl-lsp:semantic-contribution-set-id:v1";
const SEMANTIC_VIEW_ID_DOMAIN: &[u8] = b"perl-lsp:semantic-query-view-id:v1";
const SEMANTIC_INSTRUMENT_DOMAIN: &[u8] = b"perl-lsp:semantic-instrument-id:v1";
const SEMANTIC_RECEIPT_DOMAIN: &[u8] = b"perl-lsp:semantic-work-receipt-id:v1";
const PROJECT_FACT_PROJECTION_DOMAIN: &[u8] = b"perl-lsp:project-fact-projection-id:v1";
const SUBJECT_FINGERPRINT_DOMAIN: &[u8] = b"perl-lsp:semantic-subject-fingerprint:v1";
const SNAPSHOT_FINGERPRINT_DOMAIN: &[u8] = b"perl-lsp:file-semantic-snapshot-fingerprint:v1";

// ---------------------------------------------------------------------------
// Closed identity ids
// ---------------------------------------------------------------------------

/// Identity of one document instance (one open/close lifetime) of a logical
/// source.
///
/// Close/reopen of the same [`LogicalSourceId`] produces a *different*
/// document instance, so two snapshots of source-identical reopened content
/// never collapse into one current subject. The instance key is an
/// authority-provided nonce (for example an open sequence counter), never a
/// wall-clock time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct DocumentInstanceId(String);

impl DocumentInstanceId {
    /// Derive a document instance id from its logical source and an
    /// authority-provided instance key.
    #[must_use]
    pub fn from_logical_source_and_instance_key(
        logical_source: &LogicalSourceId,
        instance_key: &str,
    ) -> Self {
        let digest = semantic_domain_digest(
            DOC_INSTANCE_DOMAIN,
            &[logical_source.as_wire().as_bytes(), instance_key.as_bytes()],
        );
        Self(format!("doc-instance:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    DocumentInstanceId,
    "doc-instance:sha256:",
    "a document instance id of the form `doc-instance:sha256:<64 lowercase hex digits>`"
);

/// Identity of one accepted parser ticket (#11665 spec; constructor owner
/// #12151).
///
/// Binds one exact document instance, one accepted parse generation, and the
/// exact parse-snapshot source digest, so a ticket id can never be replayed
/// against another subject, generation, or parse input.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct AcceptedParserTicketId(String);

impl AcceptedParserTicketId {
    /// Derive the ticket id from its exact bound parts.
    #[must_use]
    pub fn from_bound_parts(
        document_instance: &DocumentInstanceId,
        accepted_generation: u64,
        snapshot_source_digest: &ContentDigest,
    ) -> Self {
        let digest = semantic_domain_digest(
            PARSE_TICKET_DOMAIN,
            &[
                document_instance.as_wire().as_bytes(),
                &accepted_generation.to_be_bytes(),
                snapshot_source_digest.as_wire().as_bytes(),
            ],
        );
        Self(format!("parse-ticket:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    AcceptedParserTicketId,
    "parse-ticket:sha256:",
    "an accepted parser ticket id of the form `parse-ticket:sha256:<64 lowercase hex digits>`"
);

/// Identity of the semantic schema family a snapshot speaks (#12121 spec
/// text).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SemanticSchemaId(String);

impl SemanticSchemaId {
    /// Derive a schema id from its authority-defined name and version.
    #[must_use]
    pub fn from_name_and_version(name: &str, version: u32) -> Self {
        let digest = semantic_domain_digest(
            SEMANTIC_SCHEMA_DOMAIN,
            &[name.as_bytes(), &version.to_be_bytes()],
        );
        Self(format!("semantic-schema:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    SemanticSchemaId,
    "semantic-schema:sha256:",
    "a semantic schema id of the form `semantic-schema:sha256:<64 lowercase hex digits>`"
);

/// Identity of the semantic implementation (producer build) that produced a
/// snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SemanticImplementationId(String);

impl SemanticImplementationId {
    /// Derive an implementation id from an authority-defined producer label
    /// (crate/version/build inputs, never a host path).
    #[must_use]
    pub fn from_producer_label(label: &str) -> Self {
        let digest = semantic_domain_digest(SEMANTIC_IMPL_DOMAIN, &[label.as_bytes()]);
        Self(format!("semantic-impl:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    SemanticImplementationId,
    "semantic-impl:sha256:",
    "a semantic implementation id of the form `semantic-impl:sha256:<64 lowercase hex digits>`"
);

/// Identity of the semantic analysis profile selected for construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SemanticProfileId(String);

impl SemanticProfileId {
    /// Derive a profile id from its authority-defined profile name.
    #[must_use]
    pub fn from_profile_name(name: &str) -> Self {
        let digest = semantic_domain_digest(SEMANTIC_PROFILE_DOMAIN, &[name.as_bytes()]);
        Self(format!("semantic-profile:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    SemanticProfileId,
    "semantic-profile:sha256:",
    "a semantic profile id of the form `semantic-profile:sha256:<64 lowercase hex digits>`"
);

/// Identity of one #12135 `FileSemanticContributionSet` for one exact
/// analysis subject.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SemanticContributionSetId(String);

impl SemanticContributionSetId {
    /// Derive the set id from the exact subject fingerprint and semantic
    /// profile triple that owns it. #12135 is the construction authority;
    /// this binding is what makes a set from another subject refuse.
    #[must_use]
    pub fn from_subject_and_profile(
        subject_fingerprint: &ContentDigest,
        profile: &SemanticProfileIdentity,
    ) -> Self {
        let digest = semantic_domain_digest(
            SEMANTIC_SET_ID_DOMAIN,
            &[
                subject_fingerprint.as_wire().as_bytes(),
                profile.schema.as_wire().as_bytes(),
                profile.implementation.as_wire().as_bytes(),
                profile.profile.as_wire().as_bytes(),
            ],
        );
        Self(format!("semantic-set:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    SemanticContributionSetId,
    "semantic-set:sha256:",
    "a semantic contribution set id of the form `semantic-set:sha256:<64 lowercase hex digits>`"
);

/// Identity of one #12138 materialized neutral query view.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct MaterializedQueryViewId(String);

impl MaterializedQueryViewId {
    /// Derive the view id from its owning contribution set and view kind.
    #[must_use]
    pub fn from_set_and_kind(set: &SemanticContributionSetId, kind: SemanticQueryViewKind) -> Self {
        let digest = semantic_domain_digest(
            SEMANTIC_VIEW_ID_DOMAIN,
            &[set.as_wire().as_bytes(), kind.as_str().as_bytes()],
        );
        Self(format!("semantic-view:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    MaterializedQueryViewId,
    "semantic-view:sha256:",
    "a materialized query view id of the form `semantic-view:sha256:<64 lowercase hex digits>`"
);

/// Identity of the concrete instrument instance that performed the work.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct InstrumentInstanceId(String);

impl InstrumentInstanceId {
    /// Derive an instrument instance id from its instrument class and an
    /// authority-provided instance key.
    #[must_use]
    pub fn from_kind_and_key(kind: SemanticInstrumentKind, key: &str) -> Self {
        let digest = semantic_domain_digest(
            SEMANTIC_INSTRUMENT_DOMAIN,
            &[kind.as_str().as_bytes(), key.as_bytes()],
        );
        Self(format!("semantic-instrument:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    InstrumentInstanceId,
    "semantic-instrument:sha256:",
    "an instrument instance id of the form `semantic-instrument:sha256:<64 lowercase hex digits>`"
);

/// Identity of one semantic work receipt.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SemanticWorkReceiptId(String);

impl SemanticWorkReceiptId {
    /// Derive the receipt id from its instrument instance and deterministic
    /// work sequence.
    #[must_use]
    pub fn from_instrument_and_sequence(
        instrument: &InstrumentIdentity,
        work_sequence: u64,
    ) -> Self {
        let digest = semantic_domain_digest(
            SEMANTIC_RECEIPT_DOMAIN,
            &[instrument.instance.as_wire().as_bytes(), &work_sequence.to_be_bytes()],
        );
        Self(format!("semantic-work-receipt:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    SemanticWorkReceiptId,
    "semantic-work-receipt:sha256:",
    "a semantic work receipt id of the form `semantic-work-receipt:sha256:<64 lowercase hex digits>`"
);

/// Identity of one project-fact projection over this snapshot (#4772 seam).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ProjectFactProjectionId(String);

impl ProjectFactProjectionId {
    /// Derive the projection id from its logical source and semantic
    /// profile triple.
    #[must_use]
    pub fn from_source_and_profile(
        logical_source: &LogicalSourceId,
        profile: &SemanticProfileIdentity,
    ) -> Self {
        let digest = semantic_domain_digest(
            PROJECT_FACT_PROJECTION_DOMAIN,
            &[
                logical_source.as_wire().as_bytes(),
                profile.schema.as_wire().as_bytes(),
                profile.implementation.as_wire().as_bytes(),
                profile.profile.as_wire().as_bytes(),
            ],
        );
        Self(format!("project-fact-projection:sha256:{}", wire_digest_suffix(&digest)))
    }
}
semantic_wire_id!(
    ProjectFactProjectionId,
    "project-fact-projection:sha256:",
    "a project-fact projection id of the form `project-fact-projection:sha256:<64 lowercase hex digits>`"
);

// ---------------------------------------------------------------------------
// Semantic profile triple
// ---------------------------------------------------------------------------

/// The schema/implementation/profile triple a snapshot was produced under.
///
/// The fingerprint is recomputed and checked by the validator, so a wire
/// payload cannot mix one part of one triple with another part of another.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticProfileIdentity {
    /// Semantic schema family (#12121 spec text).
    pub schema: SemanticSchemaId,
    /// Producer/implementation identity.
    pub implementation: SemanticImplementationId,
    /// Selected analysis profile.
    pub profile: SemanticProfileId,
    /// Deterministic digest over the triple, recomputed on validation.
    pub fingerprint: ContentDigest,
}

impl SemanticProfileIdentity {
    /// Construct the triple from authority-defined names, computing the
    /// fingerprint.
    #[must_use]
    pub fn new(
        schema_name: &str,
        schema_version: u32,
        implementation_label: &str,
        profile_name: &str,
    ) -> Self {
        let schema = SemanticSchemaId::from_name_and_version(schema_name, schema_version);
        let implementation = SemanticImplementationId::from_producer_label(implementation_label);
        let profile = SemanticProfileId::from_profile_name(profile_name);
        let fingerprint = Self::fingerprint_over(&schema, &implementation, &profile);
        Self { schema, implementation, profile, fingerprint }
    }

    /// Recompute the triple fingerprint.
    #[must_use]
    pub fn fingerprint_over(
        schema: &SemanticSchemaId,
        implementation: &SemanticImplementationId,
        profile: &SemanticProfileId,
    ) -> ContentDigest {
        semantic_domain_digest(
            SEMANTIC_PROFILE_FP_DOMAIN,
            &[
                schema.as_wire().as_bytes(),
                implementation.as_wire().as_bytes(),
                profile.as_wire().as_bytes(),
            ],
        )
    }
}

// ---------------------------------------------------------------------------
// Analysis subject
// ---------------------------------------------------------------------------

/// The parser-input revision: the exact bytes the parser consumed for this
/// ticket.
///
/// Parser input may be a projection of the full source (for example a
/// normalized or truncated variant), so it carries its own digest and length
/// instead of aliasing the full-source revision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParserInputRevision {
    /// Digest of the exact parser-input bytes.
    pub digest: ContentDigest,
    /// Byte length of the exact parser-input bytes.
    pub byte_len: u64,
}

impl ParserInputRevision {
    /// Construct a parser-input revision.
    #[must_use]
    pub fn new(digest: ContentDigest, byte_len: u64) -> Self {
        Self { digest, byte_len }
    }
}

/// The exact analysis subject of one snapshot: logical source, document
/// instance, checked source generation, and both exact revisions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticSubjectIdentity {
    /// Stable revision-independent logical source identity.
    pub logical_source_id: LogicalSourceId,
    /// One open/close lifetime of that logical source. Close/reopen
    /// produces a distinct instance and never collapses into this one.
    pub document_instance: DocumentInstanceId,
    /// Checked source generation label (`source_identity.v1`).
    pub source_generation: SourceGeneration,
    /// Exact full-source revision. Must carry the same logical source id.
    pub full_source_revision: ContentRevision,
    /// Exact parser-input revision.
    pub parser_input_revision: ParserInputRevision,
}

impl SemanticSubjectIdentity {
    /// Construct a subject, binding its full-source revision to the same
    /// logical source.
    #[must_use]
    pub fn new(
        logical_source_id: LogicalSourceId,
        document_instance: DocumentInstanceId,
        source_generation: SourceGeneration,
        full_source_digest: ContentDigest,
        parser_input_revision: ParserInputRevision,
    ) -> Self {
        let full_source_revision =
            ContentRevision::new(logical_source_id.clone(), full_source_digest);
        Self {
            logical_source_id,
            document_instance,
            source_generation,
            full_source_revision,
            parser_input_revision,
        }
    }

    /// Deterministic subject fingerprint binding every identity part:
    /// logical source, document instance, generation label, both revision
    /// digests, and the parser-input length.
    #[must_use]
    pub fn fingerprint(&self) -> ContentDigest {
        semantic_domain_digest(
            SUBJECT_FINGERPRINT_DOMAIN,
            &[
                self.logical_source_id.as_wire().as_bytes(),
                self.document_instance.as_wire().as_bytes(),
                &self.source_generation_label_bytes(),
                self.full_source_revision.content_digest.as_wire().as_bytes(),
                self.parser_input_revision.digest.as_wire().as_bytes(),
                &self.parser_input_revision.byte_len.to_be_bytes(),
            ],
        )
    }

    /// Fingerprint bytes of the source-generation part: the variant is
    /// tagged separately from the label so `Unknown`, `Known("")`, and any
    /// `Known` label whose text collides with a sentinel can never share
    /// subject fingerprints. Variants from a newer `perl-source-identity`
    /// share the `future` tag: still distinct from every local variant.
    fn source_generation_label_bytes(&self) -> Vec<u8> {
        match &self.source_generation {
            SourceGeneration::Unknown => b"unknown".to_vec(),
            SourceGeneration::Known(label) => {
                let mut bytes = Vec::with_capacity(6 + label.len());
                bytes.extend_from_slice(b"known:");
                bytes.extend_from_slice(label.as_bytes());
                bytes
            }
            _ => b"future".to_vec(),
        }
    }
}

// ---------------------------------------------------------------------------
// Parser-side identity projection
// ---------------------------------------------------------------------------

/// Closed wire mirror of `ParseTerminalDisposition`, value-aligned with
/// `perl_parser::incremental`. The constructor owner (#12151) converts live
/// parser values; this module never imports the parser crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticParseDisposition {
    /// Parsing completed without recovery, budget exhaustion, or cancellation.
    Clean,
    /// Parsing produced a current partial tree through recorded repairs.
    Recovered,
    /// Parsing could not produce an ordinary clean/recovered result.
    Catastrophic,
    /// Parsing stopped through cooperative cancellation.
    Cancelled,
    /// Parsing stopped because a parser resource budget was exhausted.
    BudgetExhausted,
}

impl SemanticParseDisposition {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Recovered => "recovered",
            Self::Catastrophic => "catastrophic",
            Self::Cancelled => "cancelled",
            Self::BudgetExhausted => "budget_exhausted",
        }
    }
}

/// Closed wire mirror of `ParseSnapshotStrategy`, value-aligned with
/// `perl_parser::incremental`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticParseStrategy {
    /// Initial or explicitly requested full fresh parse.
    Fresh,
    /// Lexer restart followed by the authoritative full parser.
    IncrementalTokenRestartThenFullParse,
    /// Incremental path failed closed to a complete full-parser fallback.
    IncrementalFullFallback,
}

impl SemanticParseStrategy {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::IncrementalTokenRestartThenFullParse => {
                "incremental_token_restart_then_full_parse"
            }
            Self::IncrementalFullFallback => "incremental_full_fallback",
        }
    }
}

/// The serializable identity projection of one `ParseSnapshot`: accepted
/// generation, exact source digest, terminal disposition, and production
/// strategy.
///
/// The snapshot *object* (with its native parser output) stays with the
/// parser crate; only its identity crosses into this envelope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseSnapshotIdentity {
    /// Accepted parse generation value (`ParseGeneration`).
    pub accepted_generation: u64,
    /// Digest of the exact source the parser consumed; must equal the
    /// subject's parser-input revision digest.
    pub source_digest: ContentDigest,
    /// Byte length of the exact parser input.
    pub source_len: u64,
    /// Terminal parser disposition.
    pub disposition: SemanticParseDisposition,
    /// Production path that created the parse.
    pub parse_strategy: SemanticParseStrategy,
}

impl ParseSnapshotIdentity {
    /// Construct the identity projection.
    #[must_use]
    pub fn new(
        accepted_generation: u64,
        source_digest: ContentDigest,
        source_len: u64,
        disposition: SemanticParseDisposition,
        parse_strategy: SemanticParseStrategy,
    ) -> Self {
        Self { accepted_generation, source_digest, source_len, disposition, parse_strategy }
    }
}

/// The accepted parser ticket reference bound into a snapshot (#11665 spec
/// text; constructor owner #12151).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedParserTicketRef {
    /// Ticket id, derived from document instance + generation + snapshot
    /// source digest and recomputed on validation.
    pub ticket_id: AcceptedParserTicketId,
    /// Document instance the ticket was accepted for.
    pub document_instance: DocumentInstanceId,
    /// Accepted parse generation the ticket was accepted for.
    pub accepted_generation: u64,
}

// ---------------------------------------------------------------------------
// Contribution set and materialized view references
// ---------------------------------------------------------------------------

/// Completeness of a referenced contribution set (#12135 vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticContributionSetCompleteness {
    /// Every required fact family is present for the exact subject.
    Complete,
    /// Some families are present; recovery/dynamic limitations apply.
    Partial,
    /// The set's own evidence is not provable.
    NotProven,
}

/// Reference to one #12135 `FileSemanticContributionSet` by identity.
///
/// The set object is *not* embedded: this envelope joins by identity, and
/// #8669 joins a matching compiler contribution the same way rather than
/// copying facts. The construction authority over set fingerprints belongs
/// to #12135; the validator checks that the set id is the id derived from
/// this exact subject and profile, and that the referenced subject is this
/// subject.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticContributionSetRef {
    /// Identity of the contribution set.
    pub set_id: SemanticContributionSetId,
    /// Subject fingerprint of the set; must equal the snapshot subject's.
    pub subject_fingerprint: ContentDigest,
    /// The set's own completeness classification.
    pub completeness: SemanticContributionSetCompleteness,
    /// Deterministic set fingerprint (#12135 authority).
    pub fingerprint: ContentDigest,
}

impl SemanticContributionSetRef {
    /// Construct a set reference from checked-owner inputs, deriving the
    /// set id from the exact subject fingerprint and profile.
    #[must_use]
    pub fn new(
        subject_fingerprint: ContentDigest,
        profile: &SemanticProfileIdentity,
        completeness: SemanticContributionSetCompleteness,
        fingerprint: ContentDigest,
    ) -> Self {
        let set_id =
            SemanticContributionSetId::from_subject_and_profile(&subject_fingerprint, profile);
        Self { set_id, subject_fingerprint, completeness, fingerprint }
    }
}

/// Kind of one #12138 materialized neutral query view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticQueryViewKind {
    /// `SymbolTable` with deterministic projected ids.
    SymbolTable,
    /// Semantic model/query views.
    SemanticModel,
    /// Visible binding/declaration/reference queries.
    VisibleBindings,
    /// Class/export/package/generated-member accessors.
    PackageExports,
    /// Semantic-token source views.
    SemanticTokens,
    /// Hover source views.
    Hover,
    /// Document-symbol/declaration inventory inputs.
    DocumentSymbols,
    /// Project-fact projection seam for #4772.
    ProjectFacts,
    /// Completeness/limitation views.
    Completeness,
}

impl SemanticQueryViewKind {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SymbolTable => "symbol_table",
            Self::SemanticModel => "semantic_model",
            Self::VisibleBindings => "visible_bindings",
            Self::PackageExports => "package_exports",
            Self::SemanticTokens => "semantic_tokens",
            Self::Hover => "hover",
            Self::DocumentSymbols => "document_symbols",
            Self::ProjectFacts => "project_facts",
            Self::Completeness => "completeness",
        }
    }

    /// View kinds required for any complete terminal state: the minimal
    /// #12138 materializer family a completed semantic result must be able
    /// to serve without re-running analysis.
    pub const REQUIRED_FOR_COMPLETE: &'static [Self] =
        &[Self::SymbolTable, Self::SemanticModel, Self::Completeness];
}

impl std::fmt::Display for SemanticQueryViewKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Reference to one #12138 materialized neutral query view by identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializedQueryViewRef {
    /// Identity of the view, derived from owning set + kind and recomputed
    /// on validation.
    pub view_id: MaterializedQueryViewId,
    /// Kind of the view.
    pub kind: SemanticQueryViewKind,
    /// Owning contribution set; must equal the snapshot's set.
    pub owning_set_id: SemanticContributionSetId,
    /// Deterministic view fingerprint (#12138 authority).
    pub fingerprint: ContentDigest,
}

impl MaterializedQueryViewRef {
    /// Construct a view reference from checked-owner inputs, deriving the
    /// view id from the owning set and kind.
    #[must_use]
    pub fn new(
        owning_set_id: &SemanticContributionSetId,
        kind: SemanticQueryViewKind,
        fingerprint: ContentDigest,
    ) -> Self {
        let view_id = MaterializedQueryViewId::from_set_and_kind(owning_set_id, kind);
        Self { view_id, kind, owning_set_id: owning_set_id.clone(), fingerprint }
    }
}

// ---------------------------------------------------------------------------
// Work receipt and predecessor
// ---------------------------------------------------------------------------

/// Class of the instrument that performed the semantic work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticInstrumentKind {
    /// #12151 shared construction cell.
    ConstructionCell,
    /// Explicit compatibility projection with a named exit.
    CompatibilityProjection,
    /// External canonical producer reference.
    ExternalProducer,
}

impl SemanticInstrumentKind {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConstructionCell => "construction_cell",
            Self::CompatibilityProjection => "compatibility_projection",
            Self::ExternalProducer => "external_producer",
        }
    }
}

/// Identity of the instrument instance that performed the work.
///
/// The instance id binds the instrument class; the validator cannot rederive
/// it from the payload alone (the instance key is authority-held), but the
/// work-receipt id that binds instance + sequence *is* rederived, so an
/// instrument instance cannot be spliced into another receipt.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentIdentity {
    /// Instrument class.
    pub kind: SemanticInstrumentKind,
    /// Concrete instrument instance.
    pub instance: InstrumentInstanceId,
}

impl InstrumentIdentity {
    /// Construct an instrument identity.
    #[must_use]
    pub fn new(kind: SemanticInstrumentKind, instance_key: &str) -> Self {
        Self { kind, instance: InstrumentInstanceId::from_kind_and_key(kind, instance_key) }
    }
}

/// Construction strategy recorded by the work receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticWorkKind {
    /// Honest fresh-full construction. Before #7308 this is ordinary
    /// successful construction and is never reported as incremental or
    /// avoided work.
    FreshFull,
    /// Incremental update over one predecessor.
    Incremental,
    /// No-change reuse of one predecessor.
    NoChangeReuse,
    /// Full fallback after a failed incremental attempt.
    FullFallback,
}

impl SemanticWorkKind {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FreshFull => "fresh_full",
            Self::Incremental => "incremental",
            Self::NoChangeReuse => "no_change_reuse",
            Self::FullFallback => "full_fallback",
        }
    }
}

impl std::fmt::Display for SemanticWorkKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Deterministic work/instrument receipt for one snapshot construction.
///
/// Deliberately carries no wall-clock time: ordering is the authority-owned
/// `work_sequence`, so fingerprints stay deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticWorkReceipt {
    /// Receipt identity, derived from instrument + sequence and recomputed
    /// on validation.
    pub receipt_id: SemanticWorkReceiptId,
    /// Construction strategy used.
    pub work_kind: SemanticWorkKind,
    /// Instrument that performed the work.
    pub instrument: InstrumentIdentity,
    /// Authority-owned deterministic work sequence.
    pub work_sequence: u64,
}

impl SemanticWorkReceipt {
    /// Construct a receipt, deriving its id.
    #[must_use]
    pub fn new(
        work_kind: SemanticWorkKind,
        instrument: InstrumentIdentity,
        work_sequence: u64,
    ) -> Self {
        let receipt_id =
            SemanticWorkReceiptId::from_instrument_and_sequence(&instrument, work_sequence);
        Self { receipt_id, work_kind, instrument, work_sequence }
    }
}

/// How a completed snapshot reused one predecessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticReuseRelation {
    /// Incremental update over the predecessor's facts.
    Incremental,
    /// Exact no-change reuse of the predecessor's facts.
    NoChangeReuse,
}

impl SemanticReuseRelation {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Incremental => "incremental",
            Self::NoChangeReuse => "no_change_reuse",
        }
    }
}

/// Predecessor/reuse identity, applicable only where a reuse-capable
/// producer was selected (#12151).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticPredecessorRef {
    /// Fingerprint of the predecessor snapshot.
    pub predecessor_fingerprint: ContentDigest,
    /// Accepted parse generation of the predecessor.
    pub predecessor_generation: u64,
    /// Reuse relation.
    pub relation: SemanticReuseRelation,
}

// ---------------------------------------------------------------------------
// Completeness, confidence, limitations
// ---------------------------------------------------------------------------

/// Fact-family completeness of this snapshot's evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticCompleteness {
    /// Every required fact family is present for the exact subject.
    Complete,
    /// Some families are present; recovery/dynamic limitations apply.
    Partial,
    /// The evidence is not provable; never treated as empty success.
    NotProven,
}

/// Confidence classification of this snapshot's evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticConfidence {
    /// Exact facts over the exact subject.
    Exact,
    /// Recovered facts over the exact subject.
    Recovered,
    /// Exact-under-stated-dynamic-bounds facts.
    DynamicBounded,
    /// No provable confidence.
    Unprovable,
}

/// Kind of one limitation entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticLimitationKind {
    /// A region was recovered by the parser.
    RecoveredRegion,
    /// A synthetic repair was inserted.
    SyntheticRepair,
    /// Runtime dynamic evaluation bounds the exactness.
    DynamicEval,
    /// Runtime `require`/load bounds the exactness.
    DynamicRequire,
    /// A construct is unsupported by the schema/implementation.
    UnsupportedConstruct,
    /// A parse ambiguity bounded the analysis.
    AmbiguousParse,
    /// A budget truncated the analysis.
    BudgetTruncated,
}

impl SemanticLimitationKind {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecoveredRegion => "recovered_region",
            Self::SyntheticRepair => "synthetic_repair",
            Self::DynamicEval => "dynamic_eval",
            Self::DynamicRequire => "dynamic_require",
            Self::UnsupportedConstruct => "unsupported_construct",
            Self::AmbiguousParse => "ambiguous_parse",
            Self::BudgetTruncated => "budget_truncated",
        }
    }

    /// Whether this kind records parser recovery.
    #[must_use]
    pub const fn is_recovery(self) -> bool {
        matches!(self, Self::RecoveredRegion | Self::SyntheticRepair)
    }

    /// Whether this kind records a dynamic boundary.
    #[must_use]
    pub const fn is_dynamic(self) -> bool {
        matches!(self, Self::DynamicEval | Self::DynamicRequire)
    }
}

/// One limitation entry: a closed kind plus a bounded count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticLimitationEntry {
    /// Limitation kind.
    pub kind: SemanticLimitationKind,
    /// Bounded occurrence count (never used to infer success).
    pub count: u32,
}

/// Recovery/dynamic/unsupported limitation inventory, canonically ordered by
/// kind with duplicate kinds merged (counts summed) at construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticLimitations {
    /// Canonical, kind-sorted, duplicate-free entries.
    pub entries: Vec<SemanticLimitationEntry>,
}

impl SemanticLimitations {
    /// Construct limitations from raw entries, canonicalizing the order and
    /// merging duplicate kinds by summing counts.
    #[must_use]
    pub fn new(entries: Vec<SemanticLimitationEntry>) -> Self {
        let mut merged: Vec<SemanticLimitationEntry> = Vec::with_capacity(entries.len());
        for entry in entries {
            match merged.iter_mut().find(|e| e.kind == entry.kind) {
                Some(existing) => {
                    existing.count = existing.count.saturating_add(entry.count);
                }
                None => merged.push(entry),
            }
        }
        merged.sort_by_key(|e| e.kind);
        Self { entries: merged }
    }

    /// Whether there are no limitation entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total count recorded for one kind.
    #[must_use]
    pub fn count_of(&self, kind: SemanticLimitationKind) -> u32 {
        self.entries.iter().find(|e| e.kind == kind).map_or(0, |e| e.count)
    }

    /// Whether any recovery limitation is recorded.
    #[must_use]
    pub fn has_recovery_limitation(&self) -> bool {
        self.entries.iter().any(|e| e.kind.is_recovery() && e.count > 0)
    }

    /// Whether any dynamic limitation is recorded.
    #[must_use]
    pub fn has_dynamic_limitation(&self) -> bool {
        self.entries.iter().any(|e| e.kind.is_dynamic() && e.count > 0)
    }

    /// Canonical fingerprint input: `kind:count` joined in kind order.
    fn canonical_wire(&self) -> String {
        let mut wire = String::new();
        for entry in &self.entries {
            let _ = write!(wire, "{}:{},", entry.kind.as_str(), entry.count);
        }
        wire
    }
}

// ---------------------------------------------------------------------------
// Project-fact projection reference
// ---------------------------------------------------------------------------

/// Reference to one project-fact projection derived from this snapshot
/// (#4772 seam). The projection fingerprint is #4772 authority; the id is
/// rederived from the logical source and profile triple on validation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFactProjectionRef {
    /// Identity of the projection.
    pub projection_id: ProjectFactProjectionId,
    /// Deterministic projection fingerprint (#4772 authority).
    pub fingerprint: ContentDigest,
}

// ---------------------------------------------------------------------------
// Terminal state
// ---------------------------------------------------------------------------

/// Terminal disposition of one completed semantic operation.
///
/// Fresh, incremental, reuse, fallback, partial, unavailable, stale,
/// product-failure, budget, cancellation, instrument and not-proven states
/// remain distinct; empty or missing facts never determine terminal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticSnapshotTerminalState {
    /// Completed by honest fresh-full construction. Before #7308 this is
    /// ordinary successful construction and is never reported as incremental
    /// or avoided work.
    CompleteFreshFull,
    /// Completed by incremental update over one predecessor.
    CompleteIncremental,
    /// Completed by exact no-change reuse of one predecessor.
    CompleteNoChangeReuse,
    /// Completed by full fallback after a failed incremental attempt.
    CompleteFullFallback,
    /// Partial result recovered from a bounded parse/analysis recovery.
    PartialRecovered,
    /// No result is available for this ticket.
    Unavailable,
    /// Construction stopped through cooperative cancellation.
    Cancelled,
    /// Construction stopped because a semantic resource budget was exhausted.
    BudgetExhausted,
    /// Construction finished but was superseded before completion.
    StaleOrSuperseded,
    /// Construction failed for a product reason.
    ProductFailure,
    /// An instrument/schema needed for the result failed; evidence is
    /// unreliable rather than absent.
    InstrumentOrSchemaFailure,
    /// Completion could not be observed; nothing may be claimed.
    NotProven,
}

impl SemanticSnapshotTerminalState {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompleteFreshFull => "complete_fresh_full",
            Self::CompleteIncremental => "complete_incremental",
            Self::CompleteNoChangeReuse => "complete_no_change_reuse",
            Self::CompleteFullFallback => "complete_full_fallback",
            Self::PartialRecovered => "partial_recovered",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
            Self::BudgetExhausted => "budget_exhausted",
            Self::StaleOrSuperseded => "stale_or_superseded",
            Self::ProductFailure => "product_failure",
            Self::InstrumentOrSchemaFailure => "instrument_or_schema_failure",
            Self::NotProven => "not_proven",
        }
    }

    /// Whether this is one of the complete family.
    #[must_use]
    pub const fn is_complete_family(self) -> bool {
        matches!(
            self,
            Self::CompleteFreshFull
                | Self::CompleteIncremental
                | Self::CompleteNoChangeReuse
                | Self::CompleteFullFallback
        )
    }

    /// Whether this is an absent family state: unavailable, cancelled,
    /// budget-exhausted, superseded, failed, instrument-failed or
    /// not-proven. Nothing was completed, so no completed facts may be
    /// carried.
    #[must_use]
    pub const fn is_absent_family(self) -> bool {
        matches!(
            self,
            Self::Unavailable
                | Self::Cancelled
                | Self::BudgetExhausted
                | Self::StaleOrSuperseded
                | Self::ProductFailure
                | Self::InstrumentOrSchemaFailure
                | Self::NotProven
        )
    }
}

impl std::fmt::Display for SemanticSnapshotTerminalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Fingerprint
// ---------------------------------------------------------------------------

/// Deterministic fingerprint of one complete snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticSnapshotFingerprint(ContentDigest);

impl SemanticSnapshotFingerprint {
    /// Parse from wire form.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        ContentDigest::from_wire(s).map(Self)
    }

    /// Wire form.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        self.0.as_wire()
    }
}

impl std::fmt::Display for SemanticSnapshotFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wire())
    }
}

/// Compute the fingerprint over the canonical identity parts of a validated
/// snapshot shape.
///
/// Inputs are wire/canonical forms only; absent options contribute a fixed
/// marker so `Some`/`None` never collide. The fingerprint covers every
/// identity-bearing field including the full work-receipt record (work kind,
/// instrument class, instrument instance, and sequence — the receipt id alone
/// is not sufficient because the validator cannot rederive an instrument
/// instance from its authority-held key), the full predecessor record
/// (fingerprint, generation, and relation), terminal state, completeness,
/// confidence, limitations, and generation — so source-identical later
/// generations and close/reopen instances stay distinct, and no relabeling of
/// a validated envelope keeps its fingerprint.
#[allow(clippy::too_many_arguments)]
fn snapshot_fingerprint_over(
    schema_version: u32,
    profile: &SemanticProfileIdentity,
    subject_fingerprint: &ContentDigest,
    accepted_ticket: &AcceptedParserTicketRef,
    parse_snapshot: &ParseSnapshotIdentity,
    contribution_set: Option<&SemanticContributionSetRef>,
    materialized_views: &[MaterializedQueryViewRef],
    work_receipt: &SemanticWorkReceipt,
    predecessor: Option<&SemanticPredecessorRef>,
    terminal_state: SemanticSnapshotTerminalState,
    completeness: SemanticCompleteness,
    confidence: SemanticConfidence,
    limitations: &SemanticLimitations,
    project_fact_projection: Option<&ProjectFactProjectionRef>,
) -> SemanticSnapshotFingerprint {
    const ABSENT: &[u8] = b"-";
    let mut parts: Vec<Vec<u8>> = Vec::new();
    let mut push = |part: &[u8]| parts.push(part.to_vec());
    push(&schema_version.to_be_bytes());
    push(profile.fingerprint.as_wire().as_bytes());
    push(subject_fingerprint.as_wire().as_bytes());
    push(accepted_ticket.ticket_id.as_wire().as_bytes());
    push(&parse_snapshot.accepted_generation.to_be_bytes());
    push(parse_snapshot.source_digest.as_wire().as_bytes());
    push(&parse_snapshot.source_len.to_be_bytes());
    push(parse_snapshot.disposition.as_str().as_bytes());
    push(parse_snapshot.parse_strategy.as_str().as_bytes());
    push(contribution_set.map_or(ABSENT, |s| s.fingerprint.as_wire().as_bytes()));
    for view in materialized_views {
        push(view.view_id.as_wire().as_bytes());
        push(view.fingerprint.as_wire().as_bytes());
    }
    push(work_receipt.receipt_id.as_wire().as_bytes());
    push(work_receipt.work_kind.as_str().as_bytes());
    push(work_receipt.instrument.kind.as_str().as_bytes());
    push(work_receipt.instrument.instance.as_wire().as_bytes());
    push(&work_receipt.work_sequence.to_be_bytes());
    match predecessor {
        None => push(ABSENT),
        Some(predecessor) => {
            push(predecessor.predecessor_fingerprint.as_wire().as_bytes());
            push(&predecessor.predecessor_generation.to_be_bytes());
            push(predecessor.relation.as_str().as_bytes());
        }
    }
    push(terminal_state.as_str().as_bytes());
    push(match completeness {
        SemanticCompleteness::Complete => b"complete",
        SemanticCompleteness::Partial => b"partial",
        SemanticCompleteness::NotProven => b"not_proven",
    });
    push(match confidence {
        SemanticConfidence::Exact => b"exact",
        SemanticConfidence::Recovered => b"recovered",
        SemanticConfidence::DynamicBounded => b"dynamic_bounded",
        SemanticConfidence::Unprovable => b"unprovable",
    });
    push(limitations.canonical_wire().as_bytes());
    push(project_fact_projection.map_or(ABSENT, |p| p.fingerprint.as_wire().as_bytes()));
    let borrowed: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    SemanticSnapshotFingerprint(semantic_domain_digest(SNAPSHOT_FINGERPRINT_DOMAIN, &borrowed))
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// Typed validation refusal for a `file_semantic_snapshot.v1` payload.
///
/// Every variant is a distinct falsifier family from the controlling issue;
/// none is silently normalized into success.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FileSemanticSnapshotValidationError {
    /// The schema version is not one this build supports.
    #[error("unsupported file_semantic_snapshot schema version {version}")]
    SchemaVersionUnsupported {
        /// The unsupported version value.
        version: u32,
    },
    /// The full-source revision names a different logical source than the
    /// subject.
    #[error(
        "full-source revision logical source {revision_source} does not match subject {subject_source}"
    )]
    FullSourceSubjectMismatch {
        /// Logical source carried by the full-source revision.
        revision_source: LogicalSourceId,
        /// Logical source carried by the subject.
        subject_source: LogicalSourceId,
    },
    /// The parse snapshot identity names a different parser input than the
    /// subject.
    #[error("parse snapshot digest {snapshot_digest} does not match parser input {input_digest}")]
    ParserInputDigestMismatch {
        /// Digest carried by the parse snapshot identity.
        snapshot_digest: ContentDigest,
        /// Digest carried by the subject's parser-input revision.
        input_digest: ContentDigest,
    },
    /// The parse snapshot length disagrees with the parser-input length.
    #[error("parse snapshot length {snapshot_len} does not match parser input length {input_len}")]
    ParserInputLengthMismatch {
        /// Length carried by the parse snapshot identity.
        snapshot_len: u64,
        /// Length carried by the subject's parser-input revision.
        input_len: u64,
    },
    /// The accepted ticket names a different document instance.
    #[error("accepted ticket document instance does not match the subject")]
    TicketDocumentInstanceMismatch,
    /// The accepted ticket names a different generation than the parse
    /// snapshot identity.
    #[error(
        "accepted ticket generation {ticket_generation} does not match parse snapshot generation {snapshot_generation}"
    )]
    TicketGenerationMismatch {
        /// Generation carried by the ticket.
        ticket_generation: u64,
        /// Generation carried by the parse snapshot identity.
        snapshot_generation: u64,
    },
    /// The accepted ticket id is not the id derived from its bound parts.
    #[error(
        "accepted ticket id does not match its document instance, generation and parse snapshot digest"
    )]
    TicketIdentityMismatch,
    /// The profile triple fingerprint is not the fingerprint of its parts.
    #[error("semantic profile fingerprint does not match its schema/implementation/profile triple")]
    ProfileFingerprintMismatch,
    /// The contribution set belongs to another analysis subject.
    #[error("contribution set belongs to another subject")]
    ContributionSetSubjectMismatch,
    /// The contribution set id is not the id derived from this subject and
    /// profile.
    #[error("contribution set id does not match this subject and profile")]
    ContributionSetIdentityMismatch,
    /// A materialized view is owned by a different contribution set.
    #[error("materialized view of kind {kind} is owned by a different contribution set")]
    MaterializedViewSetMismatch {
        /// Kind of the mis-owned view.
        kind: SemanticQueryViewKind,
    },
    /// A materialized view id is not the id derived from its owning set and
    /// kind.
    #[error("materialized view id does not match its owning set and kind")]
    MaterializedViewIdentityMismatch,
    /// A work receipt id is not the id derived from its instrument and
    /// sequence.
    #[error("work receipt id does not match its instrument and work sequence")]
    WorkReceiptIdentityMismatch,
    /// A project-fact projection id is not the id derived from this subject
    /// and profile.
    #[error("project-fact projection id does not match this subject and profile")]
    ProjectFactProjectionIdentityMismatch,
    /// Materialized views exist without a contribution set.
    #[error("materialized views require a contribution set")]
    ContributionSetRequiredForViews,
    /// A complete terminal state carries no contribution set: empty facts
    /// never determine completeness.
    #[error(
        "complete terminal state {terminal} requires a contribution set; empty or missing facts never complete automatically"
    )]
    CompleteStateWithoutContributionSet {
        /// The complete terminal state.
        terminal: SemanticSnapshotTerminalState,
    },
    /// A complete terminal state references an incomplete set.
    #[error(
        "complete terminal state {terminal} references a {set_completeness:?} contribution set"
    )]
    CompleteStateWithIncompleteSet {
        /// The complete terminal state.
        terminal: SemanticSnapshotTerminalState,
        /// The referenced set's completeness.
        set_completeness: SemanticContributionSetCompleteness,
    },
    /// A complete terminal state is missing a required view family.
    #[error("complete terminal state {terminal} is missing required view kind {kind}")]
    RequiredViewFamilyMissing {
        /// The complete terminal state.
        terminal: SemanticSnapshotTerminalState,
        /// The missing required view kind.
        kind: SemanticQueryViewKind,
    },
    /// A recovered partial state omits its recovery limitations.
    #[error("partial-recovered terminal state must record recovery limitations")]
    MissingRecoveryLimitations,
    /// A dynamic-bounded confidence omits its dynamic limitations.
    #[error("dynamic-bounded confidence must record dynamic limitations")]
    MissingDynamicLimitations,
    /// Terminal state and completeness classification contradict.
    #[error("terminal state {terminal} contradicts completeness {completeness:?}")]
    TerminalCompletenessContradiction {
        /// The terminal state.
        terminal: SemanticSnapshotTerminalState,
        /// The completeness classification.
        completeness: SemanticCompleteness,
    },
    /// Terminal state and confidence classification contradict.
    #[error("terminal state {terminal} contradicts confidence {confidence:?}")]
    TerminalConfidenceContradiction {
        /// The terminal state.
        terminal: SemanticSnapshotTerminalState,
        /// The confidence classification.
        confidence: SemanticConfidence,
    },
    /// Work kind and terminal state contradict (for example a fresh-full
    /// terminal reported as incremental work).
    #[error("work kind {work_kind} contradicts terminal state {terminal}")]
    WorkKindTerminalContradiction {
        /// The recorded work kind.
        work_kind: SemanticWorkKind,
        /// The terminal state.
        terminal: SemanticSnapshotTerminalState,
    },
    /// A predecessor is carried where it is not applicable (fresh-full
    /// construction claims no reuse).
    #[error(
        "terminal state {terminal} must not carry a predecessor; fresh full is not avoided work"
    )]
    PredecessorNotApplicable {
        /// The terminal state.
        terminal: SemanticSnapshotTerminalState,
    },
    /// An incremental/reuse terminal state omits its predecessor.
    #[error("terminal state {terminal} requires a predecessor reference")]
    PredecessorRequired {
        /// The terminal state.
        terminal: SemanticSnapshotTerminalState,
    },
    /// A predecessor reference uses the wrong reuse relation for the
    /// terminal state.
    #[error("terminal state {terminal} requires a {relation:?} predecessor relation")]
    PredecessorRelationMismatch {
        /// The terminal state.
        terminal: SemanticSnapshotTerminalState,
        /// The required reuse relation.
        relation: SemanticReuseRelation,
    },
    /// An absent-family terminal state carries completed facts: old exact
    /// facts may not mask a current failure.
    #[error("terminal state {terminal} is absent-family and must not carry completed facts")]
    AbsentStateCarryingCompletedFacts {
        /// The terminal state.
        terminal: SemanticSnapshotTerminalState,
    },
    /// A complete terminal state carries an unknown (not known-and-nonempty)
    /// source generation.
    #[error("complete terminal state requires a known checked source generation")]
    UnknownSourceGeneration,
    /// Duplicate materialized view kinds.
    #[error("duplicate materialized view kind {kind}")]
    DuplicateMaterializedViewKind {
        /// The duplicated kind.
        kind: SemanticQueryViewKind,
    },
    /// Materialized views are not in canonical kind order.
    #[error("materialized views must be canonically ordered by kind")]
    MaterializedViewOrder,
    /// Limitation entries are not in canonical kind order or contain
    /// duplicates.
    #[error("limitation entries must be canonically ordered and duplicate-free")]
    LimitationOrder,
    /// The parse disposition contradicts the terminal-state family: a
    /// complete family requires a clean parse, and a partial-recovered
    /// terminal requires a recovered parse. Any other pairing presents
    /// partial or failed parser evidence as exact complete facts.
    #[error("parse disposition {disposition:?} contradicts terminal state {terminal}")]
    ParseDispositionContradiction {
        /// The terminal state.
        terminal: SemanticSnapshotTerminalState,
        /// The contradicting parse disposition.
        disposition: SemanticParseDisposition,
    },
    /// The stored fingerprint is not the fingerprint of the payload.
    #[error("snapshot fingerprint does not match its canonical identity parts")]
    FingerprintMismatch,
}

/// Checked assembly parts for one snapshot.
///
/// The accepted-ticket reference and fingerprint are *derived* by the
/// constructor, never supplied: a ticket id cannot be mixed in from another
/// subject and a fingerprint cannot be asserted.
#[derive(Debug, Clone)]
pub struct FileSemanticSnapshotParts {
    /// Schema/implementation/profile triple.
    pub profile: SemanticProfileIdentity,
    /// Exact analysis subject.
    pub subject: SemanticSubjectIdentity,
    /// Parse snapshot identity projection.
    pub parse_snapshot: ParseSnapshotIdentity,
    /// Contribution set reference, when facts were completed.
    pub contribution_set: Option<SemanticContributionSetRef>,
    /// Materialized query view references.
    pub materialized_views: Vec<MaterializedQueryViewRef>,
    /// Work receipt.
    pub work_receipt: SemanticWorkReceipt,
    /// Predecessor/reuse identity, where applicable.
    pub predecessor: Option<SemanticPredecessorRef>,
    /// Terminal disposition.
    pub terminal_state: SemanticSnapshotTerminalState,
    /// Fact-family completeness.
    pub completeness: SemanticCompleteness,
    /// Confidence classification.
    pub confidence: SemanticConfidence,
    /// Recovery/dynamic/unsupported limitations.
    pub limitations: SemanticLimitations,
    /// Optional project-fact projection reference.
    pub project_fact_projection: Option<ProjectFactProjectionRef>,
}

/// One immutable ticket-bound completed semantic operation result
/// (`file_semantic_snapshot.v1`).
///
/// Construct through [`FileSemanticSnapshotV1::from_parts`] (checked) or
/// deserialize (checked through the same validator). All fields are private;
/// there is no mutation surface, no `Default`, and no unchecked constructor.
/// Read access is bounded, performs no semantic work, and cannot flatten
/// partial, stale, unavailable, failed or instrument-incomplete state into
/// empty success — use [`FileSemanticSnapshotV1::as_current_complete`],
/// which returns `None` for every non-complete state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct FileSemanticSnapshotV1 {
    schema_version: FileSemanticSnapshotSchemaVersion,
    profile: SemanticProfileIdentity,
    subject: SemanticSubjectIdentity,
    /// Derived from `subject`; never serialized, always re-derived on
    /// deserialization so a payload cannot assert its own subject identity.
    #[serde(skip_serializing)]
    subject_fingerprint: ContentDigest,
    accepted_ticket: AcceptedParserTicketRef,
    parse_snapshot: ParseSnapshotIdentity,
    contribution_set: Option<SemanticContributionSetRef>,
    materialized_views: Vec<MaterializedQueryViewRef>,
    work_receipt: SemanticWorkReceipt,
    predecessor: Option<SemanticPredecessorRef>,
    terminal_state: SemanticSnapshotTerminalState,
    completeness: SemanticCompleteness,
    confidence: SemanticConfidence,
    limitations: SemanticLimitations,
    project_fact_projection: Option<ProjectFactProjectionRef>,
    fingerprint: SemanticSnapshotFingerprint,
}

impl FileSemanticSnapshotV1 {
    /// Checked constructor: validates every identity binding and derives the
    /// accepted-ticket reference and snapshot fingerprint.
    ///
    /// Refuses (typed) mixed subjects, mixed tickets, profile/set/view
    /// ownership violations, complete-state-without-facts, absent-state
    /// carrying facts, strategy/receipt contradictions, missing recovery or
    /// dynamic limitations, non-canonical orderings, and unknown schema or
    /// instrument states.
    pub fn from_parts(
        parts: FileSemanticSnapshotParts,
    ) -> Result<Self, FileSemanticSnapshotValidationError> {
        let FileSemanticSnapshotParts {
            profile,
            subject,
            parse_snapshot,
            contribution_set,
            materialized_views,
            work_receipt,
            predecessor,
            terminal_state,
            completeness,
            confidence,
            limitations,
            project_fact_projection,
        } = parts;

        // The constructor owns canonical ordering (views by kind);
        // limitation entries were canonicalized by `SemanticLimitations::new`.
        // Duplicate view kinds are still a typed refusal, never a merge.
        let mut canonical_views = materialized_views;
        canonical_views.sort_by_key(|v| v.kind);

        Self::validate_shape(
            &profile,
            &subject,
            &parse_snapshot,
            contribution_set.as_ref(),
            &canonical_views,
            &limitations,
            &work_receipt,
            terminal_state,
            completeness,
            confidence,
            predecessor.as_ref(),
            project_fact_projection.as_ref(),
        )?;

        let subject_fingerprint = subject.fingerprint();
        let accepted_ticket = AcceptedParserTicketRef {
            ticket_id: AcceptedParserTicketId::from_bound_parts(
                &subject.document_instance,
                parse_snapshot.accepted_generation,
                &parse_snapshot.source_digest,
            ),
            document_instance: subject.document_instance.clone(),
            accepted_generation: parse_snapshot.accepted_generation,
        };
        let fingerprint = snapshot_fingerprint_over(
            FileSemanticSnapshotSchemaVersion::V1.as_u32(),
            &profile,
            &subject_fingerprint,
            &accepted_ticket,
            &parse_snapshot,
            contribution_set.as_ref(),
            &canonical_views,
            &work_receipt,
            predecessor.as_ref(),
            terminal_state,
            completeness,
            confidence,
            &limitations,
            project_fact_projection.as_ref(),
        );

        Ok(Self {
            schema_version: FileSemanticSnapshotSchemaVersion::V1,
            profile,
            subject,
            subject_fingerprint,
            accepted_ticket,
            parse_snapshot,
            contribution_set,
            materialized_views: canonical_views,
            work_receipt,
            predecessor,
            terminal_state,
            completeness,
            confidence,
            limitations,
            project_fact_projection,
            fingerprint,
        })
    }

    /// Structural validation shared by the constructor and the wire path —
    /// one authority for both.
    ///
    /// The constructor path satisfies the derivation checks by construction
    /// (ids are derived inside this module's own constructors); the wire
    /// path reaches the same checks with payload-supplied ids, so a
    /// mixed-in ticket, set, view, receipt or projection id refuses.
    #[allow(clippy::too_many_arguments)]
    fn validate_shape(
        profile: &SemanticProfileIdentity,
        subject: &SemanticSubjectIdentity,
        parse_snapshot: &ParseSnapshotIdentity,
        contribution_set: Option<&SemanticContributionSetRef>,
        materialized_views: &[MaterializedQueryViewRef],
        limitations: &SemanticLimitations,
        work_receipt: &SemanticWorkReceipt,
        terminal_state: SemanticSnapshotTerminalState,
        completeness: SemanticCompleteness,
        confidence: SemanticConfidence,
        predecessor: Option<&SemanticPredecessorRef>,
        project_fact_projection: Option<&ProjectFactProjectionRef>,
    ) -> Result<(), FileSemanticSnapshotValidationError> {
        use FileSemanticSnapshotValidationError as E;

        // Subject coherence: full-source revision names this subject.
        if subject.full_source_revision.logical_source_id != subject.logical_source_id {
            return Err(E::FullSourceSubjectMismatch {
                revision_source: subject.full_source_revision.logical_source_id.clone(),
                subject_source: subject.logical_source_id.clone(),
            });
        }

        // Parse snapshot names this subject's exact parser input.
        if parse_snapshot.source_digest != subject.parser_input_revision.digest {
            return Err(E::ParserInputDigestMismatch {
                snapshot_digest: parse_snapshot.source_digest.clone(),
                input_digest: subject.parser_input_revision.digest.clone(),
            });
        }
        if parse_snapshot.source_len != subject.parser_input_revision.byte_len {
            return Err(E::ParserInputLengthMismatch {
                snapshot_len: parse_snapshot.source_len,
                input_len: subject.parser_input_revision.byte_len,
            });
        }

        // Profile triple is internally consistent.
        if profile.fingerprint
            != SemanticProfileIdentity::fingerprint_over(
                &profile.schema,
                &profile.implementation,
                &profile.profile,
            )
        {
            return Err(E::ProfileFingerprintMismatch);
        }

        // Contribution set belongs to this exact subject and profile.
        let subject_fingerprint = subject.fingerprint();
        if let Some(set) = contribution_set {
            if set.subject_fingerprint != subject_fingerprint {
                return Err(E::ContributionSetSubjectMismatch);
            }
            if set.set_id
                != SemanticContributionSetId::from_subject_and_profile(
                    &subject_fingerprint,
                    profile,
                )
            {
                return Err(E::ContributionSetIdentityMismatch);
            }
        }

        // Views are owned by this set, canonically ordered, duplicate-free,
        // and carry the id derived from their owning set and kind.
        if !materialized_views.is_empty() && contribution_set.is_none() {
            return Err(E::ContributionSetRequiredForViews);
        }
        let mut previous_kind: Option<SemanticQueryViewKind> = None;
        for view in materialized_views {
            if let Some(set) = contribution_set
                && view.owning_set_id != set.set_id
            {
                return Err(E::MaterializedViewSetMismatch { kind: view.kind });
            }
            if view.view_id
                != MaterializedQueryViewId::from_set_and_kind(&view.owning_set_id, view.kind)
            {
                return Err(E::MaterializedViewIdentityMismatch);
            }
            if previous_kind.is_some_and(|k| k >= view.kind) {
                if previous_kind == Some(view.kind) {
                    return Err(E::DuplicateMaterializedViewKind { kind: view.kind });
                }
                return Err(E::MaterializedViewOrder);
            }
            previous_kind = Some(view.kind);
        }

        // Work receipt id is the id derived from its instrument + sequence.
        if work_receipt.receipt_id
            != SemanticWorkReceiptId::from_instrument_and_sequence(
                &work_receipt.instrument,
                work_receipt.work_sequence,
            )
        {
            return Err(E::WorkReceiptIdentityMismatch);
        }

        // Project-fact projection id is the id derived from this subject and
        // profile.
        if let Some(projection) = project_fact_projection
            && projection.projection_id
                != ProjectFactProjectionId::from_source_and_profile(
                    &subject.logical_source_id,
                    profile,
                )
        {
            return Err(E::ProjectFactProjectionIdentityMismatch);
        }

        // Limitations are canonical (sorted, duplicate-free).
        for pair in limitations.entries.windows(2) {
            if pair[0].kind >= pair[1].kind {
                return Err(E::LimitationOrder);
            }
        }

        // Parse disposition agrees with the terminal-state family: only a
        // clean parse can back exact complete facts, and a partial-recovered
        // terminal is parser recovery. Absent-family terminals are
        // deliberately unconstrained: a clean parse can legitimately precede
        // a later cancellation, budget stop, supersession, or product
        // failure, and a failed parse can precede an absent-family failure.
        if terminal_state.is_complete_family()
            && parse_snapshot.disposition != SemanticParseDisposition::Clean
        {
            return Err(E::ParseDispositionContradiction {
                terminal: terminal_state,
                disposition: parse_snapshot.disposition,
            });
        }
        if terminal_state == SemanticSnapshotTerminalState::PartialRecovered
            && parse_snapshot.disposition != SemanticParseDisposition::Recovered
        {
            return Err(E::ParseDispositionContradiction {
                terminal: terminal_state,
                disposition: parse_snapshot.disposition,
            });
        }

        // Terminal-state families govern facts, completeness, confidence.
        if terminal_state.is_complete_family() {
            if !subject.source_generation.is_known() {
                return Err(E::UnknownSourceGeneration);
            }
            let set = contribution_set
                .ok_or(E::CompleteStateWithoutContributionSet { terminal: terminal_state })?;
            if set.completeness != SemanticContributionSetCompleteness::Complete {
                return Err(E::CompleteStateWithIncompleteSet {
                    terminal: terminal_state,
                    set_completeness: set.completeness,
                });
            }
            for kind in SemanticQueryViewKind::REQUIRED_FOR_COMPLETE {
                if !materialized_views.iter().any(|v| v.kind == *kind) {
                    return Err(E::RequiredViewFamilyMissing {
                        terminal: terminal_state,
                        kind: *kind,
                    });
                }
            }
            if completeness != SemanticCompleteness::Complete {
                return Err(E::TerminalCompletenessContradiction {
                    terminal: terminal_state,
                    completeness,
                });
            }
            if confidence != SemanticConfidence::Exact {
                return Err(E::TerminalConfidenceContradiction {
                    terminal: terminal_state,
                    confidence,
                });
            }
        } else if terminal_state.is_absent_family() {
            if contribution_set.is_some()
                || !materialized_views.is_empty()
                || project_fact_projection.is_some()
            {
                return Err(E::AbsentStateCarryingCompletedFacts { terminal: terminal_state });
            }
            if completeness != SemanticCompleteness::NotProven {
                return Err(E::TerminalCompletenessContradiction {
                    terminal: terminal_state,
                    completeness,
                });
            }
            if confidence != SemanticConfidence::Unprovable {
                return Err(E::TerminalConfidenceContradiction {
                    terminal: terminal_state,
                    confidence,
                });
            }
        } else {
            // PartialRecovered.
            if completeness != SemanticCompleteness::Partial {
                return Err(E::TerminalCompletenessContradiction {
                    terminal: terminal_state,
                    completeness,
                });
            }
            if !matches!(
                confidence,
                SemanticConfidence::Recovered | SemanticConfidence::DynamicBounded
            ) {
                return Err(E::TerminalConfidenceContradiction {
                    terminal: terminal_state,
                    confidence,
                });
            }
            if !limitations.has_recovery_limitation() {
                return Err(E::MissingRecoveryLimitations);
            }
        }

        // Work kind agrees with the terminal state's construction strategy:
        // a fresh-full terminal is never reported as incremental or avoided
        // work, and incremental/reuse/fallback terminals never claim fresh
        // work.
        let required_work_kind = match terminal_state {
            SemanticSnapshotTerminalState::CompleteFreshFull => Some(SemanticWorkKind::FreshFull),
            SemanticSnapshotTerminalState::CompleteIncremental => {
                Some(SemanticWorkKind::Incremental)
            }
            SemanticSnapshotTerminalState::CompleteNoChangeReuse => {
                Some(SemanticWorkKind::NoChangeReuse)
            }
            SemanticSnapshotTerminalState::CompleteFullFallback => {
                Some(SemanticWorkKind::FullFallback)
            }
            SemanticSnapshotTerminalState::PartialRecovered
            | SemanticSnapshotTerminalState::Unavailable
            | SemanticSnapshotTerminalState::Cancelled
            | SemanticSnapshotTerminalState::BudgetExhausted
            | SemanticSnapshotTerminalState::StaleOrSuperseded
            | SemanticSnapshotTerminalState::ProductFailure
            | SemanticSnapshotTerminalState::InstrumentOrSchemaFailure
            | SemanticSnapshotTerminalState::NotProven => None,
        };
        if let Some(required) = required_work_kind
            && work_receipt.work_kind != required
        {
            return Err(E::WorkKindTerminalContradiction {
                work_kind: work_receipt.work_kind,
                terminal: terminal_state,
            });
        }

        // Predecessor applicability and relation.
        match terminal_state {
            SemanticSnapshotTerminalState::CompleteFreshFull if predecessor.is_some() => {
                return Err(E::PredecessorNotApplicable { terminal: terminal_state });
            }
            SemanticSnapshotTerminalState::CompleteIncremental => {
                let pred =
                    predecessor.ok_or(E::PredecessorRequired { terminal: terminal_state })?;
                if pred.relation != SemanticReuseRelation::Incremental {
                    return Err(E::PredecessorRelationMismatch {
                        terminal: terminal_state,
                        relation: SemanticReuseRelation::Incremental,
                    });
                }
            }
            SemanticSnapshotTerminalState::CompleteNoChangeReuse => {
                let pred =
                    predecessor.ok_or(E::PredecessorRequired { terminal: terminal_state })?;
                if pred.relation != SemanticReuseRelation::NoChangeReuse {
                    return Err(E::PredecessorRelationMismatch {
                        terminal: terminal_state,
                        relation: SemanticReuseRelation::NoChangeReuse,
                    });
                }
            }
            _ => {}
        }

        // Dynamic-bounded confidence must state its dynamic bounds.
        if confidence == SemanticConfidence::DynamicBounded && !limitations.has_dynamic_limitation()
        {
            return Err(E::MissingDynamicLimitations);
        }

        Ok(())
    }

    /// Schema version of this snapshot.
    #[must_use]
    pub const fn schema_version(&self) -> FileSemanticSnapshotSchemaVersion {
        self.schema_version
    }

    /// Semantic profile triple.
    #[must_use]
    pub const fn profile(&self) -> &SemanticProfileIdentity {
        &self.profile
    }

    /// Exact analysis subject.
    #[must_use]
    pub const fn subject(&self) -> &SemanticSubjectIdentity {
        &self.subject
    }

    /// Deterministic subject fingerprint (identity of the exact subject).
    #[must_use]
    pub const fn subject_fingerprint(&self) -> &ContentDigest {
        &self.subject_fingerprint
    }

    /// Accepted parser ticket reference.
    #[must_use]
    pub const fn accepted_ticket(&self) -> &AcceptedParserTicketRef {
        &self.accepted_ticket
    }

    /// Parse snapshot identity projection.
    #[must_use]
    pub const fn parse_snapshot_identity(&self) -> &ParseSnapshotIdentity {
        &self.parse_snapshot
    }

    /// Contribution set reference, when facts were completed.
    #[must_use]
    pub fn contribution_set(&self) -> Option<&SemanticContributionSetRef> {
        self.contribution_set.as_ref()
    }

    /// Contribution set id for identity joins: #8669 joins a matching
    /// compiler contribution by this identity rather than copying facts.
    #[must_use]
    pub fn contribution_set_id(&self) -> Option<&SemanticContributionSetId> {
        self.contribution_set.as_ref().map(|set| &set.set_id)
    }

    /// All materialized view references in canonical kind order.
    #[must_use]
    pub fn materialized_views(&self) -> &[MaterializedQueryViewRef] {
        &self.materialized_views
    }

    /// One materialized view reference by kind, if materialized.
    #[must_use]
    pub fn materialized_view(
        &self,
        kind: SemanticQueryViewKind,
    ) -> Option<&MaterializedQueryViewRef> {
        self.materialized_views.iter().find(|v| v.kind == kind)
    }

    /// Whether one requested view kind is available for this snapshot.
    #[must_use]
    pub fn is_view_available(&self, kind: SemanticQueryViewKind) -> bool {
        self.materialized_view(kind).is_some()
    }

    /// Work receipt.
    #[must_use]
    pub const fn work_receipt(&self) -> &SemanticWorkReceipt {
        &self.work_receipt
    }

    /// Predecessor/reuse identity, where applicable.
    #[must_use]
    pub fn predecessor(&self) -> Option<&SemanticPredecessorRef> {
        self.predecessor.as_ref()
    }

    /// Terminal disposition.
    #[must_use]
    pub const fn terminal_state(&self) -> SemanticSnapshotTerminalState {
        self.terminal_state
    }

    /// Whether the terminal state is one of the complete family.
    #[must_use]
    pub const fn is_complete_family(&self) -> bool {
        self.terminal_state.is_complete_family()
    }

    /// Fact-family completeness.
    #[must_use]
    pub const fn completeness(&self) -> SemanticCompleteness {
        self.completeness
    }

    /// Confidence classification.
    #[must_use]
    pub const fn confidence(&self) -> SemanticConfidence {
        self.confidence
    }

    /// Recovery/dynamic/unsupported limitations.
    #[must_use]
    pub const fn limitations(&self) -> &SemanticLimitations {
        &self.limitations
    }

    /// Optional project-fact projection reference (#4772 seam).
    #[must_use]
    pub fn project_fact_projection(&self) -> Option<&ProjectFactProjectionRef> {
        self.project_fact_projection.as_ref()
    }

    /// Deterministic snapshot fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> &SemanticSnapshotFingerprint {
        &self.fingerprint
    }

    /// Bounded current-facts view, `Some` only when the terminal state is
    /// complete-family, completeness is complete, and the contribution set
    /// is bound.
    ///
    /// This is the seam #8575 acceptance and #8669 joins consume. It never
    /// flattens partial, stale, unavailable, failed or instrument-incomplete
    /// state into empty success: those states return `None`.
    #[must_use]
    pub fn as_current_complete(&self) -> Option<CurrentSemanticFacts<'_>> {
        if self.is_complete_family()
            && self.completeness == SemanticCompleteness::Complete
            && let Some(set) = self.contribution_set.as_ref()
        {
            return Some(CurrentSemanticFacts {
                contribution_set: set,
                materialized_views: &self.materialized_views,
                profile: &self.profile,
                subject_fingerprint: &self.subject_fingerprint,
            });
        }
        None
    }
}

/// Read-only view over the current completed facts of one snapshot.
///
/// Borrows only; performs no semantic work and cannot outlive the snapshot.
#[derive(Debug, Clone, Copy)]
pub struct CurrentSemanticFacts<'a> {
    contribution_set: &'a SemanticContributionSetRef,
    materialized_views: &'a [MaterializedQueryViewRef],
    profile: &'a SemanticProfileIdentity,
    subject_fingerprint: &'a ContentDigest,
}

impl CurrentSemanticFacts<'_> {
    /// The bound contribution set reference.
    #[must_use]
    pub const fn contribution_set(&self) -> &SemanticContributionSetRef {
        self.contribution_set
    }

    /// Materialized view references in canonical kind order.
    #[must_use]
    pub const fn materialized_views(&self) -> &[MaterializedQueryViewRef] {
        self.materialized_views
    }

    /// The semantic profile triple the facts hold under.
    #[must_use]
    pub const fn profile(&self) -> &SemanticProfileIdentity {
        self.profile
    }

    /// The exact subject fingerprint the facts belong to.
    #[must_use]
    pub const fn subject_fingerprint(&self) -> &ContentDigest {
        self.subject_fingerprint
    }
}

// ---------------------------------------------------------------------------
// Checked deserialization
// ---------------------------------------------------------------------------

/// Raw wire mirror used only as an intermediate for checked
/// deserialization. Never escapes this module.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSemanticSnapshotWire {
    schema_version: FileSemanticSnapshotSchemaVersion,
    profile: SemanticProfileIdentity,
    subject: SemanticSubjectIdentity,
    accepted_ticket: AcceptedParserTicketRef,
    parse_snapshot: ParseSnapshotIdentity,
    contribution_set: Option<SemanticContributionSetRef>,
    materialized_views: Vec<MaterializedQueryViewRef>,
    work_receipt: SemanticWorkReceipt,
    predecessor: Option<SemanticPredecessorRef>,
    terminal_state: SemanticSnapshotTerminalState,
    completeness: SemanticCompleteness,
    confidence: SemanticConfidence,
    limitations: SemanticLimitations,
    project_fact_projection: Option<ProjectFactProjectionRef>,
    fingerprint: SemanticSnapshotFingerprint,
}

impl<'de> Deserialize<'de> for FileSemanticSnapshotV1 {
    /// Checked deserialization: the wire payload is validated through the
    /// same authority as [`FileSemanticSnapshotV1::from_parts`], and the
    /// stored ticket id and fingerprint must match the values derived from
    /// the payload itself. Malformed, mixed or contradictory payloads are
    /// refused at the serde boundary rather than silently defaulted.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = FileSemanticSnapshotWire::deserialize(deserializer)?;

        if !wire.schema_version.is_supported() {
            // Unreachable through serde (the version type is fail-closed)
            // but kept as the authority for any future in-code path.
            return Err(serde::de::Error::custom(
                FileSemanticSnapshotValidationError::SchemaVersionUnsupported {
                    version: wire.schema_version.as_u32(),
                },
            ));
        }

        // The ticket must be the ticket derived from its own bound parts.
        let derived_ticket = AcceptedParserTicketId::from_bound_parts(
            &wire.accepted_ticket.document_instance,
            wire.accepted_ticket.accepted_generation,
            &wire.parse_snapshot.source_digest,
        );
        if wire.accepted_ticket.ticket_id != derived_ticket {
            return Err(serde::de::Error::custom(
                FileSemanticSnapshotValidationError::TicketIdentityMismatch,
            ));
        }

        // The ticket must bind this subject and this parse snapshot.
        if wire.accepted_ticket.document_instance != wire.subject.document_instance {
            return Err(serde::de::Error::custom(
                FileSemanticSnapshotValidationError::TicketDocumentInstanceMismatch,
            ));
        }
        if wire.accepted_ticket.accepted_generation != wire.parse_snapshot.accepted_generation {
            return Err(serde::de::Error::custom(
                FileSemanticSnapshotValidationError::TicketGenerationMismatch {
                    ticket_generation: wire.accepted_ticket.accepted_generation,
                    snapshot_generation: wire.parse_snapshot.accepted_generation,
                },
            ));
        }

        // One structural authority for subject/set/view/receipt/terminal
        // shape, shared with the checked constructor.
        FileSemanticSnapshotV1::validate_shape(
            &wire.profile,
            &wire.subject,
            &wire.parse_snapshot,
            wire.contribution_set.as_ref(),
            &wire.materialized_views,
            &wire.limitations,
            &wire.work_receipt,
            wire.terminal_state,
            wire.completeness,
            wire.confidence,
            wire.predecessor.as_ref(),
            wire.project_fact_projection.as_ref(),
        )
        .map_err(serde::de::Error::custom)?;

        // The fingerprint must be the fingerprint of this payload.
        let subject_fingerprint = wire.subject.fingerprint();
        let derived_fingerprint = snapshot_fingerprint_over(
            wire.schema_version.as_u32(),
            &wire.profile,
            &subject_fingerprint,
            &wire.accepted_ticket,
            &wire.parse_snapshot,
            wire.contribution_set.as_ref(),
            &wire.materialized_views,
            &wire.work_receipt,
            wire.predecessor.as_ref(),
            wire.terminal_state,
            wire.completeness,
            wire.confidence,
            &wire.limitations,
            wire.project_fact_projection.as_ref(),
        );
        if wire.fingerprint != derived_fingerprint {
            return Err(serde::de::Error::custom(
                FileSemanticSnapshotValidationError::FingerprintMismatch,
            ));
        }

        Ok(Self {
            schema_version: wire.schema_version,
            profile: wire.profile,
            subject: wire.subject,
            subject_fingerprint,
            accepted_ticket: wire.accepted_ticket,
            parse_snapshot: wire.parse_snapshot,
            contribution_set: wire.contribution_set,
            materialized_views: wire.materialized_views,
            work_receipt: wire.work_receipt,
            predecessor: wire.predecessor,
            terminal_state: wire.terminal_state,
            completeness: wire.completeness,
            confidence: wire.confidence,
            limitations: wire.limitations,
            project_fact_projection: wire.project_fact_projection,
            fingerprint: wire.fingerprint,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use perl_source_identity::{
        ContentDigest, LogicalSourceId, ProjectId, SourceGeneration, WorkspaceRootId,
    };
    use serde_json::json;

    const SOURCE: &[u8] = b"package Widget;\n1;\n";

    fn logical_source() -> LogicalSourceId {
        let project = ProjectId::from_canonical_name("acme/widget");
        let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
        LogicalSourceId::from_root_and_path(&root, "lib/Widget.pm")
    }

    fn profile() -> SemanticProfileIdentity {
        SemanticProfileIdentity::new("file-semantic", 1, "perl-semantic-analyzer/0.19", "default")
    }

    fn subject(instance_key: &str, generation_label: &str) -> SemanticSubjectIdentity {
        let logical_source = logical_source();
        let document_instance =
            DocumentInstanceId::from_logical_source_and_instance_key(&logical_source, instance_key);
        let digest = ContentDigest::of_bytes(SOURCE);
        SemanticSubjectIdentity::new(
            logical_source,
            document_instance,
            SourceGeneration::known(generation_label),
            digest.clone(),
            ParserInputRevision::new(digest, SOURCE.len() as u64),
        )
    }

    fn parse_snapshot(generation: u64) -> ParseSnapshotIdentity {
        ParseSnapshotIdentity::new(
            generation,
            ContentDigest::of_bytes(SOURCE),
            SOURCE.len() as u64,
            SemanticParseDisposition::Clean,
            SemanticParseStrategy::Fresh,
        )
    }

    fn required_views(set_id: &SemanticContributionSetId) -> Vec<MaterializedQueryViewRef> {
        SemanticQueryViewKind::REQUIRED_FOR_COMPLETE
            .iter()
            .map(|kind| {
                MaterializedQueryViewRef::new(
                    set_id,
                    *kind,
                    ContentDigest::of_bytes(kind.as_str().as_bytes()),
                )
            })
            .collect()
    }

    fn complete_set(
        subject: &SemanticSubjectIdentity,
        profile: &SemanticProfileIdentity,
    ) -> SemanticContributionSetRef {
        SemanticContributionSetRef::new(
            subject.fingerprint(),
            profile,
            SemanticContributionSetCompleteness::Complete,
            ContentDigest::of_bytes(b"contribution-set"),
        )
    }

    fn complete_fresh_full_parts() -> FileSemanticSnapshotParts {
        complete_fresh_full_parts_for(subject("open-1", "7"), profile())
    }

    fn complete_fresh_full_parts_for(
        subject: SemanticSubjectIdentity,
        profile: SemanticProfileIdentity,
    ) -> FileSemanticSnapshotParts {
        let set = complete_set(&subject, &profile);
        FileSemanticSnapshotParts {
            profile,
            subject,
            parse_snapshot: parse_snapshot(7),
            contribution_set: Some(set.clone()),
            materialized_views: required_views(&set.set_id),
            work_receipt: SemanticWorkReceipt::new(
                SemanticWorkKind::FreshFull,
                InstrumentIdentity::new(SemanticInstrumentKind::ConstructionCell, "cell-1"),
                42,
            ),
            predecessor: None,
            terminal_state: SemanticSnapshotTerminalState::CompleteFreshFull,
            completeness: SemanticCompleteness::Complete,
            confidence: SemanticConfidence::Exact,
            limitations: SemanticLimitations::new(vec![]),
            project_fact_projection: None,
        }
    }

    fn unavailable_parts() -> FileSemanticSnapshotParts {
        FileSemanticSnapshotParts {
            profile: profile(),
            subject: subject("open-1", "7"),
            parse_snapshot: parse_snapshot(7),
            contribution_set: None,
            materialized_views: vec![],
            work_receipt: SemanticWorkReceipt::new(
                SemanticWorkKind::FreshFull,
                InstrumentIdentity::new(SemanticInstrumentKind::ConstructionCell, "cell-1"),
                43,
            ),
            predecessor: None,
            terminal_state: SemanticSnapshotTerminalState::Unavailable,
            completeness: SemanticCompleteness::NotProven,
            confidence: SemanticConfidence::Unprovable,
            limitations: SemanticLimitations::new(vec![]),
            project_fact_projection: None,
        }
    }

    /// Build valid parts for each of the twelve terminal states.
    fn parts_for_terminal(terminal: SemanticSnapshotTerminalState) -> FileSemanticSnapshotParts {
        let parts = match terminal {
            SemanticSnapshotTerminalState::Unavailable
            | SemanticSnapshotTerminalState::Cancelled
            | SemanticSnapshotTerminalState::BudgetExhausted
            | SemanticSnapshotTerminalState::StaleOrSuperseded
            | SemanticSnapshotTerminalState::ProductFailure
            | SemanticSnapshotTerminalState::InstrumentOrSchemaFailure
            | SemanticSnapshotTerminalState::NotProven => {
                let mut parts = unavailable_parts();
                parts.terminal_state = terminal;
                parts
            }
            SemanticSnapshotTerminalState::PartialRecovered => {
                let profile = profile();
                let subject = subject("open-1", "7");
                let set = SemanticContributionSetRef::new(
                    subject.fingerprint(),
                    &profile,
                    SemanticContributionSetCompleteness::Partial,
                    ContentDigest::of_bytes(b"partial-set"),
                );
                FileSemanticSnapshotParts {
                    profile,
                    subject,
                    parse_snapshot: ParseSnapshotIdentity::new(
                        7,
                        ContentDigest::of_bytes(SOURCE),
                        SOURCE.len() as u64,
                        SemanticParseDisposition::Recovered,
                        SemanticParseStrategy::Fresh,
                    ),
                    contribution_set: Some(set.clone()),
                    materialized_views: required_views(&set.set_id),
                    work_receipt: SemanticWorkReceipt::new(
                        SemanticWorkKind::FreshFull,
                        InstrumentIdentity::new(SemanticInstrumentKind::ConstructionCell, "cell-1"),
                        42,
                    ),
                    predecessor: None,
                    terminal_state: terminal,
                    completeness: SemanticCompleteness::Partial,
                    confidence: SemanticConfidence::Recovered,
                    limitations: SemanticLimitations::new(vec![SemanticLimitationEntry {
                        kind: SemanticLimitationKind::RecoveredRegion,
                        count: 2,
                    }]),
                    project_fact_projection: None,
                }
            }
            SemanticSnapshotTerminalState::CompleteFreshFull => complete_fresh_full_parts(),
            SemanticSnapshotTerminalState::CompleteIncremental
            | SemanticSnapshotTerminalState::CompleteNoChangeReuse => {
                let mut parts = complete_fresh_full_parts();
                let work_kind = match terminal {
                    SemanticSnapshotTerminalState::CompleteIncremental => {
                        SemanticWorkKind::Incremental
                    }
                    _ => SemanticWorkKind::NoChangeReuse,
                };
                let relation = match work_kind {
                    SemanticWorkKind::Incremental => SemanticReuseRelation::Incremental,
                    _ => SemanticReuseRelation::NoChangeReuse,
                };
                parts.work_receipt =
                    SemanticWorkReceipt::new(work_kind, parts.work_receipt.instrument.clone(), 44);
                parts.terminal_state = terminal;
                parts.predecessor = Some(SemanticPredecessorRef {
                    predecessor_fingerprint: ContentDigest::of_bytes(b"predecessor"),
                    predecessor_generation: 6,
                    relation,
                });
                parts
            }
            SemanticSnapshotTerminalState::CompleteFullFallback => {
                let mut parts = complete_fresh_full_parts();
                parts.work_receipt = SemanticWorkReceipt::new(
                    SemanticWorkKind::FullFallback,
                    parts.work_receipt.instrument.clone(),
                    45,
                );
                parts.terminal_state = terminal;
                parts
            }
        };
        match terminal {
            SemanticSnapshotTerminalState::Unavailable
            | SemanticSnapshotTerminalState::Cancelled
            | SemanticSnapshotTerminalState::BudgetExhausted
            | SemanticSnapshotTerminalState::StaleOrSuperseded
            | SemanticSnapshotTerminalState::ProductFailure
            | SemanticSnapshotTerminalState::InstrumentOrSchemaFailure
            | SemanticSnapshotTerminalState::NotProven => {
                let mut parts = unavailable_parts();
                parts.terminal_state = terminal;
                parts
            }
            SemanticSnapshotTerminalState::PartialRecovered => {
                let profile = profile();
                let subject = subject("open-1", "7");
                let set = SemanticContributionSetRef::new(
                    subject.fingerprint(),
                    &profile,
                    SemanticContributionSetCompleteness::Partial,
                    ContentDigest::of_bytes(b"partial-set"),
                );
                FileSemanticSnapshotParts {
                    profile,
                    subject,
                    parse_snapshot: ParseSnapshotIdentity::new(
                        7,
                        ContentDigest::of_bytes(SOURCE),
                        SOURCE.len() as u64,
                        SemanticParseDisposition::Recovered,
                        SemanticParseStrategy::Fresh,
                    ),
                    contribution_set: Some(set.clone()),
                    materialized_views: required_views(&set.set_id),
                    work_receipt: SemanticWorkReceipt::new(
                        SemanticWorkKind::FreshFull,
                        InstrumentIdentity::new(SemanticInstrumentKind::ConstructionCell, "cell-1"),
                        42,
                    ),
                    predecessor: None,
                    terminal_state: terminal,
                    completeness: SemanticCompleteness::Partial,
                    confidence: SemanticConfidence::Recovered,
                    limitations: SemanticLimitations::new(vec![SemanticLimitationEntry {
                        kind: SemanticLimitationKind::RecoveredRegion,
                        count: 2,
                    }]),
                    project_fact_projection: None,
                }
            }
            SemanticSnapshotTerminalState::CompleteFreshFull => complete_fresh_full_parts(),
            SemanticSnapshotTerminalState::CompleteIncremental
            | SemanticSnapshotTerminalState::CompleteNoChangeReuse => {
                let mut parts = complete_fresh_full_parts();
                let work_kind = match terminal {
                    SemanticSnapshotTerminalState::CompleteIncremental => {
                        SemanticWorkKind::Incremental
                    }
                    _ => SemanticWorkKind::NoChangeReuse,
                };
                let relation = match work_kind {
                    SemanticWorkKind::Incremental => SemanticReuseRelation::Incremental,
                    _ => SemanticReuseRelation::NoChangeReuse,
                };
                parts.work_receipt =
                    SemanticWorkReceipt::new(work_kind, parts.work_receipt.instrument.clone(), 44);
                parts.terminal_state = terminal;
                parts.predecessor = Some(SemanticPredecessorRef {
                    predecessor_fingerprint: ContentDigest::of_bytes(b"predecessor"),
                    predecessor_generation: 6,
                    relation,
                });
                parts
            }
            SemanticSnapshotTerminalState::CompleteFullFallback => {
                let mut parts = complete_fresh_full_parts();
                parts.work_receipt = SemanticWorkReceipt::new(
                    SemanticWorkKind::FullFallback,
                    parts.work_receipt.instrument.clone(),
                    45,
                );
                parts.terminal_state = terminal;
                parts
            }
        }
    }

    // ── Schema version ────────────────────────────────────────────────────

    #[test]
    fn schema_version_v1_is_supported_and_displayed() {
        assert!(FileSemanticSnapshotSchemaVersion::V1.is_supported());
        assert!(!FileSemanticSnapshotSchemaVersion(0).is_supported());
        assert!(!FileSemanticSnapshotSchemaVersion(2).is_supported());
        assert_eq!(FileSemanticSnapshotSchemaVersion::V1.to_string(), "file_semantic_snapshot.v1");
    }

    #[test]
    fn wire_rejects_unsupported_schema_version() {
        let snapshot = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        let value = serde_json::to_value(&snapshot).unwrap();
        for bad in [0u32, 2, 99, u32::MAX] {
            let mut mutated = value.clone();
            mutated["schema_version"] = json!(bad);
            assert!(
                serde_json::from_value::<FileSemanticSnapshotV1>(mutated).is_err(),
                "schema version {bad} must be refused"
            );
        }
    }

    // ── Round-trips ───────────────────────────────────────────────────────

    #[test]
    fn round_trip_complete_fresh_full() {
        let snapshot = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        let json = serde_json::to_string(&snapshot).unwrap();
        let back: FileSemanticSnapshotV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot, back, "serde round-trip must be lossless");
        assert_eq!(back.fingerprint(), snapshot.fingerprint());
    }

    #[test]
    fn round_trip_every_terminal_state() {
        let states = [
            SemanticSnapshotTerminalState::CompleteFreshFull,
            SemanticSnapshotTerminalState::CompleteIncremental,
            SemanticSnapshotTerminalState::CompleteNoChangeReuse,
            SemanticSnapshotTerminalState::CompleteFullFallback,
            SemanticSnapshotTerminalState::PartialRecovered,
            SemanticSnapshotTerminalState::Unavailable,
            SemanticSnapshotTerminalState::Cancelled,
            SemanticSnapshotTerminalState::BudgetExhausted,
            SemanticSnapshotTerminalState::StaleOrSuperseded,
            SemanticSnapshotTerminalState::ProductFailure,
            SemanticSnapshotTerminalState::InstrumentOrSchemaFailure,
            SemanticSnapshotTerminalState::NotProven,
        ];
        assert_eq!(states.len(), 12, "the twelve terminal states stay closed");
        for state in states {
            let snapshot = FileSemanticSnapshotV1::from_parts(parts_for_terminal(state))
                .unwrap_or_else(|e| panic!("{state:?} must be constructible: {e}"));
            let json = serde_json::to_string(&snapshot).unwrap();
            let back: FileSemanticSnapshotV1 = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{state:?} must round-trip: {e}\n{json}"));
            assert_eq!(snapshot, back, "{state:?} round-trip must be lossless");
        }
    }

    #[test]
    fn round_trip_with_projection_and_dynamic_limitations() {
        let profile = profile();
        let mut parts = parts_for_terminal(SemanticSnapshotTerminalState::PartialRecovered);
        parts.confidence = SemanticConfidence::DynamicBounded;
        parts.limitations = SemanticLimitations::new(vec![
            SemanticLimitationEntry { kind: SemanticLimitationKind::RecoveredRegion, count: 1 },
            SemanticLimitationEntry { kind: SemanticLimitationKind::DynamicEval, count: 3 },
        ]);
        parts.project_fact_projection = Some(ProjectFactProjectionRef {
            projection_id: ProjectFactProjectionId::from_source_and_profile(
                &parts.subject.logical_source_id,
                &profile,
            ),
            fingerprint: ContentDigest::of_bytes(b"projection"),
        });
        let snapshot = FileSemanticSnapshotV1::from_parts(parts).unwrap();
        let json = serde_json::to_string(&snapshot).unwrap();
        let back: FileSemanticSnapshotV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot, back);
        assert!(back.project_fact_projection().is_some());
    }

    // ── Determinism and distinctness ──────────────────────────────────────

    #[test]
    fn identical_parts_produce_identical_fingerprints() {
        let a = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        let b = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_covers_terminal_state_and_limitations() {
        // Two absent-family snapshots differing only in terminal state.
        let unavailable = FileSemanticSnapshotV1::from_parts(parts_for_terminal(
            SemanticSnapshotTerminalState::Unavailable,
        ))
        .unwrap();
        let cancelled = FileSemanticSnapshotV1::from_parts(parts_for_terminal(
            SemanticSnapshotTerminalState::Cancelled,
        ))
        .unwrap();
        assert_ne!(
            unavailable.fingerprint(),
            cancelled.fingerprint(),
            "terminal state alone must change the fingerprint"
        );

        // Two partial-recovered snapshots differing only in limitation counts.
        let base = FileSemanticSnapshotV1::from_parts(parts_for_terminal(
            SemanticSnapshotTerminalState::PartialRecovered,
        ))
        .unwrap();
        let mut more = parts_for_terminal(SemanticSnapshotTerminalState::PartialRecovered);
        more.limitations = SemanticLimitations::new(vec![SemanticLimitationEntry {
            kind: SemanticLimitationKind::RecoveredRegion,
            count: 9,
        }]);
        let more = FileSemanticSnapshotV1::from_parts(more).unwrap();
        assert_ne!(
            base.fingerprint(),
            more.fingerprint(),
            "limitation inventory must change the fingerprint"
        );
    }

    #[test]
    fn source_identical_later_generation_does_not_collapse() {
        let earlier = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        let later = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts_for(
            subject("open-1", "8"),
            profile(),
        ))
        .unwrap();
        assert_eq!(
            earlier.subject().full_source_revision.content_digest,
            later.subject().full_source_revision.content_digest,
            "same bytes must give the same content revision"
        );
        assert_ne!(
            earlier.subject_fingerprint(),
            later.subject_fingerprint(),
            "later generation must stay a distinct subject"
        );
        assert_ne!(
            earlier.fingerprint(),
            later.fingerprint(),
            "source-identical later generations must never collapse"
        );
    }

    #[test]
    fn close_reopen_instances_do_not_collapse() {
        let first = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        let reopened = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts_for(
            subject("open-2", "7"),
            profile(),
        ))
        .unwrap();
        assert_eq!(
            first.subject().logical_source_id,
            reopened.subject().logical_source_id,
            "close/reopen keeps the same logical source"
        );
        assert_ne!(
            first.subject().document_instance,
            reopened.subject().document_instance,
            "close/reopen must be a distinct document instance"
        );
        assert_ne!(first.fingerprint(), reopened.fingerprint());
    }

    #[test]
    fn fingerprint_covers_every_identity_part() {
        let base = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts())
            .unwrap()
            .fingerprint()
            .clone();

        // Different profile triple.
        let profile = SemanticProfileIdentity::new(
            "file-semantic",
            1,
            "perl-semantic-analyzer/0.20",
            "default",
        );
        let subject = subject("open-1", "7");
        let set = complete_set(&subject, &profile);
        let mut parts = complete_fresh_full_parts();
        parts.profile = profile;
        parts.contribution_set = Some(set.clone());
        parts.materialized_views = required_views(&set.set_id);
        parts.subject = subject;
        assert_ne!(FileSemanticSnapshotV1::from_parts(parts).unwrap().fingerprint(), &base);

        // Different contribution-set fingerprint.
        let mut parts = complete_fresh_full_parts();
        parts.contribution_set = Some(SemanticContributionSetRef::new(
            parts.subject.fingerprint(),
            &parts.profile,
            SemanticContributionSetCompleteness::Complete,
            ContentDigest::of_bytes(b"other-set"),
        ));
        assert_ne!(FileSemanticSnapshotV1::from_parts(parts).unwrap().fingerprint(), &base);

        // Different work sequence.
        let mut parts = complete_fresh_full_parts();
        parts.work_receipt = SemanticWorkReceipt::new(
            SemanticWorkKind::FreshFull,
            parts.work_receipt.instrument.clone(),
            99,
        );
        assert_ne!(FileSemanticSnapshotV1::from_parts(parts).unwrap().fingerprint(), &base);

        // Different parse generation.
        let mut parts = complete_fresh_full_parts();
        parts.parse_snapshot = parse_snapshot(8);
        assert_ne!(FileSemanticSnapshotV1::from_parts(parts).unwrap().fingerprint(), &base);

        // Different parse disposition. Complete terminals can only carry a
        // clean parse (family law), so distinctness is asserted through an
        // absent-family pair where both dispositions validate.
        let with_clean =
            FileSemanticSnapshotV1::from_parts(unavailable_parts()).unwrap().fingerprint().clone();
        let mut parts = unavailable_parts();
        parts.parse_snapshot = ParseSnapshotIdentity::new(
            7,
            ContentDigest::of_bytes(SOURCE),
            SOURCE.len() as u64,
            SemanticParseDisposition::Recovered,
            SemanticParseStrategy::Fresh,
        );
        assert_ne!(FileSemanticSnapshotV1::from_parts(parts).unwrap().fingerprint(), &with_clean);
    }

    #[test]
    fn fingerprint_covers_receipt_and_predecessor_records() {
        // An absent-family terminal leaves work_kind free (only the complete
        // family pins it), so two valid receipts differing only in work kind
        // must not share a snapshot fingerprint.
        let base = FileSemanticSnapshotV1::from_parts(unavailable_parts()).unwrap();
        let mut parts = unavailable_parts();
        parts.work_receipt = SemanticWorkReceipt::new(
            SemanticWorkKind::NoChangeReuse,
            parts.work_receipt.instrument.clone(),
            parts.work_receipt.work_sequence,
        );
        let relabeled = FileSemanticSnapshotV1::from_parts(parts).unwrap();
        assert_ne!(
            base.fingerprint(),
            relabeled.fingerprint(),
            "work-kind relabeling must change the snapshot fingerprint"
        );

        // The validator cannot rederive an instrument instance from its
        // authority-held key, so relabeling the instrument class while
        // keeping the opaque instance passes validation — the fingerprint
        // must still change.
        let mut parts = unavailable_parts();
        let same_instance = parts.work_receipt.instrument.instance.clone();
        parts.work_receipt.instrument = InstrumentIdentity {
            kind: SemanticInstrumentKind::ExternalProducer,
            instance: same_instance,
        };
        let relabeled = FileSemanticSnapshotV1::from_parts(parts).unwrap();
        assert_ne!(
            base.fingerprint(),
            relabeled.fingerprint(),
            "instrument-class relabeling over the same opaque instance must change the snapshot \
             fingerprint"
        );

        // Predecessor generation is not derivable from the predecessor
        // fingerprint, so a validated incremental pair differing only in the
        // predecessor generation must not share a fingerprint.
        let incremental = FileSemanticSnapshotV1::from_parts(parts_for_terminal(
            SemanticSnapshotTerminalState::CompleteIncremental,
        ))
        .unwrap();
        let mut parts = parts_for_terminal(SemanticSnapshotTerminalState::CompleteIncremental);
        if let Some(predecessor) = &mut parts.predecessor {
            predecessor.predecessor_generation = 5;
        }
        assert_ne!(
            incremental.fingerprint(),
            FileSemanticSnapshotV1::from_parts(parts).unwrap().fingerprint(),
            "predecessor generation must change the snapshot fingerprint"
        );

        // Non-complete terminals do not pin the predecessor relation, so a
        // validated absent-family pair differing only in relation must not
        // share a fingerprint.
        let mut with_incremental = unavailable_parts();
        with_incremental.predecessor = Some(SemanticPredecessorRef {
            predecessor_fingerprint: ContentDigest::of_bytes(b"predecessor"),
            predecessor_generation: 6,
            relation: SemanticReuseRelation::Incremental,
        });
        let incremental = FileSemanticSnapshotV1::from_parts(with_incremental).unwrap();
        let mut with_reuse = unavailable_parts();
        with_reuse.predecessor = Some(SemanticPredecessorRef {
            predecessor_fingerprint: ContentDigest::of_bytes(b"predecessor"),
            predecessor_generation: 6,
            relation: SemanticReuseRelation::NoChangeReuse,
        });
        assert_ne!(
            incremental.fingerprint(),
            FileSemanticSnapshotV1::from_parts(with_reuse).unwrap().fingerprint(),
            "predecessor relation must change the snapshot fingerprint"
        );
    }

    #[test]
    fn subject_fingerprint_distinguishes_source_generation_variants() {
        let known = subject("open-1", "7");
        let unknown = SemanticSubjectIdentity {
            source_generation: SourceGeneration::Unknown,
            ..known.clone()
        };
        let empty_known = SemanticSubjectIdentity {
            source_generation: SourceGeneration::known(""),
            ..known.clone()
        };
        // A known label whose text collides with the previous `Unknown`
        // sentinel must not collapse onto the unknown variant.
        let sentinel_named = subject("open-1", "generation:unknown");
        let fingerprints = [
            known.fingerprint(),
            unknown.fingerprint(),
            empty_known.fingerprint(),
            sentinel_named.fingerprint(),
        ];
        for (left, right) in fingerprints.iter().enumerate().flat_map(|(left, value)| {
            fingerprints.iter().skip(left + 1).map(move |right| (value, right))
        }) {
            assert_ne!(
                left, right,
                "distinct source-generation states must not share subject fingerprints"
            );
        }
    }

    // ── Constructor falsifiers: empty-as-complete, families ───────────────

    #[test]
    fn refuses_complete_state_without_contribution_set() {
        let mut parts = complete_fresh_full_parts();
        parts.contribution_set = None;
        parts.materialized_views = vec![];
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::CompleteStateWithoutContributionSet { .. }
        ));
    }

    #[test]
    fn refuses_complete_state_with_incomplete_set() {
        let mut parts = complete_fresh_full_parts();
        let subject = parts.subject.clone();
        let profile = parts.profile.clone();
        parts.contribution_set = Some(SemanticContributionSetRef::new(
            subject.fingerprint(),
            &profile,
            SemanticContributionSetCompleteness::Partial,
            ContentDigest::of_bytes(b"partial-set"),
        ));
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::CompleteStateWithIncompleteSet { .. }
        ));
    }

    #[test]
    fn refuses_complete_state_missing_required_view_family() {
        let mut parts = complete_fresh_full_parts();
        parts.materialized_views.retain(|v| v.kind != SemanticQueryViewKind::SemanticModel);
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::RequiredViewFamilyMissing { .. }
        ));
    }

    #[test]
    fn refuses_parse_dispositions_contradicting_the_terminal_family() {
        let non_clean = [
            SemanticParseDisposition::Recovered,
            SemanticParseDisposition::Catastrophic,
            SemanticParseDisposition::Cancelled,
            SemanticParseDisposition::BudgetExhausted,
        ];
        let complete_family = [
            SemanticSnapshotTerminalState::CompleteFreshFull,
            SemanticSnapshotTerminalState::CompleteIncremental,
            SemanticSnapshotTerminalState::CompleteNoChangeReuse,
            SemanticSnapshotTerminalState::CompleteFullFallback,
        ];
        for terminal in complete_family {
            for disposition in non_clean {
                let mut parts = parts_for_terminal(terminal);
                parts.parse_snapshot = ParseSnapshotIdentity::new(
                    7,
                    ContentDigest::of_bytes(SOURCE),
                    SOURCE.len() as u64,
                    disposition,
                    SemanticParseStrategy::Fresh,
                );
                let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
                assert!(
                    matches!(
                        err,
                        FileSemanticSnapshotValidationError::ParseDispositionContradiction {
                            terminal: found,
                            ..
                        } if found == terminal
                    ),
                    "{terminal:?} with {disposition:?} must be refused as contradicting the \
                         terminal family: {err}"
                );
            }
        }

        // PartialRecovered is parser recovery: a clean parse cannot back it.
        let mut parts = parts_for_terminal(SemanticSnapshotTerminalState::PartialRecovered);
        parts.parse_snapshot = ParseSnapshotIdentity::new(
            7,
            ContentDigest::of_bytes(SOURCE),
            SOURCE.len() as u64,
            SemanticParseDisposition::Clean,
            SemanticParseStrategy::Fresh,
        );
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::ParseDispositionContradiction { .. }
        ));

        // Absent-family terminals stay unconstrained: a clean parse can
        // precede a later cancellation, and a failed parse can precede an
        // absent-family failure.
        for disposition in non_clean {
            let mut parts = unavailable_parts();
            parts.parse_snapshot = ParseSnapshotIdentity::new(
                7,
                ContentDigest::of_bytes(SOURCE),
                SOURCE.len() as u64,
                disposition,
                SemanticParseStrategy::Fresh,
            );
            assert!(
                FileSemanticSnapshotV1::from_parts(parts).is_ok(),
                "absent-family terminals accept a {disposition:?} parse"
            );
        }
    }

    #[test]
    fn refuses_complete_state_with_unknown_source_generation() {
        let logical_source = logical_source();
        let unknown_generation_subject = SemanticSubjectIdentity::new(
            logical_source.clone(),
            DocumentInstanceId::from_logical_source_and_instance_key(&logical_source, "open-1"),
            SourceGeneration::Unknown,
            ContentDigest::of_bytes(SOURCE),
            ParserInputRevision::new(ContentDigest::of_bytes(SOURCE), SOURCE.len() as u64),
        );
        let parts = complete_fresh_full_parts_for(unknown_generation_subject, profile());
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(err, FileSemanticSnapshotValidationError::UnknownSourceGeneration));
    }

    // ── Constructor falsifiers: fresh-as-incremental / strategy ───────────

    #[test]
    fn refuses_fresh_full_terminal_with_incremental_work() {
        let mut parts = complete_fresh_full_parts();
        parts.work_receipt = SemanticWorkReceipt::new(
            SemanticWorkKind::Incremental,
            parts.work_receipt.instrument.clone(),
            50,
        );
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::WorkKindTerminalContradiction { .. }
        ));
    }

    #[test]
    fn refuses_fresh_full_terminal_with_predecessor() {
        let mut parts = complete_fresh_full_parts();
        parts.predecessor = Some(SemanticPredecessorRef {
            predecessor_fingerprint: ContentDigest::of_bytes(b"predecessor"),
            predecessor_generation: 6,
            relation: SemanticReuseRelation::NoChangeReuse,
        });
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::PredecessorNotApplicable { .. }
        ));
    }

    #[test]
    fn refuses_incremental_terminal_without_predecessor() {
        let mut parts = parts_for_terminal(SemanticSnapshotTerminalState::CompleteIncremental);
        parts.predecessor = None;
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(err, FileSemanticSnapshotValidationError::PredecessorRequired { .. }));
    }

    #[test]
    fn refuses_reuse_terminal_with_wrong_relation() {
        let mut parts = parts_for_terminal(SemanticSnapshotTerminalState::CompleteNoChangeReuse);
        parts.predecessor = Some(SemanticPredecessorRef {
            predecessor_fingerprint: ContentDigest::of_bytes(b"predecessor"),
            predecessor_generation: 6,
            relation: SemanticReuseRelation::Incremental,
        });
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::PredecessorRelationMismatch { .. }
        ));
    }

    // ── Constructor falsifiers: old-success-masks-failure ─────────────────

    #[test]
    fn refuses_unavailable_terminal_carrying_completed_facts() {
        let mut parts = unavailable_parts();
        let subject = parts.subject.clone();
        let profile = parts.profile.clone();
        let set = complete_set(&subject, &profile);
        parts.contribution_set = Some(set.clone());
        parts.materialized_views = required_views(&set.set_id);
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::AbsentStateCarryingCompletedFacts { .. }
        ));
    }

    #[test]
    fn refuses_failure_terminal_carrying_projection() {
        let mut parts = unavailable_parts();
        parts.terminal_state = SemanticSnapshotTerminalState::ProductFailure;
        parts.project_fact_projection = Some(ProjectFactProjectionRef {
            projection_id: ProjectFactProjectionId::from_source_and_profile(
                &parts.subject.logical_source_id,
                &parts.profile,
            ),
            fingerprint: ContentDigest::of_bytes(b"projection"),
        });
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::AbsentStateCarryingCompletedFacts { .. }
        ));
    }

    #[test]
    fn refuses_terminal_completeness_contradiction() {
        let mut parts = complete_fresh_full_parts();
        parts.completeness = SemanticCompleteness::Partial;
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::TerminalCompletenessContradiction { .. }
        ));
    }

    #[test]
    fn refuses_terminal_confidence_contradiction() {
        let mut parts = unavailable_parts();
        parts.confidence = SemanticConfidence::Exact;
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::TerminalConfidenceContradiction { .. }
        ));
    }

    // ── Constructor falsifiers: limitations ───────────────────────────────

    #[test]
    fn refuses_partial_recovered_without_recovery_limitations() {
        let mut parts = parts_for_terminal(SemanticSnapshotTerminalState::PartialRecovered);
        parts.limitations = SemanticLimitations::new(vec![]);
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(err, FileSemanticSnapshotValidationError::MissingRecoveryLimitations));
    }

    #[test]
    fn refuses_dynamic_bounded_confidence_without_dynamic_limitations() {
        let mut parts = parts_for_terminal(SemanticSnapshotTerminalState::PartialRecovered);
        parts.confidence = SemanticConfidence::DynamicBounded;
        parts.limitations = SemanticLimitations::new(vec![SemanticLimitationEntry {
            kind: SemanticLimitationKind::RecoveredRegion,
            count: 1,
        }]);
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(err, FileSemanticSnapshotValidationError::MissingDynamicLimitations));
    }

    // ── Constructor falsifiers: mixed identities ──────────────────────────

    #[test]
    fn refuses_full_source_revision_of_another_logical_source() {
        let mut parts = complete_fresh_full_parts();
        let other_source = {
            let project = ProjectId::from_canonical_name("acme/other");
            let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
            LogicalSourceId::from_root_and_path(&root, "lib/Other.pm")
        };
        parts.subject.full_source_revision =
            ContentRevision::new(other_source, ContentDigest::of_bytes(SOURCE));
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::FullSourceSubjectMismatch { .. }
        ));
    }

    #[test]
    fn refuses_parse_snapshot_digest_mismatch() {
        let mut parts = complete_fresh_full_parts();
        parts.parse_snapshot = ParseSnapshotIdentity::new(
            7,
            ContentDigest::of_bytes(b"other parser input"),
            SOURCE.len() as u64,
            SemanticParseDisposition::Clean,
            SemanticParseStrategy::Fresh,
        );
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::ParserInputDigestMismatch { .. }
        ));
    }

    #[test]
    fn refuses_parse_snapshot_length_mismatch() {
        let mut parts = complete_fresh_full_parts();
        parts.parse_snapshot = ParseSnapshotIdentity::new(
            7,
            ContentDigest::of_bytes(SOURCE),
            (SOURCE.len() + 1) as u64,
            SemanticParseDisposition::Clean,
            SemanticParseStrategy::Fresh,
        );
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::ParserInputLengthMismatch { .. }
        ));
    }

    #[test]
    fn refuses_contribution_set_of_another_subject() {
        let mut parts = complete_fresh_full_parts();
        let other_subject = subject("open-9", "7");
        parts.contribution_set = Some(SemanticContributionSetRef::new(
            other_subject.fingerprint(),
            &parts.profile,
            SemanticContributionSetCompleteness::Complete,
            ContentDigest::of_bytes(b"contribution-set"),
        ));
        parts.materialized_views = vec![];
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(err, FileSemanticSnapshotValidationError::ContributionSetSubjectMismatch));
    }

    #[test]
    fn refuses_view_owned_by_another_set() {
        let mut parts = complete_fresh_full_parts();
        let other_subject = subject("open-9", "7");
        let other_set = complete_set(&other_subject, &parts.profile);
        parts.materialized_views = vec![MaterializedQueryViewRef::new(
            &other_set.set_id,
            SemanticQueryViewKind::SymbolTable,
            ContentDigest::of_bytes(b"view"),
        )];
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::MaterializedViewSetMismatch { .. }
        ));
    }

    #[test]
    fn refuses_views_without_contribution_set() {
        let mut parts = unavailable_parts();
        parts.materialized_views = vec![MaterializedQueryViewRef::new(
            &SemanticContributionSetId::from_subject_and_profile(
                &parts.subject.fingerprint(),
                &parts.profile,
            ),
            SemanticQueryViewKind::SymbolTable,
            ContentDigest::of_bytes(b"view"),
        )];
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::ContributionSetRequiredForViews
        ));
    }

    #[test]
    fn refuses_duplicate_view_kinds() {
        let mut parts = complete_fresh_full_parts();
        let set_id = parts.contribution_set.as_ref().unwrap().set_id.clone();
        parts.materialized_views.push(MaterializedQueryViewRef::new(
            &set_id,
            SemanticQueryViewKind::SymbolTable,
            ContentDigest::of_bytes(b"second-symbol-table"),
        ));
        let err = FileSemanticSnapshotV1::from_parts(parts).unwrap_err();
        assert!(matches!(
            err,
            FileSemanticSnapshotValidationError::DuplicateMaterializedViewKind { .. }
        ));
    }

    // ── Canonicalization ──────────────────────────────────────────────────

    #[test]
    fn constructor_canonicalizes_unsorted_views() {
        let mut parts = complete_fresh_full_parts();
        parts.materialized_views.reverse();
        let snapshot = FileSemanticSnapshotV1::from_parts(parts).unwrap();
        let kinds: Vec<_> = snapshot.materialized_views().iter().map(|v| v.kind).collect();
        let mut sorted = kinds.clone();
        sorted.sort();
        assert_eq!(kinds, sorted, "views must be stored canonically ordered");
        let json = serde_json::to_string(&snapshot).unwrap();
        let back: FileSemanticSnapshotV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot, back);
    }

    #[test]
    fn limitations_merge_duplicate_kinds_and_sort() {
        let limitations = SemanticLimitations::new(vec![
            SemanticLimitationEntry { kind: SemanticLimitationKind::DynamicRequire, count: 2 },
            SemanticLimitationEntry { kind: SemanticLimitationKind::RecoveredRegion, count: 1 },
            SemanticLimitationEntry { kind: SemanticLimitationKind::DynamicRequire, count: 3 },
        ]);
        assert_eq!(
            limitations.entries,
            vec![
                SemanticLimitationEntry { kind: SemanticLimitationKind::RecoveredRegion, count: 1 },
                SemanticLimitationEntry { kind: SemanticLimitationKind::DynamicRequire, count: 5 },
            ]
        );
        assert!(limitations.has_recovery_limitation());
        assert!(limitations.has_dynamic_limitation());
        assert_eq!(limitations.count_of(SemanticLimitationKind::DynamicRequire), 5);
    }

    // ── Wire falsifiers (checked deserialization through the constructor) ─

    fn complete_json() -> serde_json::Value {
        let snapshot = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        serde_json::to_value(&snapshot).unwrap()
    }

    #[test]
    fn wire_refuses_mixed_ticket_id() {
        let mut value = complete_json();
        let mixed_ticket = AcceptedParserTicketId::from_bound_parts(
            &DocumentInstanceId::from_logical_source_and_instance_key(&logical_source(), "open-1"),
            7,
            &ContentDigest::of_bytes(b"other parser input"),
        );
        value["accepted_ticket"]["ticket_id"] = json!(mixed_ticket.as_wire());
        let err = serde_json::from_value::<FileSemanticSnapshotV1>(value).unwrap_err();
        assert!(
            err.to_string().contains("ticket id"),
            "mixed ticket must be refused with the ticket-identity falsifier: {err}"
        );
    }

    #[test]
    fn wire_refuses_ticket_bound_to_another_document_instance() {
        let mut value = complete_json();
        let other_instance =
            DocumentInstanceId::from_logical_source_and_instance_key(&logical_source(), "open-2");
        value["accepted_ticket"]["document_instance"] = json!(other_instance.as_wire());
        assert!(serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err());
    }

    #[test]
    fn wire_refuses_ticket_generation_mismatch() {
        let mut value = complete_json();
        value["accepted_ticket"]["accepted_generation"] = json!(8);
        assert!(serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err());
    }

    #[test]
    fn wire_refuses_tampered_profile_fingerprint() {
        let mut value = complete_json();
        let tampered = ContentDigest::of_bytes(b"tampered profile");
        value["profile"]["fingerprint"] = json!(tampered.as_wire());
        let err = serde_json::from_value::<FileSemanticSnapshotV1>(value).unwrap_err();
        assert!(
            err.to_string().contains("profile fingerprint"),
            "tampered profile fingerprint must be refused: {err}"
        );
    }

    #[test]
    fn wire_refuses_tampered_snapshot_fingerprint() {
        let mut value = complete_json();
        let tampered = ContentDigest::of_bytes(b"tampered snapshot");
        value["fingerprint"] = json!(tampered.as_wire());
        let err = serde_json::from_value::<FileSemanticSnapshotV1>(value).unwrap_err();
        assert!(
            err.to_string().contains("fingerprint"),
            "tampered fingerprint must be refused: {err}"
        );
    }

    #[test]
    fn wire_refuses_complete_state_after_dropping_contribution_set() {
        let mut value = complete_json();
        value["contribution_set"].take();
        let err = serde_json::from_value::<FileSemanticSnapshotV1>(value).unwrap_err();
        assert!(
            err.to_string().contains("contribution set"),
            "unchecked wire construction of complete state must be refused: {err}"
        );
    }

    #[test]
    fn wire_refuses_mismatched_work_receipt_id() {
        let mut value = complete_json();
        let other_receipt = SemanticWorkReceiptId::from_instrument_and_sequence(
            &InstrumentIdentity::new(SemanticInstrumentKind::ConstructionCell, "cell-1"),
            999,
        );
        value["work_receipt"]["receipt_id"] = json!(other_receipt.as_wire());
        let err = serde_json::from_value::<FileSemanticSnapshotV1>(value).unwrap_err();
        assert!(err.to_string().contains("receipt"), "mismatched receipt must be refused: {err}");
    }

    #[test]
    fn wire_refuses_unknown_fields() {
        let mut value = complete_json();
        value["pir_lexical_contribution"] = json!({"facts": []});
        assert!(
            serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err(),
            "unknown fields (including compiler contributions) must be refused"
        );
    }

    #[test]
    fn wire_refuses_unknown_fields_inside_nested_records() {
        // Unknown keys must be refused inside every module-owned nested
        // record, not only at the top level: an extra nested key is a newer
        // schema this version cannot interpret, and silently dropping it
        // would reserialize a different payload under the same fingerprint.
        let injections: [(&str, serde_json::Value); 8] = [
            ("parse_snapshot", json!({"future_disposition_detail": "notes"})),
            ("subject", json!({"ephemeral_instance_flag": true})),
            ("accepted_ticket", json!({"queue_position": 3})),
            ("profile", json!({"experimental_profile": "beta"})),
            ("work_receipt", json!({"retry_count": 1})),
            ("limitations", json!({"future_limitation_scope": "v2"})),
            ("materialized_views", json!([{"future_view_field": 1}])),
            ("contribution_set", json!({"future_set_scope": "v2"})),
        ];
        for (record, addition) in injections {
            let mut value = complete_json();
            match (record, &addition) {
                ("materialized_views", serde_json::Value::Array(entries)) => {
                    let mut views = value["materialized_views"].as_array().unwrap().clone();
                    assert!(!views.is_empty());
                    for entry in entries {
                        if let Some(object) = views[0].as_object_mut() {
                            for (key, field) in entry.as_object().into_iter().flatten() {
                                object.insert(key.clone(), field.clone());
                            }
                        }
                    }
                    value["materialized_views"] = json!(views);
                }
                _ => {
                    for (key, field) in addition.as_object().into_iter().flatten() {
                        value[record][key] = field.clone();
                    }
                }
            }
            assert!(
                serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err(),
                "unknown field inside `{record}` must be refused at the serde boundary"
            );
        }

        // The instrument record nests inside the work receipt: an extra key
        // there must be refused too, not silently dropped while the receipt
        // id (derived from instance + sequence) still validates.
        let mut value = complete_json();
        value["work_receipt"]["instrument"]["host_path"] = json!("/tmp/cell");
        assert!(
            serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err(),
            "unknown field inside `work_receipt.instrument` must be refused"
        );
    }

    #[test]
    fn wire_refuses_missing_required_fields() {
        for missing in [
            "schema_version",
            "profile",
            "subject",
            "accepted_ticket",
            "parse_snapshot",
            "work_receipt",
            "terminal_state",
            "completeness",
            "confidence",
            "limitations",
            "fingerprint",
        ] {
            let mut value = complete_json();
            assert!(value.as_object_mut().unwrap().remove(missing).is_some());
            assert!(
                serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err(),
                "missing `{missing}` must be refused"
            );
        }
    }

    #[test]
    fn wire_refuses_uppercase_id_spelling() {
        let mut value = complete_json();
        let instance = value["subject"]["document_instance"].as_str().unwrap().to_string();
        let upper = format!(
            "doc-instance:sha256:{}",
            instance["doc-instance:sha256:".len()..].to_ascii_uppercase()
        );
        value["subject"]["document_instance"] = json!(upper);
        assert!(serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err());
    }

    #[test]
    fn wire_refuses_malformed_digest() {
        let mut value = complete_json();
        let upper = value["subject"]["full_source_revision"]["content_digest"]
            .as_str()
            .unwrap()
            .to_ascii_uppercase();
        value["subject"]["full_source_revision"]["content_digest"] = json!(upper);
        assert!(serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err());
    }

    #[test]
    fn wire_refuses_unsorted_views() {
        let mut value = complete_json();
        let mut views = value["materialized_views"].as_array().unwrap().clone();
        views.reverse();
        value["materialized_views"] = json!(views);
        let err = serde_json::from_value::<FileSemanticSnapshotV1>(value).unwrap_err();
        assert!(
            err.to_string().contains("ordered"),
            "unsorted views must be refused, not normalized: {err}"
        );
    }

    #[test]
    fn wire_refuses_unknown_terminal_variant() {
        let mut value = complete_json();
        value["terminal_state"] = json!("complete_magical");
        assert!(serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err());
    }

    #[test]
    fn wire_refuses_unknown_instrument_kind() {
        let mut value = complete_json();
        value["work_receipt"]["instrument"]["kind"] = json!("mystery_instrument");
        assert!(serde_json::from_value::<FileSemanticSnapshotV1>(value).is_err());
    }

    // ── Read-only boundary ───────────────────────────────────────────────

    #[test]
    fn as_current_complete_is_none_for_every_non_complete_state() {
        let non_complete = [
            SemanticSnapshotTerminalState::PartialRecovered,
            SemanticSnapshotTerminalState::Unavailable,
            SemanticSnapshotTerminalState::Cancelled,
            SemanticSnapshotTerminalState::BudgetExhausted,
            SemanticSnapshotTerminalState::StaleOrSuperseded,
            SemanticSnapshotTerminalState::ProductFailure,
            SemanticSnapshotTerminalState::InstrumentOrSchemaFailure,
            SemanticSnapshotTerminalState::NotProven,
        ];
        for state in non_complete {
            let snapshot = FileSemanticSnapshotV1::from_parts(parts_for_terminal(state)).unwrap();
            assert!(
                snapshot.as_current_complete().is_none(),
                "{state:?} must never flatten into current-complete facts"
            );
        }
    }

    #[test]
    fn as_current_complete_exposes_join_identity_without_work() {
        let snapshot = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        let facts = snapshot.as_current_complete().expect("complete snapshot");
        assert_eq!(
            facts.contribution_set().set_id,
            snapshot.contribution_set_id().unwrap().clone()
        );
        assert_eq!(facts.subject_fingerprint(), snapshot.subject_fingerprint());
        assert_eq!(facts.materialized_views().len(), 3);
        // Read access is repeatable and borrow-only.
        let again = snapshot.as_current_complete().expect("complete snapshot");
        assert_eq!(facts.contribution_set(), again.contribution_set());
    }

    #[test]
    fn view_availability_queries_by_kind() {
        let snapshot = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        assert!(snapshot.is_view_available(SemanticQueryViewKind::SymbolTable));
        assert!(snapshot.is_view_available(SemanticQueryViewKind::Completeness));
        assert!(!snapshot.is_view_available(SemanticQueryViewKind::Hover));
        assert!(snapshot.materialized_view(SemanticQueryViewKind::Hover).is_none());
    }

    #[test]
    fn snapshot_is_immutable_and_shareable() {
        fn assert_send_sync_clone<T: Send + Sync + Clone>() {}
        assert_send_sync_clone::<FileSemanticSnapshotV1>();
        let snapshot = FileSemanticSnapshotV1::from_parts(complete_fresh_full_parts()).unwrap();
        let clone = snapshot.clone();
        assert_eq!(clone.fingerprint(), snapshot.fingerprint());
        assert_eq!(clone.terminal_state(), snapshot.terminal_state());
        // Repeated reads agree: no read path mutates observable state.
        assert_eq!(snapshot.terminal_state(), snapshot.terminal_state());
        assert_eq!(snapshot.materialized_views(), snapshot.materialized_views());
    }

    // ── Wire vocabulary ───────────────────────────────────────────────────

    #[test]
    fn wire_names_match_as_str() {
        assert_eq!(
            serde_json::to_value(SemanticSnapshotTerminalState::CompleteFreshFull).unwrap(),
            json!("complete_fresh_full")
        );
        assert_eq!(
            serde_json::to_value(SemanticSnapshotTerminalState::InstrumentOrSchemaFailure).unwrap(),
            json!("instrument_or_schema_failure")
        );
        assert_eq!(
            serde_json::to_value(SemanticWorkKind::NoChangeReuse).unwrap(),
            json!("no_change_reuse")
        );
        assert_eq!(
            serde_json::to_value(SemanticParseDisposition::BudgetExhausted).unwrap(),
            json!("budget_exhausted")
        );
        assert_eq!(
            serde_json::to_value(SemanticParseStrategy::IncrementalFullFallback).unwrap(),
            json!("incremental_full_fallback")
        );
        assert_eq!(
            serde_json::to_value(SemanticLimitationKind::SyntheticRepair).unwrap(),
            json!("synthetic_repair")
        );
    }

    // ── Architecture: below provider/LSP types, no analysis ───────────────

    #[test]
    fn module_references_no_provider_lsp_or_analysis_crates() {
        let source = include_str!("semantic_snapshot.rs");
        // The claim covers production code; test code may name forbidden
        // surfaces inside its own assertions.
        let production = source.split("#[cfg(test)]").next().unwrap();
        for forbidden in [
            "use lsp_types",
            "use perl_parser",
            "use perl_semantic_analyzer",
            "use perl_ripr_facts",
            "use perl_dap",
            "use crate::providers",
            "use crate::protocol",
        ] {
            assert!(
                !production.contains(forbidden),
                "the envelope must stay below provider/LSP types and perform no \
                 analysis: found `{forbidden}`"
            );
        }
    }
}
