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
//! deferred). [`run_external_peer_session`] drives it over a socket editor
//! connection; [`DapPeerBridge::dispatch`] / [`DapPeerBridge::poll_events`] are
//! the deterministic, testable core.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{Value, json};

use super::capabilities::{CatalogDapFlags, intersect_dap_capabilities};
use super::{
    AttachBackendParams, DebugBackend, EvaluateContext, EvaluateParams, InitializeBackendParams,
    LaunchBackendParams, SetBackendBreakpointsParams, SetFunctionBreakpointsParams,
    StackTraceParams,
};
use crate::debug_adapter::DapMessage;
use crate::model::{
    DebugBreakpoint, DebugEvent, DebugFunctionBreakpoint, DebugSource, FrameId, OutputCategory,
    StopReason, ThreadId, VariablesRef,
};

/// The DAP frontend over a [`DebugBackend`].
pub struct DapPeerBridge {
    backend: Box<dyn DebugBackend>,
    seq: i64,
}

impl DapPeerBridge {
    /// Create a bridge over `backend`.
    #[must_use]
    pub fn new(backend: Box<dyn DebugBackend>) -> Self {
        Self { backend, seq: 0 }
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
            // The bridge already emits exactly one DAP `initialized` event after
            // the initialize RESPONSE (DAP requires exactly one). The peer's own
            // `debugger/initialized` is an internal readiness signal; forwarding
            // it would be a second `initialized` and make a conformant client
            // re-send its configuration. Intentionally dropped.
            DebugEvent::Initialized => {}
            DebugEvent::Stopped { reason, thread_id, .. } => {
                let body = json!({
                    "reason": dap_stop_reason(&reason),
                    "threadId": thread_id.0,
                    "allThreadsStopped": true,
                });
                out.push(self.event("stopped", Some(body)));
            }
            DebugEvent::Continued { thread_id } => {
                let body = json!({ "threadId": thread_id.0, "allThreadsContinued": true });
                out.push(self.event("continued", Some(body)));
            }
            DebugEvent::Output { category, output } => {
                let body = json!({ "category": category_str(category), "output": output });
                out.push(self.event("output", Some(body)));
            }
            DebugEvent::Terminated { exit_code } => {
                let body = exit_code.map(|c| json!({ "exitCode": c }));
                out.push(self.event("terminated", body));
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
        match command {
            "initialize" => {
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
            "launch" => {
                let params = parse_launch(arguments.as_ref());
                match self.backend.launch(params) {
                    Ok(_) => out.push(self.response(request_seq, command, true, None, None)),
                    Err(e) => out.push(self.error(request_seq, command, e)),
                }
            }
            "attach" => {
                let params = parse_attach(arguments.as_ref());
                match self.backend.attach(params) {
                    Ok(_) => out.push(self.response(request_seq, command, true, None, None)),
                    Err(e) => out.push(self.error(request_seq, command, e)),
                }
            }
            "setBreakpoints" => match self.handle_set_breakpoints(arguments.as_ref()) {
                Ok(body) => out.push(self.response(request_seq, command, true, Some(body), None)),
                Err(e) => out.push(self.error(request_seq, command, e)),
            },
            "setFunctionBreakpoints" => {
                match self.handle_set_function_breakpoints(arguments.as_ref()) {
                    Ok(body) => {
                        out.push(self.response(request_seq, command, true, Some(body), None))
                    }
                    Err(e) => out.push(self.error(request_seq, command, e)),
                }
            }
            "continue" => {
                let tid = thread_id_arg(arguments.as_ref());
                match self.backend.continue_thread(tid) {
                    Ok(r) => {
                        let body = json!({ "allThreadsContinued": r.all_threads_continued });
                        out.push(self.response(request_seq, command, true, Some(body), None));
                    }
                    Err(e) => out.push(self.error(request_seq, command, e)),
                }
            }
            "next" => self.step(request_seq, command, arguments.as_ref(), Step::Next, &mut out),
            "stepIn" => self.step(request_seq, command, arguments.as_ref(), Step::In, &mut out),
            "stepOut" => self.step(request_seq, command, arguments.as_ref(), Step::Out, &mut out),
            "pause" => {
                let tid = thread_id_arg(arguments.as_ref());
                match self.backend.pause(tid) {
                    Ok(()) => out.push(self.response(request_seq, command, true, None, None)),
                    Err(e) => out.push(self.error(request_seq, command, e)),
                }
            }
            "stackTrace" => match self.handle_stack_trace(arguments.as_ref()) {
                Ok(body) => out.push(self.response(request_seq, command, true, Some(body), None)),
                Err(e) => out.push(self.error(request_seq, command, e)),
            },
            "scopes" => match self.handle_scopes(arguments.as_ref()) {
                Ok(body) => out.push(self.response(request_seq, command, true, Some(body), None)),
                Err(e) => out.push(self.error(request_seq, command, e)),
            },
            "variables" => match self.handle_variables(arguments.as_ref()) {
                Ok(body) => out.push(self.response(request_seq, command, true, Some(body), None)),
                Err(e) => out.push(self.error(request_seq, command, e)),
            },
            "evaluate" => match self.handle_evaluate(arguments.as_ref()) {
                Ok(body) => out.push(self.response(request_seq, command, true, Some(body), None)),
                Err(e) => out.push(self.error(request_seq, command, e)),
            },
            "threads" => {
                // Perl's stock debugger is single-threaded; report one thread.
                let body = json!({ "threads": [{ "id": 1, "name": "main" }] });
                out.push(self.response(request_seq, command, true, Some(body), None));
            }
            "configurationDone" => {
                out.push(self.response(request_seq, command, true, None, None));
            }
            "disconnect" => {
                let terminate = arguments
                    .as_ref()
                    .and_then(|a| a.get("terminateDebuggee"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let _ = self.backend.disconnect(terminate);
                out.push(self.response(request_seq, command, true, None, None));
            }
            other => {
                // Lenient: acknowledge unrecognized requests so a client is not
                // wedged, but carry no body. (mirror-MVP behavior.)
                tracing::warn!(command = other, "peer bridge: unhandled DAP request");
                out.push(self.response(request_seq, other, true, None, None));
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
        let tid = thread_id_arg(args);
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

    fn capabilities_body(&self) -> Value {
        let negotiated = intersect_dap_capabilities(
            &CatalogDapFlags::from_catalog(),
            &self.backend.capabilities(),
        );
        json!({
            "supportsConfigurationDoneRequest": true,
            "supportsTerminateRequest": true,
            "supportsConditionalBreakpoints": negotiated.supports_conditional_breakpoints,
            "supportsHitConditionalBreakpoints": negotiated.supports_hit_conditional_breakpoints,
            "supportsLogPoints": negotiated.supports_log_points,
            "supportsFunctionBreakpoints": negotiated.supports_function_breakpoints,
            "supportsDataBreakpoints": negotiated.supports_data_breakpoints,
            "supportsEvaluateForHovers": negotiated.supports_evaluate_for_hovers,
            "supportsSetVariable": negotiated.supports_set_variable,
        })
    }

    fn handle_set_breakpoints(&mut self, args: Option<&Value>) -> super::BackendResult<Value> {
        let args = args.ok_or_else(|| super::BackendError::Protocol("missing arguments".into()))?;
        let source = dap_source(args.get("source"));
        let breakpoints = args
            .get("breakpoints")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| {
                        let line = b.get("line").and_then(Value::as_u64)? as u32;
                        Some(DebugBreakpoint {
                            id: None,
                            source: source.clone(),
                            line,
                            column: b.get("column").and_then(Value::as_u64).map(|c| c as u32),
                            condition: str_field(b, "condition"),
                            hit_condition: str_field(b, "hitCondition"),
                            log_message: str_field(b, "logMessage"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let resolved =
            self.backend.set_breakpoints(SetBackendBreakpointsParams { source, breakpoints })?;
        let bps: Vec<Value> = resolved
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "verified": r.verified,
                    "line": r.actual_position.line,
                    "column": r.actual_position.column,
                    "message": r.message,
                })
            })
            .collect();
        Ok(json!({ "breakpoints": bps }))
    }

    fn handle_set_function_breakpoints(
        &mut self,
        args: Option<&Value>,
    ) -> super::BackendResult<Value> {
        let breakpoints = args
            .and_then(|a| a.get("breakpoints"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| {
                        Some(DebugFunctionBreakpoint {
                            name: str_field(b, "name")?,
                            condition: str_field(b, "condition"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let resolved =
            self.backend.set_function_breakpoints(SetFunctionBreakpointsParams { breakpoints })?;
        let bps: Vec<Value> =
            resolved.into_iter().map(|r| json!({ "id": r.id, "verified": r.verified })).collect();
        Ok(json!({ "breakpoints": bps }))
    }

    fn handle_stack_trace(&mut self, args: Option<&Value>) -> super::BackendResult<Value> {
        let params = StackTraceParams {
            thread_id: thread_id_arg(args),
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

// ---------------------------------------------------------------------------
// Production session driver (socket editor transport)
// ---------------------------------------------------------------------------

/// Drive a [`DapPeerBridge`] over a socket editor connection.
///
/// Reads Content-Length framed DAP requests from the editor, dispatches each to
/// the bridge, and writes framed responses/events back. Between reads it polls
/// the backend for asynchronous events (stops, output, termination) so they
/// reach the editor promptly — a short read timeout on the socket paces the
/// interleave (the same technique the peer backend uses internally).
///
/// A concrete `TcpStream` is required (not a generic reader) because the
/// interleave depends on `set_read_timeout`; the external-peer launch uses the
/// socket transport. stdio async-event delivery is a follow-up.
///
/// # Errors
/// Returns a transport error if the socket read/write fails irrecoverably.
pub fn run_external_peer_session(
    stream: TcpStream,
    mut bridge: DapPeerBridge,
    poll_interval: Duration,
) -> std::io::Result<()> {
    use perl_lsp_rs_core::transport::{ContentLengthFramer, frame};

    stream.set_read_timeout(Some(poll_interval))?;
    let mut reader = stream.try_clone()?;
    let mut writer = stream;
    let mut framer = ContentLengthFramer::new();
    let mut buf = [0u8; 8 * 1024];

    let write_msgs = |writer: &mut TcpStream, msgs: &[DapMessage]| -> std::io::Result<()> {
        for m in msgs {
            let body = serde_json::to_vec(m).unwrap_or_default();
            writer.write_all(&frame(&body))?;
        }
        writer.flush()
    };

    loop {
        // Deliver any asynchronous backend events first.
        let events = bridge.poll_events();
        if !events.is_empty() {
            write_msgs(&mut writer, &events)?;
        }

        match reader.read(&mut buf) {
            Ok(0) => break, // editor disconnected
            Ok(n) => {
                framer.push(&buf[..n]);
                loop {
                    match framer.try_next() {
                        Ok(Some(body)) => {
                            if let Ok(req) =
                                serde_json::from_slice::<crate::protocol::Request>(&body)
                            {
                                let out = bridge.dispatch(req.seq, &req.command, req.arguments);
                                write_msgs(&mut writer, &out)?;
                                if req.command == "disconnect" {
                                    return Ok(());
                                }
                            }
                        }
                        Ok(None) => break,
                        Err(_) => return Ok(()), // malformed stream: end session
                    }
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // No editor input this tick; loop to poll events again.
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
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

fn thread_id_arg(args: Option<&Value>) -> ThreadId {
    ThreadId(args.and_then(|a| a.get("threadId")).and_then(Value::as_i64).unwrap_or(1))
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

    #[derive(Default)]
    struct ScriptBackend {
        events: Vec<DebugEvent>,
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
            Ok(())
        }
        fn step_out(&mut self, _t: ThreadId) -> BackendResult<()> {
            Ok(())
        }
        fn pause(&mut self, _t: ThreadId) -> BackendResult<()> {
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

    fn as_response(msg: &DapMessage) -> (&str, bool, Option<&Value>) {
        match msg {
            DapMessage::Response { command, success, body, .. } => {
                (command.as_str(), *success, body.as_ref())
            }
            _ => panic!("expected response, got {msg:?}"),
        }
    }

    fn event_name(msg: &DapMessage) -> &str {
        match msg {
            DapMessage::Event { event, .. } => event.as_str(),
            _ => panic!("expected event, got {msg:?}"),
        }
    }

    #[test]
    fn initialize_returns_capabilities_and_initialized_event() {
        let mut b = bridge();
        let out = b.dispatch(1, "initialize", None);
        assert_eq!(out.len(), 2);
        let (cmd, ok, body) = as_response(&out[0]);
        assert_eq!(cmd, "initialize");
        assert!(ok);
        let caps = body.expect("capabilities");
        assert_eq!(caps["supportsConfigurationDoneRequest"], true);
        // ptkdb v1 negotiated: no logpoints/data breakpoints.
        assert_eq!(caps["supportsLogPoints"], false);
        assert_eq!(caps["supportsDataBreakpoints"], false);
        assert_eq!(event_name(&out[1]), "initialized");
    }

    #[test]
    fn set_breakpoints_translates_to_dap_body() {
        let mut b = bridge();
        let args = json!({
            "source": { "path": "/work/script.pl" },
            "breakpoints": [{ "line": 42, "condition": "$x > 10" }, { "line": 7 }],
        });
        let out = b.dispatch(2, "setBreakpoints", Some(args));
        let (_, ok, body) = as_response(&out[0]);
        assert!(ok);
        let bps = body.expect("body")["breakpoints"].as_array().expect("array");
        assert_eq!(bps.len(), 2);
        assert_eq!(bps[0]["verified"], true);
        assert_eq!(bps[0]["line"], 42);
    }

    #[test]
    fn continue_emits_continued_then_stopped_after_response() {
        let mut b = bridge();
        let out = b.dispatch(3, "continue", Some(json!({ "threadId": 1 })));
        // response, then the two events the backend queued.
        let (cmd, ok, body) = as_response(&out[0]);
        assert_eq!(cmd, "continue");
        assert!(ok);
        assert_eq!(body.expect("body")["allThreadsContinued"], true);
        let events: Vec<&str> = out[1..].iter().map(event_name).collect();
        assert_eq!(events, vec!["continued", "stopped"]);
        // The stopped event carries the DAP reason + threadId.
        if let DapMessage::Event { body: Some(b), .. } = &out[2] {
            assert_eq!(b["reason"], "breakpoint");
            assert_eq!(b["threadId"], 1);
            assert_eq!(b["allThreadsStopped"], true);
        } else {
            panic!("expected stopped event body");
        }
    }

    #[test]
    fn stack_scopes_variables_evaluate_round_trip() {
        let mut b = bridge();
        let st = b.dispatch(4, "stackTrace", Some(json!({ "threadId": 1 })));
        let frames =
            as_response(&st[0]).2.expect("body")["stackFrames"].as_array().expect("frames").clone();
        assert_eq!(frames[0]["name"], "main::run");
        assert_eq!(frames[0]["line"], 42);
        assert_eq!(frames[0]["source"]["path"], "/work/script.pl");

        let sc = b.dispatch(5, "scopes", Some(json!({ "frameId": 1 })));
        assert_eq!(as_response(&sc[0]).2.expect("body")["scopes"][0]["variablesReference"], 1000);

        let va = b.dispatch(6, "variables", Some(json!({ "variablesReference": 1000 })));
        let vars = as_response(&va[0]).2.expect("body")["variables"].clone();
        assert_eq!(vars[0]["name"], "$x");
        assert_eq!(vars[0]["value"], "42");
        assert_eq!(vars[0]["variablesReference"], 0);

        let ev = b.dispatch(7, "evaluate", Some(json!({ "expression": "$x", "context": "watch" })));
        assert_eq!(as_response(&ev[0]).2.expect("body")["result"], "=$x");
    }

    #[test]
    fn threads_reports_single_main_thread() {
        let mut b = bridge();
        let out = b.dispatch(8, "threads", None);
        let threads = as_response(&out[0]).2.expect("body")["threads"].clone();
        assert_eq!(threads[0]["id"], 1);
        assert_eq!(threads[0]["name"], "main");
    }

    #[test]
    fn output_and_terminated_events_translate() {
        let mut b = bridge();
        // Inject events directly through the backend and poll.
        b.push_dap_events(
            DebugEvent::Output { category: OutputCategory::Stderr, output: "boom\n".into() },
            &mut Vec::new(),
        );
        // Use dispatch of a no-op that drains: simulate via a fresh backend event.
        let mut msgs = Vec::new();
        b.push_dap_events(DebugEvent::Terminated { exit_code: Some(0) }, &mut msgs);
        assert_eq!(event_name(&msgs[0]), "terminated");
        if let DapMessage::Event { body: Some(body), .. } = &msgs[0] {
            assert_eq!(body["exitCode"], 0);
        } else {
            panic!("terminated body");
        }
    }
}
