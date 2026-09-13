//! Negative control: ephemeral identity must not contaminate domain content.
//!
//! `OperationId` exists purely for correlation. If a future change ever
//! folded an operation's id, its parent's id, or its `OperationKind` into the
//! event fields themselves, two operations recording identical domain
//! material that differ in exactly one of those three properties would stop
//! producing byte-identical serialized event payloads. These tests would
//! fail the moment that happened. Each test below varies exactly one of the
//! three properties (id, kind, parent) while holding the other two fixed —
//! sharing a kind across the id-varying case (as here) sharpens that one
//! proof, but only proves independence from `OperationId`, not from `kind`
//! or `parent` on its own; the two additional tests close that gap.
//!
//! This crate's fallible test convention: as an integration test file (not
//! an in-module `src/*.rs` unit test), this file does not carry a file-level
//! `#![allow(clippy::unwrap_used, clippy::expect_used)]`. Every test returns
//! `Result<(), Box<dyn std::error::Error>>` and uses `?` (or a `map_err` that
//! preserves the original diagnostic context, matching #14825's "without
//! losing context" principle) instead of `.unwrap()`/`.expect()`.
//! `assert!`/`assert_eq!` remain: they are the actual property checks, not
//! fallible setup, and the tests would be strictly weaker without them.

use perl_operation_trace::{
    EventFieldValue, OperationContext, OperationEvent, OperationEventKind, OperationId,
    OperationIdAllocator, OperationKind, OperationRecorder, RecorderBounds, SessionId,
};

fn domain_event() -> OperationEvent {
    OperationEvent::new(OperationEventKind::StageStarted)
        .with_field("stage", EventFieldValue::PublicString("parse".into()))
}

/// The serialized `events` payload this crate recorded for `operation` under
/// `recorder`, with the non-vacuity guard (exactly one recorded event) baked
/// in so every caller gets it for free.
fn recorded_events_json(
    recorder: &OperationRecorder,
    operation: &OperationId,
) -> Result<String, Box<dyn std::error::Error>> {
    let snapshot = recorder.snapshot();
    let recorded = snapshot
        .operations
        .iter()
        .find(|op| &op.operation == operation)
        .ok_or("operation was not present in the snapshot")?;
    // Non-vacuity guard. Everything the callers of this helper do next
    // compares two domain payloads and asserts absences in them; all of
    // that is trivially true of an empty payload. So first prove there *is*
    // a payload: if the recorder ever silently dropped event bodies, a test
    // built on this helper would otherwise still pass while proving nothing
    // at all.
    assert_eq!(recorded.events.len(), 1, "expected exactly the one recorded domain event");
    serde_json::to_string(&recorded.events)
        .map_err(|e| format!("serialize recorded events: {e}").into())
}

/// Second half of the non-vacuity guard: the serialized payload must
/// actually carry the field content [`domain_event`] supplied, so that
/// equality and absence assertions made against it are made against real
/// material.
fn assert_domain_payload_is_non_vacuous(json: &str) {
    assert!(json.contains("StageStarted"), "payload lost its event kind: {json}");
    assert!(json.contains("stage"), "payload lost its field name: {json}");
    assert!(json.contains("parse"), "payload lost its public field value: {json}");
}

#[test]
fn identical_domain_material_serializes_identically_under_different_operation_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let mut recorder = OperationRecorder::new(RecorderBounds::new(100, 10_000, 100));

    let mut allocator_a = OperationIdAllocator::new(
        SessionId::new("session-a").map_err(|e| format!("build fixture session id: {e}"))?,
    );
    let mut allocator_b = OperationIdAllocator::new(
        SessionId::new("session-b").map_err(|e| format!("build fixture session id: {e}"))?,
    );

    // Both contexts share the same kind and no parent — the only thing that
    // differs is the OperationId itself.
    let context_a = OperationContext::root(
        allocator_a.next().ok_or("fresh allocator unexpectedly exhausted its sequence space")?,
        OperationKind::WorkspaceIndexing,
    );
    let context_b = OperationContext::root(
        allocator_b.next().ok_or("fresh allocator unexpectedly exhausted its sequence space")?,
        OperationKind::WorkspaceIndexing,
    );

    // Sanity: the two operations really do have different ids, otherwise
    // this test would prove nothing.
    assert_ne!(context_a.operation(), context_b.operation());

    recorder.record(&context_a, domain_event()).map_err(|e| format!("record context a: {e}"))?;
    recorder.record(&context_b, domain_event()).map_err(|e| format!("record context b: {e}"))?;

    let events_a = recorded_events_json(&recorder, context_a.operation())?;
    let events_b = recorded_events_json(&recorder, context_b.operation())?;
    assert_domain_payload_is_non_vacuous(&events_a);
    assert_domain_payload_is_non_vacuous(&events_b);

    // ...but the *domain payload* — the events, independent of which
    // operation recorded them — must be byte-identical. This is the
    // property that would break if an id were ever folded into event
    // content: the serialized `events` field would then differ between the
    // two operations even though the caller supplied identical domain
    // material.
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

    Ok(())
}

/// Companion to the id-varying test above: holds the operation id and the
/// (absent) parent fixed and varies only [`OperationKind`]. Two *separate*
/// recorders are required — recording two different kinds against the same
/// operation id in one recorder is itself rejected as a `KindConflict`
/// (see `src/recorder.rs`'s tests), a different, already-covered property.
#[test]
fn identical_domain_material_serializes_identically_under_different_kinds()
-> Result<(), Box<dyn std::error::Error>> {
    let operation = OperationId::new(
        SessionId::new("kind-fixture").map_err(|e| format!("build fixture session id: {e}"))?,
        0,
    );

    let context_a = OperationContext::root(operation.clone(), OperationKind::WorkspaceIndexing);
    let context_b = OperationContext::root(operation.clone(), OperationKind::LspRequest);

    // Sanity: same operation id, no parent on either side; only the kind
    // differs.
    assert_eq!(context_a.operation(), &operation);
    assert_eq!(context_b.operation(), &operation);
    assert_eq!(context_a.parent(), None);
    assert_eq!(context_b.parent(), None);
    assert_ne!(context_a.kind(), context_b.kind());

    let mut recorder_a = OperationRecorder::new(RecorderBounds::new(100, 10_000, 100));
    let mut recorder_b = OperationRecorder::new(RecorderBounds::new(100, 10_000, 100));
    recorder_a
        .record(&context_a, domain_event())
        .map_err(|e| format!("record under kind a: {e}"))?;
    recorder_b
        .record(&context_b, domain_event())
        .map_err(|e| format!("record under kind b: {e}"))?;

    let events_a = recorded_events_json(&recorder_a, &operation)?;
    let events_b = recorded_events_json(&recorder_b, &operation)?;
    assert_domain_payload_is_non_vacuous(&events_a);
    assert_domain_payload_is_non_vacuous(&events_b);
    assert_eq!(
        events_a, events_b,
        "domain event payload must not depend on the operation's OperationKind"
    );

    Ok(())
}

/// Companion to the id-varying test above: holds the operation id and its
/// kind fixed and varies only the parent. Two *separate* recorders are
/// required — recording two different parents for the same operation id in
/// one recorder is itself rejected as a `ParentConflict`, a different,
/// already-covered property.
#[test]
fn identical_domain_material_serializes_identically_under_different_parents()
-> Result<(), Box<dyn std::error::Error>> {
    let session =
        SessionId::new("parent-fixture").map_err(|e| format!("build fixture session id: {e}"))?;
    let operation = OperationId::new(session.clone(), 1);
    let parent_a = OperationId::new(session.clone(), 0);
    let parent_b = OperationId::new(
        SessionId::new("parent-fixture-other")
            .map_err(|e| format!("build fixture session id: {e}"))?,
        0,
    );

    let context_a = OperationContext::root(parent_a, OperationKind::WorkspaceIndexing)
        .child(operation.clone(), OperationKind::WorkspaceIndexing);
    let context_b = OperationContext::root(parent_b, OperationKind::WorkspaceIndexing)
        .child(operation.clone(), OperationKind::WorkspaceIndexing);

    // Sanity: same operation id and kind on both sides; only the parent
    // differs.
    assert_eq!(context_a.operation(), &operation);
    assert_eq!(context_b.operation(), &operation);
    assert_eq!(context_a.kind(), context_b.kind());
    assert_ne!(
        context_a.parent().map(|p| p.operation_id()),
        context_b.parent().map(|p| p.operation_id())
    );

    let mut recorder_a = OperationRecorder::new(RecorderBounds::new(100, 10_000, 100));
    let mut recorder_b = OperationRecorder::new(RecorderBounds::new(100, 10_000, 100));
    recorder_a
        .record(&context_a, domain_event())
        .map_err(|e| format!("record under parent a: {e}"))?;
    recorder_b
        .record(&context_b, domain_event())
        .map_err(|e| format!("record under parent b: {e}"))?;

    let events_a = recorded_events_json(&recorder_a, &operation)?;
    let events_b = recorded_events_json(&recorder_b, &operation)?;
    assert_domain_payload_is_non_vacuous(&events_a);
    assert_domain_payload_is_non_vacuous(&events_b);
    assert_eq!(
        events_a, events_b,
        "domain event payload must not depend on the operation's parent"
    );

    Ok(())
}

/// This crate exposes no API that folds an `OperationId` (or a
/// `SessionId`/`ParentOperationId`) into a digest or fingerprint, and no type
/// in its public surface returns anything resembling a durable
/// `sha256:`-prefixed identity from an `OperationId`. What is actually
/// proven, and no more: `tests/dependency_contract.rs` proves this crate's
/// own dependency *closure* contains no hash function (`sha2` is forbidden,
/// fail-closed) and this file's other tests prove recorded event payloads
/// are independent of which operation id, kind, or parent recorded them.
/// Neither proves — and this crate does not claim — that fingerprinting is
/// unreachable in any absolute sense: a caller can implement a digest in
/// ordinary safe Rust with no dependency at all, and `OperationId::as_wire`
/// deliberately exposes the identity as a string a caller can hash. This
/// test additionally pins the narrower, actually-proven property at the
/// value level: an `OperationId`'s own wire form never begins with the
/// durable `sha256:` prefix `perl-source-identity` uses.
#[test]
fn operation_id_wire_form_never_looks_like_durable_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut allocator = OperationIdAllocator::new(
        SessionId::new("s1").map_err(|e| format!("build fixture session id: {e}"))?,
    );
    let operation =
        allocator.next().ok_or("fresh allocator unexpectedly exhausted its sequence space")?;
    assert!(!operation.as_wire().starts_with("sha256:"));
    assert!(operation.as_wire().starts_with("op:"));
    Ok(())
}
