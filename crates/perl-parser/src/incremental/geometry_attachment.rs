use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use perl_source_identity::ContentDigest;
use thiserror::Error;

use super::snapshot::{ParseGeneration, ParseSnapshotStrategy, ParseTerminalDisposition};

/// Schema identity for the snapshot-bound source-geometry attachment.
pub const SOURCE_GEOMETRY_ATTACHMENT_SCHEMA_VERSION: &str = "source_geometry_attachment.v1";

/// Process-local identity of one parser-state lifetime.
///
/// The marker is intentionally opaque and pointer-compared. Cloning an
/// `IncrementalState` or committing its next generation retains the marker;
/// constructing an independent or reopened state allocates another marker.
/// Durable cross-process acceptance/currentness belongs to the accepted parser
/// ticket and document-instance authorities above this parser-local layer.
#[derive(Clone)]
struct SourceGeometryInstanceIdentity(Arc<SourceGeometryInstanceMarker>);

struct SourceGeometryInstanceMarker;

impl SourceGeometryInstanceIdentity {
    fn new() -> Self {
        Self(Arc::new(SourceGeometryInstanceMarker))
    }
}

impl fmt::Debug for SourceGeometryInstanceIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SourceGeometryInstanceIdentity(..)")
    }
}

impl PartialEq for SourceGeometryInstanceIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SourceGeometryInstanceIdentity {}

impl Hash for SourceGeometryInstanceIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.0), state);
    }
}

/// Exact parser-snapshot subject to which source geometry belongs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SourceGeometrySubject {
    instance_identity: SourceGeometryInstanceIdentity,
    generation: ParseGeneration,
    content_digest: ContentDigest,
    source_len: usize,
    disposition: ParseTerminalDisposition,
    strategy: ParseSnapshotStrategy,
}

impl SourceGeometrySubject {
    pub(super) fn new(
        generation: ParseGeneration,
        content_digest: ContentDigest,
        source_len: usize,
        disposition: ParseTerminalDisposition,
        strategy: ParseSnapshotStrategy,
    ) -> Self {
        Self {
            instance_identity: SourceGeometryInstanceIdentity::new(),
            generation,
            content_digest,
            source_len,
            disposition,
            strategy,
        }
    }

    pub(super) fn next_for_same_instance(
        previous: &Self,
        generation: ParseGeneration,
        content_digest: ContentDigest,
        source_len: usize,
        disposition: ParseTerminalDisposition,
        strategy: ParseSnapshotStrategy,
    ) -> Self {
        Self {
            instance_identity: previous.instance_identity.clone(),
            generation,
            content_digest,
            source_len,
            disposition,
            strategy,
        }
    }

    /// Whether both subjects belong to generations of the same parser-state lifetime.
    #[must_use]
    pub fn same_instance_as(&self, other: &Self) -> bool {
        self.instance_identity == other.instance_identity
    }

    /// Monotonic parser generation represented by this subject.
    #[must_use]
    pub const fn generation(&self) -> ParseGeneration {
        self.generation
    }

    /// Canonical exact-source digest represented by this subject.
    #[must_use]
    pub const fn content_digest(&self) -> &ContentDigest {
        &self.content_digest
    }

    /// Exact source length represented by this subject.
    #[must_use]
    pub const fn source_len(&self) -> usize {
        self.source_len
    }

    /// Terminal parser disposition represented by this subject.
    #[must_use]
    pub const fn disposition(&self) -> ParseTerminalDisposition {
        self.disposition
    }

    /// Parser strategy represented by this subject.
    #[must_use]
    pub const fn strategy(&self) -> ParseSnapshotStrategy {
        self.strategy
    }
}

/// Reason geometry is not available for an otherwise valid parser snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SourceGeometryUnavailableReason {
    /// The canonical same-operation geometry producer has not run.
    ProducerNotRun,
    /// The current parser path does not expose the required observations.
    ProducerUnavailable,
}

/// Typed limitation retained with a partial geometry payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SourceGeometryLimitation {
    /// The canonical lexical observation stream is incomplete.
    IncompleteObservations,
    /// Recovery or unsupported syntax prevented a complete partition.
    RecoveryOrUnsupportedSyntax,
}

/// Reason geometry instrumentation failed independently of parser output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SourceGeometryInstrumentFailureReason {
    /// The producer failed before publishing a validated payload.
    ProducerFailed,
    /// The producer returned structurally invalid geometry.
    InvalidPayload,
}

/// Identity-bearing source-geometry payload reserved for the canonical partition producer.
///
/// Fields are private so callers cannot manufacture completeness. #13980 will
/// populate the typed segment payload behind this stable snapshot attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceGeometryPayload {
    subject: SourceGeometrySubject,
    producer_schema_version: Arc<str>,
    payload_digest: ContentDigest,
    segment_count: usize,
}

impl SourceGeometryPayload {
    /// Exact parser-snapshot subject represented by the payload.
    #[must_use]
    pub const fn subject(&self) -> &SourceGeometrySubject {
        &self.subject
    }

    /// Version of the canonical geometry producer/schema.
    #[must_use]
    pub fn producer_schema_version(&self) -> &str {
        &self.producer_schema_version
    }

    /// Digest of the canonical payload bytes.
    #[must_use]
    pub const fn payload_digest(&self) -> &ContentDigest {
        &self.payload_digest
    }

    /// Number of primary segments represented by the payload.
    #[must_use]
    pub const fn segment_count(&self) -> usize {
        self.segment_count
    }
}

/// Availability and payload state for one exact parser snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceGeometryAttachmentState {
    /// No geometry payload was produced.
    Unavailable {
        /// Why the payload is unavailable.
        reason: SourceGeometryUnavailableReason,
    },
    /// A current payload exists but cannot claim a complete partition.
    Partial {
        /// Identity-bearing partial payload.
        payload: Arc<SourceGeometryPayload>,
        /// Non-empty limitations preventing completeness.
        limitations: Arc<[SourceGeometryLimitation]>,
    },
    /// A validated complete payload exists for this exact subject.
    Complete {
        /// Identity-bearing complete payload.
        payload: Arc<SourceGeometryPayload>,
    },
    /// Geometry instrumentation failed independently of parser output.
    InstrumentFailure {
        /// Why instrumentation failed.
        reason: SourceGeometryInstrumentFailureReason,
    },
}

/// Snapshot-bound source-geometry attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceGeometryAttachment {
    schema_version: &'static str,
    subject: SourceGeometrySubject,
    state: SourceGeometryAttachmentState,
}

impl SourceGeometryAttachment {
    pub(super) fn unavailable(subject: SourceGeometrySubject) -> Self {
        Self {
            schema_version: SOURCE_GEOMETRY_ATTACHMENT_SCHEMA_VERSION,
            subject,
            state: SourceGeometryAttachmentState::Unavailable {
                reason: SourceGeometryUnavailableReason::ProducerNotRun,
            },
        }
    }

    /// Attachment schema version.
    #[must_use]
    pub const fn schema_version(&self) -> &'static str {
        self.schema_version
    }

    /// Exact parser-snapshot subject represented by this attachment.
    #[must_use]
    pub const fn subject(&self) -> &SourceGeometrySubject {
        &self.subject
    }

    /// Current availability/payload state.
    #[must_use]
    pub const fn state(&self) -> &SourceGeometryAttachmentState {
        &self.state
    }

    pub(super) fn validate_for(
        &self,
        expected: &SourceGeometrySubject,
    ) -> Result<(), SourceGeometryValidationError> {
        if self.schema_version != SOURCE_GEOMETRY_ATTACHMENT_SCHEMA_VERSION {
            return Err(SourceGeometryValidationError::SchemaVersion);
        }
        if &self.subject != expected {
            return Err(SourceGeometryValidationError::AttachmentSubject);
        }

        match &self.state {
            SourceGeometryAttachmentState::Unavailable { .. }
            | SourceGeometryAttachmentState::InstrumentFailure { .. } => Ok(()),
            SourceGeometryAttachmentState::Partial { payload, limitations } => {
                if limitations.is_empty() {
                    return Err(SourceGeometryValidationError::PartialWithoutLimitations);
                }
                validate_payload_subject(payload, expected)
            }
            SourceGeometryAttachmentState::Complete { payload } => {
                validate_payload_subject(payload, expected)
            }
        }
    }
}

fn validate_payload_subject(
    payload: &SourceGeometryPayload,
    expected: &SourceGeometrySubject,
) -> Result<(), SourceGeometryValidationError> {
    if payload.subject() != expected {
        return Err(SourceGeometryValidationError::PayloadSubject);
    }
    Ok(())
}

/// Structural or subject mismatch in a source-geometry attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SourceGeometryValidationError {
    /// The attachment schema version is unsupported.
    #[error("source geometry attachment schema version is unsupported")]
    SchemaVersion,
    /// The attachment belongs to another parser snapshot.
    #[error("source geometry attachment subject does not match the parser snapshot")]
    AttachmentSubject,
    /// A partial payload omitted the limitations that prevent completeness.
    #[error("partial source geometry must retain at least one limitation")]
    PartialWithoutLimitations,
    /// The payload belongs to another parser snapshot.
    #[error("source geometry payload subject does not match the parser snapshot")]
    PayloadSubject,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(source: &str, generation: ParseGeneration) -> SourceGeometrySubject {
        SourceGeometrySubject::new(
            generation,
            ContentDigest::of_bytes(source.as_bytes()),
            source.len(),
            ParseTerminalDisposition::Clean,
            ParseSnapshotStrategy::Fresh,
        )
    }

    fn payload(subject: SourceGeometrySubject) -> Arc<SourceGeometryPayload> {
        Arc::new(SourceGeometryPayload {
            subject,
            producer_schema_version: Arc::from("source_geometry.v1"),
            payload_digest: ContentDigest::of_bytes(b"test geometry payload"),
            segment_count: 1,
        })
    }

    #[test]
    fn unavailable_partial_complete_and_instrument_failure_remain_distinct() {
        let subject = subject("my $x = 1;", ParseGeneration::INITIAL);
        let unavailable = SourceGeometryAttachment::unavailable(subject.clone());
        let partial = SourceGeometryAttachment {
            schema_version: SOURCE_GEOMETRY_ATTACHMENT_SCHEMA_VERSION,
            subject: subject.clone(),
            state: SourceGeometryAttachmentState::Partial {
                payload: payload(subject.clone()),
                limitations: Arc::from([SourceGeometryLimitation::IncompleteObservations]),
            },
        };
        let complete = SourceGeometryAttachment {
            schema_version: SOURCE_GEOMETRY_ATTACHMENT_SCHEMA_VERSION,
            subject: subject.clone(),
            state: SourceGeometryAttachmentState::Complete { payload: payload(subject.clone()) },
        };
        let failed = SourceGeometryAttachment {
            schema_version: SOURCE_GEOMETRY_ATTACHMENT_SCHEMA_VERSION,
            subject: subject.clone(),
            state: SourceGeometryAttachmentState::InstrumentFailure {
                reason: SourceGeometryInstrumentFailureReason::ProducerFailed,
            },
        };

        assert!(unavailable.validate_for(&subject).is_ok());
        assert!(partial.validate_for(&subject).is_ok());
        assert!(complete.validate_for(&subject).is_ok());
        assert!(failed.validate_for(&subject).is_ok());
        assert_ne!(unavailable.state(), partial.state());
        assert_ne!(partial.state(), complete.state());
        assert_ne!(complete.state(), failed.state());
    }

    #[test]
    fn generation_and_content_are_load_bearing_payload_identity() {
        let first = subject("my $x = 1;", ParseGeneration::INITIAL);
        let later = SourceGeometrySubject::next_for_same_instance(
            &first,
            ParseGeneration::INITIAL.checked_next().unwrap_or(ParseGeneration::INITIAL),
            ContentDigest::of_bytes(b"my $x = 1;"),
            "my $x = 1;".len(),
            ParseTerminalDisposition::Clean,
            ParseSnapshotStrategy::Fresh,
        );
        let different_same_length = subject("my $y = 1;", ParseGeneration::INITIAL);
        let attachment = SourceGeometryAttachment {
            schema_version: SOURCE_GEOMETRY_ATTACHMENT_SCHEMA_VERSION,
            subject: later.clone(),
            state: SourceGeometryAttachmentState::Complete { payload: payload(first) },
        };

        assert!(attachment.subject().same_instance_as(&later));
        assert_eq!(
            attachment.validate_for(&later),
            Err(SourceGeometryValidationError::PayloadSubject)
        );
        assert_eq!(
            SourceGeometryAttachment::unavailable(later.clone())
                .validate_for(&different_same_length),
            Err(SourceGeometryValidationError::AttachmentSubject)
        );
    }

    #[test]
    fn independent_identical_subjects_do_not_exchange_payloads() {
        let first = subject("my $x = 1;", ParseGeneration::INITIAL);
        let independent = subject("my $x = 1;", ParseGeneration::INITIAL);
        assert_ne!(first, independent);
        assert!(!first.same_instance_as(&independent));

        let attachment = SourceGeometryAttachment {
            schema_version: SOURCE_GEOMETRY_ATTACHMENT_SCHEMA_VERSION,
            subject: independent.clone(),
            state: SourceGeometryAttachmentState::Complete { payload: payload(first) },
        };

        assert_eq!(
            attachment.validate_for(&independent),
            Err(SourceGeometryValidationError::PayloadSubject)
        );
    }

    #[test]
    fn partial_payload_requires_a_real_limitation() {
        let subject = subject("my $x = 1;", ParseGeneration::INITIAL);
        let attachment = SourceGeometryAttachment {
            schema_version: SOURCE_GEOMETRY_ATTACHMENT_SCHEMA_VERSION,
            subject: subject.clone(),
            state: SourceGeometryAttachmentState::Partial {
                payload: payload(subject.clone()),
                limitations: Arc::from([]),
            },
        };

        assert_eq!(
            attachment.validate_for(&subject),
            Err(SourceGeometryValidationError::PartialWithoutLimitations)
        );
    }
}
