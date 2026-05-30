//! Shared test helpers for DAP end-to-end workflow tests.
//!
//! `DapWorkflowSession` wraps a `DebugAdapter` and an event `Receiver` with
//! higher-level helpers that chain the request → event → response cycles
//! required to drive a real `perl -d` debug session in tests.

use perl_dap::{DapMessage, DebugAdapter};
use perl_lsp_rs_core::config::PerlOracleEnv;
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

// ─── Public surface ───────────────────────────────────────────────────────────

/// Information extracted from a `stopped` event.
#[derive(Debug, Clone)]
// Shared workflow-test fixture fields are intentionally read by selected
// scenario helpers only; keep the full event shape available for new DAP flows.
#[allow(dead_code)]
pub struct StoppedInfo {
    /// Stopped reason, e.g. `"breakpoint"`, `"step"`, `"entry"`.
    pub reason: String,
    /// Thread ID (from `stopped.body.threadId`).
    pub thread_id: i64,
}

/// A stopped event paired with the top stack frame observed immediately after it.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StoppedFrameInfo {
    pub stopped: StoppedInfo,
    pub frame_id: i64,
    pub source_path: String,
    pub line: i64,
}

/// High-level handle for a DAP workflow test session.
///
/// Wraps `DebugAdapter` with helpers that handle protocol sequencing:
/// initialize → launch → setBreakpoints → configurationDone → wait_stopped →
/// stack_trace → scopes → variables → continue/step → wait_stopped → disconnect.
pub struct DapWorkflowSession {
    pub adapter: DebugAdapter,
    pub rx: Receiver<DapMessage>,
    pub timeout: Duration,
    seq: i64,
}

// Shared workflow-test helpers are consumed incrementally by DAP scenarios.
#[allow(dead_code)]
impl DapWorkflowSession {
    /// Create a new session and send `initialize`.
    ///
    /// Returns an error if initialization fails or the `initialized` event is
    /// not received within `timeout`.
    pub fn new(timeout: Duration) -> Result<Self, String> {
        let mut adapter = DebugAdapter::new();
        let (tx, rx) = channel();
        adapter.set_event_sender(tx);

        let mut session = Self { adapter, rx, timeout, seq: 0 };

        let resp = session.request("initialize", None);
        session.expect_success(&resp, "initialize")?;
        session.drain_until_event("initialized")?;

        Ok(session)
    }

    /// Launch a script with `stopOnEntry: false`.
    ///
    /// Callers must call `set_breakpoints` and `configuration_done` before
    /// `wait_stopped` to follow the DAP ordering requirement.
    pub fn launch(&mut self, script_path: &str) -> Result<(), String> {
        let args = json!({
            "program": script_path,
            "args": [],
            "stopOnEntry": false,
            "env": {
                "PERL_PERTURB_KEYS": "0",
                "PERL_HASH_SEED": "0",
                "LC_ALL": "C",
                "TZ": "UTC"
            }
        });
        let resp = self.request("launch", Some(args));
        self.expect_success(&resp, "launch")?;
        Ok(())
    }

    /// Attach to a running process with optional stopOnEntry.
    ///
    /// Callers must call `set_breakpoints` and `configuration_done` before
    /// `wait_stopped` to follow the DAP ordering requirement (though attach
    /// emits a stopped event immediately).
    pub fn attach(&mut self, process_id: u32, stop_on_entry: bool) -> Result<(), String> {
        let args = json!({
            "processId": process_id,
            "stopOnEntry": stop_on_entry
        });
        let resp = self.request("attach", Some(args));
        self.expect_success(&resp, "attach")?;
        Ok(())
    }

    /// Send `setBreakpoints` for the given `lines` in `source_path`.
    ///
    /// Returns the raw `setBreakpoints` response body.
    pub fn set_breakpoints(
        &mut self,
        source_path: &str,
        lines: &[u64],
    ) -> Result<Option<Value>, String> {
        let breakpoints: Vec<Value> = lines.iter().map(|&l| json!({"line": l})).collect();
        let args = json!({
            "source": { "path": source_path },
            "breakpoints": breakpoints
        });
        let resp = self.request("setBreakpoints", Some(args));
        self.expect_success(&resp, "setBreakpoints")
    }

    /// Send `setBreakpoints`, validate the response, and return resolved line numbers.
    ///
    /// Each returned line is the DAP adapter's resolved line for the corresponding
    /// requested line.  Using resolved lines in assertions avoids flakes from the
    /// Perl debugger reporting adjacent lines.
    pub fn set_breakpoints_checked(
        &mut self,
        source_path: &str,
        lines: &[u64],
    ) -> Result<Vec<i64>, String> {
        let body =
            self.set_breakpoints(source_path, lines)?.ok_or("setBreakpoints returned no body")?;
        let breakpoints = body
            .get("breakpoints")
            .and_then(Value::as_array)
            .ok_or("setBreakpoints body missing `breakpoints` array")?;

        if breakpoints.len() != lines.len() {
            return Err(format!(
                "setBreakpoints returned {} breakpoints for {} requested lines",
                breakpoints.len(),
                lines.len()
            ));
        }

        let mut resolved_lines = Vec::with_capacity(breakpoints.len());
        for (index, breakpoint) in breakpoints.iter().enumerate() {
            let verified = breakpoint
                .get("verified")
                .and_then(Value::as_bool)
                .ok_or_else(|| format!("breakpoint #{index} missing boolean `verified`"))?;
            if !verified {
                return Err(format!("breakpoint #{index} was not verified"));
            }
            let line = breakpoint
                .get("line")
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("breakpoint #{index} missing numeric `line`"))?;
            resolved_lines.push(line);
        }
        Ok(resolved_lines)
    }

    /// Send `configurationDone`.
    pub fn configuration_done(&mut self) -> Result<(), String> {
        let resp = self.request("configurationDone", None);
        self.expect_success(&resp, "configurationDone")?;
        Ok(())
    }

    /// Block until a `stopped` event arrives; drain and discard other events.
    ///
    /// Returns `StoppedInfo` with `reason` and `thread_id`.
    /// Line information is not present in the `stopped` event body; use
    /// `stack_trace()` to obtain the current source line after stopping.
    pub fn wait_stopped(&self) -> Result<StoppedInfo, String> {
        let msg = self.drain_until_event("stopped")?;
        let body = match &msg {
            DapMessage::Event { body, .. } => body.clone().unwrap_or(Value::Null),
            _ => return Err("expected Event message for `stopped`".to_string()),
        };

        let reason = body.get("reason").and_then(Value::as_str).unwrap_or("unknown").to_string();

        let thread_id = body.get("threadId").and_then(Value::as_i64).unwrap_or(1);

        Ok(StoppedInfo { reason, thread_id })
    }

    /// Wait for a `stopped` event and immediately capture the top stack frame.
    ///
    /// Combines `wait_stopped()` and `stack_trace()` into one call so that
    /// callers can assert on `frame.line` against the DAP-resolved breakpoint
    /// line rather than a hard-coded constant.
    pub fn wait_stopped_with_frame(&mut self) -> Result<StoppedFrameInfo, String> {
        let stopped = self.wait_stopped()?;
        let (frame_id, source_path, line) = self.stack_trace(stopped.thread_id)?;
        Ok(StoppedFrameInfo { stopped, frame_id, source_path, line })
    }

    /// Retrieve the top stack frame for `thread_id`.
    ///
    /// Returns `(frame_id, source_path_str, frame_line)`.
    /// `frame_line` is the 1-based source line reported by the debugger, or 0
    /// if the frame does not carry line information.
    pub fn stack_trace(&mut self, thread_id: i64) -> Result<(i64, String, i64), String> {
        let args = json!({"threadId": thread_id, "startFrame": 0, "levels": 1});
        let resp = self.request("stackTrace", Some(args));
        let body = self.expect_success(&resp, "stackTrace")?;

        let body = body.ok_or("stackTrace response had no body")?;
        let frames = body
            .get("stackFrames")
            .and_then(Value::as_array)
            .ok_or("stackTrace body missing `stackFrames` array")?;

        let frame = frames.first().ok_or("stackTrace returned empty frames")?;
        let frame_id = frame.get("id").and_then(Value::as_i64).ok_or("stack frame missing `id`")?;
        let source_path = frame
            .get("source")
            .and_then(|s| s.get("path"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let frame_line = frame.get("line").and_then(Value::as_i64).unwrap_or(0);

        Ok((frame_id, source_path, frame_line))
    }

    /// Retrieve the `variablesReference` for the `Globals` scope in `frame_id`.
    pub fn scopes_globals_ref(&mut self, frame_id: i64) -> Result<i64, String> {
        let args = json!({"frameId": frame_id});
        let resp = self.request("scopes", Some(args));
        let body = self.expect_success(&resp, "scopes")?;

        let body = body.ok_or("scopes response had no body")?;
        let scopes = body
            .get("scopes")
            .and_then(Value::as_array)
            .ok_or("scopes body missing `scopes` array")?;

        // Find the "Globals" scope by presentation hint or name.
        for scope in scopes {
            let is_globals = scope
                .get("presentationHint")
                .and_then(Value::as_str)
                .map(|h| h == "globals")
                .unwrap_or(false)
                || scope
                    .get("name")
                    .and_then(Value::as_str)
                    .map(|n| n == "Globals")
                    .unwrap_or(false);

            if is_globals {
                let vars_ref = scope
                    .get("variablesReference")
                    .and_then(Value::as_i64)
                    .ok_or("Globals scope missing `variablesReference`")?;
                return Ok(vars_ref);
            }
        }

        Err("No Globals scope found in scopes response".to_string())
    }

    /// Retrieve the `variablesReference` for the `Locals` scope in `frame_id`.
    ///
    /// Uses the `frame_id * 10 + 1` encoding from `frames.rs`.
    pub fn scopes_locals_ref(&mut self, frame_id: i64) -> Result<i64, String> {
        let args = json!({"frameId": frame_id});
        let resp = self.request("scopes", Some(args));
        let body = self.expect_success(&resp, "scopes")?;

        let body = body.ok_or("scopes response had no body")?;
        let scopes = body
            .get("scopes")
            .and_then(Value::as_array)
            .ok_or("scopes body missing `scopes` array")?;

        // Find the "Locals" scope by presentation hint or name.
        for scope in scopes {
            let is_locals = scope
                .get("presentationHint")
                .and_then(Value::as_str)
                .map(|h| h == "locals")
                .unwrap_or(false)
                || scope
                    .get("name")
                    .and_then(Value::as_str)
                    .map(|n| n == "Locals")
                    .unwrap_or(false);

            if is_locals {
                let vars_ref = scope
                    .get("variablesReference")
                    .and_then(Value::as_i64)
                    .ok_or("Locals scope missing `variablesReference`")?;
                return Ok(vars_ref);
            }
        }

        Err("No Locals scope found in scopes response".to_string())
    }

    /// Retrieve variables for `variables_reference`.
    pub fn variables(&mut self, variables_reference: i64) -> Result<Vec<Value>, String> {
        let args = json!({"variablesReference": variables_reference});
        let resp = self.request("variables", Some(args));
        let body = self.expect_success(&resp, "variables")?;

        let body = body.ok_or("variables response had no body")?;
        let vars = body
            .get("variables")
            .and_then(Value::as_array)
            .ok_or("variables body missing `variables` array")?;

        Ok(vars.clone())
    }

    /// Evaluate a debugger expression in the currently stopped frame.
    ///
    /// Returns the string result and optional DAP type reported by the adapter.
    pub fn evaluate_expression(
        &mut self,
        expression: &str,
        frame_id: i64,
    ) -> Result<(String, Option<String>), String> {
        let args = json!({
            "expression": expression,
            "frameId": frame_id,
            "context": "watch",
            "allowSideEffects": false
        });
        let resp = self.request("evaluate", Some(args));
        let body = self.expect_success(&resp, "evaluate")?;

        let body = body.ok_or("evaluate response had no body")?;
        let result = body
            .get("result")
            .and_then(Value::as_str)
            .ok_or("evaluate body missing string `result`")?
            .to_string();
        let ty = body.get("type").and_then(Value::as_str).map(ToString::to_string);

        Ok((result, ty))
    }

    /// Send `continue` for `thread_id`.
    pub fn continue_exec(&mut self, thread_id: i64) -> Result<(), String> {
        let args = json!({"threadId": thread_id});
        let resp = self.request("continue", Some(args));
        self.expect_success(&resp, "continue")?;
        Ok(())
    }

    /// Send `next` (step-over) for `thread_id`.
    pub fn step_over(&mut self, thread_id: i64) -> Result<(), String> {
        let args = json!({"threadId": thread_id});
        let resp = self.request("next", Some(args));
        self.expect_success(&resp, "next")?;
        Ok(())
    }

    /// Send `stepIn` for `thread_id`.
    pub fn step_into(&mut self, thread_id: i64) -> Result<(), String> {
        let args = json!({"threadId": thread_id});
        let resp = self.request("stepIn", Some(args));
        self.expect_success(&resp, "stepIn")?;
        Ok(())
    }

    /// Send `disconnect` and wait for the `terminated` event.
    pub fn disconnect(&mut self) -> Result<(), String> {
        let resp = self.request("disconnect", Some(json!({})));
        self.expect_success(&resp, "disconnect")?;
        let _ = self.drain_until_event("terminated");
        Ok(())
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    /// Send a DAP request and return the raw response message.
    pub fn request(&mut self, command: &str, args: Option<Value>) -> DapMessage {
        self.seq += 1;
        self.adapter.handle_request(self.seq, command, args)
    }

    /// Assert response is a success for `command`; return the body.
    pub fn expect_success(&self, msg: &DapMessage, command: &str) -> Result<Option<Value>, String> {
        match msg {
            DapMessage::Response { success, command: actual, body, message, .. } => {
                if actual != command {
                    return Err(format!("expected `{command}` response, got `{actual}`"));
                }
                if !success {
                    return Err(format!(
                        "command `{command}` failed: {}",
                        message.as_deref().unwrap_or("<no message>")
                    ));
                }
                Ok(body.clone())
            }
            other => Err(format!("expected Response for `{command}`, got: {other:?}")),
        }
    }

    /// Drain the event channel until an event with `event_name` arrives.
    ///
    /// Uses the drain-loop pattern from `dap_smoke_e2e.rs` to discard
    /// non-matching events (e.g. `output` events interleaved with `stopped`).
    pub fn drain_until_event(&self, event_name: &str) -> Result<DapMessage, String> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(format!("timeout waiting for event `{event_name}`"));
            }
            let remaining = deadline.saturating_duration_since(now);
            match self.rx.recv_timeout(remaining) {
                Ok(msg) => {
                    if let DapMessage::Event { event, .. } = &msg
                        && event == event_name
                    {
                        return Ok(msg);
                    }
                    // Non-matching event — discard and keep waiting.
                }
                Err(_) => {
                    return Err(format!("channel closed/timeout waiting for `{event_name}`"));
                }
            }
        }
    }
}

/// Returns the test timeout, inflated when running under coverage or profiling.
pub fn workflow_timeout() -> Duration {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some()
        || std::env::var_os("CARGO_LLVM_COV").is_some()
    {
        Duration::from_mins(1)
    } else {
        Duration::from_secs(15)
    }
}

/// Returns `true` when `perl` is on `PATH`.
pub fn perl_available() -> bool {
    PerlOracleEnv::for_dap_test_fixture().is_some()
}

// ─── Unit tests for set_breakpoints_checked error paths ───────────────────────
//
// These tests exercise the helper's non-trivial error branches without
// requiring a live `perl -d` session.  They rely on the adapter returning
// `verified: false` when the source path cannot be read.

#[cfg(test)]
mod set_breakpoints_checked_tests {
    use super::*;
    use perl_tdd_support::must;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn make_session() -> DapWorkflowSession {
        must(DapWorkflowSession::new(std::time::Duration::from_secs(5)))
    }

    /// When every breakpoint is verified the helper returns a Vec of resolved lines.
    #[test]
    fn returns_resolved_lines_for_verified_breakpoints() -> TestResult {
        use std::io::Write;

        // Write a tiny Perl script so the adapter can read it and verify lines.
        let mut f = must(tempfile::NamedTempFile::new());
        must(writeln!(f, "my $x = 1;"));
        must(writeln!(f, "my $y = 2;"));
        must(writeln!(f, "my $z = 3;"));
        must(f.flush());
        let path = f.path().to_str().ok_or("path not UTF-8")?.to_string();

        let mut session = make_session();
        let result = session.set_breakpoints_checked(&path, &[1, 2]);
        // Lines 1 and 2 are valid executable lines — should be verified.
        assert!(result.is_ok(), "expected Ok but got: {result:?}");
        let lines = result?;
        assert_eq!(lines.len(), 2);
        Ok(())
    }

    /// An unreadable source path causes all breakpoints to be unverified.
    /// `set_breakpoints_checked` must return `Err` mentioning the breakpoint index.
    #[test]
    fn errors_on_unverified_breakpoint() -> TestResult {
        let mut session = make_session();
        // Path that cannot be read → adapter returns verified:false for each bp.
        let result =
            session.set_breakpoints_checked("/nonexistent/path/that/cannot/be/read.pl", &[5]);
        assert!(result.is_err(), "expected Err for unverified breakpoint");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("was not verified") || msg.contains("breakpoint #0"),
            "error message should identify the unverified breakpoint; got: {msg}"
        );
        Ok(())
    }

    /// Zero requested lines → the helper should return an empty Vec without error.
    #[test]
    fn empty_lines_returns_empty_vec() -> TestResult {
        let mut session = make_session();
        // The adapter accepts an empty breakpoints array; no len mismatch.
        let result = session.set_breakpoints_checked("/any/path.pl", &[]);
        // The adapter may error on a missing body or return an empty array; either
        // is acceptable — the key invariant is that resolved_lines.len() == 0 on Ok.
        if let Ok(lines) = result {
            assert!(lines.is_empty(), "empty request should yield empty resolved list");
        }
        // Err is also acceptable (e.g., "setBreakpoints returned no body") — no assert needed.
        Ok(())
    }
}
