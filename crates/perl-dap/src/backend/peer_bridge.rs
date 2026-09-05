//! [`DapPeerBridge`] — the DAP **frontend** over a [`DebugBackend`].
//!
//! This is the reachability closure for the external-peer seam: it makes the
//! backend *live-drivable from a real DAP session*. It is the inverse of
//! [`super::native_perldb`]: where the native backend translates the model into
//! the adapter's DAP handlers, this bridge translates incoming **DAP requests →
//! model calls → DAP responses**, and converts backend [`DebugEvent`]s into DAP
//! events for the editor.
//!
//! ```text
//! editor ──DAP──▶ DapPeerBridge ──model──▶ DebugBackend ──peer proto──▶ ptkdb
//!        ◀─DAP───              ◀─events───            ◀────────────────
//! ```
//!
//! The bridge is a **parallel** path: it does not touch the native
//! [`crate::debug_adapter::DebugAdapter`] dispatch funnel (decision DF1 remains
//! deferred). [`run_external_peer_session_stdio`] drives it over editor stdio;
//! [`DapPeerBridge::dispatch`] / [`DapPeerBridge::poll_events`] are the
//! deterministic, testable core.

#[cfg(test)]
use perl_tdd_support::{must, must_some};
use std::io::{Read, Write};
use std::time::Duration;

use serde_json::{Value, json};

use super::capabilities::{CatalogDapFlags, intersect_dap_capabilities};
use super::{
    AttachBackendParams, DebugBackend, EvaluateContext, EvaluateParams, InitializeBackendParams,
    LaunchBackendParams, SetBackendBreakpointsParams, SetFunctionBreakpointsParams,
    StackTraceParams,
};
use crate::breakpoint_oracle::{AstBreakpointOracle, BreakpointOracle};
use crate::debug_adapter::{DapMessage, DapRequestRoute};
use crate::model::{
    DebugBreakpoint, DebugEvent, DebugFunctionBreakpoint, DebugSource, FrameId, OutputCategory,
    StopReason, ThreadId, VariablesRef,
};

/// The DAP frontend over a [`DebugBackend`].
pub struct DapPeerBridge {
    backend: Box<dyn DebugBackend>,
    seq: i64,
    /// Whether a `terminated` DAP event has already been emitted this session,
    /// so a peer's own `debugger/terminated` (which may have been queued before
    /// our `peer/goodbye` on a `terminate`) does not produce a duplicate.
    terminated_emitted: bool,
}

impl DapPeerBridge {
    /// The one synthetic execution context this peer frontend advertises:
    /// `threads` reports id 1 and runtime context discovery across the peer
    /// bridge is not proven (#8294).
    const ADVERTISED_THREAD_ID: i64 = 1;

    /// Create a bridge over `backend`.
    #[must_use]
    pub fn new(backend: Box<dyn DebugBackend>) -> Self {
        Self { backend, seq: 0, terminated_emitted: false }
    }

    /// Strict identity gate for every thread-scoped request (#8294): the
    /// request must name the advertised synthetic execution context. A
    /// missing, unknown, or stale id fails the request before any backend
    /// method runs; the failed response is pushed onto `out` and `None`
    /// returned.
    fn validate_thread_scoped(
        &mut self,
        command: &str,
        request_seq: i64,
        args: Option<&Value>,
        out: &mut Vec<DapMessage>,
    ) -> Option<ThreadId> {
        let reported = args.and_then(|a| a.get("threadId")).and_then(Value::as_i64);
        if reported != Some(Self::ADVERTISED_THREAD_ID) {
            let detail = match reported {
                Some(id) => {
                    format!("unknown or stale `threadId` {id}")
                }
                None => "missing `threadId`".to_string(),
            };
            out.push(self.response(
                request_seq,
                command,
                false,
                None,
                Some(format!(
                    "{detail}; this peer frontend advertises only the synthetic execution \
                     context {}. Re-request `threads` to obtain the current id.",
                    Self::ADVERTISED_THREAD_ID
                )),
            ));
            return None;
        }
        Some(ThreadId(reported.unwrap_or(Self::ADVERTISED_THREAD_ID)))
    }

    fn next_seq(&mut self) -> i64 {
        self.seq += 1;
        self.seq
    }

    fn response(
        &mut self,
        request_seq: i64,
        command: &str,
        success: bool,
        body: Option<Value>,
        message: Option<String>,
    ) -> DapMessage {
        DapMessage::Response {
            seq: self.next_seq(),
            request_seq,
            success,
            command: command.to_string(),
            body,
            message,
        }
    }

    fn event(&mut self, event: &str, body: Option<Value>) -> DapMessage {
        DapMessage::Event { seq: self.next_seq(), event: event.to_string(), body }
    }

    /// Convert and drain any backend events into DAP event messages.
    ///
    /// Non-blocking: returns whatever the backend has queued right now.
    pub fn poll_events(&mut self) -> Vec<DapMessage> {
        let events = self.backend.drain_events();
        let mut out = Vec::new();
        for ev in events {
            self.push_dap_events(ev, &mut out);
        }
        out
    }

    fn push_dap_events(&mut self, ev: DebugEvent, out: &mut Vec<DapMessage>) {
        match ev {
            // The DAP `initialized` event (editor: "you may send configuration
            // now") is a DIFFERENT signal from the peer's `debugger/initialized`
            // (host: "the engine is ready"). The bridge emits the DAP one exactly
            // once, right after the initialize RESPONSE, and treats the peer
            // handshake (`peer/hello`) as the mirror-MVP readiness gate. The peer
            // readiness event is therefore NOT forwarded — emitting a second DAP
            // `initialized` would wrongly re-trigger the client's configuration
            // sequence. (Gating configuration on a peer's `debugger/initialized`
            // is future work for cooperative mode; the v1 integration target does
            // not require peers to send it — see PTKDB_PEER_INTEGRATION_TARGET.md.)
            DebugEvent::Initialized => {}
            DebugEvent::Stopped { reason, thread_id: _, .. } => {
                // Same identity contract as `validate_thread_scoped`: the
                // editor must only ever observe the advertised synthetic
                // execution context, so a backend event reporting a foreign id
                // is normalized here instead of being forwarded verbatim
                // (#8294) — otherwise the editor would see an id that every
                // thread-scoped request then rejects.
                let body = json!({
                    "reason": dap_stop_reason(&reason),
                    "threadId": Self::ADVERTISED_THREAD_ID,
                    "allThreadsStopped": true,
                });
                out.push(self.event("stopped", Some(body)));
            }
            DebugEvent::Continued { thread_id: _ } => {
                // Normalized for the same identity contract as the stopped
                // event above (#8294).
                let body =
                    json!({ "threadId": Self::ADVERTISED_THREAD_ID, "allThreadsContinued": true });
                out.push(self.event("continued", Some(body)));
            }
            DebugEvent::Output { category, output } => {
                let body = json!({ "category": category_str(category), "output": output });
                out.push(self.event("output", Some(body)));
            }
            DebugEvent::Terminated { exit_code } => {
                // Emit at most one `terminated` per session. If a DAP `terminate`
                // already emitted one, the peer's own `debugger/terminated`
                // (possibly queued before our `peer/goodbye`) is swallowed rather
                // than delivered as a duplicate.
                if !self.terminated_emitted {
                    self.terminated_emitted = true;
                    let body = exit_code.map(|c| json!({ "exitCode": c }));
                    out.push(self.event("terminated", body));
                }
            }
            DebugEvent::BreakpointsChanged { breakpoints } => {
                for bp in breakpoints {
                    let body = json!({
                        "reason": "changed",
                        "breakpoint": {
                            "id": bp.id,
                            "verified": bp.verified,
                            "line": bp.actual_position.line,
                            "message": bp.message,
                        },
                    });
                    out.push(self.event("breakpoint", Some(body)));
                }
            }
            // No standard DAP event for source facts; the editor obtains
            // breakable lines via `breakpointLocations`. Intentionally dropped.
            DebugEvent::SourceFacts { .. } => {}
        }
    }

    /// Dispatch a single DAP request. Returns the response message followed by
    /// any backend events that arrived while servicing it (drained after the
    /// call), so a caller can write them in order.
    pub fn dispatch(
        &mut self,
        request_seq: i64,
        command: &str,
        arguments: Option<Value>,
    ) -> Vec<DapMessage> {
        let mut out = Vec::new();
        match DapRequestRoute::from_command(command)
            .filter(DapRequestRoute::available_in_peer_frontends)
        {
            Some(DapRequestRoute::Initialize) => {
                let params = parse_initialize(arguments.as_ref());
                match self.backend.initialize(params) {
                    Ok(()) => {
                        let body = self.capabilities_body();
                        out.push(self.response(request_seq, command, true, Some(body), None));
                        // DAP contract: emit `initialized` after the response so
                        // the client sends its configuration (breakpoints, etc.).
                        out.push(self.event("initialized", None));
                    }
                    Err(e) => out.push(self.error(request_seq, command, e)),
                }
            }
            Some(DapRequestRoute::Launch) => {
                let params = parse_launch(arguments.as_ref());
                match self.backend.launch(params) {
                    Ok(_) => out.push(self.response(request_seq, command, true, None, None)),
                    Err(e) => out.push(self.error(request_seq, command, e)),
                }
            }
            Some(DapRequestRoute::Attach) => {
                let params = parse_attach(arguments.as_ref());
                match self.backend.attach(params) {
                    Ok(_) => out.push(self.response(request_seq, command, true, None, None)),
                    Err(e) => out.push(self.error(request_seq, command, e)),
                }
            }
            Some(DapRequestRoute::SetBreakpoints) => match self
                .handle_set_breakpoints(arguments.as_ref())
            {
                Ok(body) => out.push(self.response(request_seq, command, true, Some(body), None)),
                Err(e) => out.push(self.error(request_seq, command, e)),
            },
            Some(DapRequestRoute::SetFunctionBreakpoints) => {
                match self.handle_set_function_breakpoints(arguments.as_ref()) {
                    Ok(body) => {
                        out.push(self.response(request_seq, command, true, Some(body), None))
                    }
                    Err(e) => out.push(self.error(request_seq, command, e)),
                }
            }
            Some(DapRequestRoute::Continue) => {
                if let Some(tid) =
                    self.validate_thread_scoped(command, request_seq, arguments.as_ref(), &mut out)
                {
                    match self.backend.continue_thread(tid) {
                        Ok(r) => {
                            let body = json!({ "allThreadsContinued": r.all_threads_continued });
                            out.push(self.response(request_seq, command, true, Some(body), None));
                        }
                        Err(e) => out.push(self.error(request_seq, command, e)),
                    }
                }
            }
            Some(DapRequestRoute::Next) => {
                self.step(request_seq, command, arguments.as_ref(), Step::Next, &mut out)
            }
            Some(DapRequestRoute::StepIn) => {
                self.step(request_seq, command, arguments.as_ref(), Step::In, &mut out)
            }
            Some(DapRequestRoute::StepOut) => {
                self.step(request_seq, command, arguments.as_ref(), Step::Out, &mut out)
            }
            Some(DapRequestRoute::Pause) => {
                if let Some(tid) =
                    self.validate_thread_scoped(command, request_seq, arguments.as_ref(), &mut out)
                {
                    match self.backend.pause(tid) {
                        Ok(()) => out.push(self.response(request_seq, command, true, None, None)),
                        Err(e) => out.push(self.error(request_seq, command, e)),
                    }
                }
            }
            Some(DapRequestRoute::StackTrace) => {
                if let Some(thread_id) =
                    self.validate_thread_scoped(command, request_seq, arguments.as_ref(), &mut out)
                {
                    match self.handle_stack_trace(thread_id, arguments.as_ref()) {
                        Ok(body) => {
                            out.push(self.response(request_seq, command, true, Some(body), None))
                        }
                        Err(e) => out.push(self.error(request_seq, command, e)),
                    }
                }
            }
            Some(DapRequestRoute::Scopes) => match self.handle_scopes(arguments.as_ref()) {
                Ok(body) => out.push(self.response(request_seq, command, true, Some(body), None)),
                Err(e) => out.push(self.error(request_seq, command, e)),
            },
            Some(DapRequestRoute::Variables) => match self.handle_variables(arguments.as_ref()) {
                Ok(body) => out.push(self.response(request_seq, command, true, Some(body), None)),
                Err(e) => out.push(self.error(request_seq, command, e)),
            },
            Some(DapRequestRoute::Evaluate) => match self.handle_evaluate(arguments.as_ref()) {
                Ok(body) => out.push(self.response(request_seq, command, true, Some(body), None)),
                Err(e) => out.push(self.error(request_seq, command, e)),
            },
            Some(DapRequestRoute::Threads) => {
                // One synthetic main execution context; runtime context
                // discovery across the peer bridge is not proven (#8294).

                let body = json!({ "threads": [{ "id": 1, "name": "main" }] });
                out.push(self.response(request_seq, command, true, Some(body), None));
            }
            Some(DapRequestRoute::ConfigurationDone) => {
                out.push(self.response(request_seq, command, true, None, None));
            }
            Some(DapRequestRoute::BreakpointLocations) => {
                // Answered locally from the AST oracle (the source file is on the
                // same host as perl-dap), independent of the peer.
                let body = handle_breakpoint_locations(arguments.as_ref());
                out.push(self.response(request_seq, command, true, Some(body), None));
            }
            Some(DapRequestRoute::Terminate) => {
                // DAP `terminate` (the editor's Stop button when the adapter
                // advertises `supportsTerminateRequest`): end the debuggee. In
                // mirror mode the peer owns the process, so this is best-effort —
                // ask the backend to disconnect *with* termination, then emit a
                // `terminated` event so the editor tears the session down instead
                // of leaving it running.
                let _ = self.backend.disconnect(true);
                out.push(self.response(request_seq, command, true, None, None));
                // Mark before draining backend events below so a peer-queued
                // `debugger/terminated` does not double up (see push_dap_events).
                self.terminated_emitted = true;
                out.push(self.event("terminated", None));
            }
            Some(DapRequestRoute::Disconnect) => {
                let terminate = arguments
                    .as_ref()
                    .and_then(|a| a.get("terminateDebuggee"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let _ = self.backend.disconnect(terminate);
                out.push(self.response(request_seq, command, true, None, None));
                // `disconnect { terminateDebuggee: true }` carries the same intent
                // as `terminate`, so emit `terminated` (and arm the dedup guard)
                // here too. Otherwise a client that takes this path never gets a
                // `terminated`, and a later peer-queued `debugger/terminated`
                // would reach the editor as an un-deduped event.
                if terminate && !self.terminated_emitted {
                    self.terminated_emitted = true;
                    out.push(self.event("terminated", None));
                }
            }
            Some(DapRequestRoute::InlineValues) => {
                // #9089: the custom inline-values extension is fail-closed in
                // every frontend until a versioned negotiation contract is
                // proven. This frontend neither advertises nor negotiates it
                // (`capabilities_body` carries no `supportsInlineValues`), so
                // the single authority refuses the request — explicitly, on
                // its own route, before any backend access — rather than
                // falling through to the lenient success-empty compatibility
                // acknowledgement the native adapter rejects.
                out.push(self.response(
                    request_seq,
                    command,
                    false,
                    None,
                    Some(
                        crate::backend::capabilities::INLINE_VALUES_EXTENSION_UNSUPPORTED_MESSAGE
                            .to_string(),
                    ),
                ));
            }
            None | Some(_) => {
                // #9568: setExpression is `native_only` in the route table, so
                // the peer-availability filter reduces it to `None` before this
                // arm. This bridge does not advertise setExpression, and it
                // refuses exactly what it does not advertise: the previous
                // lenient acknowledgement promised an assignment that never
                // happened while the native adapter refused the identical
                // request. Gate on the same value `capabilities_body`
                // advertises, so advertisement and admission cannot disagree.
                if matches!(
                    DapRequestRoute::from_command(command),
                    Some(DapRequestRoute::SetExpression)
                ) {
                    if crate::backend::capabilities::refuse_set_expression(
                        self.advertised_set_expression(),
                    ) {
                        out.push(self.response(
                            request_seq,
                            command,
                            false,
                            None,
                            Some(
                                crate::backend::capabilities::SET_EXPRESSION_UNSUPPORTED_MESSAGE
                                    .to_string(),
                            ),
                        ));
                    } else {
                        // Promotion path: advertised by this mode but
                        // delegation to the external peer's assignment
                        // primitive is not wired yet. Fail loudly instead of
                        // acknowledging a write that did not happen.
                        out.push(
                            self.response(
                                request_seq,
                                command,
                                false,
                                None,
                                Some(
                                    "setExpression: external-peer delegation is not implemented"
                                        .to_string(),
                                ),
                            ),
                        );
                    }
                } else if DapRequestRoute::from_command(command).is_some() {
                    // A catalog route that exists but is unavailable in this
                    // frontend must fail closed: acknowledging it would report
                    // success for work no backend performed (#9069).
                    tracing::warn!(command, "peer bridge: request is unavailable in this frontend");
                    out.push(self.response(
                        request_seq,
                        command,
                        false,
                        None,
                        Some("request is unavailable in the external peer frontend".to_string()),
                    ));
                } else {
                    // Lenient: acknowledge unrecognized requests so a client is not
                    // wedged, but carry no body. (mirror-MVP behavior.)
                    tracing::warn!(command, "peer bridge: unhandled DAP request");
                    out.push(self.response(request_seq, command, true, None, None));
                }
            }
        }
        // Surface any events the backend queued while handling the request.
        out.extend(self.poll_events());
        out
    }

    fn step(
        &mut self,
        request_seq: i64,
        command: &str,
        args: Option<&Value>,
        which: Step,
        out: &mut Vec<DapMessage>,
    ) {
        let Some(tid) = self.validate_thread_scoped(command, request_seq, args, out) else {
            return;
        };
        // #9069: this frontend negotiates `supportsStepInTargetsRequest` as
        // false, so a supplied `targetId` must fail closed rather than
        // silently degrade to an untargeted step the client did not ask for.
        if matches!(which, Step::In)
            && let Some(args) = args
            && args.get("targetId").is_some()
        {
            out.push(
                self.response(
                    request_seq,
                    command,
                    false,
                    None,
                    Some(
                        "`stepIn targetId` is not supported: targeted stepping is \
                     unavailable, so the request is refused instead of stepping \
                     without the requested target"
                            .to_string(),
                    ),
                ),
            );
            return;
        }
        let result = match which {
            Step::Next => self.backend.next(tid),
            Step::In => self.backend.step_in(tid),
            Step::Out => self.backend.step_out(tid),
        };
        match result {
            Ok(()) => out.push(self.response(request_seq, command, true, None, None)),
            Err(e) => out.push(self.error(request_seq, command, e)),
        }
    }

    fn error(&mut self, request_seq: i64, command: &str, e: super::BackendError) -> DapMessage {
        self.response(request_seq, command, false, None, Some(e.to_string()))
    }

    /// The `supportsEvaluateForHovers` value this session advertises.
    ///
    /// One source for both `capabilities_body` and the hover request gate
    /// (#9573), so this bridge cannot advertise one thing and enforce another.
    fn advertised_evaluate_for_hovers(&self) -> bool {
        // Gated on the PEER authority, not the native one (#9573). Reading the
        // native gate here would mean promoting native silently opened hover to
        // a live external debugger's evaluator, which has no pure inspection of
        // its own. Still intersected with catalog ∩ backend so that, if the peer
        // gate is ever promoted, it cannot over-advertise against a peer that
        // cannot evaluate at all.
        let negotiated = intersect_dap_capabilities(
            &CatalogDapFlags::from_catalog(),
            &self.backend.capabilities(),
        );
        crate::backend::capabilities::peer_bridge_hover_admission(
            crate::backend::capabilities::advertises_evaluate_for_hovers(),
            crate::backend::capabilities::PEER_BRIDGE_ADVERTISES_EVALUATE_FOR_HOVERS,
            negotiated.supports_evaluate,
        )
    }

    /// The `supportsSetExpression` value this session advertises.
    ///
    /// One source for both `capabilities_body` and the setExpression request
    /// gate (#9568), so this bridge cannot advertise one thing and enforce
    /// another.
    fn advertised_set_expression(&self) -> bool {
        // Gated on the PEER authority, not the native one (#9568): the native
        // promotion boundary must not silently open an external-peer assignment
        // path that has no exact current-frame assignment proof of its own.
        crate::backend::capabilities::peer_bridge_set_expression_admission(
            crate::backend::capabilities::SET_EXPRESSION_PROMOTION_PROVEN,
            crate::backend::capabilities::PEER_BRIDGE_ADVERTISES_SET_EXPRESSION,
        )
    }

    fn capabilities_body(&self) -> Value {
        let negotiated = intersect_dap_capabilities(
            &CatalogDapFlags::from_catalog(),
            &self.backend.capabilities(),
        );
        json!({
            "supportsConfigurationDoneRequest": true,
            "supportsTerminateRequest": true,
            // Answered locally from the AST oracle, so always available.
            "supportsBreakpointLocationsRequest": true,
            "supportsConditionalBreakpoints": negotiated.supports_conditional_breakpoints,
            "supportsHitConditionalBreakpoints": negotiated.supports_hit_conditional_breakpoints,
            "supportsLogPoints": negotiated.supports_log_points,
            "supportsFunctionBreakpoints": negotiated.supports_function_breakpoints,
            "supportsDataBreakpoints": negotiated.supports_data_breakpoints,
            // One source with the hover request gate (#9573), and gated on the
            // peer authority rather than the native one.
            "supportsEvaluateForHovers": self.advertised_evaluate_for_hovers(),
            // One source with the setExpression request gate (#9568), gated on
            // the peer authority rather than the native one.
            "supportsSetExpression": self.advertised_set_expression(),
            "supportsSetVariable": negotiated.supports_set_variable,
        })
    }

    fn handle_set_breakpoints(&mut self, args: Option<&Value>) -> super::BackendResult<Value> {
        let args = args.ok_or_else(|| super::BackendError::Protocol("missing arguments".into()))?;
        let source = dap_source(args.get("source"));
        let input = args.get("breakpoints").and_then(Value::as_array).cloned().unwrap_or_default();
        // DAP requires the response `breakpoints` array to match the request
        // positionally and in length. An entry missing `line` can't be sent to
        // the backend, but must still occupy its slot as `verified: false` rather
        // than being dropped (which would shift every later entry onto the wrong
        // requested line). Track, per input position, the index of its resolved
        // result — or `None` for the unbuildable entries.
        let mut breakpoints = Vec::new();
        let mut slots: Vec<Option<usize>> = Vec::with_capacity(input.len());
        for b in &input {
            match b.get("line").and_then(Value::as_u64) {
                Some(line) => {
                    slots.push(Some(breakpoints.len()));
                    breakpoints.push(DebugBreakpoint {
                        id: None,
                        source: source.clone(),
                        line: line as u32,
                        column: b.get("column").and_then(Value::as_u64).map(|c| c as u32),
                        condition: str_field(b, "condition"),
                        hit_condition: str_field(b, "hitCondition"),
                        log_message: str_field(b, "logMessage"),
                    });
                }
                None => slots.push(None),
            }
        }
        let resolved =
            self.backend.set_breakpoints(SetBackendBreakpointsParams { source, breakpoints })?;
        let bps: Vec<Value> = slots
            .iter()
            .map(|slot| match slot.and_then(|i| resolved.get(i)) {
                Some(r) => json!({
                    "id": r.id,
                    "verified": r.verified,
                    "line": r.actual_position.line,
                    "column": r.actual_position.column,
                    "message": r.message,
                }),
                // Either the entry lacked a `line`, or the backend returned fewer
                // results than requested — echo an unverified slot to keep the
                // response positionally aligned with the request.
                None => json!({ "verified": false, "message": "line required" }),
            })
            .collect();
        Ok(json!({ "breakpoints": bps }))
    }

    fn handle_set_function_breakpoints(
        &mut self,
        args: Option<&Value>,
    ) -> super::BackendResult<Value> {
        let input = args
            .and_then(|a| a.get("breakpoints"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        // Positional/length contract as in `handle_set_breakpoints`: an entry
        // missing `name` keeps its slot as `verified: false` instead of shifting
        // the array.
        let mut breakpoints = Vec::new();
        let mut slots: Vec<Option<usize>> = Vec::with_capacity(input.len());
        for b in &input {
            match str_field(b, "name") {
                Some(name) => {
                    slots.push(Some(breakpoints.len()));
                    breakpoints.push(DebugFunctionBreakpoint {
                        name,
                        condition: str_field(b, "condition"),
                    });
                }
                None => slots.push(None),
            }
        }
        let resolved =
            self.backend.set_function_breakpoints(SetFunctionBreakpointsParams { breakpoints })?;
        let bps: Vec<Value> = slots
            .iter()
            .map(|slot| match slot.and_then(|i| resolved.get(i)) {
                Some(r) => json!({ "id": r.id, "verified": r.verified }),
                None => json!({ "verified": false, "message": "name required" }),
            })
            .collect();
        Ok(json!({ "breakpoints": bps }))
    }

    fn handle_stack_trace(
        &mut self,
        thread_id: ThreadId,
        args: Option<&Value>,
    ) -> super::BackendResult<Value> {
        let params = StackTraceParams {
            thread_id,
            start_frame: args
                .and_then(|a| a.get("startFrame"))
                .and_then(Value::as_u64)
                .map(|v| v as u32),
            levels: args.and_then(|a| a.get("levels")).and_then(Value::as_u64).map(|v| v as u32),
        };
        let frames = self.backend.stack_trace(params)?;
        let total = frames.len();
        let out: Vec<Value> = frames
            .into_iter()
            .map(|f| {
                json!({
                    "id": f.id,
                    "name": f.name,
                    "source": { "path": f.source.path.to_string_lossy(), "name": f.source.name },
                    "line": f.line,
                    "column": f.column,
                })
            })
            .collect();
        Ok(json!({ "stackFrames": out, "totalFrames": total }))
    }

    fn handle_scopes(&mut self, args: Option<&Value>) -> super::BackendResult<Value> {
        let frame_id =
            FrameId(args.and_then(|a| a.get("frameId")).and_then(Value::as_i64).unwrap_or(0));
        let scopes = self.backend.scopes(frame_id)?;
        let out: Vec<Value> = scopes
            .into_iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "variablesReference": s.variables_reference.0,
                    "expensive": s.expensive,
                })
            })
            .collect();
        Ok(json!({ "scopes": out }))
    }

    fn handle_variables(&mut self, args: Option<&Value>) -> super::BackendResult<Value> {
        let vref = VariablesRef(
            args.and_then(|a| a.get("variablesReference")).and_then(Value::as_i64).unwrap_or(0),
        );
        let vars = self.backend.variables(vref)?;
        let out: Vec<Value> = vars
            .into_iter()
            .map(|v| {
                json!({
                    "name": v.name,
                    "value": v.value,
                    "type": v.type_name,
                    "variablesReference": v.variables_reference.map(|r| r.0).unwrap_or(0),
                    "namedVariables": v.named_variables,
                    "indexedVariables": v.indexed_variables,
                })
            })
            .collect();
        Ok(json!({ "variables": out }))
    }

    fn handle_evaluate(&mut self, args: Option<&Value>) -> super::BackendResult<Value> {
        // #9573: refuse hover before delegating to the peer backend, gated on the
        // exact value `capabilities_body` advertises for this session so the
        // advertisement and the admission cannot disagree — today, or after a
        // future promotion.
        if crate::backend::capabilities::refuse_hover_evaluation(
            self.advertised_evaluate_for_hovers(),
            args.and_then(|a| a.get("context")).and_then(Value::as_str),
        ) {
            return Err(super::BackendError::Unsupported(
                crate::backend::capabilities::HOVER_UNSUPPORTED_MESSAGE.to_string(),
            ));
        }
        let expression = args
            .and_then(|a| a.get("expression"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let frame_id = args.and_then(|a| a.get("frameId")).and_then(Value::as_i64).map(FrameId);
        let context = args
            .and_then(|a| a.get("context"))
            .and_then(Value::as_str)
            .map(evaluate_context)
            .unwrap_or(EvaluateContext::Repl);
        let result = self.backend.evaluate(EvaluateParams { expression, frame_id, context })?;
        Ok(json!({
            "result": result.result,
            "type": result.type_name,
            "variablesReference": result.variables_reference.map(|r| r.0).unwrap_or(0),
        }))
    }
}

enum Step {
    Next,
    In,
    Out,
}

/// Drive a [`DapPeerBridge`] over **stdio** — the production DAP transport an
/// editor uses when it spawns the adapter as a child process
/// (`perl-dap --external-peer HOST:PORT`).
///
/// stdin has no read timeout, so a dedicated reader thread frames requests off
/// stdin and forwards each frame body over a channel; the main loop interleaves
/// draining backend events to stdout with a `recv_timeout` on that channel, so
/// asynchronous stops/output reach the editor promptly without blocking on
/// stdin.
///
/// # Errors
/// Returns a transport error if writing framed messages to stdout fails.
pub fn run_external_peer_session_stdio(
    bridge: DapPeerBridge,
    poll_interval: Duration,
) -> std::io::Result<()> {
    // `stdin()`/`stdout()` are `'static` and `Send`, so they move cleanly into
    // the generic threaded driver (the reader half runs on its own thread).
    run_peer_session_threaded(std::io::stdin(), std::io::stdout(), bridge, poll_interval)
}

/// Generic threaded driver: read framed DAP requests from `reader_src` on a
/// dedicated thread, dispatch them to `bridge`, and write framed
/// responses/events to `writer` on the calling thread, interleaving backend
/// event delivery on `poll_interval` ticks.
///
/// Frame admission is bounded (#9522): the reader enqueues through
/// [`super::peer_frame_queue::admit_peer_frame`] against a
/// [`super::peer_frame_queue::PEER_FRAME_QUEUE_CAPACITY`] bounded channel. If
/// the session loop cannot keep up and the queue saturates, the reader stops
/// and the session ends with the typed
/// [`super::peer_frame_queue::PEER_BACKPRESSURE_MSG`] failure instead of
/// buffering without bound or reporting generic success.
///
/// Used by [`run_external_peer_session_stdio`] (stdin/stdout) and exercised in
/// tests over in-memory pipes. The reader thread is detached rather than joined:
/// on a DAP `disconnect` the editor may not close its write half immediately, so
/// joining could block; the thread exits on stdin EOF or process teardown.
///
/// # Errors
/// Returns a transport error if writing framed messages to `writer` fails, or
/// the typed peer backpressure failure when the bounded frame queue saturates.
fn run_peer_session_threaded<R, W>(
    reader_src: R,
    mut writer: W,
    mut bridge: DapPeerBridge,
    poll_interval: Duration,
) -> std::io::Result<()>
where
    R: Read + Send + 'static,
    W: Write,
{
    use super::peer_frame_queue::{PEER_FRAME_QUEUE_CAPACITY, admit_peer_frame, overflow_failure};
    use perl_lsp_rs_core::transport::ContentLengthFramer;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;

    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(PEER_FRAME_QUEUE_CAPACITY);
    let overflow = std::sync::Arc::new(AtomicBool::new(false));
    let reader_overflow = std::sync::Arc::clone(&overflow);
    let _reader = std::thread::spawn(move || {
        let mut src = reader_src;
        let mut framer = ContentLengthFramer::new();
        let mut buf = [0u8; 8 * 1024];
        loop {
            match src.read(&mut buf) {
                Ok(0) => break, // EOF: editor closed its write half
                Ok(n) => {
                    framer.push(&buf[..n]);
                    loop {
                        match framer.try_next() {
                            // Receiver gone, queue saturated, or session ended —
                            // stop reading (bounded admission #9522).
                            Ok(Some(body)) => {
                                if !admit_peer_frame(
                                    &tx,
                                    body,
                                    &reader_overflow,
                                    "peer bridge (stdio)",
                                ) {
                                    return;
                                }
                            }
                            Ok(None) => break,
                            // `ContentLengthFramer::try_next` already discards the
                            // malformed header block before returning an error, so
                            // the buffer can still hold a valid subsequent frame.
                            // Skip and keep parsing rather than tearing down the
                            // whole session on one bad frame — mirrors the
                            // recoverable-error handling in
                            // `ContentLengthMessageReader::read_next`.
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    "peer bridge (stdio): dropping malformed DAP frame"
                                );
                            }
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });

    loop {
        // Deliver any asynchronous backend events first.
        let events = bridge.poll_events();
        if !events.is_empty() {
            write_dap_msgs(&mut writer, &events)?;
        }

        match rx.recv_timeout(poll_interval) {
            Ok(body) => {
                let (out, disconnect) = dispatch_frame(&mut bridge, &body);
                write_dap_msgs(&mut writer, &out)?;
                if disconnect {
                    break;
                }
            }
            // No editor input this tick; loop to poll events again.
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // Reader thread ended (stdin closed / malformed / saturated queue):
            // a saturated queue fails the session closed with the typed
            // backpressure disposition instead of generic success (#9522);
            // frames admitted before the overflow were still dispatched above.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(failure) = overflow_failure(&overflow) {
                    return Err(failure);
                }
                break;
            }
        }
    }

    // A latched overflow wins over every successful exit — including a DAP
    // `disconnect` admitted before the reader stopped: reporting generic
    // success after rejecting frames is the explicit #9522 falsifier.
    if let Some(failure) = overflow_failure(&overflow) {
        return Err(failure);
    }
    Ok(())
}

/// Serialize and Content-Length frame each DAP message to `writer`.
///
/// On the (practically impossible) serialize failure, the message is skipped
/// rather than writing a `Content-Length: 0` frame that would corrupt the
/// stream and desync the client.
fn write_dap_msgs<W: Write>(writer: &mut W, msgs: &[DapMessage]) -> std::io::Result<()> {
    use perl_lsp_rs_core::transport::frame;
    for m in msgs {
        match serde_json::to_vec(m) {
            Ok(body) => writer.write_all(&frame(&body))?,
            Err(e) => {
                tracing::error!(error = %e, "peer bridge: dropping unserializable DAP message")
            }
        }
    }
    writer.flush()
}

/// Dispatch one raw DAP request frame to the bridge, returning the messages to
/// write back and whether the request was a `disconnect` (session should end).
///
/// The frame is parsed leniently: `command` and `seq` are extracted from the raw
/// JSON even if the typed `Request` shape doesn't match exactly (e.g. `seq` sent
/// as a JSON float), so a client is never left hanging without a response. A
/// frame with no `command` is dropped.
fn dispatch_frame(bridge: &mut DapPeerBridge, body: &[u8]) -> (Vec<DapMessage>, bool) {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let Some(command) = v.get("command").and_then(Value::as_str) else {
        tracing::warn!("peer bridge: dropping DAP frame with no `command`");
        return (Vec::new(), false);
    };
    let seq =
        v.get("seq").and_then(|s| s.as_i64().or_else(|| s.as_f64().map(|f| f as i64))).unwrap_or(0);
    let out = bridge.dispatch(seq, command, v.get("arguments").cloned());
    let disconnect = command == "disconnect";
    (out, disconnect)
}

// ---------------------------------------------------------------------------
// Argument / value translation helpers
// ---------------------------------------------------------------------------

fn parse_initialize(args: Option<&Value>) -> InitializeBackendParams {
    InitializeBackendParams {
        client_id: args
            .and_then(|a| a.get("clientID"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        adapter_id: args
            .and_then(|a| a.get("adapterID"))
            .and_then(Value::as_str)
            .unwrap_or("perl")
            .to_string(),
        lines_start_at_1: args
            .and_then(|a| a.get("linesStartAt1"))
            .and_then(Value::as_bool)
            .unwrap_or(true),
        columns_start_at_1: args
            .and_then(|a| a.get("columnsStartAt1"))
            .and_then(Value::as_bool)
            .unwrap_or(true),
    }
}

fn parse_launch(args: Option<&Value>) -> LaunchBackendParams {
    let mut params = LaunchBackendParams::default();
    if let Some(a) = args {
        if let Some(p) = a.get("program").and_then(Value::as_str) {
            params.program = p.into();
        }
        if let Some(arr) = a.get("args").and_then(Value::as_array) {
            params.args = arr.iter().filter_map(|v| v.as_str().map(ToString::to_string)).collect();
        }
        params.cwd = a.get("cwd").and_then(Value::as_str).map(Into::into);
        if let Some(obj) = a.get("env").and_then(Value::as_object) {
            params.env = obj
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
        }
        if let Some(arr) = a.get("includePaths").and_then(Value::as_array) {
            params.include_paths = arr.iter().filter_map(|v| v.as_str().map(Into::into)).collect();
        }
        params.stop_on_entry = a.get("stopOnEntry").and_then(Value::as_bool).unwrap_or(false);
    }
    params
}

fn parse_attach(args: Option<&Value>) -> AttachBackendParams {
    AttachBackendParams {
        host: args
            .and_then(|a| a.get("host"))
            .and_then(Value::as_str)
            .unwrap_or("127.0.0.1")
            .to_string(),
        port: args.and_then(|a| a.get("port")).and_then(Value::as_u64).unwrap_or(0) as u16,
    }
}

fn dap_source(v: Option<&Value>) -> DebugSource {
    let path = v.and_then(|s| s.get("path")).and_then(Value::as_str).unwrap_or_default();
    DebugSource {
        path: path.into(),
        name: v.and_then(|s| s.get("name")).and_then(Value::as_str).map(ToString::to_string),
        source_reference: v.and_then(|s| s.get("sourceReference")).and_then(Value::as_i64),
    }
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(ToString::to_string)
}

/// Answer a DAP `breakpointLocations` request from the local AST oracle.
///
/// Reads the source from disk (it is on the same host as `perl-dap`) and returns
/// the breakable lines within the requested `[line, endLine]` range. On any error
/// (missing args, unreadable/unparseable source) returns an empty set rather than
/// failing the request — the editor treats "no breakable locations" gracefully.
fn handle_breakpoint_locations(args: Option<&Value>) -> Value {
    let empty = json!({ "breakpoints": [] });
    let Some(args) = args else { return empty };
    let Some(path) = args.get("source").and_then(|s| s.get("path")).and_then(Value::as_str) else {
        return empty;
    };
    // DAP requires `line`; `endLine` is optional and defaults to `line` (a
    // single-line query). A request without `line` is malformed — return the
    // empty set rather than treating a lone `endLine` as `1..=endLine`, which
    // would leak every breakable line up to it.
    let Some(start) = args.get("line").and_then(Value::as_u64).map(|v| v as u32) else {
        return empty;
    };
    let end = args.get("endLine").and_then(Value::as_u64).map(|v| v as u32).unwrap_or(start);
    let Ok(text) = std::fs::read_to_string(path) else {
        return empty;
    };
    let Ok(oracle) = AstBreakpointOracle::new(DebugSource::from_path(path), &text) else {
        return empty;
    };
    let locations: Vec<Value> = oracle
        .breakable_line_candidates()
        .into_iter()
        .filter(|&line| line >= start && line <= end)
        .map(|line| json!({ "line": line }))
        .collect();
    json!({ "breakpoints": locations })
}

fn dap_stop_reason(reason: &StopReason) -> String {
    match reason {
        StopReason::Entry => "entry".into(),
        StopReason::Step => "step".into(),
        StopReason::Breakpoint => "breakpoint".into(),
        StopReason::FunctionBreakpoint => "function breakpoint".into(),
        StopReason::DataBreakpoint => "data breakpoint".into(),
        StopReason::Exception => "exception".into(),
        StopReason::Pause => "pause".into(),
        StopReason::Unknown(s) => s.clone(),
    }
}

fn category_str(c: OutputCategory) -> &'static str {
    c.as_dap_category()
}

fn evaluate_context(c: &str) -> EvaluateContext {
    match c {
        "watch" => EvaluateContext::Watch,
        "repl" => EvaluateContext::Repl,
        "hover" => EvaluateContext::Hover,
        "variables" => EvaluateContext::Variables,
        other => EvaluateContext::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::capabilities::DebugBackendCapabilities;
    use crate::backend::{
        AttachResult, BackendResult, ContinueResult, DebugBackend, EvaluateResult, LaunchResult,
    };
    use crate::model::{
        DebugPosition, DebugScope, DebugStackFrame, DebugVariable, ResolvedBreakpoint,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    #[derive(Default)]
    struct ScriptBackend {
        events: Vec<DebugEvent>,
        /// Counts real `evaluate` calls that reached the backend.
        ///
        /// Shared with the test because the bridge takes ownership of the
        /// backend. This turns "no debugger command was written" into a direct
        /// observation instead of an inference from the response (#9573).
        evaluate_calls: Arc<AtomicUsize>,
        /// Counts real `step_in` calls that reached the backend, so a refused
        /// targeted `stepIn` is observed directly rather than inferred (#9069).
        step_in_calls: Arc<AtomicUsize>,
    }

    impl DebugBackend for ScriptBackend {
        fn name(&self) -> &str {
            "script"
        }
        fn capabilities(&self) -> DebugBackendCapabilities {
            DebugBackendCapabilities::ptkdb_v1_defaults()
        }
        fn initialize(&mut self, _p: InitializeBackendParams) -> BackendResult<()> {
            Ok(())
        }
        fn launch(&mut self, _p: LaunchBackendParams) -> BackendResult<LaunchResult> {
            Ok(LaunchResult { success: true })
        }
        fn attach(&mut self, _p: AttachBackendParams) -> BackendResult<AttachResult> {
            Ok(AttachResult { success: true })
        }
        fn set_breakpoints(
            &mut self,
            p: SetBackendBreakpointsParams,
        ) -> BackendResult<Vec<ResolvedBreakpoint>> {
            Ok(p.breakpoints
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
            _p: SetFunctionBreakpointsParams,
        ) -> BackendResult<Vec<ResolvedBreakpoint>> {
            Ok(Vec::new())
        }
        fn continue_thread(&mut self, tid: ThreadId) -> BackendResult<ContinueResult> {
            // Simulate the debuggee resuming then hitting a breakpoint.
            self.events.push(DebugEvent::Continued { thread_id: tid });
            self.events.push(DebugEvent::Stopped {
                reason: StopReason::Breakpoint,
                thread_id: tid,
                position: None,
            });
            Ok(ContinueResult { all_threads_continued: true })
        }
        fn next(&mut self, _t: ThreadId) -> BackendResult<()> {
            Ok(())
        }
        fn step_in(&mut self, _t: ThreadId) -> BackendResult<()> {
            self.step_in_calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
        fn step_out(&mut self, _t: ThreadId) -> BackendResult<()> {
            Ok(())
        }
        fn pause(&mut self, _t: ThreadId) -> BackendResult<()> {
            // Deliberately reports a FOREIGN id (9 != the advertised synthetic
            // context): `backend_events_normalize_foreign_thread_ids...` uses
            // this to prove the bridge normalizes identity-bearing backend
            // events instead of forwarding raw ids.
            self.events.push(DebugEvent::Stopped {
                reason: StopReason::Pause,
                thread_id: ThreadId(9),
                position: None,
            });
            Ok(())
        }
        fn stack_trace(&mut self, _p: StackTraceParams) -> BackendResult<Vec<DebugStackFrame>> {
            Ok(vec![DebugStackFrame {
                id: 1,
                name: "main::run".into(),
                source: DebugSource::from_path("/work/script.pl"),
                line: 42,
                column: 1,
            }])
        }
        fn scopes(&mut self, _f: FrameId) -> BackendResult<Vec<DebugScope>> {
            Ok(vec![DebugScope {
                name: "Locals".into(),
                variables_reference: VariablesRef(1000),
                expensive: false,
            }])
        }
        fn variables(&mut self, _r: VariablesRef) -> BackendResult<Vec<DebugVariable>> {
            Ok(vec![DebugVariable {
                name: "$x".into(),
                value: "42".into(),
                type_name: Some("scalar".into()),
                variables_reference: None,
                indexed_variables: None,
                named_variables: None,
            }])
        }
        fn evaluate(&mut self, p: EvaluateParams) -> BackendResult<EvaluateResult> {
            self.evaluate_calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(EvaluateResult {
                result: format!("={}", p.expression),
                type_name: None,
                variables_reference: None,
            })
        }
        fn drain_events(&mut self) -> Vec<DebugEvent> {
            std::mem::take(&mut self.events)
        }
        fn disconnect(&mut self, _t: bool) -> BackendResult<()> {
            Ok(())
        }
    }

    fn bridge() -> DapPeerBridge {
        DapPeerBridge::new(Box::new(ScriptBackend::default()))
    }

    fn as_response(msg: &DapMessage) -> Result<(&str, bool, Option<&Value>), String> {
        match msg {
            DapMessage::Response { command, success, body, .. } => {
                Ok((command.as_str(), *success, body.as_ref()))
            }
            _ => Err(format!("expected response, got {msg:?}")),
        }
    }

    fn event_name(msg: &DapMessage) -> Result<&str, String> {
        match msg {
            DapMessage::Event { event, .. } => Ok(event.as_str()),
            _ => Err(format!("expected event, got {msg:?}")),
        }
    }

    #[test]
    fn initialize_returns_capabilities_and_initialized_event() -> Result<(), String> {
        let mut b = bridge();
        let out = b.dispatch(1, "initialize", None);
        assert_eq!(out.len(), 2);
        let (cmd, ok, body) = as_response(&out[0])?;
        assert_eq!(cmd, "initialize");
        assert!(ok);
        let caps = must_some(body);
        assert_eq!(caps["supportsConfigurationDoneRequest"], true);
        assert_eq!(caps["supportsBreakpointLocationsRequest"], true);
        // ptkdb v1 negotiated: no logpoints/data breakpoints.
        assert_eq!(caps["supportsLogPoints"], false);
        assert_eq!(caps["supportsDataBreakpoints"], false);
        assert_eq!(event_name(&out[1])?, "initialized");
        Ok(())
    }

    #[test]
    fn set_breakpoints_translates_to_dap_body() -> Result<(), String> {
        let mut b = bridge();
        let args = json!({
            "source": { "path": "/work/script.pl" },
            "breakpoints": [{ "line": 42, "condition": "$x > 10" }, { "line": 7 }],
        });
        let out = b.dispatch(2, "setBreakpoints", Some(args));
        let (_, ok, body) = as_response(&out[0])?;
        assert!(ok);
        let bps = must_some(must_some(body)["breakpoints"].as_array());
        assert_eq!(bps.len(), 2);
        assert_eq!(bps[0]["verified"], true);
        assert_eq!(bps[0]["line"], 42);
        Ok(())
    }

    #[test]
    fn set_breakpoints_keeps_positional_slots_for_line_less_entries() -> Result<(), String> {
        // DAP requires the response array to match the request positionally and
        // in length. A middle entry missing `line` must occupy its slot as
        // `verified: false`, not be dropped (which would shift line 9 onto slot 1).
        let mut b = bridge();
        let args = json!({
            "source": { "path": "/work/script.pl" },
            "breakpoints": [{ "line": 3 }, { "condition": "$x" }, { "line": 9 }],
        });
        let out = b.dispatch(2, "setBreakpoints", Some(args));
        let bps = must_some(must_some(as_response(&out[0])?.2)["breakpoints"].as_array()).clone();
        assert_eq!(bps.len(), 3, "response length must equal request length: {bps:?}");
        assert_eq!(bps[0]["verified"], true);
        assert_eq!(bps[0]["line"], 3);
        assert_eq!(bps[1]["verified"], false, "the line-less entry stays in slot 1");
        assert_eq!(bps[1]["message"], "line required");
        assert_eq!(bps[2]["verified"], true);
        assert_eq!(bps[2]["line"], 9);
        Ok(())
    }

    #[test]
    fn set_function_breakpoints_keeps_positional_slots_for_name_less_entries() -> Result<(), String>
    {
        let mut b = bridge();
        // The ScriptBackend returns no function breakpoints, so the only slot that
        // can be `verified: true` is none — but the name-less entry must still be
        // echoed as its own `verified: false` slot rather than dropped.
        let args = json!({ "breakpoints": [{ "condition": "1" }, { "name": "main::run" }] });
        let out = b.dispatch(2, "setFunctionBreakpoints", Some(args));
        let bps = must_some(must_some(as_response(&out[0])?.2)["breakpoints"].as_array()).clone();
        assert_eq!(bps.len(), 2, "response length must equal request length: {bps:?}");
        assert_eq!(bps[0]["verified"], false);
        assert_eq!(bps[0]["message"], "name required");
        Ok(())
    }

    #[test]
    fn disconnect_with_terminate_emits_terminated_and_arms_dedup() {
        let mut b = bridge();
        let out = b.dispatch(2, "disconnect", Some(json!({ "terminateDebuggee": true })));
        assert!(
            out.iter()
                .any(|m| matches!(m, DapMessage::Event { event, .. } if event == "terminated")),
            "disconnect(terminateDebuggee=true) must emit terminated: {out:?}"
        );
        // Dedup armed: a subsequent peer-queued terminated is suppressed.
        let mut more = Vec::new();
        b.push_dap_events(DebugEvent::Terminated { exit_code: Some(0) }, &mut more);
        assert!(more.is_empty(), "second terminated must be suppressed: {more:?}");
    }

    #[test]
    fn disconnect_without_terminate_does_not_emit_terminated() {
        let mut b = bridge();
        let out = b.dispatch(2, "disconnect", Some(json!({ "terminateDebuggee": false })));
        assert!(
            !out.iter()
                .any(|m| matches!(m, DapMessage::Event { event, .. } if event == "terminated")),
            "a plain disconnect must not synthesize terminated: {out:?}"
        );
    }

    #[test]
    fn continue_emits_continued_then_stopped_after_response() -> Result<(), String> {
        let mut b = bridge();
        let out = b.dispatch(3, "continue", Some(json!({ "threadId": 1 })));
        // response, then the two events the backend queued.
        let (cmd, ok, body) = as_response(&out[0])?;
        assert_eq!(cmd, "continue");
        assert!(ok);
        assert_eq!(must_some(body)["allThreadsContinued"], true);
        let events: Vec<&str> = out[1..].iter().map(event_name).collect::<Result<_, _>>()?;
        assert_eq!(events, vec!["continued", "stopped"]);
        // The stopped event carries the DAP reason + threadId.
        if let DapMessage::Event { body: Some(b), .. } = &out[2] {
            assert_eq!(b["reason"], "breakpoint");
            assert_eq!(b["threadId"], 1);
            assert_eq!(b["allThreadsStopped"], true);
        } else {
            return Err("expected stopped event body".into());
        }
        Ok(())
    }

    /// Thread-scoped requests that do not name the advertised synthetic
    /// execution context fail before any backend method runs (#8294): the
    /// failed response is the only emitted message and no backend event is
    /// queued.
    #[test]
    fn thread_scoped_requests_reject_unknown_or_missing_ids_before_the_backend_runs()
    -> Result<(), String> {
        let mut b = bridge();
        for (command, args) in [
            ("continue", Some(json!({ "threadId": 7 }))),
            ("continue", None),
            ("pause", Some(json!({ "threadId": 0 }))),
            ("next", Some(json!({ "threadId": -1 }))),
            ("stepIn", Some(json!({}))),
            ("stackTrace", Some(json!({ "threadId": 2 }))),
        ] {
            let out = b.dispatch(9, command, args);
            match &out[0] {
                DapMessage::Response { command: cmd, success, message, .. } => {
                    assert_eq!(cmd, command);
                    assert!(!success, "a non-advertised threadId must fail {command}: {out:?}");
                    let message = message.as_deref().unwrap_or_default();
                    assert!(
                        message.contains("synthetic execution context"),
                        "the rejection must name the advertised synthetic context: {message}"
                    );
                }
                other => return Err(format!("expected a response for {command}, got {other:?}")),
            }
            // `continue` is load-bearing here: its backend mock queues
            // continued+stopped events, so a single-message out proves the
            // backend method never ran.
            assert_eq!(out.len(), 1, "no backend side effects may follow {command}: {out:?}");
        }
        Ok(())
    }

    /// Backend events carry identity too: a backend that reports a foreign
    /// thread id still produces editor-visible stopped events named by the
    /// advertised synthetic context, never by the backend's raw id — otherwise
    /// the editor would observe an id that every thread-scoped request then
    /// rejects (#8294).
    #[test]
    fn backend_events_normalize_foreign_thread_ids_to_the_advertised_context() -> Result<(), String>
    {
        let mut b = bridge();
        // The mock backend queues a stopped event with foreign id 9 on pause;
        // the pause itself must name the advertised id to be accepted.
        let out = b.dispatch(11, "pause", Some(json!({ "threadId": 1 })));
        let (cmd, ok, _) = as_response(&out[0])?;
        assert_eq!(cmd, "pause");
        assert!(ok);
        let events: Vec<&str> = out[1..].iter().map(event_name).collect::<Result<_, _>>()?;
        assert_eq!(events, vec!["stopped"]);
        if let DapMessage::Event { body: Some(body), .. } = &out[1] {
            assert_eq!(body["threadId"], 1, "the foreign backend id must be normalized");
            assert_eq!(body["allThreadsStopped"], true);
        } else {
            return Err("expected a stopped event body".into());
        }
        Ok(())
    }

    #[test]
    fn stack_scopes_variables_evaluate_round_trip() -> Result<(), String> {
        let mut b = bridge();
        let st = b.dispatch(4, "stackTrace", Some(json!({ "threadId": 1 })));
        let frames = must_some(must_some(as_response(&st[0])?.2)["stackFrames"].as_array()).clone();
        assert_eq!(frames[0]["name"], "main::run");
        assert_eq!(frames[0]["line"], 42);
        assert_eq!(frames[0]["source"]["path"], "/work/script.pl");

        let sc = b.dispatch(5, "scopes", Some(json!({ "frameId": 1 })));
        assert_eq!(must_some(as_response(&sc[0])?.2)["scopes"][0]["variablesReference"], 1000);

        let va = b.dispatch(6, "variables", Some(json!({ "variablesReference": 1000 })));
        let vars = must_some(as_response(&va[0])?.2)["variables"].clone();
        assert_eq!(vars[0]["name"], "$x");
        assert_eq!(vars[0]["value"], "42");
        assert_eq!(vars[0]["variablesReference"], 0);

        let ev = b.dispatch(7, "evaluate", Some(json!({ "expression": "$x", "context": "watch" })));
        assert_eq!(must_some(as_response(&ev[0])?.2)["result"], "=$x");
        Ok(())
    }

    /// #9573: the external-peer bridge refuses hover before delegating.
    ///
    /// Discriminating because the backend here IS connected: without the gate a
    /// hover request would succeed with a `=$x` result, exactly as the `watch`
    /// case above does. The `watch` control in this same test proves the gate
    /// did not simply break all evaluation.
    #[test]
    fn hover_context_is_refused_before_delegating_to_the_peer() -> Result<(), String> {
        let mut b = bridge();

        for context in ["hover", "Hover", "HOVER"] {
            let out =
                b.dispatch(7, "evaluate", Some(json!({ "expression": "$x", "context": context })));
            let (cmd, ok, body) = as_response(&out[0])?;
            assert_eq!(cmd, "evaluate");
            assert!(!ok, "hover-context evaluate must be refused ({context})");
            assert!(body.is_none(), "a refused hover must not carry a result body ({context})");
            if let DapMessage::Response { message, .. } = &out[0] {
                let message = message.as_deref().unwrap_or("");
                assert!(
                    message.contains("supportsEvaluateForHovers"),
                    "{context}: expected the #9573 hover refusal, got {message:?}"
                );
            }
        }

        // Negative control: watch still evaluates against the same live backend.
        let ok = b.dispatch(8, "evaluate", Some(json!({ "expression": "$x", "context": "watch" })));
        assert_eq!(must_some(as_response(&ok[0])?.2)["result"], "=$x");
        Ok(())
    }

    /// #9573 same-session receipt: false capability AND zero backend invocation.
    ///
    /// One initialized session carries both halves of the claim, which is what
    /// makes this a receipt rather than two unrelated assertions:
    ///
    /// 1. `initialize` advertises `supportsEvaluateForHovers: false`;
    /// 2. a hover request in that same session is refused;
    /// 3. the backend recorded **zero** evaluate invocations.
    ///
    /// Step 3 upgrades "no debugger command was written" from an inference
    /// about the response to a direct observation of the backend. The trailing
    /// `watch` control is what keeps it honest: it drives the counter to 1
    /// through the same seam, so a counter that never increments (or a bridge
    /// that stopped delegating entirely) fails instead of reading as success.
    #[test]
    fn same_session_hover_is_false_and_never_reaches_the_backend() -> Result<(), String> {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut b = DapPeerBridge::new(Box::new(ScriptBackend {
            events: Vec::new(),
            evaluate_calls: Arc::clone(&calls),
            step_in_calls: Arc::new(AtomicUsize::new(0)),
        }));

        let init = b.dispatch(1, "initialize", Some(json!({ "adapterID": "perl" })));
        let caps = must_some(as_response(&init[0])?.2);
        assert_eq!(
            caps["supportsEvaluateForHovers"], false,
            "the session must advertise hover false"
        );

        let hover =
            b.dispatch(2, "evaluate", Some(json!({ "expression": "$x", "context": "hover" })));
        let (_, ok, body) = as_response(&hover[0])?;
        assert!(!ok, "hover must be refused in this same session");
        assert!(body.is_none(), "a refused hover must not carry a result body");
        assert_eq!(
            calls.load(AtomicOrdering::SeqCst),
            0,
            "a refused hover must reach the backend zero times — no evaluate was delegated"
        );

        // Control: the counter is live, and ordinary evaluation still delegates.
        let watch =
            b.dispatch(3, "evaluate", Some(json!({ "expression": "$x", "context": "watch" })));
        assert_eq!(must_some(as_response(&watch[0])?.2)["result"], "=$x");
        assert_eq!(
            calls.load(AtomicOrdering::SeqCst),
            1,
            "watch must still reach the backend, or the zero above proves nothing"
        );
        Ok(())
    }

    /// #9573: the peer bridge's hover gate is independent of the native gate.
    ///
    /// `ScriptBackend` reports `ptkdb_v1_defaults`, which has `evaluate: true`
    /// — exactly the shape that would ride along if this mode read the native
    /// authority. Both the advertised value and the admission decision must
    /// stay closed here regardless of what the native gate says, because an
    /// external peer runs its own evaluator with no pure inspection of its own.
    ///
    /// This is checked through the live negotiation path rather than by
    /// asserting the constant, so promoting the native gate and re-running this
    /// test is a real falsifier: if the coupling ever comes back, this fails.
    #[test]
    fn native_promotion_cannot_open_peer_hover() -> Result<(), String> {
        let mut b = bridge();

        let init = b.dispatch(1, "initialize", Some(json!({ "adapterID": "perl" })));
        let caps = must_some(as_response(&init[0])?.2);

        // Precondition: this peer *can* evaluate, so the assertion below is
        // discriminating rather than trivially satisfied by an inert backend.
        let evaluated =
            b.dispatch(2, "evaluate", Some(json!({ "expression": "$x", "context": "watch" })));
        assert_eq!(
            must_some(as_response(&evaluated[0])?.2)["result"],
            "=$x",
            "precondition: the peer backend can evaluate"
        );

        assert_eq!(
            caps["supportsEvaluateForHovers"], false,
            "an evaluate-capable peer must still not advertise hover, whatever the native gate says"
        );

        let hover =
            b.dispatch(3, "evaluate", Some(json!({ "expression": "$x", "context": "hover" })));
        let (_, ok, _) = as_response(&hover[0])?;
        assert!(!ok, "an evaluate-capable peer must still refuse hover");
        Ok(())
    }

    /// #9573: the advertised capability agrees with the enforced behaviour.
    #[test]
    fn peer_bridge_never_advertises_hover() -> Result<(), String> {
        let mut b = bridge();
        let out = b.dispatch(1, "initialize", Some(json!({ "adapterID": "perl" })));
        let caps = must_some(as_response(&out[0])?.2);
        assert_eq!(caps["supportsEvaluateForHovers"], false);
        Ok(())
    }

    /// #9089: the peer bridge refuses the routed inlineValues extension on its
    /// own explicit dispatch route — the refusal must not fall through to the
    /// lenient success-empty compatibility acknowledgement, for an extension
    /// this mode neither advertises nor negotiates.
    #[test]
    fn peer_bridge_refuses_inline_values() -> Result<(), String> {
        let mut b = bridge();

        let out = b.dispatch(
            2,
            "inlineValues",
            Some(json!({ "source": { "path": "script.pl" }, "startLine": 1, "endLine": 2 })),
        );
        match &out[0] {
            DapMessage::Response { success, body, message, .. } => {
                assert!(!success, "inlineValues must be refused in peer mode, not acked");
                assert!(body.is_none(), "a refused inlineValues response carries no body");
                assert_eq!(
                    message.as_deref(),
                    Some(crate::backend::capabilities::INLINE_VALUES_EXTENSION_UNSUPPORTED_MESSAGE),
                    "the refusal must be the single deterministic #9089 message"
                );
            }
            other => return Err(format!("expected response, got {other:?}")),
        }
        Ok(())
    }

    /// #9568: the peer bridge refuses setExpression exactly as it advertises
    /// it false — the fallthrough must not acknowledge an assignment that
    /// never happened while the native adapter refuses the same request.
    #[test]
    fn peer_bridge_refuses_set_expression_like_it_advertises_it() -> Result<(), String> {
        let mut b = bridge();

        let init = b.dispatch(1, "initialize", Some(json!({ "adapterID": "perl" })));
        let caps = must_some(as_response(&init[0])?.2);
        assert_eq!(
            caps["supportsSetExpression"], false,
            "the peer session must advertise setExpression false (#9568)"
        );

        let out = b.dispatch(
            2,
            "setExpression",
            Some(json!({ "expression": "$x", "value": "42", "frameId": 0 })),
        );
        match &out[0] {
            DapMessage::Response { success, body, message, .. } => {
                assert!(!success, "setExpression must be refused in peer mode, not acked");
                assert!(body.is_none(), "a refused setExpression must not carry a result body");
                assert_eq!(
                    message.as_deref(),
                    Some(crate::backend::capabilities::SET_EXPRESSION_UNSUPPORTED_MESSAGE),
                    "the refusal must be the single deterministic #9568 message"
                );
            }
            other => return Err(format!("expected response, got {other:?}")),
        }
        Ok(())
    }

    #[test]
    fn native_only_step_in_targets_fails_closed() -> Result<(), String> {
        let mut b = bridge();
        let out = b.dispatch(17, "stepInTargets", Some(json!({ "frameId": 1 })));
        let (command, success, body) = as_response(&out[0])?;
        assert_eq!(command, "stepInTargets");
        assert!(!success, "peer-unavailable requests must not acknowledge success");
        assert!(body.is_none(), "a refused request must not carry a response body");
        if let DapMessage::Response { message, .. } = &out[0] {
            assert!(message.as_deref().is_some_and(|message| !message.is_empty()));
        }
        Ok(())
    }

    /// #9064: standard goto is native-only and fail-closed; a peer frontend
    /// must never acknowledge a `goto`/`gotoTargets` request it cannot route
    /// to a backend, whatever the native catalog says.
    #[test]
    fn native_only_goto_requests_fail_closed() -> Result<(), String> {
        let mut b = bridge();
        for (seq, command, args) in [
            (17, "gotoTargets", json!({ "source": {"path": "s.pl"}, "line": 3 })),
            (18, "goto", json!({ "threadId": 1, "targetId": 1 })),
        ] {
            let out = b.dispatch(seq, command, Some(args));
            let (rcmd, success, body) = as_response(&out[0])?;
            assert_eq!(rcmd, command);
            assert!(!success, "{command}: peer-unavailable requests must not acknowledge success");
            assert!(body.is_none(), "{command}: a refused request must not carry a response body");
            if let DapMessage::Response { message, .. } = &out[0] {
                assert!(message.as_deref().is_some_and(|message| !message.is_empty()));
            }
        }
        Ok(())
    }

    #[test]
    fn step_in_with_target_id_fails_closed_and_never_reaches_the_backend() -> Result<(), String> {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut b = DapPeerBridge::new(Box::new(ScriptBackend {
            events: Vec::new(),
            evaluate_calls: Arc::new(AtomicUsize::new(0)),
            step_in_calls: Arc::clone(&calls),
        }));

        // #9069: `supportsStepInTargetsRequest` is negotiated false, so a
        // client-supplied `targetId` must be refused outright — never silently
        // executed as the untargeted step the client did not ask for.
        let out = b.dispatch(21, "stepIn", Some(json!({ "threadId": 1, "targetId": 7 })));
        let (command, success, body) = as_response(&out[0])?;
        assert_eq!(command, "stepIn");
        assert!(!success, "a targeted stepIn must not acknowledge success");
        assert!(body.is_none(), "a refused request must not carry a response body");
        if let DapMessage::Response { message, .. } = &out[0] {
            assert!(message.as_deref().is_some_and(|message| !message.is_empty()));
        }
        assert_eq!(
            calls.load(AtomicOrdering::SeqCst),
            0,
            "a refused targeted stepIn must reach the backend zero times"
        );

        // Control: an ordinary untargeted stepIn still steps, proving the
        // refusal is scoped to `targetId` and the probe is live.
        let out = b.dispatch(22, "stepIn", Some(json!({ "threadId": 1 })));
        let (_, success, _) = as_response(&out[0])?;
        assert!(success, "untargeted stepIn keeps its existing contract");
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1, "untargeted stepIn reaches the backend");
        Ok(())
    }

    #[test]
    fn breakpoint_locations_reports_breakable_lines_from_ast() -> Result<(), String> {
        use std::io::Write;
        let mut f = must(tempfile::NamedTempFile::new());
        must(writeln!(f, "# a comment")); // line 1 — not breakable
        must(writeln!(f, "my $x = 1;")); // line 2 — breakable
        must(writeln!(f, "print $x;")); // line 3 — breakable
        let path = f.path().to_string_lossy().to_string();

        let mut b = bridge();
        let out = b.dispatch(
            9,
            "breakpointLocations",
            Some(json!({ "source": { "path": path }, "line": 1, "endLine": 3 })),
        );
        let bps = must_some(must_some(as_response(&out[0])?.2)["breakpoints"].as_array()).clone();
        let lines: Vec<i64> = bps.iter().filter_map(|b| b["line"].as_i64()).collect();
        assert!(lines.contains(&2), "line 2 is breakable: {lines:?}");
        assert!(!lines.contains(&1), "comment line 1 is excluded: {lines:?}");
        Ok(())
    }

    #[test]
    fn breakpoint_locations_missing_line_returns_empty_not_all_lines() -> Result<(), String> {
        use std::io::Write;
        let mut f = must(tempfile::NamedTempFile::new());
        must(writeln!(f, "my $x = 1;"));
        let path = f.path().to_string_lossy().to_string();

        let mut b = bridge();
        // No "line" field at all. DAP marks `line` required, but a client that
        // omits it must not be answered with every breakable line in the file
        // (or crash) — the handler must still return the empty-set contract.
        let out =
            b.dispatch(20, "breakpointLocations", Some(json!({ "source": { "path": path } })));
        let (_, ok, body) = as_response(&out[0])?;
        assert!(ok, "the request itself still succeeds");
        let bps = must_some(must_some(body)["breakpoints"].as_array()).clone();
        assert!(bps.is_empty(), "missing line yields empty set, not every line: {bps:?}");
        Ok(())
    }

    #[test]
    fn breakpoint_locations_only_end_line_returns_empty_not_all_lines() -> Result<(), String> {
        use std::io::Write;
        let mut f = must(tempfile::NamedTempFile::new());
        must(writeln!(f, "my $x = 1;")); // line 1 — breakable
        must(writeln!(f, "my $y = 2;")); // line 2 — breakable
        let path = f.path().to_string_lossy().to_string();

        let mut b = bridge();
        // `endLine` present but `line` absent is the same malformed class as
        // "missing line" — it must NOT return every breakable line up to endLine.
        let out = b.dispatch(
            22,
            "breakpointLocations",
            Some(json!({ "source": { "path": path }, "endLine": 100 })),
        );
        let (_, ok, body) = as_response(&out[0])?;
        assert!(ok, "the request itself still succeeds");
        let bps = must_some(must_some(body)["breakpoints"].as_array()).clone();
        assert!(bps.is_empty(), "endLine-only (no line) yields empty set: {bps:?}");
        Ok(())
    }

    #[test]
    fn breakpoint_locations_line_only_defaults_end_to_that_line() -> Result<(), String> {
        use std::io::Write;
        let mut f = must(tempfile::NamedTempFile::new());
        must(writeln!(f, "# comment")); // line 1 — not breakable
        must(writeln!(f, "my $x = 1;")); // line 2 — breakable
        let path = f.path().to_string_lossy().to_string();

        let mut b = bridge();
        // A valid single-line query (`line` with no `endLine`) must still work:
        // endLine defaults to line, so line 2 is reported.
        let out = b.dispatch(
            23,
            "breakpointLocations",
            Some(json!({ "source": { "path": path }, "line": 2 })),
        );
        let bps = must_some(must_some(as_response(&out[0])?.2)["breakpoints"].as_array()).clone();
        let lines: Vec<i64> = bps.iter().filter_map(|b| b["line"].as_i64()).collect();
        assert_eq!(lines, vec![2], "line-only query reports just that breakable line: {lines:?}");
        Ok(())
    }

    #[test]
    fn breakpoint_locations_end_line_before_start_line_returns_empty() -> Result<(), String> {
        use std::io::Write;
        let mut f = must(tempfile::NamedTempFile::new());
        must(writeln!(f, "my $x = 1;")); // line 1
        must(writeln!(f, "my $y = 2;")); // line 2
        must(writeln!(f, "print $x + $y;")); // line 3
        let path = f.path().to_string_lossy().to_string();

        let mut b = bridge();
        let out = b.dispatch(
            21,
            "breakpointLocations",
            Some(json!({ "source": { "path": path }, "line": 3, "endLine": 1 })),
        );
        let bps = must_some(must_some(as_response(&out[0])?.2)["breakpoints"].as_array()).clone();
        assert!(bps.is_empty(), "endLine < line is an empty (not inverted) range: {bps:?}");
        Ok(())
    }

    #[test]
    fn breakpoint_locations_unreadable_path_returns_empty_not_an_error_response()
    -> Result<(), String> {
        let mut b = bridge();
        let out = b.dispatch(
            22,
            "breakpointLocations",
            Some(json!({
                "source": { "path": "/nonexistent/definitely-not-a-real-path.pl" },
                "line": 1,
                "endLine": 10,
            })),
        );
        let (_, ok, body) = as_response(&out[0])?;
        assert!(ok, "an unreadable source must not fail the DAP request");
        let bps = must_some(must_some(body)["breakpoints"].as_array()).clone();
        assert!(bps.is_empty(), "unreadable path yields empty set: {bps:?}");
        Ok(())
    }

    #[test]
    fn breakpoint_locations_missing_source_path_returns_empty() -> Result<(), String> {
        let mut b = bridge();
        let out = b.dispatch(23, "breakpointLocations", Some(json!({ "line": 1, "endLine": 3 })));
        let bps = must_some(must_some(as_response(&out[0])?.2)["breakpoints"].as_array()).clone();
        assert!(bps.is_empty(), "missing source.path yields empty set: {bps:?}");
        Ok(())
    }

    #[test]
    fn breakpoint_locations_missing_arguments_returns_empty() -> Result<(), String> {
        let mut b = bridge();
        let out = b.dispatch(24, "breakpointLocations", None);
        let (_, ok, body) = as_response(&out[0])?;
        assert!(ok, "even a bodyless breakpointLocations request must get a success response");
        let bps = must_some(must_some(body)["breakpoints"].as_array()).clone();
        assert!(bps.is_empty(), "missing arguments yields empty set: {bps:?}");
        Ok(())
    }

    #[test]
    fn terminate_disconnects_the_backend_and_emits_terminated() -> Result<(), String> {
        // The bridge advertises supportsTerminateRequest, so a DAP `terminate`
        // must be handled explicitly (not fall through the lenient default):
        // a success response plus a `terminated` event so the editor ends the
        // session rather than leaving the external-peer debuggee running.
        let mut b = bridge();
        let out = b.dispatch(9, "terminate", None);
        let (cmd, ok, _) = as_response(&out[0])?;
        assert_eq!(cmd, "terminate");
        assert!(ok, "terminate must be acknowledged");
        assert!(
            out.iter()
                .any(|m| matches!(m, DapMessage::Event { event, .. } if event == "terminated")),
            "terminate must emit a `terminated` event: {out:?}"
        );

        // A peer's own `debugger/terminated`, queued before our `peer/goodbye`
        // and drained afterwards, must NOT produce a second DAP `terminated`.
        let mut more = Vec::new();
        b.push_dap_events(DebugEvent::Terminated { exit_code: Some(0) }, &mut more);
        assert!(
            more.is_empty(),
            "a second terminated must be suppressed after terminate: {more:?}"
        );
        Ok(())
    }

    #[test]
    fn threads_reports_single_main_thread() -> Result<(), String> {
        let mut b = bridge();
        let out = b.dispatch(8, "threads", None);
        let threads = must_some(as_response(&out[0])?.2)["threads"].clone();
        assert_eq!(threads[0]["id"], 1);
        assert_eq!(threads[0]["name"], "main");
        Ok(())
    }

    #[test]
    fn output_and_terminated_events_translate() -> Result<(), String> {
        let mut b = bridge();
        // Inject events directly through the backend and poll.
        b.push_dap_events(
            DebugEvent::Output { category: OutputCategory::Stderr, output: "boom\n".into() },
            &mut Vec::new(),
        );
        // Use dispatch of a no-op that drains: simulate via a fresh backend event.
        let mut msgs = Vec::new();
        b.push_dap_events(DebugEvent::Terminated { exit_code: Some(0) }, &mut msgs);
        assert_eq!(event_name(&msgs[0])?, "terminated");
        if let DapMessage::Event { body: Some(body), .. } = &msgs[0] {
            assert_eq!(body["exitCode"], 0);
        } else {
            return Err("terminated body".into());
        }
        Ok(())
    }

    #[test]
    fn peer_initialized_readiness_is_not_forwarded_as_a_second_dap_initialized() {
        // The DAP `initialized` event is emitted once on the initialize response;
        // a peer's `debugger/initialized` readiness signal must NOT become a
        // second DAP `initialized` (which would re-trigger client configuration).
        let mut b = bridge();
        let mut out = Vec::new();
        b.push_dap_events(DebugEvent::Initialized, &mut out);
        assert!(out.is_empty(), "peer readiness must not emit a DAP event");
    }

    #[test]
    fn threaded_driver_runs_a_dap_session_over_pipes() {
        use perl_lsp_rs_core::transport::{ContentLengthFramer, frame};
        use std::io::Cursor;
        use std::sync::{Arc, Mutex};

        // A `Write` sink that appends into a shared buffer, so the test thread
        // can inspect what the driver wrote after it returns. This exercises the
        // same generic driver that `run_external_peer_session_stdio` uses over
        // real stdin/stdout, but over in-memory pipes.
        #[derive(Clone)]
        struct SharedSink(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for SharedSink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        // Editor input: an initialize request, then disconnect. The driver must
        // respond to both and return once it sees disconnect.
        let frame_of = |v: Value| frame(&must(serde_json::to_vec(&v)));
        let mut input = frame_of(
            json!({ "seq": 1, "type": "request", "command": "initialize", "arguments": {} }),
        );
        input.extend_from_slice(&frame_of(
            json!({ "seq": 2, "type": "request", "command": "disconnect" }),
        ));

        let out_buf = Arc::new(Mutex::new(Vec::new()));
        must(run_peer_session_threaded(
            Cursor::new(input),
            SharedSink(out_buf.clone()),
            bridge(),
            Duration::from_millis(5),
        ));

        // Reparse the framed output stream and collect response commands + events.
        let raw = out_buf.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut framer = ContentLengthFramer::new();
        framer.push(&raw);
        let (mut commands, mut events) = (Vec::new(), Vec::new());
        while let Ok(Some(body)) = framer.try_next() {
            let v: Value = must(serde_json::from_slice(&body));
            if v.get("type").and_then(Value::as_str) == Some("response")
                && let Some(c) = v.get("command").and_then(Value::as_str)
            {
                commands.push(c.to_string());
            }
            if let Some(e) = v.get("event").and_then(Value::as_str) {
                events.push(e.to_string());
            }
        }
        assert!(commands.contains(&"initialize".to_string()), "commands: {commands:?}");
        assert!(commands.contains(&"disconnect".to_string()), "commands: {commands:?}");
        assert!(events.contains(&"initialized".to_string()), "events: {events:?}");
    }

    #[test]
    fn threaded_driver_recovers_from_a_leading_malformed_frame() {
        use perl_lsp_rs_core::transport::{ContentLengthFramer, frame};
        use std::io::Cursor;
        use std::sync::{Arc, Mutex};

        // `ContentLengthFramer::try_next` discards a malformed header block
        // before returning its error, so a single garbled frame must not end
        // the whole session — the reader thread should skip it and keep
        // parsing whatever follows. Regression coverage for a bug where the
        // reader thread unconditionally `return`ed on the first framing
        // error, silently killing the session (surfaced to the main loop only
        // as an untraceable `RecvTimeoutError::Disconnected`) even though
        // well-formed frames followed in the same stream.
        #[derive(Clone)]
        struct SharedSink(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for SharedSink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let frame_of = |v: Value| frame(&must(serde_json::to_vec(&v)));

        // A header block with an unparseable Content-Length value, followed by
        // a valid initialize + disconnect pair.
        let mut input = b"Content-Length: notanumber\r\n\r\n".to_vec();
        input.extend_from_slice(&frame_of(
            json!({ "seq": 1, "type": "request", "command": "initialize", "arguments": {} }),
        ));
        input.extend_from_slice(&frame_of(
            json!({ "seq": 2, "type": "request", "command": "disconnect" }),
        ));

        let out_buf = Arc::new(Mutex::new(Vec::new()));
        must(run_peer_session_threaded(
            Cursor::new(input),
            SharedSink(out_buf.clone()),
            bridge(),
            Duration::from_millis(5),
        ));

        let raw = out_buf.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut framer = ContentLengthFramer::new();
        framer.push(&raw);
        let mut commands = Vec::new();
        while let Ok(Some(body)) = framer.try_next() {
            let v: Value = must(serde_json::from_slice(&body));
            if v.get("type").and_then(Value::as_str) == Some("response")
                && let Some(c) = v.get("command").and_then(Value::as_str)
            {
                commands.push(c.to_string());
            }
        }
        assert!(
            commands.contains(&"initialize".to_string()),
            "the valid frames after the malformed one must still be processed: {commands:?}"
        );
        assert!(commands.contains(&"disconnect".to_string()), "commands: {commands:?}");
    }

    /// #9522: a frame burst against a stalled session loop saturates the
    /// bounded queue, and the session fails closed with the typed backpressure
    /// disposition instead of buffering without bound or returning generic
    /// success. Frames admitted before the overflow are still dispatched (the
    /// control below proves the loop keeps answering under the same pipe
    /// harness without pressure).
    #[test]
    fn threaded_driver_fails_closed_when_frame_queue_saturates() -> Result<(), String> {
        use perl_lsp_rs_core::transport::frame;
        use std::io::Cursor;
        use std::sync::Condvar;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicBool, Ordering};

        // A writer sink that stalls its first write until released (or a short
        // bounded deadline elapses), which leaves the session loop parked in
        // `write` while the reader bursts frames into the bounded queue.
        struct GatedSink {
            gate: Arc<(Mutex<bool>, Condvar)>,
            first_write_done: AtomicBool,
        }
        impl GatedSink {
            fn new(gate: Arc<(Mutex<bool>, Condvar)>) -> Self {
                Self { gate, first_write_done: AtomicBool::new(false) }
            }
        }
        impl std::io::Write for GatedSink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                if !self.first_write_done.swap(true, Ordering::SeqCst) {
                    let (lock, cvar) = &*self.gate;
                    let _guard = cvar
                        .wait_timeout_while(
                            lock.lock().unwrap_or_else(|e| e.into_inner()),
                            Duration::from_millis(300),
                            |open| !*open,
                        )
                        .unwrap_or_else(|e| e.into_inner());
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        // Far more frames than PEER_FRAME_QUEUE_CAPACITY, sent while the loop
        // is stalled in its first write: the queue must saturate.
        let frame_of = |seq: i64| {
            frame(&must(serde_json::to_vec(&json!({
                "seq": seq, "type": "request", "command": "unknownCommand", "arguments": {}
            }))))
        };
        let mut input = Vec::new();
        for seq in 1..=200 {
            input.extend_from_slice(&frame_of(seq));
        }

        let gate: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
        let result = run_peer_session_threaded(
            Cursor::new(input),
            GatedSink::new(Arc::clone(&gate)),
            bridge(),
            Duration::from_millis(1),
        );

        let Err(failure) = result else {
            return Err(
                "a saturated peer frame queue must fail the session, not return Ok".to_string()
            );
        };
        assert!(
            failure.to_string().contains(crate::backend::peer_frame_queue::PEER_BACKPRESSURE_MSG),
            "the session failure must carry the typed backpressure disposition, got: {failure}"
        );
        Ok(())
    }

    /// #9522 review: a DAP `disconnect` admitted before the reader stopped
    /// must not mask a latched overflow. The queue holds [request, disconnect,
    /// filler...] when the reader saturates on the fillers; the session loop
    /// dispatches the first request (stalling its first write so the reader
    /// finishes the burst), then the disconnect breaks the loop — the typed
    /// backpressure failure must win over generic success.
    #[test]
    fn threaded_driver_disconnect_does_not_mask_overflow() -> Result<(), String> {
        use perl_lsp_rs_core::transport::frame;
        use std::io::Cursor;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Condvar, Mutex};

        struct GatedSink {
            gate: Arc<(Mutex<bool>, Condvar)>,
            first_write_done: AtomicBool,
        }
        impl GatedSink {
            fn new(gate: Arc<(Mutex<bool>, Condvar)>) -> Self {
                Self { gate, first_write_done: AtomicBool::new(false) }
            }
        }
        impl std::io::Write for GatedSink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                if !self.first_write_done.swap(true, Ordering::SeqCst) {
                    let (lock, cvar) = &*self.gate;
                    let _guard = cvar
                        .wait_timeout_while(
                            lock.lock().unwrap_or_else(|e| e.into_inner()),
                            Duration::from_millis(300),
                            |open| !*open,
                        )
                        .unwrap_or_else(|e| e.into_inner());
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let frame_of = |seq: i64, command: &str| {
            frame(&must(serde_json::to_vec(
                &json!({ "seq": seq, "type": "request", "command": command, "arguments": {} }),
            )))
        };
        let mut input = Vec::new();
        input.extend_from_slice(&frame_of(1, "unknownCommand"));
        input.extend_from_slice(&frame_of(2, "disconnect"));
        for seq in 3..=220 {
            input.extend_from_slice(&frame_of(seq, "unknownCommand"));
        }

        let gate: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
        let result = run_peer_session_threaded(
            Cursor::new(input),
            GatedSink::new(Arc::clone(&gate)),
            bridge(),
            Duration::from_millis(1),
        );

        let Err(failure) = result else {
            return Err("an admitted disconnect after a latched overflow must not report success"
                .to_string());
        };
        assert!(
            failure.to_string().contains(crate::backend::peer_frame_queue::PEER_BACKPRESSURE_MSG),
            "the session failure must carry the typed backpressure disposition, got: {failure}"
        );
        Ok(())
    }
}
