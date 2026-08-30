//! [`NativePerlDbBackend`] — a [`DebugBackend`] over the existing native
//! `perl -d` adapter ([`crate::debug_adapter::DebugAdapter`]).
//!
//! This backend establishes the seam over the real engine: it implements the
//! model-typed contract by translating to/from the adapter's DAP handlers. The
//! surface that does not need a live `perl -d` process — capabilities and
//! AST-backed `set_breakpoints` — is implemented and tested here. Process- and
//! data-fetch-dependent methods delegate to the adapter's request handlers.
//!
//! Full live delegation of the data-fetch methods is gated on the dispatch
//! migration (decision DF3 / DF1 in
//! `docs/reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md`): until the production
//! `dispatch_request` funnel is rehomed onto `DebugBackend`, those methods route
//! through the existing DAP path, and this backend surfaces them as
//! [`BackendError::Unsupported`] rather than faking data.

#[cfg(test)]
use perl_tdd_support::{must, must_err};
use serde_json::{Value, json};

use super::capabilities::{ControlMode, DebugBackendCapabilities};
use super::{
    AttachBackendParams, AttachResult, BackendError, BackendResult, ContinueResult, DebugBackend,
    EvaluateParams, EvaluateResult, InitializeBackendParams, LaunchBackendParams, LaunchResult,
    SetBackendBreakpointsParams, SetFunctionBreakpointsParams, StackTraceParams,
};
use crate::debug_adapter::{DapMessage, DebugAdapter};
use crate::model::{
    DebugPosition, DebugScope, DebugStackFrame, DebugVariable, FrameId, ResolvedBreakpoint,
    ThreadId, VariablesRef,
};

/// A [`DebugBackend`] driving the native `perl -d` adapter.
pub struct NativePerlDbBackend {
    adapter: DebugAdapter,
    seq: i64,
}

/// Evidence state for one native backend method or capability family.
///
/// `Implemented` is deliberately stronger than “there is a Rust method”: it is
/// reserved for a method with qualifying positive behavior proof.  The other
/// states preserve why a capability is currently absent so later evidence work
/// can consume this projection without inventing a second feature catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeMethodSupport {
    Implemented,
    Unsupported,
    RuntimeUnavailable,
    NotProven,
}

impl NativeMethodSupport {
    #[must_use]
    fn is_implemented(self) -> bool {
        matches!(self, Self::Implemented)
    }
}

/// The native backend's method-support inventory.
///
/// This is intentionally crate-private and capability-shaped.  It records the
/// method/family evidence that this backend owns; #7363 can consume the same
/// states when it adds runtime prerequisites and behavior receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeMethodSupportProjection {
    pub source_breakpoints: NativeMethodSupport,
    pub conditional_breakpoints: NativeMethodSupport,
    pub hit_conditions: NativeMethodSupport,
    pub logpoints: NativeMethodSupport,
    pub function_breakpoints: NativeMethodSupport,
    pub data_breakpoints: NativeMethodSupport,
    pub evaluate: NativeMethodSupport,
    pub variables: NativeMethodSupport,
    pub scopes: NativeMethodSupport,
    pub stack_trace: NativeMethodSupport,
    pub continue_execution: NativeMethodSupport,
    pub stepping: NativeMethodSupport,
    pub pause: NativeMethodSupport,
    pub set_variable: NativeMethodSupport,
}

impl NativeMethodSupportProjection {
    /// Current evidence snapshot for the partial native backend.
    #[must_use]
    pub(crate) fn current() -> Self {
        Self {
            // The AST-backed method exists, but the governing SOT entry
            // (`dap.breakpoints.basic`) is not_proven until selected-backend
            // runtime and public-transport proof exists.
            source_breakpoints: NativeMethodSupport::NotProven,
            conditional_breakpoints: NativeMethodSupport::NotProven,
            hit_conditions: NativeMethodSupport::NotProven,
            logpoints: NativeMethodSupport::NotProven,
            function_breakpoints: NativeMethodSupport::NotProven,
            data_breakpoints: NativeMethodSupport::Unsupported,
            evaluate: NativeMethodSupport::Unsupported,
            variables: NativeMethodSupport::Unsupported,
            scopes: NativeMethodSupport::Unsupported,
            stack_trace: NativeMethodSupport::Unsupported,
            continue_execution: NativeMethodSupport::RuntimeUnavailable,
            stepping: NativeMethodSupport::RuntimeUnavailable,
            pause: NativeMethodSupport::RuntimeUnavailable,
            set_variable: NativeMethodSupport::Unsupported,
        }
    }

    /// Derive advertised capability bits only from positively proven methods.
    #[must_use]
    pub(crate) fn capabilities(self) -> DebugBackendCapabilities {
        DebugBackendCapabilities {
            source_breakpoints: self.source_breakpoints.is_implemented(),
            conditional_breakpoints: self.conditional_breakpoints.is_implemented(),
            hit_conditions: self.hit_conditions.is_implemented(),
            logpoints: self.logpoints.is_implemented(),
            function_breakpoints: self.function_breakpoints.is_implemented(),
            data_breakpoints: self.data_breakpoints.is_implemented(),
            evaluate: self.evaluate.is_implemented(),
            variables: self.variables.is_implemented(),
            scopes: self.scopes.is_implemented(),
            stack_trace: self.stack_trace.is_implemented(),
            continue_execution: self.continue_execution.is_implemented(),
            stepping: self.stepping.is_implemented(),
            pause: self.pause.is_implemented(),
            set_variable: self.set_variable.is_implemented(),
            control_mode: ControlMode::DapControlled,
        }
    }
}

impl NativePerlDbBackend {
    /// Create a native backend wrapping a fresh [`DebugAdapter`].
    #[must_use]
    pub fn new() -> Self {
        Self { adapter: DebugAdapter::new(), seq: 0 }
    }

    /// Access the underlying adapter (e.g. to install an event sender).
    pub fn adapter_mut(&mut self) -> &mut DebugAdapter {
        &mut self.adapter
    }

    /// Capabilities backed by the native method-support evidence projection.
    #[must_use]
    pub(crate) fn proven_capabilities() -> DebugBackendCapabilities {
        NativeMethodSupportProjection::current().capabilities()
    }

    fn next_seq(&mut self) -> i64 {
        self.seq += 1;
        self.seq
    }

    /// Delegate a DAP request to the adapter and return its body on success.
    fn delegate(
        &mut self,
        command: &str,
        arguments: Option<Value>,
    ) -> BackendResult<Option<Value>> {
        let seq = self.next_seq();
        match self.adapter.handle_request(seq, command, arguments) {
            DapMessage::Response { success, body, message, .. } => {
                if success {
                    Ok(body)
                } else {
                    Err(BackendError::Engine(
                        message.unwrap_or_else(|| format!("{command} failed")),
                    ))
                }
            }
            other => Err(BackendError::Protocol(format!(
                "expected response to {command}, got {other:?}"
            ))),
        }
    }
}

impl Default for NativePerlDbBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugBackend for NativePerlDbBackend {
    fn name(&self) -> &str {
        "native-perl-db"
    }

    fn capabilities(&self) -> DebugBackendCapabilities {
        Self::proven_capabilities()
    }

    fn initialize(&mut self, _params: InitializeBackendParams) -> BackendResult<()> {
        self.delegate("initialize", None).map(|_| ())
    }

    fn launch(&mut self, params: LaunchBackendParams) -> BackendResult<LaunchResult> {
        let args = json!({
            "program": params.program,
            "args": params.args,
            "cwd": params.cwd,
            "env": params.env,
            "includePaths": params.include_paths,
            "stopOnEntry": params.stop_on_entry,
        });
        self.delegate("launch", Some(args)).map(|_| LaunchResult { success: true })
    }

    fn attach(&mut self, params: AttachBackendParams) -> BackendResult<AttachResult> {
        let args = json!({ "host": params.host, "port": params.port });
        self.delegate("attach", Some(args)).map(|_| AttachResult { success: true })
    }

    fn set_breakpoints(
        &mut self,
        params: SetBackendBreakpointsParams,
    ) -> BackendResult<Vec<ResolvedBreakpoint>> {
        let source_path = params.source.path.clone();
        let bps: Vec<Value> = params
            .breakpoints
            .into_iter()
            .map(|b| {
                json!({
                    "line": b.line,
                    "column": b.column,
                    "condition": b.condition,
                    "hitCondition": b.hit_condition,
                    "logMessage": b.log_message,
                })
            })
            .collect();
        let args = json!({
            "source": { "path": source_path, "name": params.source.name },
            "breakpoints": bps,
        });

        let body = self
            .delegate("setBreakpoints", Some(args))?
            .ok_or_else(|| BackendError::Protocol("setBreakpoints returned no body".to_string()))?;

        let resp: crate::protocol::SetBreakpointsResponseBody =
            serde_json::from_value(body).map_err(|e| BackendError::Protocol(e.to_string()))?;

        Ok(resp
            .breakpoints
            .into_iter()
            .map(|bp| ResolvedBreakpoint {
                id: bp.id,
                verified: bp.verified,
                actual_position: DebugPosition {
                    source: params.source.clone(),
                    line: u32::try_from(bp.line.max(0)).unwrap_or(0),
                    column: bp.column.and_then(|c| u32::try_from(c.max(0)).ok()),
                },
                message: bp.message,
            })
            .collect())
    }

    fn set_function_breakpoints(
        &mut self,
        params: SetFunctionBreakpointsParams,
    ) -> BackendResult<Vec<ResolvedBreakpoint>> {
        // Capture the placeholder source before moving `params.breakpoints`, so
        // the resolved-breakpoint mapping below does not borrow a partially-moved
        // `params`.
        let stub = params.source_stub();
        let names: Vec<Value> = params
            .breakpoints
            .into_iter()
            .map(|b| json!({ "name": b.name, "condition": b.condition }))
            .collect();
        let args = json!({ "breakpoints": names });
        // The adapter returns DAP Breakpoints without source positions for
        // function breakpoints; surface them as verified/unverified only.
        let body = self.delegate("setFunctionBreakpoints", Some(args))?.ok_or_else(|| {
            BackendError::Protocol("setFunctionBreakpoints returned no body".to_string())
        })?;
        let resp: crate::protocol::SetBreakpointsResponseBody =
            serde_json::from_value(body).map_err(|e| BackendError::Protocol(e.to_string()))?;
        Ok(resp
            .breakpoints
            .into_iter()
            .map(|bp| ResolvedBreakpoint {
                id: bp.id,
                verified: bp.verified,
                actual_position: DebugPosition {
                    source: stub.clone(),
                    line: u32::try_from(bp.line.max(0)).unwrap_or(0),
                    column: None,
                },
                message: bp.message,
            })
            .collect())
    }

    fn continue_thread(&mut self, thread_id: ThreadId) -> BackendResult<ContinueResult> {
        self.delegate("continue", Some(json!({ "threadId": thread_id.0 })))
            .map(|_| ContinueResult { all_threads_continued: true })
    }

    fn next(&mut self, thread_id: ThreadId) -> BackendResult<()> {
        self.delegate("next", Some(json!({ "threadId": thread_id.0 }))).map(|_| ())
    }

    fn step_in(&mut self, thread_id: ThreadId) -> BackendResult<()> {
        self.delegate("stepIn", Some(json!({ "threadId": thread_id.0 }))).map(|_| ())
    }

    fn step_out(&mut self, thread_id: ThreadId) -> BackendResult<()> {
        self.delegate("stepOut", Some(json!({ "threadId": thread_id.0 }))).map(|_| ())
    }

    fn pause(&mut self, thread_id: ThreadId) -> BackendResult<()> {
        self.delegate("pause", Some(json!({ "threadId": thread_id.0 }))).map(|_| ())
    }

    fn stack_trace(&mut self, _params: StackTraceParams) -> BackendResult<Vec<DebugStackFrame>> {
        // Data-fetch delegation is gated on the dispatch migration (DF3/DF1).
        Err(BackendError::Unsupported(
            "native stack_trace routes through the existing DAP dispatch until the \
             DebugBackend migration (see EXTERNAL_DEBUGGER_PEER_DECISIONS.md DF3)"
                .to_string(),
        ))
    }

    fn scopes(&mut self, _frame_id: FrameId) -> BackendResult<Vec<DebugScope>> {
        Err(BackendError::Unsupported(
            "native scopes routes through the existing DAP dispatch until the \
             DebugBackend migration (see EXTERNAL_DEBUGGER_PEER_DECISIONS.md DF3)"
                .to_string(),
        ))
    }

    fn variables(&mut self, _variables_ref: VariablesRef) -> BackendResult<Vec<DebugVariable>> {
        Err(BackendError::Unsupported(
            "native variables routes through the existing DAP dispatch until the \
             DebugBackend migration (see EXTERNAL_DEBUGGER_PEER_DECISIONS.md DF3)"
                .to_string(),
        ))
    }

    fn evaluate(&mut self, _params: EvaluateParams) -> BackendResult<EvaluateResult> {
        Err(BackendError::Unsupported(
            "native evaluate routes through the existing DAP dispatch until the \
             DebugBackend migration (see EXTERNAL_DEBUGGER_PEER_DECISIONS.md DF3)"
                .to_string(),
        ))
    }

    fn disconnect(&mut self, terminate_debuggee: bool) -> BackendResult<()> {
        let args = json!({ "terminateDebuggee": terminate_debuggee });
        self.delegate("disconnect", Some(args)).map(|_| ())
    }
}

impl SetFunctionBreakpointsParams {
    /// A placeholder source for function breakpoints, which are not tied to a
    /// single source file at request time.
    fn source_stub(&self) -> crate::model::DebugSource {
        crate::model::DebugSource {
            path: std::path::PathBuf::new(),
            name: None,
            source_reference: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DebugBreakpoint, DebugSource};
    use anyhow::ensure;
    use std::io::Write;

    #[test]
    fn capabilities_are_subset_of_authoritative_method_support() -> anyhow::Result<()> {
        let support = NativeMethodSupportProjection::current();
        let caps = support.capabilities();

        ensure!(
            support.source_breakpoints == NativeMethodSupport::NotProven,
            "source breakpoint support must follow features_sot.toml until qualifying proof"
        );
        ensure!(!caps.source_breakpoints);
        ensure!(!caps.stack_trace);
        ensure!(!caps.scopes);
        ensure!(!caps.variables);
        ensure!(!caps.evaluate);
        ensure!(caps.control_mode == ControlMode::DapControlled);
        Ok(())
    }

    #[test]
    fn replacing_proven_method_support_with_unsupported_removes_capability() -> anyhow::Result<()> {
        let mut support = NativeMethodSupportProjection::current();
        support.source_breakpoints = NativeMethodSupport::Implemented;
        ensure!(support.capabilities().source_breakpoints);

        support.source_breakpoints = NativeMethodSupport::Unsupported;
        ensure!(!support.capabilities().source_breakpoints);

        support.source_breakpoints = NativeMethodSupport::RuntimeUnavailable;
        ensure!(!support.capabilities().source_breakpoints);
        support.source_breakpoints = NativeMethodSupport::NotProven;
        ensure!(!support.capabilities().source_breakpoints);
        Ok(())
    }

    #[test]
    fn capability_projection_matrix_is_fail_closed_for_every_field() -> anyhow::Result<()> {
        type Setter = fn(&mut NativeMethodSupportProjection, NativeMethodSupport);
        type Getter = fn(NativeMethodSupportProjection) -> NativeMethodSupport;
        type Capability = fn(DebugBackendCapabilities) -> bool;

        let fields: &[(&str, Setter, Getter, Capability)] = &[
            (
                "source_breakpoints",
                |projection, state| projection.source_breakpoints = state,
                |projection| projection.source_breakpoints,
                |capabilities| capabilities.source_breakpoints,
            ),
            (
                "conditional_breakpoints",
                |projection, state| projection.conditional_breakpoints = state,
                |projection| projection.conditional_breakpoints,
                |capabilities| capabilities.conditional_breakpoints,
            ),
            (
                "hit_conditions",
                |projection, state| projection.hit_conditions = state,
                |projection| projection.hit_conditions,
                |capabilities| capabilities.hit_conditions,
            ),
            (
                "logpoints",
                |projection, state| projection.logpoints = state,
                |projection| projection.logpoints,
                |capabilities| capabilities.logpoints,
            ),
            (
                "function_breakpoints",
                |projection, state| projection.function_breakpoints = state,
                |projection| projection.function_breakpoints,
                |capabilities| capabilities.function_breakpoints,
            ),
            (
                "data_breakpoints",
                |projection, state| projection.data_breakpoints = state,
                |projection| projection.data_breakpoints,
                |capabilities| capabilities.data_breakpoints,
            ),
            (
                "evaluate",
                |projection, state| projection.evaluate = state,
                |projection| projection.evaluate,
                |capabilities| capabilities.evaluate,
            ),
            (
                "variables",
                |projection, state| projection.variables = state,
                |projection| projection.variables,
                |capabilities| capabilities.variables,
            ),
            (
                "scopes",
                |projection, state| projection.scopes = state,
                |projection| projection.scopes,
                |capabilities| capabilities.scopes,
            ),
            (
                "stack_trace",
                |projection, state| projection.stack_trace = state,
                |projection| projection.stack_trace,
                |capabilities| capabilities.stack_trace,
            ),
            (
                "continue_execution",
                |projection, state| projection.continue_execution = state,
                |projection| projection.continue_execution,
                |capabilities| capabilities.continue_execution,
            ),
            (
                "stepping",
                |projection, state| projection.stepping = state,
                |projection| projection.stepping,
                |capabilities| capabilities.stepping,
            ),
            (
                "pause",
                |projection, state| projection.pause = state,
                |projection| projection.pause,
                |capabilities| capabilities.pause,
            ),
            (
                "set_variable",
                |projection, state| projection.set_variable = state,
                |projection| projection.set_variable,
                |capabilities| capabilities.set_variable,
            ),
        ];

        let non_implemented = [
            NativeMethodSupport::Unsupported,
            NativeMethodSupport::RuntimeUnavailable,
            NativeMethodSupport::NotProven,
        ];

        for (name, set, get, capability) in fields {
            for state in non_implemented {
                let mut projection = NativeMethodSupportProjection::current();
                set(&mut projection, state);
                ensure!(get(projection) == state, "{name} test fixture did not install {state:?}");
                ensure!(
                    !capability(projection.capabilities()),
                    "{name} advertised capability for {state:?}"
                );
            }

            let mut projection = NativeMethodSupportProjection::current();
            set(&mut projection, NativeMethodSupport::Implemented);
            ensure!(
                get(projection) == NativeMethodSupport::Implemented,
                "{name} test fixture did not install Implemented"
            );
            ensure!(
                capability(projection.capabilities()),
                "{name} did not advertise its Implemented capability"
            );
        }

        Ok(())
    }

    #[test]
    fn current_projection_matches_authoritative_method_support() -> anyhow::Result<()> {
        let projection = NativeMethodSupportProjection::current();
        let expected = [
            ("source_breakpoints", projection.source_breakpoints, NativeMethodSupport::NotProven),
            (
                "conditional_breakpoints",
                projection.conditional_breakpoints,
                NativeMethodSupport::NotProven,
            ),
            ("hit_conditions", projection.hit_conditions, NativeMethodSupport::NotProven),
            ("logpoints", projection.logpoints, NativeMethodSupport::NotProven),
            (
                "function_breakpoints",
                projection.function_breakpoints,
                NativeMethodSupport::NotProven,
            ),
            ("data_breakpoints", projection.data_breakpoints, NativeMethodSupport::Unsupported),
            ("evaluate", projection.evaluate, NativeMethodSupport::Unsupported),
            ("variables", projection.variables, NativeMethodSupport::Unsupported),
            ("scopes", projection.scopes, NativeMethodSupport::Unsupported),
            ("stack_trace", projection.stack_trace, NativeMethodSupport::Unsupported),
            (
                "continue_execution",
                projection.continue_execution,
                NativeMethodSupport::RuntimeUnavailable,
            ),
            ("stepping", projection.stepping, NativeMethodSupport::RuntimeUnavailable),
            ("pause", projection.pause, NativeMethodSupport::RuntimeUnavailable),
            ("set_variable", projection.set_variable, NativeMethodSupport::Unsupported),
        ];

        for (field, actual, expected) in expected {
            ensure!(
                actual == expected,
                "{field} current support changed: {actual:?} != {expected:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn set_breakpoints_validates_via_ast_without_a_process() {
        // setBreakpoints uses the AST validator + on-disk source, no live perl.
        let mut file = must(tempfile::NamedTempFile::new());
        must(writeln!(file, "# a comment line"));
        must(writeln!(file, "my $x = 1;"));
        must(writeln!(file, "print $x;"));
        let path = file.path().to_path_buf();

        let mut backend = NativePerlDbBackend::new();
        let source = DebugSource::from_path(&path);
        let out = must(backend.set_breakpoints(SetBackendBreakpointsParams {
            source: source.clone(),
            breakpoints: vec![
                DebugBreakpoint {
                    id: None,
                    source: source.clone(),
                    line: 2, // executable
                    column: None,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                },
                DebugBreakpoint {
                    id: None,
                    source,
                    line: 3, // executable
                    column: None,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                },
            ],
        }));
        assert_eq!(out.len(), 2, "same-order resolved set");
        assert!(out[0].actual_position.line >= 2);
    }

    #[test]
    fn deferred_data_methods_are_honest_unsupported() {
        let mut backend = NativePerlDbBackend::new();
        let err = must_err(backend.scopes(FrameId(1)));
        assert!(matches!(err, BackendError::Unsupported(_)));
    }
}
