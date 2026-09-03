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
//! ## The cause axis (#14582)
//!
//! `PeerReported` was a heterogeneous bucket: ordinary debuggee outcomes *and*
//! peer-side refusals such as `no active suspension`, which are closer to a
//! client or session-state error. The distinction was genuinely absent at this
//! boundary because the wire carried no cause, and recovering it by reading
//! `message` would have made classification depend on peer-authored free text —
//! the exact coupling `classification_ignores_message_text` still forbids below.
//!
//! The wire now carries an optional machine-readable
//! [`PeerFailureCause`], negotiated through `peer/hello`, so the finer
//! categories are asserted *from that vocabulary* rather than guessed from
//! prose. The earlier note in this module said a test pinning `unknown frame id`
//! to a category would ratchet in a classification this boundary could not
//! justify. That is still true of the **text** `unknown frame id`, and the
//! samples below still assert no category for it. What is pinned now is the
//! category of a *declared cause*, which is a different and now-supported claim.
//!
//! Two invariants survive the widening, and the tests here exist mostly to
//! defend them:
//!
//! 1. **No cause makes a peer failure an adapter [`ErrorCategory::Bug`].** A
//!    peer describes its own side; it is never evidence that this adapter broke
//!    an invariant. If a peer-supplied value could reach `Bug`, a debuggee could
//!    route this adapter for repair by failing in the right shape — the coupling
//!    #8758 removed.
//! 2. **An absent or unrecognised cause classifies exactly as before.** Older
//!    peers, un-advertised peers, and newer vocabularies all land on the
//!    pre-#14582 answer instead of a guess.

use perl_dap::backend::BackendError;
use perl_dap::peer_protocol::PeerFailureCause;
use perl_parser_core::{ErrorCategory, ErrorClass};

/// Build the error the external-peer path produces for a `success: false`
/// reply, so each test states the peer's reported reason rather than the
/// variant's construction noise.
fn peer_reported(command: &str, message: &str) -> BackendError {
    BackendError::PeerReported {
        command: command.to_string(),
        message: message.to_string(),
        cause: None,
    }
}

/// The same reply from a peer that advertised the cause vocabulary and used it.
///
/// Only the negotiated construction site in `external_peer.rs` may produce a
/// `Some` here; that gate is proved separately by
/// `cause_from_an_unadvertised_peer_is_not_honoured` in that module. These tests
/// own the mapping from a cause to a category, not how the cause is admitted.
fn peer_reported_because(command: &str, message: &str, cause: PeerFailureCause) -> BackendError {
    BackendError::PeerReported {
        command: command.to_string(),
        message: message.to_string(),
        cause: Some(cause),
    }
}

/// Every cause this build understands, so the invariant tests below range over
/// the whole vocabulary instead of a flattering subset. `Unrecognized` is
/// included on purpose: it is reachable from any newer peer.
fn every_cause() -> Vec<PeerFailureCause> {
    vec![
        PeerFailureCause::Debuggee,
        PeerFailureCause::SessionState,
        PeerFailureCause::InvalidRequest,
        PeerFailureCause::Transport,
        PeerFailureCause::Unrecognized,
    ]
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

/// One instance of every [`BackendError`] variant, so the contract test below
/// covers the whole public surface rather than only the arms this slice touched.
fn backend_variants() -> Vec<BackendError> {
    vec![
        BackendError::NotConnected,
        BackendError::Timeout("timeout".to_string()),
        BackendError::ResourceLimit("limit".to_string()),
        BackendError::Engine("engine".to_string()),
        peer_reported("evaluate", "Undefined subroutine &main::foo called"),
        peer_reported_because("evaluate", "Illegal division by zero", PeerFailureCause::Debuggee),
        peer_reported_because("stackTrace", "no active suspension", PeerFailureCause::SessionState),
        peer_reported_because("scopes", "unknown frame id", PeerFailureCause::InvalidRequest),
        peer_reported_because("variables", "debuggee link lost", PeerFailureCause::Transport),
        peer_reported_because("evaluate", "something newer", PeerFailureCause::Unrecognized),
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
            BackendError::PeerReported { cause, .. } => match cause {
                Some(PeerFailureCause::Debuggee) => ErrorCategory::Advisory,
                Some(PeerFailureCause::SessionState | PeerFailureCause::InvalidRequest) => {
                    ErrorCategory::UserError
                }
                Some(PeerFailureCause::Transport) => ErrorCategory::Infra,
                Some(PeerFailureCause::Unrecognized) | None => ErrorCategory::Advisory,
                // `PeerFailureCause` is `#[non_exhaustive]`, so this crate — an
                // integration test, and therefore a downstream consumer — is
                // required to carry a fallback and cannot lock the cause axis by
                // exhaustiveness the way it locks the variant axis above.
                //
                // The guard that matters is not lost. `error_class` matches this
                // same type from *inside* `perl-dap`, where `#[non_exhaustive]`
                // does not apply, so a new cause that no arm handles fails to
                // compile there. What this arm gives up is only the independent
                // second opinion: a new cause added to `every_cause()` and
                // `backend_variants()` still trips this test unless production
                // agreed on `Advisory`, which is the honest default to assume.
                _ => ErrorCategory::Advisory,
            },
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
        BackendError::PeerReported { command, message, cause } => {
            assert_eq!(command, "stackTrace");
            assert_eq!(message, "no active suspension");
            assert_eq!(*cause, None, "this helper builds the causeless reply");
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

// ---------------------------------------------------------------------------
// The cause axis (#14582)
// ---------------------------------------------------------------------------

/// Invariant 1, over the whole vocabulary.
///
/// A peer supplies the cause, so if any value it can send reached
/// [`ErrorCategory::Bug`] a debuggee could route this adapter for repair by
/// failing in the right shape. That is the coupling #8758 removed, and widening
/// the axis must not reintroduce it through a new door.
#[test]
fn no_reported_cause_is_an_adapter_bug() {
    for cause in every_cause() {
        assert_ne!(
            peer_reported_because("evaluate", "boom", cause).error_class(),
            ErrorCategory::Bug,
            "peer-supplied cause {cause:?} reached the adapter-bug category"
        );
    }
}

/// The #14582 headline: the heterogeneous bucket actually separates.
///
/// Against `main` every row here is [`ErrorCategory::Advisory`], so the debuggee
/// rows pass and the session/client rows fail — which is the point. A debuggee's
/// own `die` is an outcome to report; being asked for a stack trace with nothing
/// suspended is not, and triage should not have to treat them alike.
#[test]
fn cause_separates_debuggee_outcomes_from_session_and_client_errors() {
    let rows = [
        // Ordinary debuggee outcomes — the request was served, this is its result.
        (
            "evaluate",
            "Undefined subroutine &main::foo called",
            PeerFailureCause::Debuggee,
            ErrorCategory::Advisory,
        ),
        (
            "evaluate",
            "Illegal division by zero at t/x.pl line 4.",
            PeerFailureCause::Debuggee,
            ErrorCategory::Advisory,
        ),
        // Peer-side refusals — something above this adapter must change.
        (
            "stackTrace",
            "no active suspension",
            PeerFailureCause::SessionState,
            ErrorCategory::UserError,
        ),
        ("scopes", "unknown frame id", PeerFailureCause::InvalidRequest, ErrorCategory::UserError),
        // The peer's own link to the debuggee died mid-request.
        ("variables", "debuggee link lost", PeerFailureCause::Transport, ErrorCategory::Infra),
    ];
    for (command, message, cause, expected) in rows {
        assert_eq!(
            peer_reported_because(command, message, cause).error_class(),
            expected,
            "`{command}` with cause {cause:?} classified wrongly"
        );
    }
}

/// The discriminating control, in both directions.
///
/// Forward: **identical reason text** under two different causes must classify
/// differently — so the category demonstrably comes from the declared cause and
/// not from the prose. Converse: **one cause under wildly different texts**,
/// including texts that impersonate other categories, must classify the same —
/// so the text cannot perturb a cause that is present either.
///
/// A classifier that peeked at `message` fails one direction or the other.
#[test]
fn identical_text_classifies_by_cause_not_text() {
    let text = "no active suspension";

    let debuggee = peer_reported_because("stackTrace", text, PeerFailureCause::Debuggee);
    let session = peer_reported_because("stackTrace", text, PeerFailureCause::SessionState);

    assert_eq!(debuggee.to_string(), session.to_string(), "same rendered text");
    assert_ne!(
        debuggee.error_class(),
        session.error_class(),
        "identical text under different causes must not force identical classification"
    );

    let impersonators = [
        "internal error: adapter invariant violated",
        "panicked at src/lib.rs:1:1",
        "protocol violation: bad frame",
        "transport closed",
        "",
    ];
    for message in impersonators {
        assert_eq!(
            peer_reported_because("evaluate", message, PeerFailureCause::Debuggee).error_class(),
            ErrorCategory::Advisory,
            "a declared debuggee cause was perturbed by message text: {message:?}"
        );
        assert_eq!(
            peer_reported_because("scopes", message, PeerFailureCause::InvalidRequest)
                .error_class(),
            ErrorCategory::UserError,
            "a declared invalid-request cause was perturbed by message text: {message:?}"
        );
    }
}

/// Invariant 2: absence is a fallback, never a guess.
///
/// An older peer, a peer that did not advertise the vocabulary, and a peer whose
/// cause is newer than this build all land on the pre-#14582 answer. The
/// `Unrecognized` half is what keeps a future vocabulary from silently acquiring
/// a category nobody chose.
#[test]
fn an_absent_or_unrecognised_cause_keeps_the_pre_cause_classification() {
    for (command, message) in peer_site_samples() {
        assert_eq!(
            peer_reported(command, message).error_class(),
            ErrorCategory::Advisory,
            "a causeless `{command}` failure must classify as it did before #14582"
        );
        assert_eq!(
            peer_reported_because(command, message, PeerFailureCause::Unrecognized).error_class(),
            peer_reported(command, message).error_class(),
            "an unrecognised cause must classify exactly as an absent one"
        );
    }
}

/// A cause is a structured field for classification and receipts, never editor
/// copy. `peer_bridge::DapPeerBridge` renders `Display` straight onto the DAP
/// response, so a cause must not change one byte the editor sees.
#[test]
fn cause_never_reaches_the_editor_visible_text() {
    let baseline = peer_reported("evaluate", "Undefined subroutine &main::foo called").to_string();
    for cause in every_cause() {
        assert_eq!(
            peer_reported_because("evaluate", "Undefined subroutine &main::foo called", cause)
                .to_string(),
            baseline,
            "cause {cause:?} altered the editor-visible text"
        );
    }
}
