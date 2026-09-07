//! Negative control: ephemeral identity must not contaminate domain content.
//!
//! `OperationId` exists purely for correlation. If a future change ever
//! folded an operation's id (or its parent's id) into the event fields
//! themselves, two operations recording identical domain material under
//! different ids would stop producing byte-identical serialized event
//! payloads. This test would fail the moment that happened.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use perl_operation_trace::{
    EventFieldValue, OperationContext, OperationEvent, OperationEventKind, OperationIdAllocator,
    OperationKind, OperationRecorder, RecorderBounds, SessionId,
};

fn domain_event() -> OperationEvent {
    OperationEvent::new(OperationEventKind::StageStarted)
        .with_field("stage", EventFieldValue::PublicString("parse".into()))
}

#[test]
fn identical_domain_material_serializes_identically_under_different_operation_ids() {
    let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000));

    let mut allocator_a = OperationIdAllocator::new(SessionId::new("session-a").expect("literal"));
    let mut allocator_b = OperationIdAllocator::new(SessionId::new("session-b").expect("literal"));

    // Both contexts share the same kind and no parent — the only thing that
    // differs is the OperationId itself.
    let context_a = OperationContext::root(allocator_a.next(), OperationKind::WorkspaceIndexing);
    let context_b = OperationContext::root(allocator_b.next(), OperationKind::WorkspaceIndexing);

    // Sanity: the two operations really do have different ids, otherwise
    // this test would prove nothing.
    assert_ne!(context_a.operation(), context_b.operation());

    recorder.record(&context_a, domain_event()).expect("valid event");
    recorder.record(&context_b, domain_event()).expect("valid event");

    let snapshot = recorder.snapshot();
    let recorded_a = snapshot
        .operations
        .iter()
        .find(|op| &op.operation == context_a.operation())
        .expect("operation_a was recorded");
    let recorded_b = snapshot
        .operations
        .iter()
        .find(|op| &op.operation == context_b.operation())
        .expect("operation_b was recorded");

    // The operation ids themselves differ (this is expected and fine)...
    assert_ne!(recorded_a.operation, recorded_b.operation);
    // ...and they share the same kind, isolating "only the id differs" as
    // the actual variable under test.
    assert_eq!(recorded_a.kind, recorded_b.kind);

    // Non-vacuity guard. Everything below compares the two domain payloads
    // and asserts absences in them; all of that is trivially true of an
    // empty payload. So first prove there *is* a payload: if the recorder
    // ever silently dropped event bodies, this test would otherwise still
    // pass while proving nothing at all.
    assert_eq!(recorded_a.events.len(), 1, "expected exactly the one recorded domain event");
    assert_eq!(recorded_b.events.len(), 1, "expected exactly the one recorded domain event");

    // ...but the *domain payload* — the events, independent of which
    // operation recorded them — must be byte-identical. This is the
    // property that would break if an id (or a kind, or a parent) were ever
    // folded into event content: the serialized `events` field would then
    // differ between the two operations even though the caller supplied
    // identical domain material.
    let events_a = serde_json::to_string(&recorded_a.events).expect("serialize");
    let events_b = serde_json::to_string(&recorded_b.events).expect("serialize");

    // Second half of the non-vacuity guard: the serialized payload must
    // actually carry the field content the fixture supplied, so that the
    // equality and absence assertions below are made against real material.
    for events in [&events_a, &events_b] {
        assert!(events.contains("StageStarted"), "payload lost its event kind: {events}");
        assert!(events.contains("stage"), "payload lost its field name: {events}");
        assert!(events.contains("parse"), "payload lost its public field value: {events}");
    }
    assert_eq!(
        events_a, events_b,
        "domain event payload must not depend on which OperationId recorded it"
    );

    // And neither operation id's wire string leaked into the other's event
    // payload (or its own) — a stronger, more direct check than payload
    // equality alone.
    let wire_a = context_a.operation().as_wire();
    let wire_b = context_b.operation().as_wire();
    assert!(!events_a.contains(&wire_a));
    assert!(!events_a.contains(&wire_b));
    assert!(!events_b.contains(&wire_a));
    assert!(!events_b.contains(&wire_b));
}

/// This crate exposes no API that folds an `OperationId` (or a
/// `SessionId`/`ParentOperationId`) into a digest or fingerprint: there is no
/// `sha2` dependency to build one with (see `tests/dependency_contract.rs`),
/// and no type in the public surface returns anything resembling a durable
/// `sha256:`-prefixed identity from an `OperationId`. The strongest
/// executable proof of "no such API exists" is the dependency-closure
/// allowlist test; this test additionally pins the property at the value
/// level: an `OperationId`'s own wire form never begins with the durable
/// `sha256:` prefix `perl-source-identity` uses.
#[test]
fn operation_id_wire_form_never_looks_like_durable_identity() {
    let mut allocator = OperationIdAllocator::new(SessionId::new("s1").expect("literal"));
    let operation = allocator.next();
    assert!(!operation.as_wire().starts_with("sha256:"));
    assert!(operation.as_wire().starts_with("op:"));
}
