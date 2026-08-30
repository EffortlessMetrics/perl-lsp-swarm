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

use super::capabilities::{CatalogDapFlags, ControlMode, DebugBackendCapabilities};
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

    /// The fail-closed capability inventory for this backend (#7339).
    ///
    /// One entry per [`DebugBackend`] method family, computed as
    /// `implemented method ∩ compiled feature catalog ∩ positive proof`.
    /// Nothing starts from [`DebugBackendCapabilities::full()`]:
    ///
    /// - families whose methods delegate to the proven native adapter path
    ///   (breakpoint setting, execution control, pause, disconnect) are true,
    ///   narrowed by the compiled catalog where the adapter narrows them;
    /// - families whose methods surface [`BackendError::Unsupported`] until
    ///   the DF3/DF1 dispatch migration (`stack_trace`, `scopes`, `variables`,
    ///   `evaluate`) are false — a compiled catalog row cannot widen them;
    /// - families with no method on this backend (`data_breakpoints`,
    ///   `set_variable`) are false; presence in the catalog or on the adapter
    ///   is not support.
    fn capability_inventory(catalog: &CatalogDapFlags) -> DebugBackendCapabilities {
        DebugBackendCapabilities {
            // set_breakpoints: implemented; delegates to the adapter's
            // AST-validated setBreakpoints.
            source_breakpoints: true,
            // set_breakpoints narrowing families: implemented; the adapter
            // honors them only when the catalog compiled them in.
            conditional_breakpoints: catalog.breakpoints_basic,
            hit_conditions: catalog.hit_condition,
            logpoints: catalog.logpoints,
            // set_function_breakpoints: implemented; delegates to the adapter.
            function_breakpoints: catalog.function_breakpoints,
            // No set_data_breakpoints method exists on this backend.
            data_breakpoints: false,
            // evaluate: Unsupported until DF3.
            evaluate: false,
            // variables: Unsupported until DF3.
            variables: false,
            // scopes: Unsupported until DF3.
            scopes: false,
            // stack_trace: Unsupported until DF3.
            stack_trace: false,
            // continue_thread/next/step_in/step_out: implemented over the
            // proven native adapter control path.
            continue_execution: true,
            stepping: true,
            // pause: implemented over the proven native adapter path.
            pause: true,
            // No set_variable method exists on this backend; the setVariable
            // capability floor is owned by #8354.
            set_variable: false,
            // The native adapter is IDE-controlled (DAP), not mirror mode.
            control_mode: ControlMode::DapControlled,
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
        Self::capability_inventory(&CatalogDapFlags::from_catalog())
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
    // Test assertions favor unwrap/panic over propagating errors; the
    // workspace-wide deny is a production-code rule.
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::model::{DebugBreakpoint, DebugSource};
    use std::io::Write;

    #[test]
    fn capabilities_reflect_catalog_and_engine() {
        // #7339: replaced by `capabilities_are_a_fail_closed_method_inventory`
        // and `catalog_rows_cannot_widen_unimplemented_families`, which pin
        // the fail-closed method inventory instead of the former catalog ∩
        // full() negotiation that advertised Unsupported families as true.
        let backend = NativePerlDbBackend::new();
        let caps = backend.capabilities();
        assert!(caps.source_breakpoints);
        assert!(!caps.stack_trace);
        assert!(!caps.variables);
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
        for err in [
            must_err(backend.stack_trace(StackTraceParams {
                thread_id: ThreadId(1),
                start_frame: None,
                levels: None,
            })),
            must_err(backend.scopes(FrameId(1))),
            must_err(backend.variables(VariablesRef(1))),
            must_err(backend.evaluate(EvaluateParams {
                expression: "$x".to_string(),
                frame_id: None,
                context: crate::backend::EvaluateContext::Repl,
            })),
        ] {
            assert!(
                matches!(err, BackendError::Unsupported(_)),
                "every deferred data method must stay explicitly unsupported"
            );
        }
    }

    /// #7339: the capability inventory is fail-closed — implemented control
    /// families report true, every family whose method is
    /// [`BackendError::Unsupported`] or absent reports false, and nothing is
    /// inherited from `DebugBackendCapabilities::full()`.
    #[test]
    fn capabilities_are_a_fail_closed_method_inventory() {
        let backend = NativePerlDbBackend::new();
        let caps = backend.capabilities();

        // Implemented over the proven native adapter path.
        assert!(caps.source_breakpoints);
        assert!(caps.continue_execution);
        assert!(caps.stepping);
        assert!(caps.pause);
        assert_eq!(caps.control_mode, ControlMode::DapControlled);

        // Deferred (Unsupported until DF3) and absent families stay false even
        // though the full() assumption used to report them true.
        assert!(!caps.stack_trace, "stack_trace method is Unsupported");
        assert!(!caps.scopes, "scopes method is Unsupported");
        assert!(!caps.variables, "variables method is Unsupported");
        assert!(!caps.evaluate, "evaluate method is Unsupported");
        assert!(!caps.data_breakpoints, "no data-breakpoint method exists");
        assert!(!caps.set_variable, "no set_variable method exists");
    }

    /// #7339 required falsifier: a compiled (or even planned) catalog row
    /// cannot turn a capability true when the owning method is Unsupported.
    /// With the real build catalog admitting `dap.core`, evaluate would have
    /// been advertised by the old catalog ∩ full() negotiation.
    #[test]
    fn catalog_rows_cannot_widen_unimplemented_families() {
        let catalog = CatalogDapFlags::from_catalog();
        let caps = NativePerlDbBackend::capability_inventory(&catalog);
        if catalog.core {
            assert!(!caps.evaluate, "catalog admits evaluate but the native method is Unsupported");
        }
        if catalog.watchpoints {
            assert!(
                !caps.data_breakpoints,
                "catalog admits watchpoints but the backend has no such method"
            );
        }
        // And with an everything-compiled catalog the floor still holds.
        let generous = CatalogDapFlags {
            core: true,
            breakpoints_basic: true,
            hit_condition: true,
            logpoints: true,
            watchpoints: true,
            function_breakpoints: true,
        };
        let caps = NativePerlDbBackend::capability_inventory(&generous);
        assert!(caps.function_breakpoints, "implemented family follows catalog");
        assert!(!caps.stack_trace);
        assert!(!caps.evaluate);
        assert!(!caps.set_variable);
        assert!(!caps.data_breakpoints);
    }

    /// #7339 required falsifier: a backend method that returns
    /// `BackendError::Unsupported` must pair with a false capability. This
    /// pins the method↔capability pairing so future drift fails here.
    #[test]
    fn unsupported_method_pairs_with_false_capability() {
        let mut backend = NativePerlDbBackend::new();
        let stack_unsupported = matches!(
            must_err(backend.stack_trace(StackTraceParams {
                thread_id: ThreadId(1),
                start_frame: None,
                levels: None,
            })),
            BackendError::Unsupported(_)
        );
        assert_eq!(
            stack_unsupported,
            !backend.capabilities().stack_trace,
            "stack_trace method support and capability support must not diverge"
        );
    }

    /// #7339: capability identity is stable for the backend lifetime, and the
    /// native inventory never inherits or leaks the external/ptkdb defaults.
    #[test]
    fn capability_identity_is_stable_and_not_leaked_from_peers() {
        let backend = NativePerlDbBackend::new();
        let first = backend.capabilities();
        let second = backend.capabilities();
        assert_eq!(first, second, "capability identity must be session-stable");

        let ptkdb = DebugBackendCapabilities::ptkdb_v1_defaults();
        assert_ne!(
            first.control_mode, ptkdb.control_mode,
            "native and peer control modes must stay distinct"
        );
        assert_ne!(first, ptkdb, "native inventory must not equal peer defaults");
    }

    /// #7339: the partial native backend must not be wired into the production
    /// DAP adapter before backend-migration parity (#4783/#4785). Until then,
    /// production initialize capability truth comes from the compiled feature
    /// catalog alone — the adapter must not reference the partial backend or
    /// its `DebugBackendCapabilities` at all. This source gate fails when a
    /// future change widens the production initialize response from the
    /// partial backend.
    #[test]
    fn production_adapter_does_not_consume_the_partial_backend() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let adapter_dir = manifest_dir.join("src/debug_adapter");
        let mut scanned = 0usize;
        let mut offenders: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&adapter_dir)
            .unwrap_or_else(|e| panic!("read_dir {adapter_dir:?}: {e}"))
        {
            let path = entry.unwrap_or_else(|e| panic!("dir entry: {e}")).path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            scanned += 1;
            let source =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {:?}: {e}", path));
            for needle in ["NativePerlDbBackend", "DebugBackendCapabilities"] {
                if source.contains(needle) {
                    offenders.push(format!("{} references {needle}", path.display()));
                }
            }
        }
        assert!(scanned > 0, "source gate must actually scan the adapter module");
        assert!(
            offenders.is_empty(),
            "production adapter must not consume the partial backend: {offenders:?}"
        );
    }
}
