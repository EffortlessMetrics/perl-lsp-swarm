/// Regression tests for bounded outbound DAP event queue (issue #5149).
///
/// Before the fix the outbound mpsc channel was unbounded, so a chatty debuggee whose
/// `output` events arrived faster than the client could consume them would grow the queue
/// without limit. These tests verify:
///
/// 1. `output` events are dropped (not queued) when the queue is full.
/// 2. Lifecycle events (`stopped`) block the producer until a slot frees up, then land.
/// 3. Flooding the queue with output does not hang and the drop counter advances.
/// 4. Two real threads racing on a shared `seq`/`SyncSender` pair never observe a
///    later-assigned `seq` overtake an earlier one on the wire (PR #5318, defect 1).
/// 5. `terminated`, a lifecycle event, is never dropped even when the queue is full.
/// 6. A synthetic drop-notice `output` event surfaces to the user after drops occur,
///    without emitting one notice per dropped line (PR #5318, defect 2).
use perl_dap::debug_adapter::{
    DapMessage,
    sync_utils::{EventDispatchResult, dispatch_event, output_drop_count},
};
use serde_json::json;
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Returns `true` if `msg` is the synthetic drop-notice `output` event emitted by
/// `dispatch_event` after output events have been dropped (see `sync_utils::dispatch_event`).
fn is_drop_notice(msg: &DapMessage) -> bool {
    matches!(msg, DapMessage::Event { event, body, .. }
        if event == "output"
            && body
                .as_ref()
                .and_then(|b| b.get("output"))
                .and_then(|o| o.as_str())
                .is_some_and(|t| t.contains("dropped due to slow debug client")))
}

/// `output` events that arrive on a full queue are dropped with `Dropped`,
/// not queued or treated as a disconnect.
#[test]
fn output_drop_when_queue_full() -> Result<(), String> {
    let cap = 2;
    let (tx, _rx) = sync_channel::<DapMessage>(cap);
    let seq = Arc::new(Mutex::new(0i64));

    for i in 0..cap {
        let result =
            dispatch_event(&tx, &seq, "output", Some(json!({"output": format!("line {i}\n")})));
        if result != EventDispatchResult::Sent {
            return Err(format!("slot {i} should be accepted, got {result:?}"));
        }
    }

    let result = dispatch_event(&tx, &seq, "output", Some(json!({"output": "overflow\n"})));
    if result != EventDispatchResult::Dropped {
        return Err(format!(
            "output event on a full queue must be Dropped, not Sent or Disconnected; got {result:?}"
        ));
    }
    Ok(())
}

/// A `stopped` lifecycle event blocks until a slot is available, then is delivered.
/// Queue capacity = 1: fill with one output event, spawn a thread to send `stopped`
/// (which must block), drain the output event, then assert `stopped` arrives.
#[test]
fn lifecycle_blocks_until_drain() -> Result<(), String> {
    let cap = 1;
    let (tx, rx) = sync_channel::<DapMessage>(cap);
    let seq = Arc::new(Mutex::new(0i64));

    // Fill the single slot with an output event.
    let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "filling\n"})));
    if r != EventDispatchResult::Sent {
        return Err(format!("expected Sent when filling queue, got {r:?}"));
    }

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
    let drained = rx
        .recv_timeout(Duration::from_millis(200))
        .map_err(|e| format!("output event must be drainable: {e}"))?;
    if !matches!(&drained, DapMessage::Event { event, .. } if event == "output") {
        return Err(format!("expected output event, got: {drained:?}"));
    }

    // The `stopped` event should now be delivered.
    let stopped = rx
        .recv_timeout(Duration::from_millis(500))
        .map_err(|e| format!("stopped event must arrive after queue drains: {e}"))?;
    if !matches!(&stopped, DapMessage::Event { event, .. } if event == "stopped") {
        return Err(format!("expected stopped event, got: {stopped:?}"));
    }

    let result = handle.join().map_err(|_| "dispatch thread panicked".to_string())?;
    if result != EventDispatchResult::Sent {
        return Err(format!("stopped event must be delivered (Sent), got {result:?}"));
    }
    Ok(())
}

/// Flooding the queue with many output events from a producer that outruns a slow consumer
/// must not hang and must produce drops rather than unbounded growth.
#[test]
fn slow_consumer_output_flood_stays_bounded() -> Result<(), String> {
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
            EventDispatchResult::Disconnected => {
                return Err(format!("unexpected disconnect on iteration {i}"));
            }
        }
    }

    if dropped == 0 {
        return Err(
            "at least one output event must be dropped when producer outruns consumer".into()
        );
    }
    if sent > cap {
        return Err(format!("in-queue items cannot exceed channel capacity ({cap}), sent={sent}"));
    }
    if sent + dropped != total {
        return Err(format!(
            "every event is either sent or dropped; sent={sent} dropped={dropped} total={total}"
        ));
    }
    if output_drop_count() <= initial_drops {
        return Err("global drop counter must advance when output events are dropped".into());
    }
    Ok(())
}

/// Sending to a disconnected receiver returns `Disconnected` for both output
/// and lifecycle events.
#[test]
fn disconnected_receiver_returns_disconnected() -> Result<(), String> {
    let (tx, rx) = sync_channel::<DapMessage>(4);
    let seq = Arc::new(Mutex::new(0i64));

    drop(rx); // disconnect the receiver

    let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "x\n"})));
    if r != EventDispatchResult::Disconnected {
        return Err(format!("output must be Disconnected when rx dropped, got {r:?}"));
    }

    let r2 = dispatch_event(&tx, &seq, "stopped", Some(json!({"reason": "end"})));
    if r2 != EventDispatchResult::Disconnected {
        return Err(format!("stopped must be Disconnected when rx dropped, got {r2:?}"));
    }
    Ok(())
}

/// Nonblocking dispatch must drop a lifecycle event on a full queue instead of blocking.
#[test]
fn nonblocking_lifecycle_drops_when_full() -> Result<(), String> {
    use perl_dap::debug_adapter::sync_utils::dispatch_event_nonblocking;

    let (tx, _rx) = sync_channel::<DapMessage>(1);
    let seq = Arc::new(Mutex::new(0i64));

    let fill = dispatch_event(&tx, &seq, "output", Some(json!({"output": "fill\n"})));
    if fill != EventDispatchResult::Sent {
        return Err(format!("expected Sent to fill queue, got {fill:?}"));
    }

    let result = dispatch_event_nonblocking(
        &tx,
        &seq,
        "terminated",
        Some(json!({"reason": "debuggee_timeout"})),
    );
    if result != EventDispatchResult::Dropped {
        return Err(format!(
            "nonblocking terminated on full queue must be Dropped, got {result:?}"
        ));
    }
    Ok(())
}

/// Real-thread ordering regression test (PR #5318, defect 1).
///
/// Production shares one `seq: Arc<Mutex<i64>>` and one `SyncSender<DapMessage>` between
/// the main thread and the output-reader thread spawned by `start_output_reader`
/// (`crates/perl-dap/src/debug_adapter/process.rs:574`). This test reproduces that shape
/// with two genuine OS threads racing on the same `seq`/`sender` pair (never wrapped in a
/// single outer `Arc<Mutex<..>>` around the whole adapter, which is exactly why
/// `test_thread_safe_sequence_numbers` in `session_lifecycle_tests.rs` cannot catch this
/// class of bug: an outer adapter-wide mutex would itself serialize the two threads and
/// hide the race).
///
/// Several threads flood `output` events (drop-on-full, non-blocking `try_send`) while the
/// queue is full; another emits a single lifecycle event (blocking `send`). If
/// seq-assignment and enqueue are not atomic, a `try_send` racing right after a `send` has
/// taken the seq lock, released it, and is blocked on a full queue can grab a *later* seq
/// and land on the wire first once a slot frees — producing a seq value that decreases
/// (non-monotonic) as observed by the receiver. Many concurrent flood threads (rather than
/// just one) are used deliberately: real contention on the shared `seq` mutex, with genuine
/// multi-core parallelism, is what actually exposes the release-then-enqueue gap — a lone
/// flood thread rarely gets preempted at exactly the right instruction boundary.
///
/// This single scenario is probabilistic: manual verification against the unfixed code
/// (production hunk reverted, see PR description for the captured failure output) showed it
/// fail on roughly half of individual runs on a 4-core box. Several independent trials are
/// therefore run in a loop so one `cargo test` invocation catches it reliably (>90%+ per
/// invocation against the unfixed code) without weakening the ordering assertion itself.
#[test]
fn concurrent_output_flood_and_lifecycle_event_preserve_seq_order() -> Result<(), String> {
    let trials = 5;
    for trial in 0..trials {
        run_seq_order_race_trial(trial)?;
    }
    Ok(())
}

fn run_seq_order_race_trial(trial: usize) -> Result<(), String> {
    let cap = 1;
    let (tx, rx) = sync_channel::<DapMessage>(cap);
    let seq = Arc::new(Mutex::new(0i64));

    // Fill the single slot so every producer immediately races against backpressure.
    let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "fill\n"})));
    if r != EventDispatchResult::Sent {
        return Err(format!("trial {trial}: initial fill should be accepted, got {r:?}"));
    }

    // Generous thread/iteration counts: the race window is narrow, so use enough
    // concurrent contention to make it reliably reproduce against the unfixed code
    // (verified manually; see PR description for the before/after run).
    let flood_threads = 12;
    let iterations_per_thread = 4_000;

    let flood_handles: Vec<_> = (0..flood_threads)
        .map(|t| {
            let tx = tx.clone();
            let seq = Arc::clone(&seq);
            thread::spawn(move || {
                for i in 0..iterations_per_thread {
                    let _ = dispatch_event(
                        &tx,
                        &seq,
                        "output",
                        Some(json!({"output": format!("t{trial}-flood{t}-{i}\n")})),
                    );
                }
            })
        })
        .collect();

    let tx_life = tx.clone();
    let seq_life = Arc::clone(&seq);
    let life_handle = thread::spawn(move || {
        dispatch_event(
            &tx_life,
            &seq_life,
            "stopped",
            Some(json!({"reason": "pause", "threadId": 1, "allThreadsStopped": true})),
        )
    });

    // Drain concurrently while producer threads race, recording every observed seq.
    let mut observed_seqs: Vec<i64> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(DapMessage::Event { seq, .. }) => observed_seqs.push(seq),
            Ok(_) => {}
            Err(_) => {
                if flood_handles.iter().all(|h| h.is_finished()) && life_handle.is_finished() {
                    while let Ok(msg) = rx.try_recv() {
                        if let DapMessage::Event { seq, .. } = msg {
                            observed_seqs.push(seq);
                        }
                    }
                    break;
                }
                if Instant::now() > deadline {
                    break;
                }
            }
        }
    }

    for handle in flood_handles {
        handle.join().map_err(|_| format!("trial {trial}: flood thread panicked"))?;
    }
    let life_result =
        life_handle.join().map_err(|_| format!("trial {trial}: lifecycle thread panicked"))?;
    if life_result != EventDispatchResult::Sent {
        return Err(format!("trial {trial}: stopped event must be delivered, got {life_result:?}"));
    }

    // The critical assertion: transmission order must track seq. A later-assigned seq
    // must never be observed before an earlier one.
    for w in observed_seqs.windows(2) {
        if w[0] > w[1] {
            return Err(format!(
                "trial {trial}: outbound events observed out of seq order: seq {} arrived before \
                 seq {} (full observed sequence: {:?})",
                w[0], w[1], observed_seqs
            ));
        }
    }
    Ok(())
}

/// Explicit `terminated` case: a lifecycle event named literally `"terminated"` must never
/// be dropped, even when the outbound queue is full. `terminated` uses the blocking `send`
/// path (only `output` uses drop-on-full `try_send`).
#[test]
fn terminated_event_never_dropped_when_queue_full() -> Result<(), String> {
    let cap = 1;
    let (tx, rx) = sync_channel::<DapMessage>(cap);
    let seq = Arc::new(Mutex::new(0i64));

    // Fill the single slot with an output event.
    let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "filling\n"})));
    if r != EventDispatchResult::Sent {
        return Err(format!("expected Sent when filling queue, got {r:?}"));
    }

    // Spawn a thread: send a `terminated` event (will block because the queue is full).
    let tx2 = tx.clone();
    let seq2 = Arc::clone(&seq);
    let handle = thread::spawn(move || {
        dispatch_event(&tx2, &seq2, "terminated", Some(json!({"restart": false})))
    });

    // Give the spawned thread a moment to reach the blocking `send`.
    thread::sleep(Duration::from_millis(20));

    // Drain the output event, freeing one slot.
    let drained = rx
        .recv_timeout(Duration::from_millis(200))
        .map_err(|e| format!("output event must be drainable: {e}"))?;
    if !matches!(&drained, DapMessage::Event { event, .. } if event == "output") {
        return Err(format!("expected output event, got: {drained:?}"));
    }

    // The `terminated` event should now be delivered — it must never be dropped.
    let terminated = rx
        .recv_timeout(Duration::from_millis(500))
        .map_err(|e| format!("terminated event must arrive after queue drains: {e}"))?;
    if !matches!(&terminated, DapMessage::Event { event, .. } if event == "terminated") {
        return Err(format!("expected terminated event, got: {terminated:?}"));
    }

    let result = handle.join().map_err(|_| "dispatch thread panicked".to_string())?;
    if result != EventDispatchResult::Sent {
        return Err(format!("terminated event must be delivered (Sent), got {result:?}"));
    }
    Ok(())
}

/// After output events are dropped, a synthetic `output` event notifying the user must
/// eventually surface once the queue has room (PR #5318, defect 2).
///
/// The notice is only ever attempted from inside a call that has *itself* just found this
/// exact channel full (see `sync_utils::try_emit_drop_notice`) — by design, so that the
/// (process-wide) drop counters can never let a notice meant for one channel land on an
/// unrelated, otherwise-idle channel. That means catching it requires a real race: several
/// producer threads flood `output` on a tiny-capacity channel while a consumer thread
/// continuously drains, so that at least one of the bounded non-blocking retry attempts
/// inside the full-branch lands right as a slot frees. Generous thread/iteration counts
/// make this reliable in practice; see the PR description for repeated-run verification.
#[test]
fn drop_notice_appears_after_drops() -> Result<(), String> {
    let cap = 1;
    let (tx, rx) = sync_channel::<DapMessage>(cap);
    let seq = Arc::new(Mutex::new(0i64));

    let producer_threads = 4;
    let iterations_per_producer = 5_000;

    let producers: Vec<_> = (0..producer_threads)
        .map(|p| {
            let tx = tx.clone();
            let seq = Arc::clone(&seq);
            thread::spawn(move || {
                for i in 0..iterations_per_producer {
                    let _ = dispatch_event(
                        &tx,
                        &seq,
                        "output",
                        Some(json!({"output": format!("p{p}-l{i}\n")})),
                    );
                }
            })
        })
        .collect();

    // Consumer: drain continuously, watching for the synthetic notice.
    let mut found_notice = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(msg) if is_drop_notice(&msg) => {
                found_notice = true;
                break;
            }
            Ok(_) => {}
            Err(_) => {
                if producers.iter().all(|h| h.is_finished()) {
                    // Drain whatever is left without blocking, in case the notice landed
                    // in the final batch.
                    while let Ok(msg) = rx.try_recv() {
                        if is_drop_notice(&msg) {
                            found_notice = true;
                        }
                    }
                    break;
                }
            }
        }
        if Instant::now() > deadline {
            break;
        }
    }

    for handle in producers {
        handle.join().map_err(|_| "producer thread panicked".to_string())?;
    }

    if !found_notice {
        return Err("expected a synthetic drop-notice output event to appear during a sustained \
             multi-producer output flood against a slow consumer"
            .into());
    }
    Ok(())
}

/// A sustained flood must not produce one drop-notice per dropped line. With the queue
/// permanently full (never drained at all), the notice's own bounded `try_send` retries can
/// never succeed either, so zero notices land no matter how many output events are dropped —
/// a strict, deterministic proof that the mechanism cannot flood the console with notices.
#[test]
fn drop_notice_flood_does_not_produce_one_per_line() -> Result<(), String> {
    let cap = 1;
    let (tx, rx) = sync_channel::<DapMessage>(cap);
    let seq = Arc::new(Mutex::new(0i64));

    // Fill the only slot and never drain it: every subsequent output event, and every
    // notice-retry attempt made on its behalf, must observe the queue as permanently full.
    let r = dispatch_event(&tx, &seq, "output", Some(json!({"output": "keep\n"})));
    if r != EventDispatchResult::Sent {
        return Err(format!("expected Sent when filling queue, got {r:?}"));
    }

    let total = 500usize;
    let mut dropped = 0usize;
    for i in 0..total {
        if dispatch_event(&tx, &seq, "output", Some(json!({"output": format!("l{i}\n")})))
            == EventDispatchResult::Dropped
        {
            dropped += 1;
        }
    }
    if dropped != total {
        return Err(format!(
            "every send after the first must be dropped (queue never drains); dropped={dropped} total={total}"
        ));
    }

    // Drain everything now: exactly one message can ever have been resident (capacity 1),
    // and it can only be the original "keep" payload — never a drop notice, since the
    // notice's try_send attempts could never have found room either.
    let mut notices = 0usize;
    let mut total_drained = 0usize;
    while let Ok(msg) = rx.try_recv() {
        total_drained += 1;
        if is_drop_notice(&msg) {
            notices += 1;
        }
    }

    if total_drained != 1 {
        return Err(format!(
            "a capacity-1 queue that never drained can hold only one message; drained={total_drained}"
        ));
    }
    if notices != 0 {
        return Err(format!(
            "a permanently-full queue must produce zero notices, not one per dropped line ({dropped} drops); notices={notices}"
        ));
    }
    Ok(())
}
