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
// tier (`stage` public, `host_path` private, `source_line`/`api_key_hint`
// secret) so producers have one obvious event to attach high-entropy or
// secret context to, and so the redaction contract has one concrete event
// kind to exercise end to end. `source_line` is `Secret`, not `Private`: see
// `crate::privacy`'s module docs for why a line of real source text is
// frequently low-entropy (`}`, `1;`, `);`, blank) and therefore belongs in
// the tier that discloses no length, not the one that does.
const ADMITTED_FIELDS: [FieldSpec; 0] = [];
const REJECTED_FIELDS: [FieldSpec; 1] = [field("reason", FieldPrivacy::Public, true)];
const GENERATION_SELECTED_FIELDS: [FieldSpec; 1] =
    [field("generation", FieldPrivacy::Public, true)];
const STAGE_STARTED_FIELDS: [FieldSpec; 4] = [
    field("stage", FieldPrivacy::Public, true),
    field("host_path", FieldPrivacy::Private, false),
    field("source_line", FieldPrivacy::Secret, false),
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
    /// The same declared field name was supplied more than once on one
    /// event.
    ///
    /// A repeated field name is not merely redundant: each occurrence still
    /// contributes its own name length and approximate payload length to
    /// [`crate::OperationRecorder`]'s `max_payload_bytes` accounting (see
    /// `crate::recorder::event_payload_len`), while serde's externally
    /// tagged encoding adds a per-value wrapper this accounting does not
    /// count. Hundreds of duplicate low-cost fields can therefore pass the
    /// approximate budget check while the actual serialized trace overshoots
    /// it by several times over. Rejecting a duplicate field name outright
    /// is what keeps that approximation honest.
    DuplicateField {
        /// The event kind the duplicate was supplied on.
        kind: OperationEventKind,
        /// The repeated field name. Never a value: `Display` must not risk
        /// echoing a `Private`/`Secret` field's content just because it
        /// happened to collide with itself.
        field: String,
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
            Self::DuplicateField { kind, field } => {
                write!(f, "event kind {kind} declares field {field:?} more than once")
            }
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
    /// name the event kind does not declare, a declared field supplied at
    /// the wrong privacy tier (in either direction — a `Secret`-declared
    /// field must not be supplied as `Private` either, since `Private`
    /// still leaks a byte length), or the same declared field name supplied
    /// more than once (see [`RegistryError::DuplicateField`]).
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

        let mut seen_fields: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (name, value) in event.fields() {
            if !seen_fields.insert(name.as_str()) {
                return Err(RegistryError::DuplicateField { kind, field: name.clone() });
            }
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

    /// This crate's core contract data — `specs`, backed by the
    /// per-event-kind `*_FIELDS` consts — is otherwise only ever exercised
    /// *indirectly*, through whether a hand-built `OperationEvent` passes or
    /// fails validation. Nothing previously asserted the declared table
    /// itself: which fields each of the 17 event kinds declares, at which
    /// [`FieldPrivacy`] tier, and which are `required`. Flipping `host_path`
    /// from `Private` to `Public`, or clearing `stage`'s `required` flag,
    /// would go undetected by every other test in this file, because most
    /// of them build events that are already well-formed under the *current*
    /// table and only check that validation accepts or rejects — they never
    /// read the table's declared shape back out. This test does: for every
    /// event kind, it asserts the exact ordered list of `(name, privacy,
    /// required)` triples `specs` returns, so a change to any single
    /// declared field's name, tier, or requiredness breaks exactly one row.
    #[test]
    fn registry_declares_the_exact_field_table_for_every_event_kind() {
        use FieldPrivacy::{Private, Public, Secret};
        use OperationEventKind as K;

        let expected: [(K, &[(&str, FieldPrivacy, bool)]); 17] = [
            (K::Admitted, &[]),
            (K::Rejected, &[("reason", Public, true)]),
            (K::GenerationSelected, &[("generation", Public, true)]),
            (
                K::StageStarted,
                &[
                    ("stage", Public, true),
                    ("host_path", Private, false),
                    ("source_line", Secret, false),
                    ("api_key_hint", Secret, false),
                ],
            ),
            (K::StageCompleted, &[("stage", Public, true)]),
            (K::ProducerSelected, &[("producer", Public, true)]),
            (K::CacheHit, &[("cache_key", Public, true)]),
            (K::CacheMiss, &[("cache_key", Public, true)]),
            (K::StalePublicationRejected, &[("reason", Public, true)]),
            (K::FallbackSelected, &[("fallback", Public, true)]),
            (
                K::ProcessPlanned,
                &[("executable_logical_name", Public, true), ("argv_digest_hint", Private, false)],
            ),
            (K::ProcessStarted, &[("pid_hint", Public, false)]),
            (K::ProcessTerminated, &[("exit_hint", Public, false)]),
            (K::Cancelled, &[("reason", Public, false)]),
            (K::DeadlineExceeded, &[("deadline_ms", Public, false)]),
            (K::ReceiptEmitted, &[("receipt_ref", Public, true)]),
            (K::Terminal, &[(OUTCOME_FIELD, Public, true)]),
        ];

        for (kind, fields) in expected {
            let declared = specs(kind);
            let actual: Vec<(&str, FieldPrivacy, bool)> =
                declared.iter().map(|spec| (spec.name, spec.privacy, spec.required)).collect();
            assert_eq!(actual, fields, "declared field table mismatch for {kind:?}");
        }
    }

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
            .with_field("source_line", EventFieldValue::Secret(SecretField::new("my $x = 1;")))
            .with_field("api_key_hint", EventFieldValue::Secret(SecretField::new("sk-abc123")));
        assert!(EventRegistry::validate(&event).is_ok());
    }

    #[test]
    fn terminal_event_built_via_constructor_is_valid() {
        use crate::event::OperationOutcome;
        let event = OperationEvent::terminal(OperationOutcome::Completed);
        assert!(EventRegistry::validate(&event).is_ok());
    }

    /// Reproduces the probe that motivated [`RegistryError::DuplicateField`]:
    /// 500 repetitions of the same declared field name (`"stage": true`, each
    /// individually well-formed and at the right privacy tier) previously
    /// passed field-level validation entirely, because nothing checked
    /// whether a name had already been seen on this event. Once accepted,
    /// each repetition's name length and `approx_payload_len()` still counted
    /// toward `OperationRecorder`'s `max_payload_bytes` budget, but serde's
    /// externally tagged encoding adds a per-value wrapper that accounting
    /// never counted — in the original probe the actual serialized trace
    /// overshot the configured budget by 4.57x. Rejecting the very first
    /// duplicate closes that gap at the registry, before the recorder's
    /// budget accounting ever runs.
    #[test]
    fn five_hundred_duplicate_stage_fields_are_rejected_not_merely_over_budget() {
        let mut event = OperationEvent::new(OperationEventKind::StageStarted);
        for _ in 0..500 {
            event = event.with_field("stage", EventFieldValue::Boolean(true));
        }
        let err = EventRegistry::validate(&event).unwrap_err();
        assert_eq!(
            err,
            RegistryError::DuplicateField {
                kind: OperationEventKind::StageStarted,
                field: "stage".to_string(),
            }
        );
    }

    /// Exhaustive over every [`RegistryError`] variant: pins `Display`'s
    /// exact text for each, rather than the existing per-variant tests each
    /// checking a *different, single* variant's structural equality only.
    #[test]
    fn registry_error_display_matches_the_exact_documented_string_for_every_variant() {
        let missing = RegistryError::MissingRequiredField {
            kind: OperationEventKind::Rejected,
            field: "reason".to_string(),
        };
        assert_eq!(missing.to_string(), "event kind rejected is missing required field \"reason\"");

        let unknown = RegistryError::UnknownField {
            kind: OperationEventKind::Rejected,
            field: "extra".to_string(),
        };
        assert_eq!(unknown.to_string(), "event kind rejected does not declare field \"extra\"");

        let mismatch = RegistryError::PrivacyMismatch {
            kind: OperationEventKind::StageStarted,
            field: "host_path".to_string(),
            expected: FieldPrivacy::Private,
            got: FieldPrivacy::Public,
        };
        assert_eq!(
            mismatch.to_string(),
            "event kind stage_started field \"host_path\" is declared private but was supplied as public"
        );

        let duplicate = RegistryError::DuplicateField {
            kind: OperationEventKind::Terminal,
            field: "outcome".to_string(),
        };
        assert_eq!(
            duplicate.to_string(),
            "event kind terminal declares field \"outcome\" more than once"
        );
    }

    #[test]
    fn duplicate_field_display_names_only_the_field_not_a_value() {
        let err = RegistryError::DuplicateField {
            kind: OperationEventKind::StageStarted,
            field: "stage".to_string(),
        };
        let message = err.to_string();
        assert!(message.contains("stage"));
        assert!(message.contains("stage_started"));
    }

    /// Sharper duplicate-field example: a `Terminal` event carrying
    /// `outcome=completed` followed by `outcome=failed`. Before duplicate
    /// -field rejection existed, this passed field-level validation
    /// entirely (each individual `outcome` value is well-formed and at the
    /// right privacy tier), `OperationEvent::outcome()` silently returned
    /// only the *first* one (`completed`), and the serialized trace would
    /// retain *both* fields -- leaving a consumer with no canonical
    /// terminal result and a trace that visibly disagrees with itself about
    /// how the operation ended.
    #[test]
    fn terminal_event_with_two_different_outcome_values_is_rejected_as_duplicate() {
        use crate::event::OUTCOME_FIELD;
        let event = OperationEvent::new(OperationEventKind::Terminal)
            .with_field(OUTCOME_FIELD, EventFieldValue::PublicString("completed".into()))
            .with_field(OUTCOME_FIELD, EventFieldValue::PublicString("failed".into()));
        let err = EventRegistry::validate(&event).unwrap_err();
        assert_eq!(
            err,
            RegistryError::DuplicateField {
                kind: OperationEventKind::Terminal,
                field: OUTCOME_FIELD.to_string(),
            }
        );
    }
}
