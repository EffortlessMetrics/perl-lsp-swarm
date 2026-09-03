//! Bounded, generation-aware TCP-attach event forwarding (issue #9521).
//!
//! The TCP-attach fan-in used to be an unbounded
//! `std::sync::mpsc::channel::<DapEvent>()` between the attach reader thread
//! and the forwarding thread, so a stalled client/adapter output path could
//! grow retained events without bound (the residual left deliberately out of
//! #5149's scope). The queue is now bounded
//! ([`TCP_ATTACH_EVENT_CAPACITY`]) with the reader-side admission policy in
//! [`crate::tcp_attach::reader`].
//!
//! This module owns the **consumption** half of that contract:
//!
//! - every admitted event is bound to the session generation captured at
//!   attach time; a replacement attach, termination, or disconnect advances
//!   the generation, and events of the dead generation are discarded **before
//!   DAP publication**, so a prior session's event can never reach the
//!   current client;
//! - publication goes through the same single-authority primitives as every
//!   other adapter event ([`dispatch_event`] for output/state events,
//!   [`emit_terminated_event`] for the once-per-generation terminal event);
//! - the forwarding thread parks only in `recv` (no lock held across queue
//!   operations) and exits when the reader side disappears or the session
//!   generation is replaced.

use super::process::emit_terminated_event;
use super::sync_utils::{dispatch_event, lock_or_recover};
use super::{DapMessage, DebugAdapter, TerminationState};
use crate::tcp_attach::DapEvent;
use serde_json::json;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

/// Capacity of the bounded TCP-attach event fan-in queue (#9521).
///
/// Debugger event bursts are small (output lines, stop/continue notifications);
/// 128 events leave wide headroom while capping retention. Per-event bytes are
/// already capped by the transport's `MAX_FRAME_SIZE`, so retained bytes are
/// bounded without a second accounting implementation.
pub(super) const TCP_ATTACH_EVENT_CAPACITY: usize = 128;

/// Spawn the forwarding thread that turns queued attach events into DAP events
/// on the adapter's bounded outbound queue.
///
/// `session_generation` is the generation captured when the attach succeeded;
/// events arriving after that generation has been replaced are stale and are
/// dropped before publication.
pub(super) fn spawn_tcp_attach_event_forwarder(
    rx: Receiver<DapEvent>,
    event_sender: Option<SyncSender<DapMessage>>,
    seq_counter: Arc<Mutex<i64>>,
    termination_state: Arc<Mutex<TerminationState>>,
    session_generation: u64,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            // Generation gate: a replacement attach, termination, or disconnect
            // advances the generation. Everything still queued for the dead
            // generation — including this event — is discarded before DAP
            // publication (#9521 test 9).
            if lock_or_recover(&termination_state, "debug_adapter.termination_state").generation
                != session_generation
            {
                break;
            }
            match event {
                DapEvent::Terminated { reason } => {
                    if let Some(ref sender) = event_sender {
                        emit_terminated_event(
                            sender,
                            &seq_counter,
                            &termination_state,
                            Some(session_generation),
                            Some(json!({"reason": reason})),
                        );
                    }
                }
                DapEvent::Error { message } => {
                    tracing::error!(message, "TCP attach error");
                }
                identity_event => {
                    // The editor must only ever observe the advertised synthetic
                    // context: `threads`, every thread-scoped request, and
                    // `stopped`/`continued` events all carry the same id on the
                    // TCP-attach path (#8294).
                    if let Some((name, body)) = DebugAdapter::tcp_event_message(identity_event)
                        && let Some(ref sender) = event_sender
                    {
                        dispatch_event(sender, &seq_counter, name, body);
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_adapter::process::reserve_terminated_event;
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    /// A `TerminationState` at generation 1, matching the generation captured
    /// by the forwarder under test.
    fn termination_state_at_generation_one() -> Arc<Mutex<TerminationState>> {
        Arc::new(Mutex::new(TerminationState { generation: 1, emitted: false }))
    }

    fn current_queue() -> (SyncSender<DapMessage>, Receiver<DapMessage>) {
        sync_channel(64)
    }

    /// Below pressure, admitted events are forwarded in order with their DAP
    /// event names intact (#9521 test 12).
    #[test]
    fn forwarder_preserves_event_order_below_pressure() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapEvent>(8);
        let (out_tx, out_rx) = current_queue();
        let state = termination_state_at_generation_one();

        let handle = spawn_tcp_attach_event_forwarder(
            rx,
            Some(out_tx),
            Arc::new(Mutex::new(0)),
            Arc::clone(&state),
            1,
        );

        tx.send(DapEvent::Output { category: "stdout".into(), output: "one".into() })
            .map_err(|e| format!("send output: {e}"))?;
        tx.send(DapEvent::Stopped { reason: "breakpoint".into(), thread_id: 5 })
            .map_err(|e| format!("send stopped: {e}"))?;
        drop(tx);

        let mut names = Vec::new();
        for _ in 0..2 {
            let msg = out_rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|e| format!("forwarded event missing: {e}"))?;
            if let DapMessage::Event { event, .. } = msg {
                names.push(event);
            }
        }
        assert_eq!(names, vec!["output".to_string(), "stopped".to_string()]);
        handle.join().map_err(|_| "forwarder panicked".to_string())?;
        Ok(())
    }

    /// After the session generation advances (replacement attach, terminate,
    /// disconnect), events still queued for the dead generation are discarded
    /// before DAP publication: the current client must never observe a prior
    /// session's event (#9521 test 9 / falsifier).
    #[test]
    fn forwarder_discards_stale_generation_events_before_publication() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapEvent>(8);
        let (out_tx, out_rx) = current_queue();
        let state = termination_state_at_generation_one();

        let handle = spawn_tcp_attach_event_forwarder(
            rx,
            Some(out_tx),
            Arc::new(Mutex::new(0)),
            Arc::clone(&state),
            1,
        );

        // Deliver one live-generation event, and wait until it is observed on
        // the outbound side, so the queue is provably drained before the
        // generation advances.
        tx.send(DapEvent::Output { category: "stdout".into(), output: "live".into() })
            .map_err(|e| format!("send live output: {e}"))?;
        let live = out_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|e| format!("live event must be forwarded: {e}"))?;
        assert!(matches!(&live, DapMessage::Event { event, .. } if event == "output"));

        // Replace the session: generation 1 is now dead.
        lock_or_recover(&state, "test.termination_state").generation = 2;

        tx.send(DapEvent::Stopped { reason: "stale-stop".into(), thread_id: 9 })
            .map_err(|e| format!("send stale stopped: {e}"))?;
        drop(tx);

        handle.join().map_err(|_| "forwarder panicked".to_string())?;
        let leftover = out_rx.try_recv().ok();
        assert!(
            leftover.is_none(),
            "a stale-generation event must be discarded before publication, got {leftover:?}"
        );
        Ok(())
    }

    /// The terminal event stays a once-per-generation publication: repeated
    /// `terminated` events for one generation publish exactly one DAP
    /// `terminated` (single-emission gate preserved through the forwarder).
    #[test]
    fn forwarder_publishes_terminated_at_most_once_per_generation() -> Result<(), String> {
        let (tx, rx) = sync_channel::<DapEvent>(8);
        let (out_tx, out_rx) = current_queue();
        let state = termination_state_at_generation_one();

        let handle = spawn_tcp_attach_event_forwarder(
            rx,
            Some(out_tx),
            Arc::new(Mutex::new(0)),
            Arc::clone(&state),
            1,
        );

        for reason in ["first", "second", "third"] {
            tx.send(DapEvent::Terminated { reason: reason.into() })
                .map_err(|e| format!("send terminated: {e}"))?;
        }
        drop(tx);
        handle.join().map_err(|_| "forwarder panicked".to_string())?;

        let mut terminated = 0;
        while let Ok(msg) = out_rx.try_recv() {
            if let DapMessage::Event { event, .. } = msg
                && event == "terminated"
            {
                terminated += 1;
            }
        }
        assert_eq!(terminated, 1, "terminated must publish at most once per generation");
        Ok(())
    }

    /// The generation gate directly guards `reserve_terminated_event`: a
    /// reservation claimed under the live generation cannot be delivered after
    /// the generation was replaced (stale terminal leak prevention, #12092).
    #[test]
    fn terminated_reservation_is_retired_when_generation_replaced() {
        let state = termination_state_at_generation_one();
        assert!(reserve_terminated_event(&state, Some(1)), "live generation reserves");
        lock_or_recover(&state, "test.termination_state").generation = 2;
        assert!(
            !reserve_terminated_event(&state, Some(1)),
            "a dead generation must not reserve a terminal emission"
        );
    }
}
