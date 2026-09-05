//! Backpressure tests for the bounded DAP outbound event queue.
//!
//! Verifies three behavioural properties of the `dispatch_event` routing:
//!
//! 1. `output` events are **dropped** (never block) when the bounded queue is full.
//! 2. Lifecycle events (`stopped`, etc.) **apply backpressure** — they block the
//!    producer until a slot opens, then land exactly once.
//! 3. A permanently-stalled receiver keeps the queue bounded: output flooding
//!    drives the drop counter up and never hangs.

use super::DapMessage;
use super::sync_utils::{EventDispatchResult, dispatch_event, dropped_output_event_count};
use perl_tdd_support::must;
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// `output` events are dropped (non-blocking) when the bounded queue is full.
#[test]
fn output_drop_when_queue_full() {
    let (tx, rx) = sync_channel::<DapMessage>(2);
    let seq = Mutex::new(0i64);

    let r1 = dispatch_event(&tx, &seq, "output", None);
    let r2 = dispatch_event(&tx, &seq, "output", None);
    assert_eq!(r1, EventDispatchResult::Sent, "first output event must be sent");
    assert_eq!(r2, EventDispatchResult::Sent, "second output event must be sent");

    // Queue is full — next output must be dropped, never block
    let r3 = dispatch_event(&tx, &seq, "output", None);
    assert_eq!(
        r3,
        EventDispatchResult::Dropped,
        "output event must be dropped when queue is at capacity"
    );

    // Channel holds exactly 2 messages; the dropped one must not appear
    assert!(rx.try_recv().is_ok(), "slot 1 must contain a message");
    assert!(rx.try_recv().is_ok(), "slot 2 must contain a message");
    assert!(rx.try_recv().is_err(), "no third message must exist (dropped event must not enqueue)");
}

/// Lifecycle events (`stopped`) apply backpressure: they block the caller until
/// a slot opens in the bounded queue.
#[test]
fn lifecycle_blocks_until_drain() {
    let (tx, rx) = sync_channel::<DapMessage>(1);
    let seq = Arc::new(Mutex::new(0i64));

    // Fill the single queue slot with an output event
    let r = dispatch_event(&tx, &seq, "output", None);
    assert_eq!(r, EventDispatchResult::Sent, "must fill the single queue slot");

    // Spawn a helper that dispatches a lifecycle event; it will block on the full queue
    let tx2 = tx.clone();
    let seq2 = Arc::clone(&seq);
    let helper = thread::spawn(move || {
        dispatch_event(
            &tx2,
            &seq2,
            "stopped",
            Some(serde_json::json!({"reason": "breakpoint", "threadId": 1})),
        )
    });

    // Give the helper time to reach the blocking send
    thread::sleep(Duration::from_millis(50));

    // Drain one slot — the blocked lifecycle send must unblock
    let _ = must(rx.recv_timeout(Duration::from_secs(2)));

    // Helper must have completed and returned Sent
    let result = must(helper.join());
    assert_eq!(result, EventDispatchResult::Sent, "lifecycle event must be sent once a slot opens");

    // The `stopped` event must now be in the channel
    let msg = must(rx.recv_timeout(Duration::from_secs(2)));
    match msg {
        DapMessage::Event { event, .. } => {
            assert_eq!(event, "stopped", "received event must be 'stopped'");
        }
        other => must(Err::<(), _>(format!("Expected stopped event, got {other:?}"))),
    }
}

/// With no reader draining the channel (simulating a slow/blocked writer),
/// flooding output events must not hang and must keep the queue bounded via
/// drop-on-full semantics.
#[test]
fn slow_writer_queue_stays_bounded() {
    const CAPACITY: usize = 4;
    const FLOOD: usize = 10_000;

    // `_rx` is intentionally not drained — simulates a slow or blocked writer
    let (tx, _rx) = sync_channel::<DapMessage>(CAPACITY);
    let seq = Mutex::new(0i64);

    let before = dropped_output_event_count();

    for _ in 0..FLOOD {
        dispatch_event(&tx, &seq, "output", None);
    }

    let dropped = dropped_output_event_count().saturating_sub(before);
    assert!(
        dropped > 0,
        "at least one output event must be dropped when the receiver is not drained"
    );
    assert!(
        dropped >= (FLOOD - CAPACITY) as u64,
        "almost all events must be dropped with a stalled receiver: dropped={dropped}, expected >= {}",
        FLOOD - CAPACITY,
    );
}
