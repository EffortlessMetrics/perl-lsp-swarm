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
//! first caller wins and every later caller observes `false`. The streaming
//! handler enqueues a final progress value before committing its sequence and
//! terminal outcome; cancellation can settle independently. Queue acceptance
//! does not acknowledge client delivery. Identity-checked removal releases the
//! manager entry without evicting a successor. The cancellation-versus-send
//! transaction and outbound delivery guarantees remain the scope of #14168.

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

/// Admission authority retained only while requests for a URI are outstanding.
#[derive(Default)]
struct SessionState {
    sessions: HashMap<SessionKey, Arc<StreamSession>>,
    admissions: HashMap<String, AdmissionCell>,
}

struct AdmissionCell {
    identity: Arc<()>,
    outstanding: usize,
    latest_admitted: Option<u64>,
}

/// Failure to reserve an ingress order. Neither counter nor count may wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadReservationError {
    OrderExhausted,
    OutstandingExhausted,
}

/// One request's owned reservation, carried unchanged from ingress to the handler.
///
/// This is deliberately not Clone. Dropping a queued, rejected, completed, or
/// unwinding request retires exactly one reservation. An admitted request also
/// owns identity-checked session cleanup, including handler unwind.
pub(crate) struct StreamAdmissionTicket {
    state: Arc<std::sync::Mutex<SessionState>>,
    uri: String,
    cell: Arc<()>,
    order: u64,
    session: Option<(SessionKey, String)>,
}

impl Drop for StreamAdmissionTicket {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((key, session_id)) = self.session.as_ref()
            && state.sessions.get(key).is_some_and(|session| session.session_id == *session_id)
            && let Some(session) = state.sessions.remove(key)
        {
            session.settle(StreamTerminalOutcome::ProtocolEndedWithoutFinal);
        }
        let remove = state.admissions.get_mut(&self.uri).is_some_and(|cell| {
            if !Arc::ptr_eq(&cell.identity, &self.cell) {
                return false;
            }
            let Some(remaining) = cell.outstanding.checked_sub(1) else {
                return false;
            };
            cell.outstanding = remaining;
            remaining == 0
        });
        if remove {
            state.admissions.remove(&self.uri);
        }
    }
}

/// Manages active sessions and transient request-order admission authority.
pub struct StreamSessionManager {
    state: Arc<std::sync::Mutex<SessionState>>,
    generation: AtomicU64,
    #[cfg(test)]
    before_start: parking_lot::Mutex<Option<Arc<dyn Fn(&SessionKey) + Send + Sync>>>,
}

impl StreamSessionManager {
    /// Create a new, empty session manager.
    pub fn new() -> Self {
        Self {
            state: Arc::new(std::sync::Mutex::new(SessionState::default())),
            generation: AtomicU64::new(0),
            #[cfg(test)]
            before_start: parking_lot::Mutex::new(None),
        }
    }

    fn next_order(counter: &AtomicU64) -> Result<u64, ReadReservationError> {
        counter
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| value.checked_add(1))
            .map_err(|_| ReadReservationError::OrderExhausted)
    }

    /// Allocate the scheduler's order and reserve its URI in one critical section.
    ///
    /// Reservation is not admission: it does not cancel a stream or advance the
    /// latest admitted order. Ordinary reads share the checked counter but own
    /// no URI state. Keeping allocation inside this lock prevents a concurrent
    /// later request from completing and erasing a cell before an older request
    /// has registered its outstanding reservation.
    pub(crate) fn reserve_read(
        &self,
        counter: &AtomicU64,
        uri: Option<&str>,
    ) -> Result<(u64, Option<StreamAdmissionTicket>), ReadReservationError> {
        let Some(uri) = uri else {
            return Self::next_order(counter).map(|order| (order, None));
        };
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let outstanding = state
            .admissions
            .get(uri)
            .map_or(0, |cell| cell.outstanding)
            .checked_add(1)
            .ok_or(ReadReservationError::OutstandingExhausted)?;
        let order = Self::next_order(counter)?;
        let cell = state.admissions.entry(uri.to_string()).or_insert_with(|| AdmissionCell {
            identity: Arc::new(()),
            outstanding: 0,
            latest_admitted: None,
        });
        cell.outstanding = outstanding;
        let ticket = StreamAdmissionTicket {
            state: Arc::clone(&self.state),
            uri: uri.to_string(),
            cell: Arc::clone(&cell.identity),
            order,
            session: None,
        };
        Ok((order, Some(ticket)))
    }

    /// Pause a test request after its handler snapshot, before session admission.
    #[cfg(test)]
    pub(crate) fn set_before_start_hook(&self, hook: Arc<dyn Fn(&SessionKey) + Send + Sync>) {
        *self.before_start.lock() = Some(hook);
    }

    #[cfg(test)]
    fn run_before_start_hook(&self, key: &SessionKey) {
        let hook = self.before_start.lock().clone();
        if let Some(hook) = hook {
            hook(key);
        }
    }

    /// Admit an eligible request only if no newer eligible request has admitted.
    ///
    /// The order comparison, watermark update, supersession, and insertion share
    /// one lock. Completing a newer stream cannot revive an older outstanding
    /// request. Eligibility belongs to the streaming handler, not this manager.
    pub(crate) fn admit_session(
        &self,
        ticket: &mut StreamAdmissionTicket,
        key: SessionKey,
    ) -> Option<Arc<StreamSession>> {
        if !Arc::ptr_eq(&self.state, &ticket.state)
            || ticket.uri != key.uri
            || ticket.session.is_some()
        {
            return None;
        }
        #[cfg(test)]
        self.run_before_start_hook(&key);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let cell = state.admissions.get_mut(&ticket.uri)?;
        if !Arc::ptr_eq(&cell.identity, &ticket.cell)
            || cell.latest_admitted.is_some_and(|latest| latest >= ticket.order)
        {
            return None;
        }
        cell.latest_admitted = Some(ticket.order);
        let session = self.insert_session(&mut state, key.clone());
        ticket.session = Some((key, session.session_id.clone()));
        Some(session)
    }

    fn insert_session(&self, state: &mut SessionState, key: SessionKey) -> Arc<StreamSession> {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let session =
            Arc::new(StreamSession::new(format!("sess-{generation:x}"), key.line, key.character));
        state.sessions.retain(|existing, existing_session| {
            if existing.uri == key.uri {
                existing_session.cancel_with(StreamTerminalOutcome::SupersededByNewRequest);
                false
            } else {
                true
            }
        });
        state.sessions.insert(key, Arc::clone(&session));
        session
    }

    /// Seed a session for existing isolated session and lifecycle test fixtures.
    ///
    /// Production requests must carry an ingress ticket through `admit_session`.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub fn start_session(&self, key: SessionKey) -> Arc<StreamSession> {
        #[cfg(test)]
        self.run_before_start_hook(&key);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        self.insert_session(&mut state, key)
    }

    /// Remove only the exact session, without evicting a successor.
    pub fn finish_if_current(
        &self,
        key: &SessionKey,
        session_id: &str,
        outcome: StreamTerminalOutcome,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.sessions.get(key).is_none_or(|session| session.session_id != session_id) {
            return false;
        }
        if let Some(session) = state.sessions.remove(key) {
            session.settle(outcome);
            return true;
        }
        false
    }

    /// Cancel and remove active sessions for a URI, not its outstanding tickets.
    pub fn cancel_for_uri(&self, uri: &str) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.sessions.retain(|key, session| {
            if key.uri == uri {
                session.cancel_with(StreamTerminalOutcome::DocumentChangedOrClosed);
                false
            } else {
                true
            }
        });
    }

    /// Cancel active sessions older than the supplied document version.
    pub fn cancel_for_uri_version(&self, uri: &str, version: i64) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.sessions.retain(|key, session| {
            if key.uri == uri && key.document_version < version {
                session.cancel_with(StreamTerminalOutcome::DocumentChangedOrClosed);
                false
            } else {
                true
            }
        });
    }

    /// Number of genuinely active sessions.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub fn len(&self) -> usize {
        self.state.lock().unwrap_or_else(|error| error.into_inner()).sessions.len()
    }

    #[cfg(test)]
    pub(crate) fn admission_count(&self) -> usize {
        self.state.lock().unwrap_or_else(|error| error.into_inner()).admissions.len()
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

#[cfg(test)]
mod admission_tests {
    use super::*;

    fn key(uri: &str, character: u64) -> SessionKey {
        SessionKey { uri: uri.to_string(), document_version: 1, line: 0, character }
    }

    fn reserve(
        manager: &StreamSessionManager,
        counter: &AtomicU64,
        uri: &str,
    ) -> Result<StreamAdmissionTicket, String> {
        manager
            .reserve_read(counter, Some(uri))
            .map_err(|error| format!("reservation failed: {error:?}"))?
            .1
            .ok_or_else(|| "scoped reservation did not return a ticket".to_string())
    }

    #[test]
    fn completed_newer_admission_still_rejects_older_ticket() -> Result<(), String> {
        let manager = StreamSessionManager::new();
        let counter = AtomicU64::new(0);
        let mut older = reserve(&manager, &counter, "file:///a.pl")?;
        let mut newer = reserve(&manager, &counter, "file:///a.pl")?;
        let newer_key = key("file:///a.pl", 11);
        let session = manager
            .admit_session(&mut newer, newer_key.clone())
            .ok_or("newer request must admit")?;
        if !manager.finish_if_current(
            &newer_key,
            &session.session_id,
            StreamTerminalOutcome::CompletedWithCandidate,
        ) {
            return Err("newer session did not finish".into());
        }
        drop(newer);
        if manager.admission_count() != 1 || manager.len() != 0 {
            return Err(
                "completed newer request must retain only the older ticket's order cell".into()
            );
        }
        if manager.admit_session(&mut older, key("file:///a.pl", 19)).is_some() {
            return Err("completed newer admission must still reject an older ticket".into());
        }
        drop(older);
        if manager.admission_count() != 0 {
            return Err("last ticket leaked URI history".into());
        }
        Ok(())
    }

    #[test]
    fn reservation_without_admission_does_not_cancel_a_stream() -> Result<(), String> {
        let manager = StreamSessionManager::new();
        let counter = AtomicU64::new(0);
        let mut older = reserve(&manager, &counter, "file:///a.pl")?;
        let session = manager
            .admit_session(&mut older, key("file:///a.pl", 19))
            .ok_or("older request must admit")?;
        let newer = reserve(&manager, &counter, "file:///a.pl")?;
        drop(newer);
        if session.is_cancelled() || session.is_settled() || manager.len() != 1 {
            return Err("an unadmitted newer request must leave the older stream active".into());
        }
        drop(older);
        if manager.len() != 0 || manager.admission_count() != 0 {
            return Err("ticket drop must release its active session and order cell".into());
        }
        if session.terminal_outcome() != Some(StreamTerminalOutcome::ProtocolEndedWithoutFinal) {
            return Err("abandoned admitted ticket must settle its exact session".into());
        }
        Ok(())
    }

    #[test]
    fn forward_supersession_and_other_uri_keep_exact_cleanup() -> Result<(), String> {
        let manager = StreamSessionManager::new();
        let counter = AtomicU64::new(0);
        let mut older = reserve(&manager, &counter, "file:///a.pl")?;
        let old_session = manager
            .admit_session(&mut older, key("file:///a.pl", 19))
            .ok_or("older request must admit")?;
        let mut other = reserve(&manager, &counter, "file:///b.pl")?;
        let other_session = manager
            .admit_session(&mut other, key("file:///b.pl", 19))
            .ok_or("other URI must admit")?;
        let mut newer = reserve(&manager, &counter, "file:///a.pl")?;
        let new_session = manager
            .admit_session(&mut newer, key("file:///a.pl", 11))
            .ok_or("newer request must admit")?;
        if !old_session.is_cancelled() || new_session.is_cancelled() || other_session.is_cancelled()
        {
            return Err("newer admission must supersede only its own URI".into());
        }
        drop(older);
        if manager.len() != 2 || new_session.is_settled() || other_session.is_settled() {
            return Err("old ticket cleanup removed a successor or another URI".into());
        }
        drop(newer);
        if manager.len() != 1 || manager.admission_count() != 1 || other_session.is_settled() {
            return Err("one URI's last ticket cleanup affected another URI".into());
        }
        drop(other);
        if manager.len() != 0 || manager.admission_count() != 0 {
            return Err("completed tickets leaked manager entries".into());
        }
        Ok(())
    }

    #[test]
    fn tickets_cannot_admit_in_another_owner_or_uri_or_twice() -> Result<(), String> {
        let manager = StreamSessionManager::new();
        let other_manager = StreamSessionManager::new();
        let counter = AtomicU64::new(0);
        let mut ticket = reserve(&manager, &counter, "file:///a.pl")?;
        if other_manager.admit_session(&mut ticket, key("file:///a.pl", 19)).is_some()
            || manager.admit_session(&mut ticket, key("file:///b.pl", 19)).is_some()
        {
            return Err("ticket must remain bound to its exact manager and URI".into());
        }
        let session = manager
            .admit_session(&mut ticket, key("file:///a.pl", 19))
            .ok_or("original owner and URI must still admit")?;
        if manager.admit_session(&mut ticket, key("file:///a.pl", 11)).is_some()
            || session.is_cancelled()
        {
            return Err("one ticket admitted twice".into());
        }
        drop(ticket);
        if manager.admission_count() != 0 || manager.len() != 0 || other_manager.len() != 0 {
            return Err("bound-ticket cleanup leaked state".into());
        }
        Ok(())
    }

    #[test]
    fn arrival_order_exhaustion_is_permanent_and_does_not_create_cells() -> Result<(), String> {
        let manager = StreamSessionManager::new();
        let counter = AtomicU64::new(u64::MAX - 1);
        let ticket = reserve(&manager, &counter, "file:///a.pl")?;
        if ticket.order != u64::MAX - 1 || counter.load(Ordering::SeqCst) != u64::MAX {
            return Err("last available order was not reserved exactly".into());
        }
        for scope in [None, Some("file:///b.pl"), Some("file:///a.pl"), None] {
            if !matches!(
                manager.reserve_read(&counter, scope),
                Err(ReadReservationError::OrderExhausted)
            ) {
                return Err("exhausted ingress counter must reject every later reservation".into());
            }
        }
        if counter.load(Ordering::SeqCst) != u64::MAX || manager.admission_count() != 1 {
            return Err("exhaustion wrapped the counter or created an admission cell".into());
        }
        drop(ticket);
        if manager.admission_count() != 0 {
            return Err("last valid ticket leaked its cell".into());
        }
        Ok(())
    }

    #[test]
    fn outstanding_count_exhaustion_does_not_consume_an_order() -> Result<(), String> {
        let manager = StreamSessionManager::new();
        let counter = AtomicU64::new(7);
        {
            let mut state = manager.state.lock().map_err(|error| error.to_string())?;
            state.admissions.insert(
                "file:///a.pl".into(),
                AdmissionCell {
                    identity: Arc::new(()),
                    outstanding: usize::MAX,
                    latest_admitted: None,
                },
            );
        }
        for _ in 0..2 {
            if !matches!(
                manager.reserve_read(&counter, Some("file:///a.pl")),
                Err(ReadReservationError::OutstandingExhausted)
            ) {
                return Err("outstanding count exhaustion must fail closed".into());
            }
        }
        if counter.load(Ordering::SeqCst) != 7 {
            return Err("failed count reservation consumed an arrival order".into());
        }
        let state = manager.state.lock().map_err(|error| error.to_string())?;
        if state.admissions.get("file:///a.pl").map(|cell| cell.outstanding) != Some(usize::MAX) {
            return Err("outstanding count wrapped or changed on failure".into());
        }
        Ok(())
    }
}
