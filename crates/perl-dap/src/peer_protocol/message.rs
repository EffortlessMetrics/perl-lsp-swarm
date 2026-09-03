//! The peer-protocol message envelope.
//!
//! Structurally identical to the DAP envelope family (a `type`-tagged
//! request/response/event), so the future ptkdb side can reuse ordinary
//! DAP-style message plumbing. Command and event names are strings (like DAP);
//! typed argument/body payloads live in [`super::payloads`].

#[cfg(test)]
use perl_tdd_support::must;
use serde::{Deserialize, Serialize};

/// A request from one peer to the other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerRequest {
    /// Monotonic sequence number from the sender.
    pub seq: i64,
    /// Command name (see [`command`]).
    pub command: String,
    /// Command arguments, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

/// Why a peer answered `success: false` (#14582).
///
/// This is a *machine-readable* cause, deliberately separate from
/// [`PeerResponse::message`]. The host classifies a reported failure from this
/// vocabulary and never from the reason text, because the reason text is
/// authored by the peer (and frequently by the debuggee itself), so deriving
/// triage from it would let a program under debug steer how this adapter is
/// diagnosed. See `crates/perl-dap/tests/backend_error_class.rs`.
///
/// The arms describe the *peer's* world, not the host's category. The mapping
/// onto [`perl_parser_core::ErrorCategory`] lives in
/// [`BackendError::error_class`] and is deliberately coarser than this
/// vocabulary, so the host can refine its own triage without a wire change.
///
/// A peer may only have its cause honoured when it advertised
/// [`PeerReportedCapabilities::can_report_failure_cause`] in `peer/hello`.
///
/// # Extensibility
///
/// `#[non_exhaustive]`, because this vocabulary is explicitly designed to grow —
/// that is the whole reason [`Self::Unrecognized`] exists. Recognising a cause
/// this build currently maps to `Unrecognized` must stay a routine addition, not
/// a source-breaking change for a downstream crate that matched exhaustively.
/// Marking it here, before the first release that carries it, is the only point
/// at which that costs nothing.
///
/// The host's own classification is unaffected: `error_class` in
/// [`crate::backend`] matches this type from inside the crate, where
/// exhaustiveness is still enforced, so a new cause cannot silently inherit a
/// category there.
///
/// [`BackendError::error_class`]: crate::backend::BackendError
/// [`PeerReportedCapabilities::can_report_failure_cause`]:
///     crate::peer_protocol::PeerReportedCapabilities::can_report_failure_cause
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PeerFailureCause {
    /// The debuggee itself failed: a `die`, an undefined subroutine, a runtime
    /// error. The request was served correctly and this is its outcome.
    Debuggee,
    /// The session is not in a state that can serve the request — for example
    /// `stackTrace` while nothing is suspended.
    SessionState,
    /// The request itself was not answerable as asked — for example an unknown
    /// frame id or an out-of-range variables reference.
    InvalidRequest,
    /// The peer's own link to the debuggee failed while serving the request.
    Transport,
    /// A cause this host does not recognise.
    ///
    /// Reached only by deserialization, when a newer peer reports a cause added
    /// after this build. It exists so an unknown vocabulary word degrades to the
    /// no-cause classification instead of failing the whole response to parse —
    /// a reported failure must never become a protocol error just because its
    /// cause is newer than the host.
    ///
    /// It is never a value the host should *send*, and it classifies exactly as
    /// an absent cause does.
    #[serde(other)]
    Unrecognized,
}

/// A response to a [`PeerRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerResponse {
    /// Monotonic sequence number from the responder.
    pub seq: i64,
    /// The `seq` of the request being answered.
    pub request_seq: i64,
    /// Whether the request succeeded.
    pub success: bool,
    /// The command being answered (echoed).
    pub command: String,
    /// Error message when `success` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Machine-readable cause when `success` is false (#14582).
    ///
    /// Absent for every peer built before this field existed, and for a peer
    /// that did not advertise
    /// [`PeerReportedCapabilities::can_report_failure_cause`]. Absence is not an
    /// error: it is the honest statement that the responder's cause is unknown,
    /// and the host classifies such a failure exactly as it did before this
    /// field existed.
    ///
    /// [`PeerReportedCapabilities::can_report_failure_cause`]:
    ///     crate::peer_protocol::PeerReportedCapabilities::can_report_failure_cause
    #[serde(
        default,
        deserialize_with = "deserialize_lenient_cause",
        skip_serializing_if = "Option::is_none"
    )]
    pub cause: Option<PeerFailureCause>,
    /// Response body, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

/// Deserialize [`PeerResponse::cause`] without letting a malformed value
/// destroy the reply that carries it (#14582).
///
/// `#[serde(other)]` rescues an unrecognised cause *word*, but not a value of
/// the wrong JSON *shape*. Without this, a peer sending `"cause": 123` — or some
/// future dialect sending a richer `{"code": …}` form — would fail the entire
/// [`PeerResponse`] to parse. The reader thread drops a frame it cannot
/// deserialize, so the pending request would never be answered and the host
/// would report a `Timeout` (`Transient`) instead of the `PeerReported` failure
/// the peer actually sent. Losing the failure is strictly worse than reporting
/// it with an unknown cause.
///
/// So anything that is not a recognised cause string degrades to
/// [`PeerFailureCause::Unrecognized`], which classifies exactly as an absent
/// cause does. Explicit JSON `null` stays `None` — the peer said nothing, rather
/// than something this build failed to read.
fn deserialize_lenient_cause<'de, D>(deserializer: D) -> Result<Option<PeerFailureCause>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match raw {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => match serde_json::from_value::<PeerFailureCause>(value) {
            Ok(cause) => Some(cause),
            Err(_) => Some(PeerFailureCause::Unrecognized),
        },
    })
}

/// An asynchronous event from one peer to the other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerEvent {
    /// Monotonic sequence number from the sender.
    pub seq: i64,
    /// Event name (see [`event`]).
    pub event: String,
    /// Event body, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

/// Any peer-protocol message.
///
/// Internally tagged on `type` (`"request"` / `"response"` / `"event"`), matching
/// the DAP envelope convention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PeerMessage {
    /// A request.
    Request(PeerRequest),
    /// A response.
    Response(PeerResponse),
    /// An event.
    Event(PeerEvent),
}

impl PeerMessage {
    /// The `seq` of the underlying message.
    #[must_use]
    pub fn seq(&self) -> i64 {
        match self {
            PeerMessage::Request(r) => r.seq,
            PeerMessage::Response(r) => r.seq,
            PeerMessage::Event(e) => e.seq,
        }
    }
}

/// Well-known command names (host↔peer requests).
pub mod command {
    /// Peer→host handshake.
    pub const HELLO: &str = "peer/hello";
    /// Either→either graceful shutdown.
    pub const GOODBYE: &str = "peer/goodbye";
    /// Host→peer: replace source breakpoints.
    pub const SET_BREAKPOINTS: &str = "debugger/setBreakpoints";
    /// Host→peer: replace function breakpoints.
    pub const SET_FUNCTION_BREAKPOINTS: &str = "debugger/setFunctionBreakpoints";
    /// Host→peer: resume.
    pub const CONTINUE: &str = "debugger/continue";
    /// Host→peer: step over.
    pub const NEXT: &str = "debugger/next";
    /// Host→peer: step in.
    pub const STEP_IN: &str = "debugger/stepIn";
    /// Host→peer: step out.
    pub const STEP_OUT: &str = "debugger/stepOut";
    /// Host→peer: pause.
    pub const PAUSE: &str = "debugger/pause";
    /// Host→peer: fetch stack trace.
    pub const STACK_TRACE: &str = "debugger/stackTrace";
    /// Host→peer: fetch scopes for a frame.
    pub const SCOPES: &str = "debugger/scopes";
    /// Host→peer: fetch variables for a reference.
    pub const VARIABLES: &str = "debugger/variables";
    /// Host→peer: evaluate an expression.
    pub const EVALUATE: &str = "debugger/evaluate";
    /// Host→peer: disconnect.
    pub const DISCONNECT: &str = "debugger/disconnect";
}

/// Well-known event names (peer→host, mostly).
pub mod event {
    /// Peer is initialized and ready for configuration.
    pub const INITIALIZED: &str = "debugger/initialized";
    /// The debuggee stopped.
    pub const STOPPED: &str = "debugger/stopped";
    /// The debuggee resumed.
    pub const CONTINUED: &str = "debugger/continued";
    /// The debuggee produced output.
    pub const OUTPUT: &str = "debugger/output";
    /// The session terminated.
    pub const TERMINATED: &str = "debugger/terminated";
    /// New static source facts are available.
    pub const SOURCE_FACTS: &str = "debugger/sourceFacts";
    /// Breakpoints changed state.
    pub const BREAKPOINTS_CHANGED: &str = "debugger/breakpointsChanged";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_message_serializes_with_type_tag() {
        let msg = PeerMessage::Request(PeerRequest {
            seq: 1,
            command: command::HELLO.to_string(),
            arguments: None,
        });
        let json = must(serde_json::to_value(&msg));
        assert_eq!(json["type"], "request");
        assert_eq!(json["command"], "peer/hello");
        assert_eq!(json["seq"], 1);
    }

    #[test]
    fn response_uses_camel_case_request_seq() {
        let msg = PeerMessage::Response(PeerResponse {
            seq: 2,
            request_seq: 1,
            success: true,
            command: command::HELLO.to_string(),
            message: None,
            cause: None,
            body: None,
        });
        let json = must(serde_json::to_value(&msg));
        assert_eq!(json["type"], "response");
        assert_eq!(json["requestSeq"], 1);
        assert!(json.get("request_seq").is_none());
        // An absent cause must not appear on the wire at all, so a response
        // from this host stays byte-identical to one a pre-#14582 build sent.
        assert!(json.get("cause").is_none(), "absent cause must not be serialized");
    }

    #[test]
    fn failure_cause_uses_snake_case_on_the_wire() {
        let msg = PeerMessage::Response(PeerResponse {
            seq: 2,
            request_seq: 1,
            success: false,
            command: command::STACK_TRACE.to_string(),
            message: Some("no active suspension".to_string()),
            cause: Some(PeerFailureCause::SessionState),
            body: None,
        });
        let json = must(serde_json::to_value(&msg));
        assert_eq!(json["cause"], "session_state");
        let back: PeerMessage = must(serde_json::from_value(json));
        assert_eq!(back, msg, "cause must round-trip");
    }

    /// A peer built before #14582 sends no `cause` key at all. That must
    /// deserialize, not fail, and must mean "unknown" rather than any cause.
    #[test]
    fn a_response_without_a_cause_still_deserializes() {
        let resp: PeerResponse = must(serde_json::from_str(
            r#"{"seq":2,"requestSeq":1,"success":false,"command":"stackTrace",
                "message":"no active suspension"}"#,
        ));
        assert_eq!(resp.cause, None);
    }

    /// Forward compatibility, at the layer where it is decided.
    ///
    /// A newer peer reporting a cause this build has never heard of must not
    /// take the whole response down with it: a reported failure would then
    /// surface as a protocol error, which is a worse answer than the causeless
    /// one this degrades to. `#[serde(other)]` is what buys that, so this test
    /// fails the moment the catch-all arm is removed.
    #[test]
    fn an_unknown_cause_degrades_instead_of_failing_the_response() {
        let resp: PeerResponse = must(serde_json::from_str(
            r#"{"seq":2,"requestSeq":1,"success":false,"command":"stackTrace",
                "message":"no active suspension","cause":"quantum_decoherence"}"#,
        ));
        assert_eq!(resp.cause, Some(PeerFailureCause::Unrecognized));
        assert_eq!(resp.message.as_deref(), Some("no active suspension"));
    }

    /// A cause of the wrong JSON *shape* must not destroy the reply either.
    ///
    /// `#[serde(other)]` alone does not cover this: it rescues an unrecognised
    /// string, but a number, array, object, or bool would fail the whole
    /// `PeerResponse` to parse. The reader thread drops a frame it cannot
    /// deserialize, so the failure the peer actually reported would surface as a
    /// `Timeout` instead — losing it entirely. Each row here fails without
    /// `deserialize_lenient_cause`.
    #[test]
    fn a_cause_of_the_wrong_json_type_still_yields_the_reply() {
        for malformed in ["123", "true", "[\"session_state\"]", "{\"code\":\"session_state\"}"] {
            let body = format!(
                r#"{{"seq":2,"requestSeq":1,"success":false,"command":"stackTrace",
                     "message":"no active suspension","cause":{malformed}}}"#
            );
            let resp: PeerResponse = must(serde_json::from_str(&body));
            assert_eq!(
                resp.cause,
                Some(PeerFailureCause::Unrecognized),
                "malformed cause {malformed} must degrade, not fail the response"
            );
            assert_eq!(
                resp.message.as_deref(),
                Some("no active suspension"),
                "the reported reason must survive a malformed cause: {malformed}"
            );
        }
    }

    /// An explicit JSON `null` is the peer saying nothing, which is different
    /// from this build failing to read something. It stays `None`.
    #[test]
    fn an_explicit_null_cause_is_absent_not_unrecognized() {
        let resp: PeerResponse = must(serde_json::from_str(
            r#"{"seq":2,"requestSeq":1,"success":false,"command":"stackTrace",
                "message":"no active suspension","cause":null}"#,
        ));
        assert_eq!(resp.cause, None);
    }

    /// Each known word maps to its own arm — a negative control against a
    /// catch-all that silently swallowed the whole vocabulary.
    #[test]
    fn every_known_cause_word_parses_to_its_own_arm() {
        let rows = [
            ("debuggee", PeerFailureCause::Debuggee),
            ("session_state", PeerFailureCause::SessionState),
            ("invalid_request", PeerFailureCause::InvalidRequest),
            ("transport", PeerFailureCause::Transport),
        ];
        for (word, expected) in rows {
            let parsed: PeerFailureCause = must(serde_json::from_str(&format!("\"{word}\"")));
            assert_eq!(parsed, expected, "{word} must not fall through to the catch-all");
        }
    }

    #[test]
    fn event_round_trips() {
        let msg = PeerMessage::Event(PeerEvent {
            seq: 10,
            event: event::STOPPED.to_string(),
            body: Some(serde_json::json!({"reason": "breakpoint"})),
        });
        let json = must(serde_json::to_string(&msg));
        let back: PeerMessage = must(serde_json::from_str(&json));
        assert_eq!(msg, back);
        assert_eq!(back.seq(), 10);
    }
}
