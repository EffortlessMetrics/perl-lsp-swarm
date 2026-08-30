//! Stream session manager for progressive inline completion.
//!
//! Manages active streaming sessions with cancel-previous semantics,
//! enabling progressive ghost text delivery via `$/progress` notifications.
//! Each session tracks cumulative text, a sequence counter, and one typed
//! terminal disposition so that stale streams are promptly terminated when
//! the user types or moves the cursor, and so that every stream — successful
//! or not — releases its manager entry exactly once.
//!
//! # Terminal ownership
//!
//! A session is *active* until exactly one [`StreamSession::settle`] call
//! records its [`StreamTerminalOutcome`]. Settling is a compare-and-set: the
//! first caller wins and every later caller observes `false`. Emission of the
//! single `isFinal: true` progress value and removal of the manager entry are
//! both gated on winning that transition, so a stream cannot emit two finals
//! and cannot leave a retained entry behind.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Key identifying a unique stream session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub uri: String,
    pub document_version: i64,
    pub line: u64,
    pub character: u64,
}

/// The typed terminal disposition of one streaming inline-completion request.
///
/// Exactly one of these is recorded per session. The variant is what
/// distinguishes "the backend produced nothing" from "the backend failed after
/// producing partial text" — a distinction the previous cancellation-only state
/// could not express, which let a partial failure present as a successful
/// final candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamTerminalOutcome {
    /// A safe, policy-compatible candidate was emitted as the final value.
    CompletedWithCandidate,
    /// The stream completed but produced no admissible candidate.
    CompletedEmptyOrFiltered,
    /// The final AI candidate was filtered and deterministic fallback owns the
    /// final content.
    CompletedWithDeterministicFallback,
    /// The backend returned an error. Partial cumulative text is never promoted
    /// to a successful final on this path.
    BackendFailed,
    /// A newer request for the same document replaced this stream.
    SupersededByNewRequest,
    /// The document changed, was saved at a newer version, or was closed.
    DocumentChangedOrClosed,
    /// The handler returned before any backend stream could produce a final
    /// value (unready context, no backend, notification failure).
    ProtocolEndedWithoutFinal,
}

/// A live stream session.
#[allow(dead_code)] // Fields used by streaming handler when AI backend is wired
pub struct StreamSession {
    /// Unique session ID.
    pub session_id: String,
    /// Cancellation flag -- set to true to stop the stream.
    pub cancelled: AtomicBool,
    /// Current cumulative text.
    pub current_text: std::sync::Mutex<String>,
    /// Monotonically increasing sequence number.
    pub sequence: AtomicU64,
    /// The replacement range start line (set once).
    pub start_line: u64,
    /// The replacement range start character (set once).
    pub start_character: u64,
    /// The one terminal disposition, once settled.
    terminal: std::sync::Mutex<Option<StreamTerminalOutcome>>,
}

impl StreamSession {
    /// Create a new session with the given ID and replacement-range start position.
    pub fn new(session_id: String, line: u64, character: u64) -> Self {
        Self {
            session_id,
            cancelled: AtomicBool::new(false),
            current_text: std::sync::Mutex::new(String::new()),
            sequence: AtomicU64::new(0),
            start_line: line,
            start_character: character,
            terminal: std::sync::Mutex::new(None),
        }
    }

    /// Signal the stream to stop and record `outcome` as its terminal
    /// disposition.
    ///
    /// Subsequent `$/progress` chunks will not be sent. A session cancelled
    /// this way is terminal, not merely flagged, so it can never later be
    /// mistaken for an active stream. When the session already settled, the
    /// earlier outcome is preserved: a late cancellation must not rewrite the
    /// disposition of a stream that already completed.
    pub fn cancel_with(&self, outcome: StreamTerminalOutcome) {
        self.settle(outcome);
        self.cancelled.store(true, Ordering::Release);
    }

    /// Return `true` if [`cancel_with`] has been called on this session.
    ///
    /// [`cancel_with`]: StreamSession::cancel_with
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Record the one terminal outcome for this session.
    ///
    /// Returns `true` only for the caller that performed the transition. Every
    /// later call returns `false` and leaves the first outcome intact. Callers
    /// use this as the guard for emitting the single `isFinal: true` value.
    pub fn settle(&self, outcome: StreamTerminalOutcome) -> bool {
        let mut terminal = self.terminal.lock().unwrap_or_else(|e| e.into_inner());
        if terminal.is_some() {
            return false;
        }
        *terminal = Some(outcome);
        true
    }

    /// Return `true` once a terminal outcome has been recorded.
    pub fn is_settled(&self) -> bool {
        self.terminal.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// The recorded terminal outcome, if the session has settled.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn terminal_outcome(&self) -> Option<StreamTerminalOutcome> {
        *self.terminal.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The sequence value the next delivered frame will carry.
    ///
    /// Reading is deliberately separate from consuming. The outbound channel is
    /// bounded, so a notification can fail transiently under backpressure
    /// (`WouldBlock`); a value consumed for a frame that never reached the
    /// client would leave a permanent gap in the sequence stream the client
    /// observes. Build the payload with this value, attempt the send, and call
    /// [`commit_sequence`] only once the send succeeded.
    ///
    /// [`commit_sequence`]: StreamSession::commit_sequence
    pub fn pending_sequence(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }

    /// Consume the pending sequence value after a frame was actually delivered.
    ///
    /// One handler thread emits for one session, so read-then-commit needs no
    /// stronger synchronization than the load and store themselves.
    pub fn commit_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }
}

/// Manages active stream sessions with cancel-previous semantics.
pub struct StreamSessionManager {
    sessions: std::sync::Mutex<HashMap<SessionKey, Arc<StreamSession>>>,
    generation: AtomicU64,
}

impl StreamSessionManager {
    /// Create a new, empty session manager.
    pub fn new() -> Self {
        Self { sessions: std::sync::Mutex::new(HashMap::new()), generation: AtomicU64::new(0) }
    }

    /// Start a new session, superseding every active session for the same
    /// document.
    ///
    /// The product exposes **one** active ghost-text stream per document, not
    /// independent backend work at every cursor the user has visited. A new
    /// request therefore supersedes older cursor and older version streams for
    /// the same URI, not merely the entry that shares its exact
    /// [`SessionKey`]. Superseded sessions are cancelled, settled as
    /// [`StreamTerminalOutcome::SupersededByNewRequest`], and evicted here.
    pub fn start_session(&self, key: SessionKey) -> Arc<StreamSession> {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let session_id = format!("sess-{generation:x}");
        let session = Arc::new(StreamSession::new(session_id, key.line, key.character));

        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());

        // Supersede every prior stream for this document, whatever cursor or
        // version it was started at.
        sessions.retain(|existing, existing_session| {
            if existing.uri == key.uri {
                existing_session.cancel_with(StreamTerminalOutcome::SupersededByNewRequest);
                false
            } else {
                true
            }
        });
        sessions.insert(key, Arc::clone(&session));

        session
    }

    /// Remove the session stored under `key` only when it is still the exact
    /// session identified by `session_id`, settling it with `outcome`.
    ///
    /// Session identity is load-bearing: a later request can reuse the same
    /// display key, and a stale task finishing afterwards must not remove its
    /// replacement. Returns `true` when this call evicted the entry.
    pub fn finish_if_current(
        &self,
        key: &SessionKey,
        session_id: &str,
        outcome: StreamTerminalOutcome,
    ) -> bool {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let matches = sessions.get(key).is_some_and(|s| s.session_id == session_id);
        if !matches {
            return false;
        }
        if let Some(session) = sessions.remove(key) {
            session.settle(outcome);
            return true;
        }
        false
    }

    /// Cancel and remove all sessions for a given URI (on didChange/didClose).
    ///
    /// Cancelled sessions are evicted immediately. Removal is never deferred to
    /// a later housekeeping sweep: every terminal path — this one, supersession
    /// in [`start_session`], and [`finish_if_current`] on the handler's own
    /// completion — evicts its entry, so the map holds only genuinely active
    /// streams.
    ///
    /// [`start_session`]: StreamSessionManager::start_session
    /// [`finish_if_current`]: StreamSessionManager::finish_if_current
    pub fn cancel_for_uri(&self, uri: &str) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|key, session| {
            if key.uri == uri {
                session.cancel_with(StreamTerminalOutcome::DocumentChangedOrClosed);
                false
            } else {
                true
            }
        });
    }

    /// Cancel and remove all sessions for a given URI where the document
    /// version is older than the supplied version (on document version change).
    pub fn cancel_for_uri_version(&self, uri: &str, version: i64) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|key, session| {
            if key.uri == uri && key.document_version < version {
                session.cancel_with(StreamTerminalOutcome::DocumentChangedOrClosed);
                false
            } else {
                true
            }
        });
    }

    /// Number of sessions currently held by the manager.
    ///
    /// Test-only; production code observes manager state through cancellation.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub fn len(&self) -> usize {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl Default for StreamSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_session_creates_unique_ids() {
        let mgr = StreamSessionManager::new();
        let key =
            SessionKey { uri: "file:///a.pl".into(), document_version: 1, line: 5, character: 10 };
        let s1 = mgr.start_session(key.clone());
        let s2 = mgr.start_session(key);
        assert_ne!(s1.session_id, s2.session_id);
    }

    #[test]
    fn start_session_cancels_previous() {
        let mgr = StreamSessionManager::new();
        let key =
            SessionKey { uri: "file:///a.pl".into(), document_version: 1, line: 5, character: 10 };
        let s1 = mgr.start_session(key.clone());
        assert!(!s1.is_cancelled());
        let _s2 = mgr.start_session(key);
        assert!(s1.is_cancelled());
    }

    #[test]
    fn start_session_supersedes_other_cursors_in_the_same_document() {
        let mgr = StreamSessionManager::new();
        let first = mgr.start_session(SessionKey {
            uri: "file:///a.pl".into(),
            document_version: 1,
            line: 5,
            character: 10,
        });
        // A different cursor in the same document is a different SessionKey.
        let second = mgr.start_session(SessionKey {
            uri: "file:///a.pl".into(),
            document_version: 1,
            line: 9,
            character: 2,
        });

        assert!(first.is_cancelled(), "the earlier cursor's stream must be superseded");
        assert_eq!(
            first.terminal_outcome(),
            Some(StreamTerminalOutcome::SupersededByNewRequest),
            "supersession is a distinct terminal cause, not a plain cancellation"
        );
        assert!(!second.is_cancelled());
        assert_eq!(mgr.len(), 1, "one document exposes exactly one active ghost-text stream");
    }

    #[test]
    fn start_session_leaves_other_documents_independent() {
        let mgr = StreamSessionManager::new();
        let a = mgr.start_session(SessionKey {
            uri: "file:///a.pl".into(),
            document_version: 1,
            line: 0,
            character: 0,
        });
        let _b = mgr.start_session(SessionKey {
            uri: "file:///b.pl".into(),
            document_version: 1,
            line: 0,
            character: 0,
        });
        assert!(!a.is_cancelled(), "a stream in another document must not be superseded");
        assert_eq!(mgr.len(), 2);
    }

    #[test]
    fn repeated_requests_never_accumulate_sessions() {
        let mgr = StreamSessionManager::new();
        for i in 0..200 {
            mgr.start_session(SessionKey {
                uri: "file:///hot.pl".into(),
                document_version: i,
                line: i as u64,
                character: 0,
            });
        }
        assert_eq!(
            mgr.len(),
            1,
            "repeated requests must supersede, not accumulate \
             (regression: one retained entry per visited cursor/version)"
        );
    }

    #[test]
    fn finish_if_current_removes_the_exact_session() {
        let mgr = StreamSessionManager::new();
        let key =
            SessionKey { uri: "file:///a.pl".into(), document_version: 1, line: 0, character: 0 };
        let session = mgr.start_session(key.clone());

        assert!(mgr.finish_if_current(
            &key,
            &session.session_id,
            StreamTerminalOutcome::CompletedWithCandidate
        ));
        assert_eq!(mgr.len(), 0, "a completed stream must release its manager entry");
        assert_eq!(session.terminal_outcome(), Some(StreamTerminalOutcome::CompletedWithCandidate));
    }

    #[test]
    fn a_stale_session_cannot_finish_its_replacement() {
        let mgr = StreamSessionManager::new();
        let key =
            SessionKey { uri: "file:///a.pl".into(), document_version: 1, line: 0, character: 0 };
        let stale = mgr.start_session(key.clone());
        let live = mgr.start_session(key.clone());
        assert_ne!(stale.session_id, live.session_id);

        // The stale task finishes late, using the display key it still holds.
        let removed = mgr.finish_if_current(
            &key,
            &stale.session_id,
            StreamTerminalOutcome::CompletedWithCandidate,
        );

        assert!(!removed, "an old session must not remove its replacement");
        assert_eq!(mgr.len(), 1, "the live session stays registered");
        assert!(!live.is_cancelled(), "the live session stays active");
    }

    #[test]
    fn settle_records_exactly_one_outcome() {
        let session = StreamSession::new("test".into(), 0, 0);
        assert!(session.settle(StreamTerminalOutcome::CompletedWithCandidate));
        assert!(
            !session.settle(StreamTerminalOutcome::BackendFailed),
            "only the first terminal transition may win"
        );
        assert_eq!(
            session.terminal_outcome(),
            Some(StreamTerminalOutcome::CompletedWithCandidate),
            "a later transition must not overwrite the recorded outcome"
        );
    }

    #[test]
    fn cancel_with_settles_when_no_outcome_was_recorded() {
        let session = StreamSession::new("test".into(), 0, 0);
        session.cancel_with(StreamTerminalOutcome::DocumentChangedOrClosed);
        assert!(session.is_settled(), "a cancelled stream is terminal, not merely flagged");
        assert_eq!(
            session.terminal_outcome(),
            Some(StreamTerminalOutcome::DocumentChangedOrClosed)
        );
    }

    #[test]
    fn cancel_with_preserves_an_already_recorded_outcome() {
        let session = StreamSession::new("test".into(), 0, 0);
        assert!(session.settle(StreamTerminalOutcome::CompletedWithCandidate));
        session.cancel_with(StreamTerminalOutcome::DocumentChangedOrClosed);
        assert_eq!(
            session.terminal_outcome(),
            Some(StreamTerminalOutcome::CompletedWithCandidate),
            "a late cancellation must not rewrite a completed stream's outcome"
        );
    }

    #[test]
    fn cancel_for_uri_cancels_matching() {
        let mgr = StreamSessionManager::new();
        let s1 = mgr.start_session(SessionKey {
            uri: "file:///a.pl".into(),
            document_version: 1,
            line: 0,
            character: 0,
        });
        let s2 = mgr.start_session(SessionKey {
            uri: "file:///b.pl".into(),
            document_version: 1,
            line: 0,
            character: 0,
        });
        mgr.cancel_for_uri("file:///a.pl");
        assert!(s1.is_cancelled());
        assert!(!s2.is_cancelled());
        // Cancelled sessions must also be evicted from the manager — this is
        // the regression guard for the leak fixed alongside this test.
        assert_eq!(mgr.len(), 1, "cancelled session for a.pl must be evicted");
    }

    #[test]
    fn cancel_for_uri_version_evicts_the_older_document_stream() {
        let mgr = StreamSessionManager::new();
        let stale = mgr.start_session(SessionKey {
            uri: "file:///v.pl".into(),
            document_version: 1,
            line: 0,
            character: 0,
        });
        let untouched = mgr.start_session(SessionKey {
            uri: "file:///other.pl".into(),
            document_version: 1,
            line: 0,
            character: 0,
        });

        mgr.cancel_for_uri_version("file:///v.pl", 3);

        assert!(stale.is_cancelled(), "a stream at an older version must be cancelled");
        assert_eq!(stale.terminal_outcome(), Some(StreamTerminalOutcome::DocumentChangedOrClosed));
        assert!(!untouched.is_cancelled(), "another document is unaffected");
        assert_eq!(mgr.len(), 1, "the stale entry must be evicted, not merely flagged");
    }

    #[test]
    fn cancel_for_uri_version_keeps_a_current_version_stream() {
        let mgr = StreamSessionManager::new();
        let current = mgr.start_session(SessionKey {
            uri: "file:///v.pl".into(),
            document_version: 5,
            line: 1,
            character: 0,
        });
        mgr.cancel_for_uri_version("file:///v.pl", 3);
        assert!(!current.is_cancelled(), "a stream at or above the version survives");
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn sequence_increments() {
        let session = StreamSession::new("test".into(), 0, 0);
        assert_eq!(session.commit_sequence(), 0);
        assert_eq!(session.commit_sequence(), 1);
        assert_eq!(session.commit_sequence(), 2);
    }

    #[test]
    fn a_pending_sequence_is_reused_until_it_is_committed() {
        let session = StreamSession::new("test".into(), 0, 0);
        // A frame that fails to reach the client must not consume its value,
        // or the client observes a gap it can never explain.
        assert_eq!(session.pending_sequence(), 0);
        assert_eq!(session.pending_sequence(), 0, "reading must not consume");
        session.commit_sequence();
        assert_eq!(session.pending_sequence(), 1);
    }
}
