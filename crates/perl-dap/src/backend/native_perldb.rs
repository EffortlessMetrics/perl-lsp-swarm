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

    /// Capabilities backed by implemented native methods and positive behavior proof.
    ///
    /// Keep this projection crate-private so later backend evidence work can consume
    /// the same fail-closed inventory without turning it into a public API contract.
    #[must_use]
    pub(crate) fn proven_capabilities() -> DebugBackendCapabilities {
        DebugBackendCapabilities {
            source_breakpoints: true,
            conditional_breakpoints: false,
            hit_conditions: false,
            logpoints: false,
            function_breakpoints: false,
            data_breakpoints: false,
            evaluate: false,
            variables: false,
            scopes: false,
            stack_trace: false,
            continue_execution: false,
            stepping: false,
            pause: false,
            set_variable: false,
            control_mode: ControlMode::DapControlled,
        }
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
    fn capabilities_fail_closed_to_proven_native_methods() -> anyhow::Result<()> {
        let backend = NativePerlDbBackend::new();
        let caps = backend.capabilities();
        let expected = DebugBackendCapabilities {
            source_breakpoints: true,
            conditional_breakpoints: false,
            hit_conditions: false,
            logpoints: false,
            function_breakpoints: false,
            data_breakpoints: false,
            evaluate: false,
            variables: false,
            scopes: false,
            stack_trace: false,
            continue_execution: false,
            stepping: false,
            pause: false,
            set_variable: false,
            control_mode: ControlMode::DapControlled,
        };
        ensure!(caps == expected, "native capabilities widened beyond proven methods: {caps:?}");
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
