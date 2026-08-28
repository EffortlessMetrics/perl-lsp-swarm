//! Shared test helpers for DAP end-to-end workflow tests.
//!
//! `DapWorkflowSession` wraps a `DebugAdapter` and an event `Receiver` with
//! higher-level helpers that chain the request → event → response cycles
//! required to drive a real `perl -d` debug session in tests.

#![allow(dead_code)] // Shared helpers; each integration target uses a subset.
use perl_dap::{DapMessage, DebugAdapter};
use perl_lsp_rs_core::config::PerlOracleEnv;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel, sync_channel};
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

/// Information from a `stopped` event paired with an immediate `stackTrace` call.
///
/// Used by `wait_stopped_with_frame()` to provide the stopped reason AND the
/// current source location in a single atomic helper — avoiding the caller
/// having to issue a separate `stackTrace` request.
#[derive(Debug, Clone)]
// Fields are read selectively in test assertions; allow unused in broader test helpers.
#[allow(dead_code)]
pub struct StoppedFrameInfo {
    /// Stopped reason, e.g. `"breakpoint"`, `"step"`, `"entry"`.
    pub reason: String,
    /// Thread ID (from `stopped.body.threadId`).
    pub thread_id: i64,
    /// Top stack frame ID (from `stackTrace.stackFrames[0].id`).
    pub frame_id: i64,
    /// Source path of the top stack frame.
    pub source_path: String,
    /// 1-based source line of the top stack frame as reported by the adapter.
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
    #[cfg(test)]
    pub fn with_receiver_for_test(rx: Receiver<DapMessage>, timeout: Duration) -> Self {
        Self { adapter: DebugAdapter::new(), rx, timeout, seq: 0 }
    }

    /// Create a new session and send `initialize`.
    ///
    /// Returns an error if initialization fails or the `initialized` event is
    /// not received within `timeout`.
    pub fn new(timeout: Duration) -> Result<Self, String> {
        let mut adapter = DebugAdapter::new();
        let (tx, rx) = sync_channel(64);
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
        self.launch_with_stop_on_entry(script_path, false)
    }

    /// Launch a script pinned to an explicitly resolved interpreter identity.
    ///
    /// Live-session journeys must pin the debuggee perl: the adapter's ambient
    /// resolution (`resolve_launch_interpreter`) inherits whatever `perl`
    /// resolves on the spawning shell's `PATH`, which is not stable across
    /// bash-spawned vs cmd-spawned test runs on Windows. Native MSWin32 perl
    /// builds (e.g. Strawberry) hang at perl5db bootstrap when stdio is piped,
    /// so an unpinned session silently never reaches its first stop (#12594
    /// item 6b). The pinned binary is passed as `perlPath`, which the adapter
    /// honors verbatim.
    pub fn launch_pinned(&mut self, perl_binary: &Path, script_path: &str) -> Result<(), String> {
        let args = json!({
            "program": script_path,
            "args": [],
            "stopOnEntry": false,
            "perlPath": perl_binary.to_string_lossy(),
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

    /// Launch a script with explicit `stopOnEntry` control.
    ///
    /// When `stop_on_entry` is `true`, the adapter emits a `stopped(reason=entry)` event
    /// immediately after launch, before any `configurationDone` is sent.
    /// When `false`, callers must call `set_breakpoints` and `configuration_done` before
    /// `wait_stopped` to follow the DAP ordering requirement.
    pub fn launch_with_stop_on_entry(
        &mut self,
        script_path: &str,
        stop_on_entry: bool,
    ) -> Result<(), String> {
        let args = json!({
            "program": script_path,
            "args": [],
            "stopOnEntry": stop_on_entry,
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

    /// Launch a script with an explicit `cwd` field.
    ///
    /// The script will run in the specified `cwd` directory, not in the directory
    /// where the script file is located.
    pub fn launch_with_cwd(&mut self, script_path: &str, cwd: &str) -> Result<(), String> {
        let args = json!({
            "program": script_path,
            "cwd": cwd,
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

    /// Send `setBreakpoints`, assert that all entries are `verified: true`, and return
    /// the adapter-resolved line numbers.
    ///
    /// The returned lines are what the adapter resolved each requested line to.
    /// For plain executable statements with no remap, resolved == requested.
    /// Tests should assert runtime stopped-frame lines against the RESOLVED lines,
    /// not the originally requested lines, to survive future breakpoint-validation
    /// remapping without false failures.
    ///
    /// # Errors
    ///
    /// Returns an error if the response is missing, the `breakpoints` array is absent,
    /// the count doesn't match, or any entry has `verified: false`.
    pub fn set_breakpoints_checked(
        &mut self,
        source_path: &str,
        lines: &[u64],
    ) -> Result<Vec<i64>, String> {
        let body =
            self.set_breakpoints(source_path, lines)?.ok_or("setBreakpoints returned no body")?;

        let bp_array = body
            .get("breakpoints")
            .and_then(|v| v.as_array())
            .ok_or("setBreakpoints response missing `breakpoints` array")?;

        if bp_array.len() != lines.len() {
            return Err(format!(
                "setBreakpoints: requested {} breakpoints, adapter returned {}",
                lines.len(),
                bp_array.len()
            ));
        }

        let mut resolved = Vec::with_capacity(lines.len());
        for (idx, bp) in bp_array.iter().enumerate() {
            let verified = bp.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
            if !verified {
                return Err(format!(
                    "setBreakpoints: entry[{idx}] (requested line {}) has verified=false",
                    lines[idx]
                ));
            }
            let adapter_line = bp.get("line").and_then(|v| v.as_i64()).unwrap_or(lines[idx] as i64);
            resolved.push(adapter_line);
        }
        Ok(resolved)
    }

    /// Block until a `stopped` event arrives, then immediately issue a `stackTrace`
    /// request to obtain the current source location.
    ///
    /// Returns a [`StoppedFrameInfo`] combining the stopped reason/thread with
    /// the top stack frame's id, source path, and 1-based line number.
    ///
    /// Use this helper when the test must assert BOTH the stop reason AND the
    /// current source line, without the latency of a separate `stack_trace()` call.
    pub fn wait_stopped_with_frame(&mut self) -> Result<StoppedFrameInfo, String> {
        let stopped = self.wait_stopped()?;
        let (frame_id, source_path, line) = self.stack_trace(stopped.thread_id)?;
        Ok(StoppedFrameInfo {
            reason: stopped.reason,
            thread_id: stopped.thread_id,
            frame_id,
            source_path,
            line,
        })
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
    ///
    /// #10563 retired Package/Globals from the advertised scope contract —
    /// `handle_scopes` intentionally never advertises them at a live stopped
    /// frame — so against a live session this always fails with `No Globals
    /// scope found in scopes response`. It remains only as the compilation
    /// contract of the ignored aspirational journeys (genuine globals
    /// enumeration, #10162) and of the compiled-out `real_session_fixture_tests`
    /// module. Live journeys must inspect through `scopes_locals_ref`.
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
            .ok_or("variables body missing `variables` array")?
            .clone();

        Ok(vars)
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
        // The terminal event is once-only: when the debuggee program already
        // ended before disconnect, `terminated` was emitted (and possibly
        // consumed) earlier and `handle_disconnect` cannot re-emit it. Waiting
        // a full workflow timeout for a duplicate that will never arrive would
        // stall every such test, so cap the confirmation drain generously but
        // finitely — a `terminated` emitted by disconnect itself is queued
        // synchronously inside `request`, so it is already observable here.
        let grace = self.timeout.min(Duration::from_secs(2));
        let _ = self.drain_until_event_within("terminated", grace);
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
        self.drain_until_event_within(event_name, self.timeout)
    }

    /// [`drain_until_event`] with an explicit wait budget.
    pub fn drain_until_event_within(
        &self,
        event_name: &str,
        wait: Duration,
    ) -> Result<DapMessage, String> {
        let deadline = Instant::now() + wait;
        let mut recent_events = VecDeque::with_capacity(RECENT_EVENT_LIMIT);
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(wait_error("timeout", event_name, &recent_events));
            }
            let remaining = deadline.saturating_duration_since(now);
            match self.rx.recv_timeout(remaining) {
                Ok(msg) => {
                    if let DapMessage::Event { event, body, .. } = &msg {
                        if event == event_name {
                            return Ok(msg);
                        }

                        push_recent_event(
                            &mut recent_events,
                            summarize_event(event, body.as_ref()),
                        );
                        if event == "terminated" {
                            let reason = body
                                .as_ref()
                                .and_then(|value| value.get("reason"))
                                .and_then(Value::as_str)
                                .unwrap_or("unspecified");
                            return Err(wait_error(
                                &format!("adapter terminated ({})", diagnostic_atom(reason)),
                                event_name,
                                &recent_events,
                            ));
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(wait_error("timeout", event_name, &recent_events));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(wait_error(
                        "event channel disconnected",
                        event_name,
                        &recent_events,
                    ));
                }
            }
        }
    }
}

const RECENT_EVENT_LIMIT: usize = 8;
const DIAGNOSTIC_ATOM_LIMIT: usize = 64;

fn push_recent_event(events: &mut VecDeque<String>, event: String) {
    if events.len() == RECENT_EVENT_LIMIT {
        let _ = events.pop_front();
    }
    events.push_back(event);
}

fn summarize_event(event: &str, body: Option<&Value>) -> String {
    if event == "output" {
        let category = body
            .and_then(|value| value.get("category"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let byte_count =
            body.and_then(|value| value.get("output")).and_then(Value::as_str).map_or(0, str::len);
        return format!("output(category={}, bytes={byte_count})", diagnostic_atom(category));
    }

    if event == "terminated" {
        let reason = body
            .and_then(|value| value.get("reason"))
            .and_then(Value::as_str)
            .unwrap_or("unspecified");
        return format!("terminated(reason={})", diagnostic_atom(reason));
    }

    diagnostic_atom(event)
}

fn wait_error(reason: &str, event_name: &str, recent_events: &VecDeque<String>) -> String {
    let recent = if recent_events.is_empty() {
        "none".to_string()
    } else {
        recent_events.iter().cloned().collect::<Vec<_>>().join(", ")
    };
    format!(
        "{} while waiting for event `{}`; recent events: [{recent}]",
        diagnostic_atom(reason),
        diagnostic_atom(event_name)
    )
}

fn diagnostic_atom(value: &str) -> String {
    let mut atom: String = value
        .chars()
        .take(DIAGNOSTIC_ATOM_LIMIT)
        .map(|character| if character.is_control() { '�' } else { character })
        .collect();
    if value.chars().count() > DIAGNOSTIC_ATOM_LIMIT {
        atom.push('…');
    }
    atom
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

/// Environment variable that flips [`perl_available`] from a silent skip
/// into a hard failure when `perl` cannot be resolved.
///
/// Every DAP integration test follows the pattern
/// `if !perl_available() { eprintln!("SKIP ..."); return Ok(()); }`, which
/// means a missing perl interpreter makes the whole suite report "N passed"
/// while exercising nothing. CI jobs that are supposed to actually run
/// these tests should set `PERL_LSP_DAP_REQUIRE_PERL=1` so a missing
/// perl fails loudly instead of vacuously greening.
pub const REQUIRE_PERL_ENV: &str = "PERL_LSP_DAP_REQUIRE_PERL";

/// Returns `true` when `perl` is on `PATH`.
///
/// When [`REQUIRE_PERL_ENV`] is set to a truthy value, a missing perl is
/// treated as a hard failure (an `assert!` panic) instead of a silent skip.
pub fn perl_available() -> bool {
    let available = PerlOracleEnv::for_dap_test_fixture().is_some();
    if !available {
        let strict = std::env::var(REQUIRE_PERL_ENV)
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        assert!(
            !strict,
            "{REQUIRE_PERL_ENV}=1 is set, which forbids the silent DAP-test \
             SKIP path — perl interpreter not found on PATH. \
             Install perl or unset the env var."
        );
    }
    available
}

// ─── Live-session debuggee perl resolution (#12594 item 6b) ──────────────────

/// Environment variable that pins the interpreter used by live DAP sessions.
///
/// When set, it names the ONLY candidate considered by
/// [`resolve_debuggee_perl`] (an absolute path to a perl executable). A pinned
/// candidate that fails the pipe-conformance probe fails resolution outright —
/// an explicitly chosen identity must never be silently replaced.
pub const DEBUGGEE_PERL_OVERRIDE_ENV: &str = "PERL_LSP_DAP_DEBUGGEE_PERL";

/// Wall-clock budget for one [`probe_debuggee_perl`] attempt.
///
/// A working perl5db emits its banner well inside a second; a broken one
/// (native MSWin32 builds at piped bootstrap) hangs forever, so the budget is
/// what bounds the probe.
const DEBUGGEE_PROBE_BUDGET: Duration = Duration::from_secs(10);

/// A debuggee interpreter proven able to run a real debugger session over
/// piped stdio.
#[derive(Debug, Clone)]
pub struct DebuggeePerl {
    /// Absolute path of the probed interpreter; passed as launch-args
    /// `perlPath`, which the adapter honors verbatim.
    pub binary: PathBuf,
    /// Diagnostic identity derived from the probe session's own output
    /// (preferentially the perl5db banner line), capped to 120 characters.
    pub identity: String,
}

struct DebuggeePerlResolution {
    resolved: Option<DebuggeePerl>,
    diagnostics: Vec<String>,
    /// Whether at least one candidate failed in the timing-sensitive class,
    /// making one retry worthwhile before caching the negative result.
    transient_failure: bool,
}

static DEBUGGEE_PERL: OnceLock<DebuggeePerlResolution> = OnceLock::new();

fn debuggee_perl_candidates() -> Vec<PathBuf> {
    if let Some(pinned) = std::env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV) {
        return vec![PathBuf::from(pinned)];
    }

    let mut candidates = vec![PathBuf::from("perl")];

    // Windows: add well-known MSYS-family perl locations. The cataloged root
    // cause (#12594 item 6b) is that native MSWin32 perl5db builds cannot run
    // over piped stdio, while MSYS/cygwin-flavored builds can; these paths are
    // only PROPOSALS — every candidate still has to pass the conformance
    // probe before it is trusted. Environments with other layouts should set
    // [`DEBUGGEE_PERL_OVERRIDE_ENV`].
    if cfg!(windows) {
        if let Some(system_drive) = std::env::var_os("SystemDrive") {
            candidates.push(PathBuf::from(format!(
                "{}\\msys64\\usr\\bin\\perl.exe",
                system_drive.to_string_lossy()
            )));
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.push(PathBuf::from(format!(
                "{}\\Git\\usr\\bin\\perl.exe",
                program_files.to_string_lossy()
            )));
        }
    }

    candidates
}

/// Why one [`probe_debuggee_perl`] attempt failed.
struct ProbeFailure {
    reason: String,
    /// Timing-sensitive failure (the deadline killed a still-running
    /// debuggee); one retry can legitimately flip it, unlike deterministic
    /// failures such as a spawn error or a missing debugger banner.
    transient: bool,
}

/// Probe whether `binary` can actually drive a debugger session over true
/// pipes.
///
/// Spawns `<binary> -d -- <fixture>` with all three stdio streams as real OS
/// pipes (`Stdio::piped()` ×3 — the exact spawn shape the adapter uses in
/// `crates/perl-dap/src/debug_adapter/process.rs`), feeds `c` then `q`
/// through the pipe write end from a dedicated writer thread, and drains
/// stdout and stderr on their own reader threads while the deadline loop
/// waits for exit. Redirection to FILES was rejected deliberately: files are
/// seekable and perl5db treats non-seekable stdin differently, so a candidate
/// can pass a file probe yet still hang over the adapter's true pipes — the
/// exact failure class being classified (#12594 item 6b). A working perl5db
/// exits well inside [`DEBUGGEE_PROBE_BUDGET`]; native MSWin32 builds produce
/// zero bytes and hang until killed.
///
/// Residual sensitivity: a single attempt is wall-clock sensitive, so the
/// resolver retries once on a transient-class failure before caching a
/// negative result ([`resolved_debuggee_perl_or_reason`]).
fn probe_debuggee_perl(binary: &Path) -> Result<DebuggeePerl, ProbeFailure> {
    let fail = |reason: String| ProbeFailure { reason, transient: false };
    let probe_dir =
        std::env::temp_dir().join(format!("perl-lsp-dap-debuggee-probe-{}", std::process::id()));
    fs::create_dir_all(&probe_dir).map_err(|e| fail(format!("cannot create probe dir: {e}")))?;
    let script = probe_dir.join("pipe_probe.pl");
    fs::write(
        &script,
        "use strict;\nuse warnings;\nmy $x = 10;\nmy $y = $x + 5;\nprint \"$y\\n\";\n",
    )
    .map_err(|e| fail(format!("cannot write probe script: {e}")))?;

    let mut child = Command::new(binary)
        .args(["-d", "--"])
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("PERL5LIB")
        .env_remove("PERL5OPT")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .spawn()
        .map_err(|e| fail(format!("cannot spawn: {e}")))?;

    let Some(stdout_pipe) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(fail("stdout pipe unavailable".to_string()));
    };
    let Some(stderr_pipe) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(fail("stderr pipe unavailable".to_string()));
    };

    // Feed the scripted debugger commands through the REAL pipe write end. The
    // payload is four bytes, so the writer cannot block on a full pipe buffer;
    // dropping `stdin` afterwards delivers EOF exactly like an editor closing
    // its side of the session.
    let stdin_pipe = child.stdin.take();
    let writer = std::thread::spawn(move || {
        if let Some(mut stdin) = stdin_pipe {
            let _ = stdin.write_all(b"c\nq\n");
            let _ = stdin.flush();
        }
    });

    let stdout_chunks = drain_pipe(stdout_pipe);
    let stderr_chunks = drain_pipe(stderr_pipe);

    let deadline = Instant::now() + DEBUGGEE_PROBE_BUDGET;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ProbeFailure {
                        reason: format!(
                            "no exit within {}s — perl5db cannot bootstrap over piped stdio",
                            DEBUGGEE_PROBE_BUDGET.as_secs()
                        ),
                        transient: true,
                    });
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(fail(format!("probe wait failed: {e}"))),
        }
    };
    let _ = writer.join();

    // The child has exited, so its pipe write ends are closing and the reader
    // threads reach EOF almost immediately; the bounded collector exists only
    // so a grandchild inheriting the write end cannot extend the probe past
    // its budget.
    let stdout = collect_pipe_output(stdout_chunks);
    let stderr = collect_pipe_output(stderr_chunks);

    // Either signal proves a live debugger session ran over pipes: the perl5db
    // banner (stderr) or the fixture program's own output (stdout, `15`).
    if !stderr.contains("Loading DB routines") && !stdout.contains("15") {
        let stderr_atom: String = stderr.chars().take(160).collect();
        return Err(fail(format!(
            "no usable debugger session over pipes (exit={status}, stdout={}B, \
             stderr={}B: {stderr_atom})",
            stdout.len(),
            stderr.len()
        )));
    }

    Ok(DebuggeePerl {
        binary: binary.to_path_buf(),
        identity: identity_from_probe_output(&stderr, &stdout),
    })
}

/// Drain `pipe` to EOF on a background thread, forwarding chunks to the
/// returned receiver. Killing the debuggee closes its pipe write ends, which
/// unblocks the reader — the deadline loop never waits on the drain.
fn drain_pipe<R: Read + Send + 'static>(pipe: R) -> Receiver<Vec<u8>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut pipe = pipe;
        let mut buf = [0u8; 4096];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

/// Collect drained chunks into a string, bounded well inside the probe budget.
fn collect_pipe_output(rx: Receiver<Vec<u8>>) -> String {
    let mut bytes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Diagnostic identity from the probe's own output — the perl5db banner line
/// when present, else the first non-empty output line. Derived here instead of
/// shelling out to an unbounded separate `--version` subprocess.
fn identity_from_probe_output(stderr: &str, stdout: &str) -> String {
    let mut lines = stderr.lines().chain(stdout.lines());
    let identity_line = lines
        .clone()
        .find(|line| line.contains("Loading DB routines"))
        .or_else(|| lines.find(|line| !line.trim().is_empty()))
        .unwrap_or("unknown perl");
    identity_line.chars().take(120).collect()
}

/// Resolve and cache a pipe-capable debuggee interpreter for live sessions.
///
/// Candidate order: [`DEBUGGEE_PERL_OVERRIDE_ENV`] (exclusive when set),
/// `perl` on `PATH`, then well-known MSYS-family locations on Windows. Every
/// candidate must pass [`probe_debuggee_perl`] — path presence alone never
/// proves session capability. Resolution runs once per test process.
pub fn resolve_debuggee_perl() -> Option<&'static DebuggeePerl> {
    resolved_debuggee_perl_or_reason().ok()
}

/// One uncached resolution sweep over every candidate interpreter.
fn resolve_debuggee_perl_uncached() -> DebuggeePerlResolution {
    let explicit = std::env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV).is_some();
    let mut diagnostics = Vec::new();
    let mut transient_failure = false;
    for candidate in &debuggee_perl_candidates() {
        match probe_debuggee_perl(candidate) {
            Ok(perl) => {
                return DebuggeePerlResolution {
                    resolved: Some(perl),
                    diagnostics,
                    transient_failure: false,
                };
            }
            Err(failure) => {
                transient_failure |= failure.transient;
                if explicit {
                    // An explicitly pinned identity must not fall through:
                    // surface its failure and stop.
                    diagnostics.push(format!(
                        "{DEBUGGEE_PERL_OVERRIDE_ENV} pin {}: {}",
                        candidate.display(),
                        failure.reason
                    ));
                    break;
                }
                diagnostics.push(format!("{}: {}", candidate.display(), failure.reason));
            }
        }
    }
    DebuggeePerlResolution { resolved: None, diagnostics, transient_failure }
}

/// [`resolve_debuggee_perl`] with the captured per-candidate failure reasons.
///
/// A transient probe failure (wall-clock deadline kill) is retried ONCE with a
/// fresh sweep over all candidates before the negative result is cached for
/// the test process. Residual sensitivity: two consecutive transient windows
/// still cache unresolved, so one heavily loaded moment can skip every live
/// test in a target for that run.
fn resolved_debuggee_perl_or_reason() -> Result<&'static DebuggeePerl, String> {
    let resolution = DEBUGGEE_PERL.get_or_init(|| {
        let first = resolve_debuggee_perl_uncached();
        if first.resolved.is_some() || !first.transient_failure {
            return first;
        }
        resolve_debuggee_perl_uncached()
    });

    match resolution.resolved.as_ref() {
        Some(perl) => Ok(perl),
        None => Err(resolution.diagnostics.join("; ")),
    }
}

/// Resolve the debuggee perl or emit a typed skip for `test_name`.
///
/// Returns `None` (after printing a SKIP line carrying the per-candidate
/// diagnostics) when no pipe-capable interpreter can be resolved. Under
/// [`REQUIRE_PERL_ENV`] strict mode an unresolved debuggee perl is a hard
/// failure instead of a skip, matching [`perl_available`].
pub fn debuggee_perl_or_typed_skip(test_name: &str) -> Option<&'static DebuggeePerl> {
    match resolved_debuggee_perl_or_reason() {
        Ok(perl) => Some(perl),
        Err(reason) => {
            let strict = std::env::var(REQUIRE_PERL_ENV)
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            assert!(
                !strict,
                "{REQUIRE_PERL_ENV}=1 is set, which forbids the silent DAP-test \
                 SKIP path — no pipe-capable perl debugger could be resolved: {reason}. \
                 Set {DEBUGGEE_PERL_OVERRIDE_ENV}=<path> to pin a capable interpreter."
            );
            eprintln!(
                "SKIP {test_name}: no pipe-capable perl debugger for live-session proof \
                 ({reason}). Live DAP journeys require a perl whose perl5db operates over \
                 piped stdio; native MSWin32 builds hang at debugger bootstrap (#12594 \
                 item 6b). Set {DEBUGGEE_PERL_OVERRIDE_ENV}=<path> to pin an interpreter."
            );
            None
        }
    }
}

/// Wait for a named DAP event on the receiver, returning the full message.
///
/// This is the canonical shared copy of the `wait_for_event` helper that was
/// previously copy-pasted across several DAP integration-test files
/// (`dap_smoke_e2e`, `dap_attach_e2e`, `dap_module_resolution_smoke`,
/// `dap_scorecard_harness`). Files whose `wait_for_event` had a materially
/// different signature (e.g. returning `Option<Value>` or taking `timeout_ms`)
/// keep their local variant — only the byte-identical copies were consolidated
/// here (#5232).
// Shared helper: each integration-test binary compiles `common` separately, so
// binaries that do not call it would otherwise trip per-target dead_code.
#[allow(dead_code)]
pub fn wait_for_event(
    rx: &Receiver<DapMessage>,
    event_name: &str,
    timeout: Duration,
) -> Result<DapMessage, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!("timeout waiting for event `{event_name}`"));
        }
        let remaining = deadline.saturating_duration_since(now);
        match rx.recv_timeout(remaining) {
            Ok(message) => {
                if let DapMessage::Event { event, .. } = &message
                    && event == event_name
                {
                    return Ok(message);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!("timeout waiting for event `{event_name}`"));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "channel disconnected waiting for `{event_name}` — debuggee exited or crashed"
                ));
            }
        }
    }
}
