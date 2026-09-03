//! Bounded frame admission shared by the external-peer stdio readers.
//!
//! Both external-peer drivers — [`super::peer_bridge::run_peer_session_threaded`]
//! and [`super::peer_launch::run_mirror_editor_loop`] — read Content-Length
//! framed DAP requests off an editor byte stream on a dedicated thread and hand
//! frame bodies to the session loop over a channel. That channel used to be an
//! unbounded `mpsc::channel::<Vec<u8>>()`, so a peer that emits frames faster
//! than the session loop can dispatch them (a stalled client write path, a slow
//! backend, or a blocked lifecycle settlement) could accumulate unbounded
//! retained frame bytes. Issue #9522.
//!
//! The contract here is one reviewed bounded policy for both readers:
//!
//! - a fixed frame-count bound ([`PEER_FRAME_QUEUE_CAPACITY`]);
//! - per-frame bytes are already capped by the shared transport authority
//!   (`ContentLengthFramer::MAX_FRAME_SIZE`), so retained bytes are bounded by
//!   `capacity × MAX_FRAME_SIZE` without a second, divergent byte-accounting
//!   implementation;
//! - nonblocking admission ([`admit_peer_frame`]): saturation is never a
//!   silent drop and never an unbounded producer wait. The first saturated
//!   frame latches a shared overflow flag, logs once, and stops the reader;
//! - frames already admitted before the overflow are still dispatched, so
//!   affected in-flight requests settle normally;
//! - the session loop then ends the session with the typed
//!   [`PEER_BACKPRESSURE_MSG`] failure instead of reporting generic success
//!   ([`overflow_failure`]).
//!
//! Requests, responses, and lifecycle frames are semantically significant:
//! none of them are dropped by this policy. The overflow disposition is
//! explicit session failure, not lossy coalescing.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};

/// Maximum queued frame bodies per external-peer session (#9522).
///
/// Real editors pipeline a handful of requests at most; 64 leaves ample room
/// for bursty clients while capping retained frames. Combined with the
/// transport's per-frame `MAX_FRAME_SIZE` cap, this bounds the queue's worst
///-case retained bytes without a second accounting implementation.
pub(crate) const PEER_FRAME_QUEUE_CAPACITY: usize = 64;

/// Typed backpressure failure returned when a reader saturated the queue.
///
/// The session must fail closed with this disposition rather than fall out of
/// the loop as `Ok(())` — generic success after overflow is an explicit
/// falsifier in #9522.
pub(crate) const PEER_BACKPRESSURE_MSG: &str =
    "external-peer frame queue overflow: backpressure failed the session closed (#9522)";

/// Latch-and-report admission of one framed request body.
///
/// Returns `true` when the frame was queued and the reader should continue.
/// On saturation the overflow flag is latched (one `error` log for the first
/// saturated frame only, so a flood cannot become unbounded log I/O) and the
/// reader must stop. On receiver loss the session is already gone and the
/// reader must also stop.
pub(crate) fn admit_peer_frame(
    tx: &SyncSender<Vec<u8>>,
    frame: Vec<u8>,
    overflow: &AtomicBool,
    ctx: &'static str,
) -> bool {
    match tx.try_send(frame) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            // `swap` latches exactly once: the first saturated frame carries the
            // diagnostic, subsequent ones cannot multiply log entries.
            if !overflow.swap(true, Ordering::SeqCst) {
                tracing::error!(ctx, PEER_BACKPRESSURE_MSG);
            }
            false
        }
        Err(TrySendError::Disconnected(_)) => false,
    }
}

/// The typed session failure for a latched overflow, or `None` when the reader
/// ended without saturation (editor EOF / session teardown) and the caller
/// should keep its normal end-of-session behavior.
pub(crate) fn overflow_failure(overflow: &AtomicBool) -> Option<io::Error> {
    if overflow.load(Ordering::SeqCst) {
        Some(io::Error::other(PEER_BACKPRESSURE_MSG))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    /// A frame is admitted while the bounded queue has room.
    #[test]
    fn admission_accepts_while_queue_has_room() {
        let (tx, _rx) = sync_channel::<Vec<u8>>(2);
        let overflow = AtomicBool::new(false);
        assert!(admit_peer_frame(&tx, b"a".to_vec(), &overflow, "test"));
        assert!(admit_peer_frame(&tx, b"b".to_vec(), &overflow, "test"));
        assert!(!overflow.load(Ordering::SeqCst), "roomy admission must not latch overflow");
    }

    /// Saturation latches the overflow flag, refuses the frame, and keeps the
    /// reader-side contract: `false` tells the reader to stop instead of
    /// blocking or dropping silently.
    #[test]
    fn saturation_latches_overflow_and_refuses_frame() {
        let (tx, _rx) = sync_channel::<Vec<u8>>(1);
        let overflow = AtomicBool::new(false);
        assert!(admit_peer_frame(&tx, b"first".to_vec(), &overflow, "test"));
        assert!(!admit_peer_frame(&tx, b"second".to_vec(), &overflow, "test"));
        assert!(overflow.load(Ordering::SeqCst), "saturation must latch the overflow flag");
        assert!(
            !admit_peer_frame(&tx, b"third".to_vec(), &overflow, "test"),
            "a saturated queue must keep refusing frames"
        );
    }

    /// A latched overflow maps to the typed backpressure failure; a clean
    /// reader end maps to `None` so the session keeps its normal EOF path.
    #[test]
    fn overflow_failure_is_typed_and_clean_end_is_none() -> Result<(), String> {
        let overflow = AtomicBool::new(false);
        if overflow_failure(&overflow).is_some() {
            return Err("clean reader end must not be a failure".to_string());
        }
        overflow.store(true, Ordering::SeqCst);
        let Some(failure) = overflow_failure(&overflow) else {
            return Err("latched overflow must fail the session with a typed error".to_string());
        };
        assert!(
            failure.to_string().contains(PEER_BACKPRESSURE_MSG),
            "the failure must carry the typed backpressure disposition, got: {failure}"
        );
        Ok(())
    }

    /// Receiver loss stops the reader without latching overflow: the session
    /// is already gone, and the reader must not spin or block.
    #[test]
    fn receiver_loss_stops_reader_without_overflow() {
        let (tx, rx) = sync_channel::<Vec<u8>>(2);
        drop(rx);
        let overflow = AtomicBool::new(false);
        assert!(!admit_peer_frame(&tx, b"lost".to_vec(), &overflow, "test"));
        assert!(!overflow.load(Ordering::SeqCst), "receiver loss is not backpressure");
    }
}
