/// Regression tests for bounded outbound DAP event queue (issue #5149).
///
/// Before the fix the outbound mpsc channel was unbounded, so a chatty debuggee whose
/// `output` events arrived faster than the client could consume them would grow the queue
/// without limit. These tests verify:
///
/// 1. `output` events are dropped (not queued) when the queue is full.
/// 2. Lifecycle events (`stopped`) block the producer until a slot frees up, then land.
/// 3. Flooding the queue with output does not hang and the drop counter advances.
use perl_dap::debug_adapter::{
    DapMessage,
    sync_utils::{EventDispatchResult, dispatch_event, output_drop_count},
};
use serde_json::json;
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `output` events that arrive on a full queue are dropped with `Dropped`,
/// not queued or treated as a disconnect.
#[test]
fn output_drop_when_queue_full() {
    let cap = 2;
    let (tx, _rx) = sync_channel::<DapMessage>(cap);
    let seq = Arc::new(Mutex::new(0i64));

    for i in 0..cap {
        let result =
            dispatch_event(&tx, &seq, "output", Some(json!({"output": format!("line {i}\n")})));
        assert_eq!(result, EventDispatchResult::Sent, "slot {i} should be accepted");
    }

    let result = dispatch_event(&tx, &seq, "output", Some(json!({"output": "overflow\n"})));
    assert_eq!(
        result,
        EventDispatchResult::Dropped,
        "output event on a full queue must be Dropped, not Sent or Disconnected"
    );
}

/// A `stopped` lifecycle event blocks until a slot is available, then is delivered.
/// Queue capacity = 1: fill with one output event, spawn a thread to send `stopped`
/// (which must block), drain the output event, then assert `stopped` arrives.
#[test]
fn lifecycle_blocks_until_drain() {
    let cap = 1;
    let (tx, rx) = sync_channel::<DapMessage>(cap);
    let seq = Arc::new(Mutex::new(0i64));

    // Fill the single slot with an output event.
    let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "filling\n"})));
    assert_eq!(r, EventDispatchResult::Sent);

    // Spawn a thread: send a `stopped` event (will block because the queue is full).
    let tx2 = tx.clone();
    let seq2 = Arc::clone(&seq);
    let handle = std::thread::spawn(move || {
        dispatch_event(
            &tx2,
            &seq2,
            "stopped",
            Some(json!({"reason": "pause", "threadId": 1, "allThreadsStopped": true})),
        )
    });

    // Give the spawned thread a moment to reach the blocking `send`.
    std::thread::sleep(Duration::from_millis(20));

    // Drain the output event, freeing one slot.
    let drained =
        rx.recv_timeout(Duration::from_millis(200)).expect("output event must be drainable");
    assert!(
        matches!(&drained, DapMessage::Event { event, .. } if event == "output"),
        "expected output event, got: {drained:?}"
    );

    // The `stopped` event should now be delivered.
    let stopped = rx
        .recv_timeout(Duration::from_millis(500))
        .expect("stopped event must arrive after queue drains");
    assert!(
        matches!(&stopped, DapMessage::Event { event, .. } if event == "stopped"),
        "expected stopped event, got: {stopped:?}"
    );

    let result = handle.join().expect("dispatch thread must not panic");
    assert_eq!(result, EventDispatchResult::Sent, "stopped event must be delivered (Sent)");
}

/// Flooding the queue with many output events from a producer that outruns a slow consumer
/// must not hang and must produce drops rather than unbounded growth.
#[test]
fn slow_consumer_output_flood_stays_bounded() {
    let cap = 4;
    let (tx, _rx) = sync_channel::<DapMessage>(cap);
    let seq = Arc::new(Mutex::new(0i64));

    let initial_drops = output_drop_count();
    let total = 50usize;
    let mut sent = 0usize;
    let mut dropped = 0usize;

    for i in 0..total {
        match dispatch_event(&tx, &seq, "output", Some(json!({"output": format!("line {i}\n")}))) {
            EventDispatchResult::Sent => sent += 1,
            EventDispatchResult::Dropped => dropped += 1,
            EventDispatchResult::Disconnected => panic!("unexpected disconnect on iteration {i}"),
        }
    }

    assert!(
        dropped > 0,
        "at least one output event must be dropped when producer outruns consumer"
    );
    assert!(sent <= cap, "in-queue items cannot exceed channel capacity ({cap})");
    assert_eq!(sent + dropped, total, "every event is either sent or dropped");
    assert!(
        output_drop_count() > initial_drops,
        "global drop counter must advance when output events are dropped"
    );
}

/// Sending to a disconnected receiver returns `Disconnected` for both output
/// and lifecycle events.
#[test]
fn disconnected_receiver_returns_disconnected() {
    let (tx, rx) = sync_channel::<DapMessage>(4);
    let seq = Arc::new(Mutex::new(0i64));

    drop(rx); // disconnect the receiver

    let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "x\n"})));
    assert_eq!(r, EventDispatchResult::Disconnected, "output must be Disconnected when rx dropped");

    let r2 = dispatch_event(&tx, &seq, "stopped", Some(json!({"reason": "end"})));
    assert_eq!(
        r2,
        EventDispatchResult::Disconnected,
        "stopped must be Disconnected when rx dropped"
    );
}
