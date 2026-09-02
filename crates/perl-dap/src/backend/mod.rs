//! Backend abstraction for Perl debug sessions.
//!
//! The DAP frontend ([`crate::debug_adapter`]) speaks DAP to editors. Behind it,
//! a [`DebugBackend`] drives *some* debugger engine — the stock `perl -d`
//! runtime, the legacy Perl::LanguageServer bridge, or an external debugger peer
//! (ptkdb-first). Every backend is expressed in terms of the canonical,
//! backend-neutral [`crate::model`] vocabulary, never DAP wire types. That is
//! what lets an external engine cooperate without implementing DAP.
//!
//! See [`docs/reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md`] (decision D1) for
//! why the contract is model-typed rather than `DapMessage`-typed.

#[cfg(test)]
use perl_tdd_support::must;
pub mod capabilities;
pub mod external_peer;
pub mod native_perldb;
pub mod peer_bridge;
pub mod peer_launch;

use std::collections::HashMap;
use std::path::PathBuf;

pub use capabilities::{DebugBackendCapabilities, intersect_dap_capabilities};
pub use external_peer::PeerSessionToken;
pub use peer_bridge::{DapPeerBridge, run_external_peer_session_stdio};
pub use peer_launch::{
    ExternalPeerLaunchConfig, MirrorPeerBridge, PeerListenEndpoint, PeerRendezvousMode,
    prepare_mirror_listen_session, run_mirror_listen_session_stdio, static_mirror_capabilities,
};

use crate::model::{
    DebugBreakpoint, DebugEvent, DebugFunctionBreakpoint, DebugScope, DebugSource, DebugStackFrame,
    DebugVariable, FrameId, ResolvedBreakpoint, ThreadId, VariablesRef,
};

/// Which responder answered a backend request unsuccessfully (#8758).
///
/// The boundary that observed the reply supplies this value literally. It is
/// never inferred from `Display`, `Debug`, or message text — the same rule
/// [`crate::parse_origin::DebuggerOutputOrigin`] establishes for parse inputs
/// (#8746).
///
/// This enum is exhaustive and has no [`Default`]. A new responder must choose
/// its category explicitly instead of inheriting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendResponseOrigin {
    /// The in-process native adapter answered `success: false`.
    ///
    /// Reachable only through [`native_perldb::NativePerlDbBackend`], which the
    /// shipped server does not use yet: `server/lifecycle.rs` drives
    /// [`crate::debug_adapter::DebugAdapter`] directly. This arm is groundwork
    /// for the DF1/DF3 dispatch migration, not a live product path today.
    NativeAdapterResponse,
    /// A negotiated external debugger peer answered `success: false`.
    ///
    /// Live on the `--external-peer` path.
    ExternalPeerResponse,
}

impl BackendResponseOrigin {
    /// Stable machine token for this origin.
    ///
    /// The token is part of the public contract: receipt and log layers may
    /// record it without depending on `Debug` formatting.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NativeAdapterResponse => "native_adapter_response",
            Self::ExternalPeerResponse => "external_peer_response",
        }
    }

    /// Category for an unsuccessful reply once the caller has named the responder.
    ///
    /// The two responders reach structurally different populations, so they do
    /// not share a category:
    ///
    /// - `NativeAdapterResponse` reaches only the eleven commands
    ///   [`native_perldb::NativePerlDbBackend`] delegates. `evaluate`,
    ///   `stack_trace`, `scopes`, and `variables` return
    ///   [`BackendError::Unsupported`] there, so no ordinary debuggee outcome is
    ///   reachable. The reachable sites are invalid client arguments, DAP
    ///   request-ordering mistakes, an unsupported mode, and transport failure —
    ///   predominantly things a client or user corrects, which is what
    ///   [`perl_parser_core::ErrorCategory::UserError`] denotes and what the
    ///   sibling [`BackendError::Unsupported`] arm already uses.
    /// - `ExternalPeerResponse` reaches `evaluate`, `stack_trace`, `scopes`, and
    ///   `variables` against a live external Perl process, so an ordinary
    ///   debuggee failure — a `die`, an undefined subroutine, a runtime error —
    ///   is reachable. Calling that invalid client input would be false: the
    ///   editor's request was well formed and the debuggee failed. It is not a
    ///   protocol violation either, because a well-formed `success: false` is
    ///   the peer using the protocol as designed. The honest category is
    ///   [`perl_parser_core::ErrorCategory::Advisory`], matching
    ///   [`crate::parse_origin::DebuggerOutputOrigin::BestEffortDebuggeeOutput`],
    ///   which maps heterogeneous debuggee-side output the same way (#8746).
    ///
    /// Neither is [`perl_parser_core::ErrorCategory::Bug`]: nothing reachable
    /// here is *this adapter* violating an internal invariant. A malformed reply
    /// that breaks the response contract already has its own home —
    /// `delegate` routes an unexpected [`crate::debug_adapter::DapMessage`]
    /// variant to [`BackendError::Protocol`] without passing through here.
    ///
    /// Known residual (#8758): seven native sites are transport failures
    /// ("Cannot start Perl debugger", "Failed to start TCP reader", "Cannot
    /// attach to Perl debugger at {host}:{port}") that would be
    /// [`perl_parser_core::ErrorCategory::Infra`]. The `success: false`
    /// projection does not distinguish them at this boundary; fixing that needs
    /// the handlers to supply their own cause.
    ///
    /// The match is exhaustive with no wildcard, so a third responder is a
    /// compile error here rather than a silent inherit.
    pub(crate) const fn reported_failure_category(self) -> perl_parser_core::ErrorCategory {
        match self {
            Self::NativeAdapterResponse => perl_parser_core::ErrorCategory::UserError,
            Self::ExternalPeerResponse => perl_parser_core::ErrorCategory::Advisory,
        }
    }
}

/// Errors a backend can surface.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    /// The backend is not connected to its engine (peer not attached yet).
    #[error("debug backend is not connected")]
    NotConnected,
    /// A timeout elapsed waiting for the engine/peer to respond.
    #[error("debug backend timed out: {0}")]
    Timeout(String),
    /// A bounded backend resource envelope was exhausted.
    #[error("debug backend resource limit exceeded: {0}")]
    ResourceLimit(String),
    /// The responder answered this request with `success: false` (#8758).
    ///
    /// This is a *reported outcome*, not evidence that the adapter violated an
    /// internal invariant. The reply carries a human-readable reason whose
    /// origin the `success: false` + message projection does not preserve, so
    /// this boundary records the responder, the command, and the reason without
    /// asserting fault. See [`BackendResponseOrigin::reported_failure_category`].
    ///
    /// `Display` is unchanged from the `Engine` variant this replaces:
    /// [`peer_bridge::DapPeerBridge`] renders it straight onto the DAP wire, so
    /// the editor-visible text must not drift. `origin` and `command` are
    /// structured fields for receipts and logs only.
    #[error("debug backend reported an error: {message}")]
    RequestFailed {
        /// Which responder reported the failure.
        origin: BackendResponseOrigin,
        /// The command that was answered.
        command: String,
        /// The reason the responder reported.
        message: String,
    },
    /// The operation is not supported by this backend/negotiated capabilities.
    #[error("operation not supported by this backend: {0}")]
    Unsupported(String),
    /// Transport/framing failure.
    #[error("debug backend transport error: {0}")]
    Transport(String),
    /// Serialization/deserialization failure.
    #[error("debug backend protocol error: {0}")]
    Protocol(String),
}

/// Result alias for backend operations.
pub type BackendResult<T> = Result<T, BackendError>;

impl perl_parser_core::ErrorClass for BackendError {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        match self {
            // Engine/peer not attached — infrastructure readiness.
            Self::NotConnected => perl_parser_core::ErrorCategory::Infra,
            // Transport/framing failure — external dependency unavailable.
            Self::Transport(_) => perl_parser_core::ErrorCategory::Infra,
            // Operation may succeed on retry.
            Self::Timeout(_) => perl_parser_core::ErrorCategory::Transient,
            // The backend deliberately terminated work at a declared bound.
            Self::ResourceLimit(_) => perl_parser_core::ErrorCategory::ResourceLimit,
            // The responder answered `success: false`. Classification follows the
            // caller-supplied responder, never the reason text (#8758).
            Self::RequestFailed { origin, .. } => origin.reported_failure_category(),
            // The requested operation isn't supported — usage/configuration.
            Self::Unsupported(_) => perl_parser_core::ErrorCategory::UserError,
            // Serialization/deserialization — the other side violated format.
            Self::Protocol(_) => perl_parser_core::ErrorCategory::Protocol,
        }
    }
}

/// Parameters for [`DebugBackend::initialize`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitializeBackendParams {
    /// Editor/client identifier, if provided.
    pub client_id: Option<String>,
    /// Adapter identifier advertised by the editor (usually `"perl"`).
    pub adapter_id: String,
    /// Whether the client uses 1-based lines.
    pub lines_start_at_1: bool,
    /// Whether the client uses 1-based columns.
    pub columns_start_at_1: bool,
}

/// Parameters for [`DebugBackend::launch`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchBackendParams {
    /// Program to debug.
    pub program: PathBuf,
    /// Program arguments.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: Option<PathBuf>,
    /// Environment overrides.
    pub env: HashMap<String, String>,
    /// `@INC` additions.
    pub include_paths: Vec<PathBuf>,
    /// Whether to stop at program entry.
    pub stop_on_entry: bool,
}

/// Outcome of a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchResult {
    /// Whether the launch was accepted.
    pub success: bool,
}

/// Parameters for [`DebugBackend::attach`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachBackendParams {
    /// Host of the running debuggee/engine.
    pub host: String,
    /// Port of the running debuggee/engine.
    pub port: u16,
}

/// Outcome of an attach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachResult {
    /// Whether the attach was accepted.
    pub success: bool,
}

/// Parameters for [`DebugBackend::set_breakpoints`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetBackendBreakpointsParams {
    /// Source the breakpoints apply to.
    pub source: DebugSource,
    /// Requested breakpoints (REPLACE semantics — this is the full set).
    pub breakpoints: Vec<DebugBreakpoint>,
}

/// Parameters for [`DebugBackend::set_function_breakpoints`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetFunctionBreakpointsParams {
    /// Requested function breakpoints (REPLACE semantics).
    pub breakpoints: Vec<DebugFunctionBreakpoint>,
}

/// Outcome of a continue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueResult {
    /// Whether every exposed context resumed. This is selected-backend
    /// behavior for the one synthetic main execution context, not a Perl
    /// language fact; runtime context discovery is not proven (#8294).
    pub all_threads_continued: bool,
}

/// Parameters for [`DebugBackend::stack_trace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackTraceParams {
    /// Thread to inspect.
    pub thread_id: ThreadId,
    /// Index of the first frame to return.
    pub start_frame: Option<u32>,
    /// Maximum number of frames to return.
    pub levels: Option<u32>,
}

/// The `context` an evaluate request was issued in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluateContext {
    /// A watch expression.
    Watch,
    /// The debug REPL / console.
    Repl,
    /// A hover in the editor.
    Hover,
    /// The variables view.
    Variables,
    /// Any other context, carrying the raw label.
    Other(String),
}

/// Parameters for [`DebugBackend::evaluate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluateParams {
    /// Expression to evaluate.
    pub expression: String,
    /// Frame to evaluate in, if any.
    pub frame_id: Option<FrameId>,
    /// Context the evaluate was requested in.
    pub context: EvaluateContext,
}

/// Result of an evaluate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluateResult {
    /// Rendered result value.
    pub result: String,
    /// Type/ref-kind, when known.
    pub type_name: Option<String>,
    /// Handle to expand children, if the value is structured.
    pub variables_reference: Option<VariablesRef>,
}

/// A debugger engine driver, expressed in the canonical [`crate::model`] terms.
///
/// Implementations translate between the model and their engine's native or
/// wire representation. Methods are synchronous; backends that talk to an
/// asynchronous engine (e.g. a socket peer) encapsulate that internally and
/// surface out-of-band engine events through [`DebugBackend::drain_events`].
pub trait DebugBackend: Send {
    /// A short, stable identifier for the backend (for logs/diagnostics).
    fn name(&self) -> &str;

    /// The capabilities this backend can offer, after any negotiation.
    fn capabilities(&self) -> DebugBackendCapabilities;

    /// Initialize the backend/session.
    fn initialize(&mut self, params: InitializeBackendParams) -> BackendResult<()>;

    /// Launch a new debuggee.
    fn launch(&mut self, params: LaunchBackendParams) -> BackendResult<LaunchResult>;

    /// Attach to a running debuggee/engine.
    fn attach(&mut self, params: AttachBackendParams) -> BackendResult<AttachResult>;

    /// Set (replace) the source breakpoints for a source.
    fn set_breakpoints(
        &mut self,
        params: SetBackendBreakpointsParams,
    ) -> BackendResult<Vec<ResolvedBreakpoint>>;

    /// Set (replace) function/subroutine breakpoints.
    fn set_function_breakpoints(
        &mut self,
        params: SetFunctionBreakpointsParams,
    ) -> BackendResult<Vec<ResolvedBreakpoint>>;

    /// Resume a thread.
    fn continue_thread(&mut self, thread_id: ThreadId) -> BackendResult<ContinueResult>;

    /// Step over (`next`).
    fn next(&mut self, thread_id: ThreadId) -> BackendResult<()>;

    /// Step into.
    fn step_in(&mut self, thread_id: ThreadId) -> BackendResult<()>;

    /// Step out.
    fn step_out(&mut self, thread_id: ThreadId) -> BackendResult<()>;

    /// Pause a running thread.
    fn pause(&mut self, thread_id: ThreadId) -> BackendResult<()>;

    /// Fetch the stack trace of a stopped thread.
    fn stack_trace(&mut self, params: StackTraceParams) -> BackendResult<Vec<DebugStackFrame>>;

    /// Fetch the scopes for a frame.
    fn scopes(&mut self, frame_id: FrameId) -> BackendResult<Vec<DebugScope>>;

    /// Fetch the variables behind a scope/variable reference.
    fn variables(&mut self, variables_ref: VariablesRef) -> BackendResult<Vec<DebugVariable>>;

    /// Evaluate an expression.
    fn evaluate(&mut self, params: EvaluateParams) -> BackendResult<EvaluateResult>;

    /// Drain any engine-originated events (stopped/output/…). Non-blocking.
    ///
    /// The default returns nothing; backends with an asynchronous engine
    /// override this to surface events the frontend forwards to the editor.
    fn drain_events(&mut self) -> Vec<DebugEvent> {
        Vec::new()
    }

    /// Whether the backend's connection to its engine/peer has closed.
    ///
    /// The default is `false` (synchronous/in-process backends never "close").
    /// Backends over an asynchronous transport (e.g. a socket peer) override
    /// this so a frontend can synthesize a `terminated` when the peer drops
    /// without sending an explicit terminate event.
    fn is_closed(&self) -> bool {
        false
    }

    /// Disconnect from the engine.
    fn disconnect(&mut self, terminate_debuggee: bool) -> BackendResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DebugPosition, StopReason};

    /// A trivial in-memory backend used to prove the trait is object-safe and
    /// the model-typed contract composes.
    #[derive(Default)]
    struct MockBackend {
        events: Vec<DebugEvent>,
        launched: bool,
    }

    impl DebugBackend for MockBackend {
        fn name(&self) -> &str {
            "mock"
        }
        fn capabilities(&self) -> DebugBackendCapabilities {
            DebugBackendCapabilities::full()
        }
        fn initialize(&mut self, _params: InitializeBackendParams) -> BackendResult<()> {
            self.events.push(DebugEvent::Initialized);
            Ok(())
        }
        fn launch(&mut self, _params: LaunchBackendParams) -> BackendResult<LaunchResult> {
            self.launched = true;
            Ok(LaunchResult { success: true })
        }
        fn attach(&mut self, _params: AttachBackendParams) -> BackendResult<AttachResult> {
            Ok(AttachResult { success: true })
        }
        fn set_breakpoints(
            &mut self,
            params: SetBackendBreakpointsParams,
        ) -> BackendResult<Vec<ResolvedBreakpoint>> {
            Ok(params
                .breakpoints
                .into_iter()
                .enumerate()
                .map(|(i, b)| ResolvedBreakpoint {
                    id: i as i64 + 1,
                    verified: true,
                    actual_position: DebugPosition {
                        source: b.source,
                        line: b.line,
                        column: b.column,
                    },
                    message: None,
                })
                .collect())
        }
        fn set_function_breakpoints(
            &mut self,
            _params: SetFunctionBreakpointsParams,
        ) -> BackendResult<Vec<ResolvedBreakpoint>> {
            Ok(Vec::new())
        }
        fn continue_thread(&mut self, _thread_id: ThreadId) -> BackendResult<ContinueResult> {
            Ok(ContinueResult { all_threads_continued: true })
        }
        fn next(&mut self, _thread_id: ThreadId) -> BackendResult<()> {
            Ok(())
        }
        fn step_in(&mut self, _thread_id: ThreadId) -> BackendResult<()> {
            Ok(())
        }
        fn step_out(&mut self, _thread_id: ThreadId) -> BackendResult<()> {
            Ok(())
        }
        fn pause(&mut self, thread_id: ThreadId) -> BackendResult<()> {
            self.events.push(DebugEvent::Stopped {
                reason: StopReason::Pause,
                thread_id,
                position: None,
            });
            Ok(())
        }
        fn stack_trace(
            &mut self,
            _params: StackTraceParams,
        ) -> BackendResult<Vec<DebugStackFrame>> {
            Ok(Vec::new())
        }
        fn scopes(&mut self, _frame_id: FrameId) -> BackendResult<Vec<DebugScope>> {
            Ok(Vec::new())
        }
        fn variables(&mut self, _variables_ref: VariablesRef) -> BackendResult<Vec<DebugVariable>> {
            Ok(Vec::new())
        }
        fn evaluate(&mut self, params: EvaluateParams) -> BackendResult<EvaluateResult> {
            Ok(EvaluateResult {
                result: format!("=> {}", params.expression),
                type_name: None,
                variables_reference: None,
            })
        }
        fn drain_events(&mut self) -> Vec<DebugEvent> {
            std::mem::take(&mut self.events)
        }
        fn disconnect(&mut self, _terminate_debuggee: bool) -> BackendResult<()> {
            Ok(())
        }
    }

    #[test]
    fn trait_is_object_safe() {
        let mut b: Box<dyn DebugBackend> = Box::new(MockBackend::default());
        assert_eq!(b.name(), "mock");
        must(b.initialize(InitializeBackendParams::default()));
        let events = b.drain_events();
        assert_eq!(events, vec![DebugEvent::Initialized]);
    }

    #[test]
    fn mock_resolves_breakpoints_from_model() {
        let mut b = MockBackend::default();
        let src = DebugSource::from_path("/work/script.pl");
        let out = must(b.set_breakpoints(SetBackendBreakpointsParams {
            source: src.clone(),
            breakpoints: vec![DebugBreakpoint {
                id: None,
                source: src,
                line: 42,
                column: None,
                condition: Some("$x > 10".to_string()),
                hit_condition: None,
                log_message: None,
            }],
        }));
        assert_eq!(out.len(), 1);
        assert!(out[0].verified);
        assert_eq!(out[0].actual_position.line, 42);
    }
}
