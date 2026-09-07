//! `operation_trace.v1`'s event-field registry: which fields each event kind
//! carries, whether they are required, and which [`FieldPrivacy`] tier the
//! field is declared at.

use std::fmt;

use crate::event::{OUTCOME_FIELD, OperationEvent, OperationEventKind};
use crate::privacy::FieldPrivacy;

/// The declared shape of one field on one event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FieldSpec {
    name: &'static str,
    privacy: FieldPrivacy,
    required: bool,
}

const fn field(name: &'static str, privacy: FieldPrivacy, required: bool) -> FieldSpec {
    FieldSpec { name, privacy, required }
}

// Each event kind's declared fields live in their own named `const`, rather
// than being built inline inside `specs`' match arms: a `const fn` call
// inside an ordinary function body is not automatically promoted to a
// `'static` temporary (rvalue static promotion does not extend to
// user-defined `const fn` calls), but a named top-level `const` item's
// initializer is always evaluated at compile time, so referencing it by name
// is trivially `'static`.
//
// `STAGE_STARTED_FIELDS` deliberately declares one field at each privacy
// tier (`stage` public, `host_path`/`source_line` private, `api_key_hint`
// secret) so producers have one obvious event to attach high-entropy or
// secret context to, and so the redaction contract has one concrete event
// kind to exercise end to end.
const ADMITTED_FIELDS: [FieldSpec; 0] = [];
const REJECTED_FIELDS: [FieldSpec; 1] = [field("reason", FieldPrivacy::Public, true)];
const GENERATION_SELECTED_FIELDS: [FieldSpec; 1] =
    [field("generation", FieldPrivacy::Public, true)];
const STAGE_STARTED_FIELDS: [FieldSpec; 4] = [
    field("stage", FieldPrivacy::Public, true),
    field("host_path", FieldPrivacy::Private, false),
    field("source_line", FieldPrivacy::Private, false),
    field("api_key_hint", FieldPrivacy::Secret, false),
];
const STAGE_COMPLETED_FIELDS: [FieldSpec; 1] = [field("stage", FieldPrivacy::Public, true)];
const PRODUCER_SELECTED_FIELDS: [FieldSpec; 1] = [field("producer", FieldPrivacy::Public, true)];
const CACHE_HIT_FIELDS: [FieldSpec; 1] = [field("cache_key", FieldPrivacy::Public, true)];
const CACHE_MISS_FIELDS: [FieldSpec; 1] = [field("cache_key", FieldPrivacy::Public, true)];
const STALE_PUBLICATION_REJECTED_FIELDS: [FieldSpec; 1] =
    [field("reason", FieldPrivacy::Public, true)];
const FALLBACK_SELECTED_FIELDS: [FieldSpec; 1] = [field("fallback", FieldPrivacy::Public, true)];
const PROCESS_PLANNED_FIELDS: [FieldSpec; 2] = [
    field("executable_logical_name", FieldPrivacy::Public, true),
    field("argv_digest_hint", FieldPrivacy::Private, false),
];
const PROCESS_STARTED_FIELDS: [FieldSpec; 1] = [field("pid_hint", FieldPrivacy::Public, false)];
const PROCESS_TERMINATED_FIELDS: [FieldSpec; 1] = [field("exit_hint", FieldPrivacy::Public, false)];
const CANCELLED_FIELDS: [FieldSpec; 1] = [field("reason", FieldPrivacy::Public, false)];
const DEADLINE_EXCEEDED_FIELDS: [FieldSpec; 1] =
    [field("deadline_ms", FieldPrivacy::Public, false)];
const RECEIPT_EMITTED_FIELDS: [FieldSpec; 1] = [field("receipt_ref", FieldPrivacy::Public, true)];
const TERMINAL_FIELDS: [FieldSpec; 1] = [field(OUTCOME_FIELD, FieldPrivacy::Public, true)];

/// The declared fields for one [`OperationEventKind`].
fn specs(kind: OperationEventKind) -> &'static [FieldSpec] {
    use OperationEventKind as K;
    match kind {
        K::Admitted => &ADMITTED_FIELDS,
        K::Rejected => &REJECTED_FIELDS,
        K::GenerationSelected => &GENERATION_SELECTED_FIELDS,
        K::StageStarted => &STAGE_STARTED_FIELDS,
        K::StageCompleted => &STAGE_COMPLETED_FIELDS,
        K::ProducerSelected => &PRODUCER_SELECTED_FIELDS,
        K::CacheHit => &CACHE_HIT_FIELDS,
        K::CacheMiss => &CACHE_MISS_FIELDS,
        K::StalePublicationRejected => &STALE_PUBLICATION_REJECTED_FIELDS,
        K::FallbackSelected => &FALLBACK_SELECTED_FIELDS,
        K::ProcessPlanned => &PROCESS_PLANNED_FIELDS,
        K::ProcessStarted => &PROCESS_STARTED_FIELDS,
        K::ProcessTerminated => &PROCESS_TERMINATED_FIELDS,
        K::Cancelled => &CANCELLED_FIELDS,
        K::DeadlineExceeded => &DEADLINE_EXCEEDED_FIELDS,
        K::ReceiptEmitted => &RECEIPT_EMITTED_FIELDS,
        K::Terminal => &TERMINAL_FIELDS,
    }
}

/// A registry validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// A field the event kind requires was not supplied.
    MissingRequiredField {
        /// The event kind that required the field.
        kind: OperationEventKind,
        /// The missing field's name.
        field: String,
    },
    /// A field name not declared for this event kind was supplied.
    UnknownField {
        /// The event kind that does not declare this field.
        kind: OperationEventKind,
        /// The unrecognized field's name.
        field: String,
    },
    /// A declared field was supplied at the wrong privacy tier.
    PrivacyMismatch {
        /// The event kind the field belongs to.
        kind: OperationEventKind,
        /// The mismatched field's name.
        field: String,
        /// The tier the registry declares for this field.
        expected: FieldPrivacy,
        /// The tier the supplied value actually carries.
        got: FieldPrivacy,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredField { kind, field } => {
                write!(f, "event kind {kind} is missing required field {field:?}")
            }
            Self::UnknownField { kind, field } => {
                write!(f, "event kind {kind} does not declare field {field:?}")
            }
            Self::PrivacyMismatch { kind, field, expected, got } => write!(
                f,
                "event kind {kind} field {field:?} is declared {expected} but was supplied as {got}"
            ),
        }
    }
}

impl std::error::Error for RegistryError {}

/// The `operation_trace.v1` event-field registry.
pub struct EventRegistry;

impl EventRegistry {
    /// Validate one event's fields against its kind's declared shape.
    ///
    /// # Errors
    ///
    /// See [`RegistryError`]'s variants: a missing required field, a field
    /// name the event kind does not declare, or a declared field supplied at
    /// the wrong privacy tier (in either direction — a `Secret`-declared
    /// field must not be supplied as `Private` either, since `Private`
    /// still leaks a byte length).
    pub fn validate(event: &OperationEvent) -> Result<(), RegistryError> {
        let kind = event.kind();
        let declared = specs(kind);

        for spec in declared.iter().filter(|s| s.required) {
            if !event.fields().iter().any(|(name, _)| name == spec.name) {
                return Err(RegistryError::MissingRequiredField {
                    kind,
                    field: spec.name.to_string(),
                });
            }
        }

        for (name, value) in event.fields() {
            let Some(spec) = declared.iter().find(|s| s.name == name) else {
                return Err(RegistryError::UnknownField { kind, field: name.clone() });
            };
            let got = value.privacy();
            if got != spec.privacy {
                return Err(RegistryError::PrivacyMismatch {
                    kind,
                    field: name.clone(),
                    expected: spec.privacy,
                    got,
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::event::EventFieldValue;
    use crate::privacy::{PrivateValue, SecretField};

    #[test]
    fn event_with_no_fields_and_no_required_fields_is_valid() {
        let event = OperationEvent::new(OperationEventKind::Admitted);
        assert!(EventRegistry::validate(&event).is_ok());
    }

    #[test]
    fn missing_required_field_is_rejected() {
        let event = OperationEvent::new(OperationEventKind::Rejected);
        let err = EventRegistry::validate(&event).unwrap_err();
        assert_eq!(
            err,
            RegistryError::MissingRequiredField {
                kind: OperationEventKind::Rejected,
                field: "reason".to_string(),
            }
        );
    }

    #[test]
    fn unknown_field_is_rejected() {
        let event = OperationEvent::new(OperationEventKind::Rejected)
            .with_field("reason", EventFieldValue::PublicString("bad input".into()))
            .with_field("extra", EventFieldValue::Integer(1));
        let err = EventRegistry::validate(&event).unwrap_err();
        assert_eq!(
            err,
            RegistryError::UnknownField {
                kind: OperationEventKind::Rejected,
                field: "extra".to_string()
            }
        );
    }

    #[test]
    fn private_declared_field_supplied_as_public_string_is_rejected() {
        let event = OperationEvent::new(OperationEventKind::StageStarted)
            .with_field("stage", EventFieldValue::PublicString("parse".into()))
            .with_field("host_path", EventFieldValue::PublicString("/etc/passwd".into()));
        let err = EventRegistry::validate(&event).unwrap_err();
        assert_eq!(
            err,
            RegistryError::PrivacyMismatch {
                kind: OperationEventKind::StageStarted,
                field: "host_path".to_string(),
                expected: FieldPrivacy::Private,
                got: FieldPrivacy::Public,
            }
        );
    }

    #[test]
    fn secret_declared_field_supplied_as_public_string_is_rejected() {
        let event = OperationEvent::new(OperationEventKind::StageStarted)
            .with_field("stage", EventFieldValue::PublicString("parse".into()))
            .with_field("api_key_hint", EventFieldValue::PublicString("sk-abc123".into()));
        let err = EventRegistry::validate(&event).unwrap_err();
        assert_eq!(
            err,
            RegistryError::PrivacyMismatch {
                kind: OperationEventKind::StageStarted,
                field: "api_key_hint".to_string(),
                expected: FieldPrivacy::Secret,
                got: FieldPrivacy::Public,
            }
        );
    }

    /// A `Secret`-declared field supplied as `Private` must also be
    /// rejected: `Private` still discloses a byte length, which is exactly
    /// the property `Secret` exists to withhold. A registry that only
    /// checked "is this redacted at all" would let this through and leak a
    /// low-entropy secret's length.
    #[test]
    fn secret_declared_field_supplied_as_private_is_rejected() {
        let event = OperationEvent::new(OperationEventKind::StageStarted)
            .with_field("stage", EventFieldValue::PublicString("parse".into()))
            .with_field("api_key_hint", EventFieldValue::Private(PrivateValue::new("sk-abc123")));
        let err = EventRegistry::validate(&event).unwrap_err();
        assert_eq!(
            err,
            RegistryError::PrivacyMismatch {
                kind: OperationEventKind::StageStarted,
                field: "api_key_hint".to_string(),
                expected: FieldPrivacy::Secret,
                got: FieldPrivacy::Private,
            }
        );
    }

    #[test]
    fn well_formed_event_at_every_declared_tier_is_valid() {
        let event = OperationEvent::new(OperationEventKind::StageStarted)
            .with_field("stage", EventFieldValue::PublicString("parse".into()))
            .with_field(
                "host_path",
                EventFieldValue::Private(PrivateValue::new("/home/alice/proj")),
            )
            .with_field("source_line", EventFieldValue::Private(PrivateValue::new("my $x = 1;")))
            .with_field("api_key_hint", EventFieldValue::Secret(SecretField::new("sk-abc123")));
        assert!(EventRegistry::validate(&event).is_ok());
    }

    #[test]
    fn terminal_event_built_via_constructor_is_valid() {
        use crate::event::OperationOutcome;
        let event = OperationEvent::terminal(OperationOutcome::Completed);
        assert!(EventRegistry::validate(&event).is_ok());
    }
}
