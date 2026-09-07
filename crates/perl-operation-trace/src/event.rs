//! Structured operation events: a closed event-kind vocabulary, bounded
//! privacy-classified fields, and the terminal-outcome vocabulary.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::privacy::{FieldPrivacy, PrivateValue, SecretField};

/// Closed vocabulary of event kinds an operation may emit.
///
/// Closed for the same reason as [`crate::OperationKind`]: an event kind is
/// part of the wire contract downstream consumers pattern-match on, so an
/// unrecognized kind must fail the decode rather than silently becoming some
/// default event a consumer treats as meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OperationEventKind {
    /// The operation was admitted for execution.
    Admitted,
    /// The operation was rejected before execution.
    Rejected,
    /// A generation/freshness cursor was selected.
    GenerationSelected,
    /// A pipeline stage started.
    StageStarted,
    /// A pipeline stage completed.
    StageCompleted,
    /// A producer implementation was selected.
    ProducerSelected,
    /// A cache lookup hit.
    CacheHit,
    /// A cache lookup missed.
    CacheMiss,
    /// A stale publication attempt was rejected.
    StalePublicationRejected,
    /// A fallback path was selected.
    FallbackSelected,
    /// A child process was planned.
    ProcessPlanned,
    /// A child process started.
    ProcessStarted,
    /// A child process terminated.
    ProcessTerminated,
    /// The operation was cancelled.
    Cancelled,
    /// The operation exceeded its deadline.
    DeadlineExceeded,
    /// A receipt was emitted for the operation.
    ReceiptEmitted,
    /// The operation reached a terminal state.
    ///
    /// Exactly one per operation — enforced by [`crate::OperationRecorder`],
    /// not by this type.
    Terminal,
}

impl fmt::Display for OperationEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Admitted => "admitted",
            Self::Rejected => "rejected",
            Self::GenerationSelected => "generation_selected",
            Self::StageStarted => "stage_started",
            Self::StageCompleted => "stage_completed",
            Self::ProducerSelected => "producer_selected",
            Self::CacheHit => "cache_hit",
            Self::CacheMiss => "cache_miss",
            Self::StalePublicationRejected => "stale_publication_rejected",
            Self::FallbackSelected => "fallback_selected",
            Self::ProcessPlanned => "process_planned",
            Self::ProcessStarted => "process_started",
            Self::ProcessTerminated => "process_terminated",
            Self::Cancelled => "cancelled",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::ReceiptEmitted => "receipt_emitted",
            Self::Terminal => "terminal",
        };
        f.write_str(s)
    }
}

/// The terminal outcome of an operation, carried by the
/// [`OperationEventKind::Terminal`] event's [`OUTCOME_FIELD`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperationOutcome {
    /// The operation ran to completion successfully.
    Completed,
    /// The operation ran and failed.
    Failed,
    /// The operation was cancelled before completion.
    Cancelled,
    /// The operation was superseded by a later one.
    Superseded,
    /// The operation was rejected before it started.
    Rejected,
}

impl OperationOutcome {
    /// The stable wire string for this outcome.
    ///
    /// This crate has no `serde_json` (or any JSON library) dependency in
    /// production code — see `tests/dependency_contract.rs` — so the
    /// `outcome` field's value is this hand-written string rather than a
    /// nested `serde`-nested representation. [`Self::from_wire_str`] is its
    /// exact inverse.
    #[must_use]
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Superseded => "superseded",
            Self::Rejected => "rejected",
        }
    }

    /// Parse an outcome from its wire string. Closed vocabulary: anything
    /// else returns `None`.
    #[must_use]
    pub fn from_wire_str(s: &str) -> Option<Self> {
        match s {
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "superseded" => Some(Self::Superseded),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

impl fmt::Display for OperationOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

/// One structured field value on an [`OperationEvent`].
///
/// # Privacy tiers
///
/// Three tiers — see `crate::privacy` for the full reasoning:
///
/// - Public ([`Self::Integer`], [`Self::Boolean`], [`Self::PublicString`]):
///   appears verbatim in a serialized trace.
/// - Private ([`Self::Private`]): redacted, byte length disclosed.
/// - Secret ([`Self::Secret`]): redacted, nothing disclosed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum EventFieldValue {
    /// A public integer.
    Integer(i64),
    /// A public boolean.
    Boolean(bool),
    /// A short public string.
    PublicString(String),
    /// A private value: redacted, length disclosed.
    Private(PrivateValue),
    /// A secret value: redacted, nothing disclosed.
    Secret(SecretField),
}

impl EventFieldValue {
    /// The privacy tier this value belongs to.
    #[must_use]
    pub fn privacy(&self) -> FieldPrivacy {
        match self {
            Self::Secret(_) => FieldPrivacy::Secret,
            Self::Private(_) => FieldPrivacy::Private,
            Self::Integer(_) | Self::Boolean(_) | Self::PublicString(_) => FieldPrivacy::Public,
        }
    }

    /// An approximate payload size in bytes, used by
    /// [`crate::OperationRecorder`]'s `max_payload_bytes` bound.
    ///
    /// This counts the *actual* underlying byte length of private and
    /// secret values for the recorder's own bound accounting (which never
    /// serializes them), not their redacted wire length.
    pub(crate) fn approx_payload_len(&self) -> usize {
        match self {
            Self::Integer(_) => std::mem::size_of::<i64>(),
            Self::Boolean(_) => std::mem::size_of::<bool>(),
            Self::PublicString(s) => s.len(),
            Self::Private(p) => p.len(),
            Self::Secret(s) => s.len(),
        }
    }
}

/// Name of the reserved field a [`OperationEventKind::Terminal`] event uses
/// to carry its [`OperationOutcome`].
pub const OUTCOME_FIELD: &str = "outcome";

/// One structured event on an operation's trace.
///
/// An event is its closed [`OperationEventKind`] plus an ordered list of
/// `(field name, value)` pairs. Order is preserved (a `Vec`, not a map) so
/// serialization stays deterministic without relying on a stable hash order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OperationEvent {
    kind: OperationEventKind,
    fields: Vec<(String, EventFieldValue)>,
}

impl OperationEvent {
    /// Start an event of the given kind with no fields.
    #[must_use]
    pub fn new(kind: OperationEventKind) -> Self {
        Self { kind, fields: Vec::new() }
    }

    /// Append a field, preserving insertion order. Builder-style.
    #[must_use]
    pub fn with_field(mut self, name: impl Into<String>, value: EventFieldValue) -> Self {
        self.fields.push((name.into(), value));
        self
    }

    /// Construct the reserved [`OperationEventKind::Terminal`] event
    /// carrying `outcome`.
    #[must_use]
    pub fn terminal(outcome: OperationOutcome) -> Self {
        Self::new(OperationEventKind::Terminal).with_field(
            OUTCOME_FIELD,
            EventFieldValue::PublicString(outcome.as_wire_str().to_owned()),
        )
    }

    /// This event's kind.
    #[must_use]
    pub fn kind(&self) -> OperationEventKind {
        self.kind
    }

    /// This event's fields, in insertion order.
    #[must_use]
    pub fn fields(&self) -> &[(String, EventFieldValue)] {
        &self.fields
    }

    /// The [`OperationOutcome`] carried by this event, if it is a
    /// well-formed [`OperationEventKind::Terminal`] event.
    ///
    /// Returns `None` for a non-`Terminal` event, a `Terminal` event with no
    /// [`OUTCOME_FIELD`], or one whose value is not one of
    /// [`OperationOutcome`]'s wire strings. [`crate::OperationRecorder`]
    /// treats all three the same way: a contract violation, not a silently
    /// tolerated terminal state.
    #[must_use]
    pub fn outcome(&self) -> Option<OperationOutcome> {
        if self.kind != OperationEventKind::Terminal {
            return None;
        }
        self.fields.iter().find_map(|(name, value)| {
            if name != OUTCOME_FIELD {
                return None;
            }
            match value {
                EventFieldValue::PublicString(s) => OperationOutcome::from_wire_str(s),
                _ => None,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const ALL_EVENT_KINDS: [OperationEventKind; 17] = [
        OperationEventKind::Admitted,
        OperationEventKind::Rejected,
        OperationEventKind::GenerationSelected,
        OperationEventKind::StageStarted,
        OperationEventKind::StageCompleted,
        OperationEventKind::ProducerSelected,
        OperationEventKind::CacheHit,
        OperationEventKind::CacheMiss,
        OperationEventKind::StalePublicationRejected,
        OperationEventKind::FallbackSelected,
        OperationEventKind::ProcessPlanned,
        OperationEventKind::ProcessStarted,
        OperationEventKind::ProcessTerminated,
        OperationEventKind::Cancelled,
        OperationEventKind::DeadlineExceeded,
        OperationEventKind::ReceiptEmitted,
        OperationEventKind::Terminal,
    ];

    const ALL_OUTCOMES: [OperationOutcome; 5] = [
        OperationOutcome::Completed,
        OperationOutcome::Failed,
        OperationOutcome::Cancelled,
        OperationOutcome::Superseded,
        OperationOutcome::Rejected,
    ];

    // ── OperationEventKind ────────────────────────────────────────────────

    #[test]
    fn event_kind_serde_round_trip_is_lossless() {
        for kind in ALL_EVENT_KINDS {
            let json = serde_json::to_string(&kind).unwrap();
            let back: OperationEventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn unknown_event_kind_fails_closed() {
        for bad in ["\"NotAKind\"", "\"\"", "\"terminal\"", "17"] {
            assert!(
                serde_json::from_str::<OperationEventKind>(bad).is_err(),
                "must reject {bad}, not silently decode a default variant"
            );
        }
    }

    // ── OperationOutcome ──────────────────────────────────────────────────

    #[test]
    fn outcome_wire_str_round_trips_for_every_variant() {
        for outcome in ALL_OUTCOMES {
            let wire = outcome.as_wire_str();
            assert_eq!(OperationOutcome::from_wire_str(wire), Some(outcome));
        }
    }

    #[test]
    fn outcome_from_wire_str_rejects_unknown_strings() {
        for bad in ["", "Completed", "done", "completed "] {
            assert_eq!(OperationOutcome::from_wire_str(bad), None, "must reject {bad:?}");
        }
    }

    #[test]
    fn outcome_serde_round_trip_is_lossless() {
        for outcome in ALL_OUTCOMES {
            let json = serde_json::to_string(&outcome).unwrap();
            let back: OperationOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(outcome, back);
        }
    }

    #[test]
    fn unknown_outcome_fails_closed() {
        assert!(serde_json::from_str::<OperationOutcome>("\"done\"").is_err());
    }

    // ── EventFieldValue privacy classification ───────────────────────────

    #[test]
    fn field_value_privacy_classification() {
        assert_eq!(EventFieldValue::Integer(1).privacy(), FieldPrivacy::Public);
        assert_eq!(EventFieldValue::Boolean(true).privacy(), FieldPrivacy::Public);
        assert_eq!(EventFieldValue::PublicString("x".into()).privacy(), FieldPrivacy::Public);
        assert_eq!(
            EventFieldValue::Private(PrivateValue::new("x")).privacy(),
            FieldPrivacy::Private
        );
        assert_eq!(EventFieldValue::Secret(SecretField::new("x")).privacy(), FieldPrivacy::Secret);
    }

    // ── OperationEvent ────────────────────────────────────────────────────

    #[test]
    fn builder_preserves_field_insertion_order() {
        let event = OperationEvent::new(OperationEventKind::StageStarted)
            .with_field("stage", EventFieldValue::PublicString("parse".into()))
            .with_field("attempt", EventFieldValue::Integer(2));
        let names: Vec<&str> = event.fields().iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["stage", "attempt"]);
    }

    #[test]
    fn terminal_constructor_sets_kind_and_outcome_field() {
        let event = OperationEvent::terminal(OperationOutcome::Completed);
        assert_eq!(event.kind(), OperationEventKind::Terminal);
        assert_eq!(event.outcome(), Some(OperationOutcome::Completed));
    }

    #[test]
    fn outcome_is_none_for_non_terminal_events() {
        let event = OperationEvent::new(OperationEventKind::Admitted);
        assert_eq!(event.outcome(), None);
    }

    #[test]
    fn outcome_is_none_for_terminal_event_missing_the_field() {
        let event = OperationEvent::new(OperationEventKind::Terminal);
        assert_eq!(event.outcome(), None, "no outcome field at all must not silently succeed");
    }

    #[test]
    fn outcome_is_none_for_terminal_event_with_malformed_value() {
        let event = OperationEvent::new(OperationEventKind::Terminal)
            .with_field(OUTCOME_FIELD, EventFieldValue::PublicString("not-a-real-outcome".into()));
        assert_eq!(event.outcome(), None);
    }

    #[test]
    fn outcome_is_none_when_the_field_is_the_wrong_privacy_tier() {
        // A Terminal event whose `outcome` field was wrapped as private
        // instead of a plain public string must not silently parse.
        let event = OperationEvent::new(OperationEventKind::Terminal)
            .with_field(OUTCOME_FIELD, EventFieldValue::Private(PrivateValue::new("completed")));
        assert_eq!(event.outcome(), None);
    }
}
