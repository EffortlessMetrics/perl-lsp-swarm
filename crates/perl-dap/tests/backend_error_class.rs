//! Discriminating `ErrorClass` matrix for [`BackendError`] (#8758).
//!
//! Oracle: #4979 / #8758 category assignments. Classification is by variant and
//! by caller-supplied [`BackendResponseOrigin`], never by `Display`/`Debug` or
//! message text.
//!
//! The claim under proof: an unsuccessful reply from the native adapter or a
//! negotiated peer is a *reported outcome*, not evidence that this adapter
//! violated an internal invariant. Before this slice every such reply
//! classified as [`ErrorCategory::Bug`], so an ordinary debuggee failure
//! reported by a live external peer was routed for adapter-bug repair.
//!
//! The two responders reach structurally different populations, so the samples
//! below are drawn from the *actual* construction sites of each, not invented:
//! an enum-level matrix proves internal consistency, but only site-sampled
//! coverage falsifies a classification that is wrong about the real population.

use perl_dap::backend::{BackendError, BackendResponseOrigin};
use perl_parser_core::{ErrorCategory, ErrorClass};

/// Exhaustive oracle, mirrored from the implementation so the two must agree.
///
/// No wildcard arm: a new [`BackendError`] variant fails to compile here until
/// its category is chosen deliberately.
fn expected_backend_class(error: &BackendError) -> ErrorCategory {
    match error {
        BackendError::NotConnected => ErrorCategory::Infra,
        BackendError::Transport(_) => ErrorCategory::Infra,
        BackendError::Timeout(_) => ErrorCategory::Transient,
        BackendError::ResourceLimit(_) => ErrorCategory::ResourceLimit,
        BackendError::RequestFailed { origin, .. } => expected_origin_class(*origin),
        BackendError::Unsupported(_) => ErrorCategory::UserError,
        BackendError::Protocol(_) => ErrorCategory::Protocol,
    }
}

/// Exhaustive oracle for the responder table. No wildcard arm: a third
/// responder cannot silently inherit a category.
fn expected_origin_class(origin: BackendResponseOrigin) -> ErrorCategory {
    match origin {
        BackendResponseOrigin::NativeAdapterResponse => ErrorCategory::UserError,
        BackendResponseOrigin::ExternalPeerResponse => ErrorCategory::Advisory,
    }
}

fn response_origins() -> [BackendResponseOrigin; 2] {
    [BackendResponseOrigin::NativeAdapterResponse, BackendResponseOrigin::ExternalPeerResponse]
}

fn request_failed(origin: BackendResponseOrigin, command: &str, message: &str) -> BackendError {
    BackendError::RequestFailed {
        origin,
        command: command.to_string(),
        message: message.to_string(),
    }
}

/// Reasons taken from real `success: false` sites reachable through the eleven
/// commands `NativePerlDbBackend::delegate` sends. `evaluate`, `stack_trace`,
/// `scopes`, and `variables` are `Unsupported` on that backend, so no debuggee
/// outcome appears here.
fn native_site_samples() -> Vec<(&'static str, &'static str)> {
    vec![
        // Invalid client arguments.
        ("setBreakpoints", "Missing arguments"),
        ("attach", "processId must be greater than zero"),
        ("attach", "Port 99999 out of range"),
        ("setFunctionBreakpoints", "Missing arguments"),
        // DAP request-ordering mistakes by the client.
        ("launch", "initialize request must be sent before launch"),
        ("next", "Cannot next because no Perl debug session is active."),
        // Unsupported mode.
        ("pause", "Pause is unsupported for PID-attached sessions on Windows"),
        // Known residual: these are really Infra (#8758).
        ("launch", "Cannot start Perl debugger: No such file or directory"),
        ("attach", "Cannot attach to Perl debugger at 127.0.0.1:5000"),
    ]
}

/// Reasons a live external peer can report. Unlike the native backend, the peer
/// backend routes `evaluate`, `stackTrace`, `scopes`, and `variables` through
/// the same `request()` path, so ordinary debuggee failures are reachable.
fn peer_site_samples() -> Vec<(&'static str, &'static str)> {
    vec![
        // Ordinary debuggee outcomes — the #8758 headline case.
        ("evaluate", "Undefined subroutine &main::foo called"),
        ("evaluate", "Can't locate object method \"bar\" via package \"Baz\""),
        ("evaluate", "Illegal division by zero at t/x.pl line 4."),
        ("variables", "Died: assertion failed"),
        // Peer-side refusals that are not debuggee outcomes.
        ("stackTrace", "no active suspension"),
        ("scopes", "unknown frame id"),
    ]
}

fn backend_variants() -> Vec<BackendError> {
    let mut variants = vec![
        BackendError::NotConnected,
        BackendError::Timeout("peer handshake".to_string()),
        BackendError::ResourceLimit("frame budget".to_string()),
        BackendError::Unsupported("data breakpoints".to_string()),
        BackendError::Transport("broken pipe".to_string()),
        BackendError::Protocol("expected response to evaluate".to_string()),
    ];
    for (command, message) in native_site_samples() {
        variants.push(request_failed(
            BackendResponseOrigin::NativeAdapterResponse,
            command,
            message,
        ));
    }
    for (command, message) in peer_site_samples() {
        variants.push(request_failed(
            BackendResponseOrigin::ExternalPeerResponse,
            command,
            message,
        ));
    }
    variants
}

#[test]
fn backend_error_current_variant_matrix() {
    for error in backend_variants() {
        assert_eq!(
            error.error_class(),
            expected_backend_class(&error),
            "category drifted from the #8758 oracle for {error:?}"
        );
    }
}

/// The core #8758 claim, asserted against real site samples: nothing reported
/// through either responder is an adapter bug. This is the assertion that fails
/// on the pre-slice behavior, where every sample here classified as
/// [`ErrorCategory::Bug`].
#[test]
fn no_reported_failure_is_an_adapter_bug() {
    for error in backend_variants() {
        assert_ne!(
            error.error_class(),
            ErrorCategory::Bug,
            "{error:?} must not be routed for adapter-bug repair"
        );
    }
}

/// A live peer reporting an ordinary debuggee failure is the case #8758 names.
/// It is not invalid client input — the editor's request was well formed and
/// the debuggee failed — and not a protocol violation, because a well-formed
/// `success: false` is the peer using the protocol as designed.
#[test]
fn peer_reported_debuggee_failure_is_advisory() {
    for (command, message) in peer_site_samples() {
        let error = request_failed(BackendResponseOrigin::ExternalPeerResponse, command, message);
        assert_eq!(error.error_class(), ErrorCategory::Advisory, "{message}");
        assert_ne!(error.error_class(), ErrorCategory::Bug);
        assert_ne!(error.error_class(), ErrorCategory::UserError);
        assert_ne!(error.error_class(), ErrorCategory::Protocol);
    }
}

/// The native backend cannot reach a debuggee outcome: its inspection families
/// are `Unsupported`. Its reachable population is dominated by client-correctable
/// argument and request-ordering mistakes.
#[test]
fn native_reported_failure_is_user_correctable() {
    for (command, message) in native_site_samples() {
        let error = request_failed(BackendResponseOrigin::NativeAdapterResponse, command, message);
        assert_eq!(error.error_class(), ErrorCategory::UserError, "{message}");
        assert_ne!(error.error_class(), ErrorCategory::Bug);
    }
}

/// The responder, not the payload, decides the category. Identical bytes under
/// two responders must classify differently — the property that proves origin is
/// load-bearing rather than decorative.
#[test]
fn identical_text_under_different_responders_classifies_differently() {
    let message = "Undefined subroutine &main::foo called";
    let native = request_failed(BackendResponseOrigin::NativeAdapterResponse, "evaluate", message);
    let peer = request_failed(BackendResponseOrigin::ExternalPeerResponse, "evaluate", message);

    assert_eq!(native.to_string(), peer.to_string(), "same rendered text");
    assert_ne!(
        native.error_class(),
        peer.error_class(),
        "classification must follow the caller-supplied responder, not the text"
    );
}

/// Classification must never be driven by the rendered message.
#[test]
fn classification_does_not_inspect_rendered_text() {
    let bait = "Bug Protocol ResourceLimit Infra Transient user_error advisory";
    for origin in response_origins() {
        let error = request_failed(origin, "evaluate", bait);
        assert_eq!(error.error_class(), expected_origin_class(origin));
        assert!(
            error.to_string().contains(bait),
            "payload text is retained on Display without driving classification"
        );
    }
}

/// Structured fields are retained for receipts and logs, so a later audit of
/// either population can be done from evidence rather than by re-reading source.
#[test]
fn responder_and_command_survive_construction() {
    let error = BackendError::RequestFailed {
        origin: BackendResponseOrigin::ExternalPeerResponse,
        command: "stackTrace".to_string(),
        message: "no active suspension".to_string(),
    };
    match &error {
        BackendError::RequestFailed { origin, command, message } => {
            assert_eq!(*origin, BackendResponseOrigin::ExternalPeerResponse);
            assert_eq!(command, "stackTrace");
            assert_eq!(message, "no active suspension");
        }
        other => panic!("expected RequestFailed, got {other:?}"),
    }
}

/// Machine tokens are part of the public contract for receipt/log layers.
#[test]
fn response_origin_tokens_are_stable() {
    assert_eq!(BackendResponseOrigin::NativeAdapterResponse.as_str(), "native_adapter_response");
    assert_eq!(BackendResponseOrigin::ExternalPeerResponse.as_str(), "external_peer_response");
}

/// `DapPeerBridge::error` renders this error straight onto the DAP wire, so the
/// editor-visible text must not drift when the variant gains structure.
#[test]
fn dap_wire_display_messages_are_unchanged() {
    let error = request_failed(
        BackendResponseOrigin::ExternalPeerResponse,
        "evaluate",
        "Undefined subroutine &main::foo called",
    );
    assert_eq!(
        error.to_string(),
        "debug backend reported an error: Undefined subroutine &main::foo called"
    );

    // Sibling variants are untouched by this slice.
    assert_eq!(BackendError::NotConnected.to_string(), "debug backend is not connected");
    assert_eq!(
        BackendError::Unsupported("data breakpoints".to_string()).to_string(),
        "operation not supported by this backend: data breakpoints"
    );
}
