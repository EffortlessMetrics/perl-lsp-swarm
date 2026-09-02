//! Discriminating `ErrorClass` proof for [`BackendError::PeerReported`] (#8758).
//!
//! Oracle: #4979 / #8758 category assignments.
//!
//! The claim under proof is deliberately one axis wide: **a failure a negotiated
//! external peer reports is a reported outcome, not evidence that this adapter
//! violated an internal invariant.** Before this slice every such reply arrived
//! as [`BackendError::Engine`] and classified as [`ErrorCategory::Bug`], so an
//! ordinary debuggee `die` reported by a live peer was routed for adapter-bug
//! repair.
//!
//! ## What is deliberately not claimed
//!
//! `PeerReported` is a heterogeneous bucket. It holds ordinary debuggee
//! outcomes *and* peer-side refusals such as `no active suspension`, which are
//! closer to a client or session-state error. `PeerResponse` carries no cause or
//! code field, so that distinction is genuinely absent at this boundary and
//! recovering it by reading `message` would make classification depend on
//! peer-authored free text — the exact coupling `classification_ignores_message_text`
//! forbids below. Separating those populations needs a cause on the peer wire
//! (#14582).
//!
//! So these tests assert only what the evidence supports. In particular there is
//! no test asserting a category for a *cause* the wire never carried: a test
//! that pinned `unknown frame id` to a specific category would ratchet in a
//! classification this boundary cannot actually justify.

use perl_dap::backend::BackendError;
use perl_parser_core::{ErrorCategory, ErrorClass};

fn peer_reported(command: &str, message: &str) -> BackendError {
    BackendError::PeerReported { command: command.to_string(), message: message.to_string() }
}

/// Reasons a live external peer can report, drawn from real construction sites
/// rather than invented. Both populations are present on purpose: the claim
/// under proof must hold for the whole bucket, not just its flattering half.
fn peer_site_samples() -> Vec<(&'static str, &'static str)> {
    vec![
        // Ordinary debuggee outcomes — the #8758 headline case.
        ("evaluate", "Undefined subroutine &main::foo called"),
        ("evaluate", "Can't locate object method \"bar\" via package \"Baz\""),
        ("evaluate", "Illegal division by zero at t/x.pl line 4."),
        ("variables", "Died: assertion failed"),
        // Peer-side refusals that are not debuggee outcomes. No finer category
        // is asserted for these (see the module note); they are here to prove
        // the not-a-bug claim covers them too.
        ("stackTrace", "no active suspension"),
        ("scopes", "unknown frame id"),
    ]
}

fn backend_variants() -> Vec<BackendError> {
    vec![
        BackendError::NotConnected,
        BackendError::Timeout("timeout".to_string()),
        BackendError::ResourceLimit("limit".to_string()),
        BackendError::Engine("engine".to_string()),
        peer_reported("evaluate", "Undefined subroutine &main::foo called"),
        BackendError::Unsupported("unsupported".to_string()),
        BackendError::Transport("transport".to_string()),
        BackendError::Protocol("protocol".to_string()),
    ]
}

/// Public-contract lock over the whole variant set.
///
/// This is an explicit change-detector, not a cause-axis proof: it fails if a
/// variant is silently recategorized. No wildcard arm, so a new variant must
/// choose its category deliberately rather than inherit one.
#[test]
fn backend_error_category_contract() {
    for error in backend_variants() {
        let expected = match &error {
            BackendError::NotConnected | BackendError::Transport(_) => ErrorCategory::Infra,
            BackendError::Timeout(_) => ErrorCategory::Transient,
            BackendError::ResourceLimit(_) => ErrorCategory::ResourceLimit,
            BackendError::Engine(_) => ErrorCategory::Bug,
            BackendError::PeerReported { .. } => ErrorCategory::Advisory,
            BackendError::Unsupported(_) => ErrorCategory::UserError,
            BackendError::Protocol(_) => ErrorCategory::Protocol,
        };
        assert_eq!(error.error_class(), expected, "{error}");
    }
}

/// The core #8758 claim. This is the assertion that fails against `main`, where
/// every one of these arrived as `Engine` and classified as [`ErrorCategory::Bug`].
///
/// It holds for the entire bucket — debuggee outcomes and peer refusals alike —
/// which is the strongest statement the peer wire actually supports.
#[test]
fn no_peer_reported_failure_is_an_adapter_bug() {
    for (command, message) in peer_site_samples() {
        assert_ne!(
            peer_reported(command, message).error_class(),
            ErrorCategory::Bug,
            "peer-reported `{command}` failure classified as an adapter bug: {message}"
        );
    }
}

/// Classification must not read the reason text. The peer authors that string,
/// so deriving a category from it would let a debuggee's own output steer
/// adapter triage — and would silently re-introduce the cause guessing this
/// slice removed.
#[test]
fn classification_ignores_message_text() {
    let adversarial = [
        "internal error: adapter invariant violated",
        "panicked at src/lib.rs:1:1",
        "protocol violation: bad frame",
        "transport closed",
        "",
    ];
    for message in adversarial {
        assert_eq!(
            peer_reported("evaluate", message).error_class(),
            ErrorCategory::Advisory,
            "category changed with message text: {message:?}"
        );
    }
}

/// Two failures whose rendered text is identical must still classify by variant.
/// This pins the *mechanism*: the category comes from which boundary observed
/// the reply, not from what the reply said.
#[test]
fn identical_text_classifies_by_variant_not_text() {
    let engine = BackendError::Engine("Undefined subroutine &main::foo called".to_string());
    let peer = peer_reported("evaluate", "Undefined subroutine &main::foo called");

    assert_eq!(engine.to_string(), peer.to_string(), "same rendered text");
    assert_ne!(
        engine.error_class(),
        peer.error_class(),
        "identical text must not force identical classification"
    );
}

/// `command` is retained for receipts and logs and must survive construction.
#[test]
fn peer_command_survives_construction() {
    let error = peer_reported("stackTrace", "no active suspension");
    match &error {
        BackendError::PeerReported { command, message } => {
            assert_eq!(command, "stackTrace");
            assert_eq!(message, "no active suspension");
        }
        other => panic!("expected PeerReported, got {other:?}"),
    }
}

/// Editor-visible text is a wire contract. `peer_bridge::DapPeerBridge` renders
/// `Display` straight onto the DAP response, so `PeerReported` must render
/// exactly as the `Engine` variant it replaces on this path.
#[test]
fn dap_wire_display_messages_are_unchanged() {
    assert_eq!(
        peer_reported("evaluate", "Undefined subroutine &main::foo called").to_string(),
        "debug backend reported an error: Undefined subroutine &main::foo called"
    );
    assert_eq!(
        BackendError::Engine("Undefined subroutine &main::foo called".to_string()).to_string(),
        "debug backend reported an error: Undefined subroutine &main::foo called"
    );
    assert_eq!(BackendError::NotConnected.to_string(), "debug backend is not connected");
    assert_eq!(
        BackendError::Transport("closed".to_string()).to_string(),
        "debug backend transport error: closed"
    );
}
