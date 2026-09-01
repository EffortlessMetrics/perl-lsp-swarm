//! Shared test helpers for DAP end-to-end workflow tests.
//!
//! `DapWorkflowSession` wraps a `DebugAdapter` and an event `Receiver` with
//! higher-level helpers that chain the request → event → response cycles
//! required to drive a real `perl -d` debug session in tests.

#![allow(dead_code)]
use perl_dap::{DapMessage, DebugAdapter};
use perl_lsp_rs_core::config::PerlOracleEnv;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

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
    perl_path: Option<PathBuf>,
    resolve_perl_on_launch: bool,
}

// Shared workflow-test helpers are consumed incrementally by DAP scenarios.
#[allow(dead_code)]
impl DapWorkflowSession {
    #[cfg(test)]
    pub fn with_receiver_for_test(rx: Receiver<DapMessage>, timeout: Duration) -> Self {
        Self {
            adapter: DebugAdapter::new(),
            rx,
            timeout,
            seq: 0,
            perl_path: None,
            resolve_perl_on_launch: false,
        }
    }

    /// Create a new session and send `initialize`.
    ///
    /// Returns an error if initialization fails or the `initialized` event is
    /// not received within `timeout`.
    pub fn new(timeout: Duration) -> Result<Self, String> {
        Self::new_initialized(timeout, None, true)
    }

    /// Create a session and make every convenience launch helper use the
    /// supplied interpreter path after explicit identity normalization.
    pub fn new_with_perl(timeout: Duration, perl_path: Option<&Path>) -> Result<Self, String> {
        let normalized_perl_path = perl_path.map(normalize_explicit_debuggee_pin).transpose()?;
        Self::new_initialized(timeout, normalized_perl_path.as_deref(), false)
    }

    fn new_initialized(
        timeout: Duration,
        perl_path: Option<&Path>,
        resolve_perl_on_launch: bool,
    ) -> Result<Self, String> {
        let mut adapter = DebugAdapter::new();
        let (tx, rx) = sync_channel(64);
        adapter.set_event_sender(tx);
        install_unbounded_test_authority(&adapter);

        let mut session = Self {
            adapter,
            rx,
            timeout,
            seq: 0,
            perl_path: perl_path.map(Path::to_path_buf),
            resolve_perl_on_launch,
        };

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
    /// item 6b). The normalized pinned binary is passed as `perlPath`, which
    /// the adapter honors as the requested interpreter identity.
    pub fn launch_pinned(&mut self, perl_binary: &Path, script_path: &str) -> Result<(), String> {
        let normalized_perl_binary = normalize_explicit_debuggee_pin(perl_binary)?;
        let args = launch_arguments(script_path, None, false, Some(&normalized_perl_binary));
        let resp = self.request("launch", Some(args));
        self.expect_success(&resp, "launch")?;
        Ok(())
    }

    /// Launch a pinned script with a conflicting PATH, keeping the proof on
    /// the real DAP launch boundary.
    #[cfg(test)]
    pub fn launch_pinned_with_env(
        &mut self,
        perl_binary: &Path,
        script_path: &str,
        env_overrides: &Value,
    ) -> Result<(), String> {
        let normalized_perl_binary = normalize_explicit_debuggee_pin(perl_binary)?;
        let mut args = launch_arguments(script_path, None, true, Some(&normalized_perl_binary));
        let launch_env = args
            .get_mut("env")
            .and_then(Value::as_object_mut)
            .ok_or("launch arguments missing env object")?;
        if let Some(env) = env_overrides.as_object() {
            for (key, value) in env {
                launch_env.insert(key.clone(), value.clone());
            }
        }
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
        // Gated callers use this helper after `perl_available()`. Resolve the
        // same pinned identity here as well so a valid pin controls the live
        // process, even when the caller uses the legacy convenience method.
        let perl_path = self.perl_path_for_launch()?;
        let args = launch_arguments(script_path, None, stop_on_entry, perl_path.as_deref());
        let resp = self.request("launch", Some(args));
        self.expect_success(&resp, "launch")?;
        Ok(())
    }

    /// Launch a script with an explicit `cwd` field.
    ///
    /// The script will run in the specified `cwd` directory, not in the directory
    /// where the script file is located.
    pub fn launch_with_cwd(&mut self, script_path: &str, cwd: &str) -> Result<(), String> {
        // Keep the explicit cwd path under the same pin-propagating contract
        // as `launch`; this is a gated live-session consumer too.
        let perl_path = self.perl_path_for_launch()?;
        let args = launch_arguments(script_path, Some(cwd), false, perl_path.as_deref());
        let resp = self.request("launch", Some(args));
        self.expect_success(&resp, "launch")?;
        Ok(())
    }

    fn perl_path_for_launch(&mut self) -> Result<Option<PathBuf>, String> {
        if self.resolve_perl_on_launch {
            self.perl_path = resolve_launch_perl_path()?;
            self.resolve_perl_on_launch = false;
        }
        Ok(self.perl_path.clone())
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

/// Build the common launch request while keeping interpreter identity explicit.
///
/// A configured debuggee pin is passed by every convenience launch helper. With
/// no pin, the resolved pipe-capable interpreter is passed when available;
/// launch cannot silently fall back to an unprobed PATH interpreter.
fn launch_arguments(
    script_path: &str,
    cwd: Option<&str>,
    stop_on_entry: bool,
    perl_binary: Option<&Path>,
) -> Value {
    let mut args = json!({
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
    if let Some(cwd) = cwd {
        args["cwd"] = Value::String(cwd.to_string());
    }
    if let Some(perl_binary) = perl_binary {
        args["perlPath"] = Value::String(perl_binary.to_string_lossy().into_owned());
    }
    args
}

#[cfg(test)]
mod launch_argument_tests {
    use super::launch_arguments;
    use serde_json::Value;
    use std::path::Path;

    #[test]
    fn launch_arguments_preserve_exact_pinned_interpreter() -> Result<(), String> {
        let pinned = Path::new("C:/controls/pinned-perl.exe");
        let args = launch_arguments("fixture.pl", Some("C:/work"), true, Some(pinned));

        if args.get("perlPath") != Some(&Value::String(pinned.to_string_lossy().into_owned())) {
            return Err("launch arguments must preserve the pinned interpreter".to_string());
        }
        if args.get("cwd") != Some(&Value::String("C:/work".to_string())) {
            return Err("launch arguments must preserve cwd".to_string());
        }
        if args.get("stopOnEntry") != Some(&Value::Bool(true)) {
            return Err("launch arguments must preserve stopOnEntry".to_string());
        }
        Ok(())
    }
}

fn resolved_launch_arguments(
    script_path: &str,
    cwd: Option<&str>,
    stop_on_entry: bool,
) -> Result<Value, String> {
    let perl_binary = resolve_launch_perl_path()?;
    Ok(launch_arguments(script_path, cwd, stop_on_entry, perl_binary.as_deref()))
}

#[cfg(test)]
pub(crate) fn resolved_launch_arguments_for_test(
    script_path: &str,
    cwd: Option<&str>,
    stop_on_entry: bool,
) -> Result<Value, String> {
    resolved_launch_arguments(script_path, cwd, stop_on_entry)
}

/// Resolve the interpreter for a shared launch convenience.
///
/// An explicit debuggee pin is an identity constraint, not a preference. If
/// its pipe-conformance probe fails, return the diagnostic instead of omitting
/// `perlPath` and allowing the adapter to fall back to PATH/profile resolution.
/// With no pin, the selected candidate must still pass the same pipe-conformance
/// probe; a PATH hit that cannot run the real debugger is not a valid launch
/// fallback.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn resolve_launch_perl_path() -> Result<Option<PathBuf>, String> {
    resolved_debuggee_perl_or_reason().map(|perl| Some(perl.binary.clone())).map_err(|reason| {
        if std::env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV).is_some() {
            format!(
                "{DEBUGGEE_PERL_OVERRIDE_ENV} is set but its pinned interpreter \
                     cannot be used for a DAP launch: {reason}"
            )
        } else {
            format!("no pipe-capable Perl interpreter can be used for a DAP launch: {reason}")
        }
    })
}

/// Normalize an explicit interpreter pin before probing or placing it in a
/// launch request. Relative paths must not be reinterpreted when a launch
/// supplies a different `cwd`, and a path that cannot be represented by the
/// DAP string field must fail closed instead of being lossy-converted.
pub(crate) fn normalize_explicit_debuggee_pin(path: &Path) -> Result<PathBuf, String> {
    if path.to_str().is_none() {
        return Err("the pinned interpreter path is not valid UTF-8".to_string());
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                format!("cannot resolve the pin relative to the test process: {error}")
            })?
            .join(path)
    };
    let canonical = fs::canonicalize(&absolute)
        .map_err(|error| format!("cannot resolve the pinned interpreter path: {error}"))?;
    #[cfg(windows)]
    let canonical = normalize_windows_path_prefix(canonical);
    if canonical.to_str().is_none() {
        return Err("the resolved pinned interpreter path is not valid UTF-8".to_string());
    }
    Ok(canonical)
}

#[cfg(test)]
pub(crate) fn assert_pinned_identity(
    reported: &str,
    pinned: &Path,
    ambient: &Path,
    label: &str,
) -> Result<(), String> {
    let expected_pinned = normalize_explicit_debuggee_pin(pinned)
        .map_err(|error| format!("{label} pinned path did not normalize: {error}"))?;
    let expected_ambient = normalize_explicit_debuggee_pin(ambient)
        .map_err(|error| format!("{label} ambient path did not normalize: {error}"))?;
    let actual = normalize_explicit_debuggee_pin(Path::new(reported.trim()))
        .map_err(|error| format!("{label} reported path did not normalize: {error}"))?;
    if actual != expected_pinned {
        return Err(format!(
            "{label} evaluated $^X from {actual:?}, expected pinned {expected_pinned:?}"
        ));
    }
    if actual == expected_ambient {
        return Err(format!("{label} evaluated $^X from the ambient path {expected_ambient:?}"));
    }
    Ok(())
}

#[cfg(windows)]
fn normalize_windows_path_prefix(path: PathBuf) -> PathBuf {
    let Some(path_text) = path.to_str() else {
        return path;
    };
    if let Some(unc_path) = path_text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{unc_path}"))
    } else if let Some(drive_path) = path_text.strip_prefix(r"\\?\") {
        PathBuf::from(drive_path)
    } else {
        path
    }
}

#[cfg(test)]
mod explicit_pin_tests {
    use super::{launch_arguments, normalize_explicit_debuggee_pin, resolve_debuggee_candidate};
    use std::fs;
    use std::path::Path;

    #[test]
    fn nested_relative_pin_is_frozen_before_different_launch_cwd() -> Result<(), String> {
        let process_cwd = std::env::current_dir()
            .map_err(|error| format!("the current directory should resolve: {error}"))?;
        let controls = tempfile::Builder::new()
            .prefix("perl-lsp-dap-relative-pin-")
            .tempdir_in(&process_cwd)
            .map_err(|error| format!("relative-pin controls should be created: {error}"))?;
        let nested_dir = controls.path().join("nested").join("bin");
        fs::create_dir_all(&nested_dir)
            .map_err(|error| format!("nested relative-pin directory should be created: {error}"))?;
        let binary = nested_dir.join(if cfg!(windows) { "pinned-perl.exe" } else { "pinned-perl" });
        fs::write(&binary, b"pin identity control")
            .map_err(|error| format!("relative-pin executable should be created: {error}"))?;
        let relative = binary
            .strip_prefix(&process_cwd)
            .map_err(|error| format!("control path should be relative to the test cwd: {error}"))?;
        let expected = fs::canonicalize(&binary).map_err(|error| error.to_string())?;
        #[cfg(windows)]
        let expected = super::normalize_windows_path_prefix(expected);
        let resolved = normalize_explicit_debuggee_pin(relative)
            .map_err(|error| format!("the nested relative pin should canonicalize: {error}"))?;
        if resolved != expected {
            return Err(format!(
                "resolved pin differs from canonical path: {resolved:?} != {expected:?}"
            ));
        }

        let different_cwd = controls.path().join("different-launch-cwd");
        fs::create_dir(&different_cwd)
            .map_err(|error| format!("different launch cwd should be created: {error}"))?;
        let different_cwd =
            different_cwd.to_str().ok_or("different launch cwd should be valid UTF-8")?;
        let launch = launch_arguments("fixture.pl", Some(different_cwd), false, Some(&resolved));
        if launch.get("perlPath").and_then(|value| value.as_str())
            != Some(resolved.to_string_lossy().as_ref())
        {
            return Err(
                "the launch must carry the canonical pin, not reinterpret it under cwd".to_string()
            );
        }
        if launch.get("cwd").and_then(|value| value.as_str()) != Some(different_cwd) {
            return Err("the launch must retain its independent working directory".to_string());
        }
        Ok(())
    }

    #[test]
    fn path_only_candidate_is_frozen_to_absolute_path() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let binary = controls.path().join("perl");
        fs::write(&binary, b"path candidate").map_err(|error| error.to_string())?;
        let search_path =
            std::env::join_paths([controls.path()]).map_err(|error| error.to_string())?;
        let resolved =
            resolve_debuggee_candidate(Path::new("perl"), Some(search_path.as_os_str()))?;
        let expected = fs::canonicalize(binary).map_err(|error| error.to_string())?;
        if resolved != expected {
            return Err(format!(
                "PATH candidate was not frozen absolutely: {resolved:?} != {expected:?}"
            ));
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn path_only_candidate_honors_pathext_for_perl_exe() -> Result<(), String> {
        let controls = tempfile::tempdir().map_err(|error| error.to_string())?;
        let binary = controls.path().join("perl.exe");
        fs::write(&binary, b"path candidate").map_err(|error| error.to_string())?;
        let search_path =
            std::env::join_paths([controls.path()]).map_err(|error| error.to_string())?;
        let resolved =
            resolve_debuggee_candidate(Path::new("perl"), Some(search_path.as_os_str()))?;
        let expected = fs::canonicalize(binary).map_err(|error| error.to_string())?;
        if resolved != expected {
            return Err(format!(
                "PATHEXT candidate was not resolved: {resolved:?} != {expected:?}"
            ));
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_pin_is_rejected_before_launch() -> Result<(), String> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        use std::path::Path;

        let path_value = OsString::from_vec(vec![b'/', b't', b'm', b'p', 0xff]);
        let path = Path::new(&path_value);
        let error = match normalize_explicit_debuggee_pin(path) {
            Ok(_) => return Err("non-UTF-8 pin unexpectedly resolved".to_string()),
            Err(error) => error,
        };
        if !error.contains("not valid UTF-8") {
            return Err(format!("unexpected error: {error}"));
        }
        Ok(())
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

/// Returns `true` when an interpreter is available for DAP tests.
///
/// Availability follows the same single source of truth as live-session
/// resolution: when [`DEBUGGEE_PERL_OVERRIDE_ENV`] pins an interpreter, that
/// pin — actually probed over piped stdio — exclusively decides availability;
/// PATH presence neither rescues a rejected pin nor vetoes a capable one
/// (claim #12594 repair r2, finding 1). A PATH-only early return used to win
/// here, letting gates such as the scorecard harness proceed while the pin
/// named the only interpreter live sessions were allowed to touch.
///
/// Without a pin, availability stays the cheap PATH oracle.
///
/// When [`REQUIRE_PERL_ENV`] is set to a truthy value, unavailability is a
/// hard failure (an `assert!` panic) instead of a silent skip, diagnosing
/// whichever source was consulted and why it rejected — including the full
/// per-candidate probe failures of a rejected pin.
pub fn perl_available() -> bool {
    let unavailable_reason: Option<String> =
        if std::env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV).is_some() {
            match resolved_debuggee_perl_or_reason() {
                Ok(_) => None,
                Err(diagnostics) => Some(format!(
                    "the {DEBUGGEE_PERL_OVERRIDE_ENV} pinned interpreter failed its \
                     pipe-capability probe ({diagnostics})"
                )),
            }
        } else if PerlOracleEnv::for_dap_test_fixture().is_some() {
            None
        } else {
            Some("perl interpreter not found on PATH".to_string())
        };
    if let Some(reason) = unavailable_reason {
        let strict = std::env::var(REQUIRE_PERL_ENV)
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        assert!(
            !strict,
            "{REQUIRE_PERL_ENV}=1 is set, which forbids the silent DAP-test \
             SKIP path — {reason}. Install/repair a pipe-capable perl, fix the \
             pin, or unset the env vars."
        );
        return false;
    }
    true
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
#[cfg(test)]
static LAST_PROBE_PID: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static ACTIVE_PROBE_READERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(test, unix))]
static LAST_PROBE_USED_SIGKILL: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static DEFERRED_PROBE_READERS: OnceLock<Mutex<Vec<JoinHandle<Result<(), String>>>>> =
    OnceLock::new();
#[cfg(test)]
static DEFERRED_PROBE_CHILDREN: OnceLock<Mutex<Vec<Child>>> = OnceLock::new();

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
    probe_debuggee_perl_with_options(binary, DEBUGGEE_PROBE_BUDGET, false, None, CleanupFault::None)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanupFault {
    None,
    TerminationOperations,
    WorkspaceRemoval,
    ThreadSpawn(ProbeThreadSpawnFailure),
    #[cfg(windows)]
    JobAssignment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProbeThreadSpawnFailure {
    Writer,
    StdoutReader,
    StderrReader,
}

impl CleanupFault {
    fn termination_failed(self) -> bool {
        matches!(self, Self::TerminationOperations)
    }

    fn workspace_removal_failed(self) -> bool {
        matches!(self, Self::WorkspaceRemoval)
    }

    #[cfg(windows)]
    fn assignment_failed(self) -> bool {
        matches!(self, Self::JobAssignment)
    }

    fn thread_spawn_failed(self, stage: ProbeThreadSpawnFailure) -> bool {
        matches!(self, Self::ThreadSpawn(actual) if actual == stage)
    }
}

/// Test-only entry point for exercising every child/workspace exit path with a
/// short deadline and a deterministic `try_wait` failure injection. The
/// production resolver always uses `probe_debuggee_perl` above, so this seam
/// cannot alter shipped adapter behavior.
#[cfg(test)]
pub(crate) fn probe_debuggee_perl_for_test(
    binary: &Path,
    budget: Duration,
    simulate_wait_error: bool,
) -> Result<DebuggeePerl, String> {
    probe_debuggee_perl_with_options(binary, budget, simulate_wait_error, None, CleanupFault::None)
        .map_err(|failure| failure.reason)
}

#[cfg(test)]
pub(crate) fn probe_debuggee_perl_for_test_with_descendant_pid(
    binary: &Path,
    budget: Duration,
    simulate_wait_error: bool,
    descendant_pid_file: &Path,
) -> Result<DebuggeePerl, String> {
    probe_debuggee_perl_with_options(
        binary,
        budget,
        simulate_wait_error,
        Some(descendant_pid_file),
        CleanupFault::None,
    )
    .map_err(|failure| failure.reason)
}

#[cfg(test)]
pub(crate) fn probe_debuggee_perl_for_test_with_termination_failure(
    binary: &Path,
    budget: Duration,
    descendant_pid_file: &Path,
) -> Result<DebuggeePerl, String> {
    probe_debuggee_perl_with_options(
        binary,
        budget,
        false,
        Some(descendant_pid_file),
        CleanupFault::TerminationOperations,
    )
    .map_err(|failure| failure.reason)
}

#[cfg(test)]
pub(crate) fn probe_debuggee_perl_for_test_with_workspace_cleanup_failure(
    binary: &Path,
    budget: Duration,
) -> Result<DebuggeePerl, String> {
    probe_debuggee_perl_with_options(binary, budget, false, None, CleanupFault::WorkspaceRemoval)
        .map_err(|failure| failure.reason)
}

#[cfg(test)]
pub(crate) fn probe_debuggee_perl_for_test_with_thread_spawn_failure(
    binary: &Path,
    budget: Duration,
    descendant_pid_file: &Path,
    stage: ProbeThreadSpawnFailure,
) -> Result<DebuggeePerl, String> {
    probe_debuggee_perl_with_options(
        binary,
        budget,
        false,
        Some(descendant_pid_file),
        CleanupFault::ThreadSpawn(stage),
    )
    .map_err(|failure| failure.reason)
}

#[cfg(test)]
pub(crate) fn active_probe_reader_count() -> usize {
    reap_deferred_probe_readers();
    ACTIVE_PROBE_READERS.load(Ordering::Acquire)
}

#[cfg(all(test, unix))]
pub(crate) fn reset_sigkill_escalation_observation() {
    LAST_PROBE_USED_SIGKILL.store(false, Ordering::Release);
}

#[cfg(all(test, unix))]
pub(crate) fn sigkill_escalation_was_observed() -> bool {
    LAST_PROBE_USED_SIGKILL.load(Ordering::Acquire)
}

#[cfg(test)]
fn reap_deferred_probe_readers() {
    let Some(readers) = DEFERRED_PROBE_READERS.get() else { return };
    let mut readers = match readers.lock() {
        Ok(readers) => readers,
        Err(poisoned) => poisoned.into_inner(),
    };
    let mut pending = Vec::with_capacity(readers.len());
    for reader in readers.drain(..) {
        if reader.is_finished() {
            let _ = reader.join();
        } else {
            pending.push(reader);
        }
    }
    *readers = pending;
}

#[cfg(all(test, windows))]
pub(crate) fn probe_debuggee_perl_for_test_with_job_assignment_failure(
    binary: &Path,
    budget: Duration,
    descendant_pid_file: &Path,
) -> Result<DebuggeePerl, String> {
    probe_debuggee_perl_with_options(
        binary,
        budget,
        false,
        Some(descendant_pid_file),
        CleanupFault::JobAssignment,
    )
    .map_err(|failure| failure.reason)
}

#[cfg(windows)]
struct ProbeJob {
    handle: winapi::shared::ntdef::HANDLE,
}

#[cfg(windows)]
const CREATE_SUSPENDED_FLAG: u32 = 0x0000_0004;

#[cfg(windows)]
impl ProbeJob {
    fn assign(child: &Child) -> io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use std::ptr::null_mut;
        use winapi::um::jobapi2::{
            AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        };
        use winapi::um::winnt::{
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation,
        };

        let handle = unsafe { CreateJobObjectW(null_mut(), null_mut()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) != 0
        };
        let assigned = configured
            && unsafe { AssignProcessToJobObject(handle, child.as_raw_handle() as _) != 0 };
        if !assigned {
            let error = io::Error::last_os_error();
            unsafe { winapi::um::handleapi::CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self { handle })
    }
}

#[cfg(windows)]
impl Drop for ProbeJob {
    fn drop(&mut self) {
        unsafe { winapi::um::handleapi::CloseHandle(self.handle) };
    }
}

#[cfg(windows)]
fn resume_suspended_probe_process(child: &Child) -> io::Result<()> {
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::processthreadsapi::{OpenThread, ResumeThread};
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use winapi::um::winnt::THREAD_SUSPEND_RESUME;

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut has_thread = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while has_thread {
            if entry.th32OwnerProcessID == child.id() {
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let previous_count = unsafe { ResumeThread(thread) };
                let resume_error = if previous_count == u32::MAX {
                    Some(io::Error::last_os_error())
                } else {
                    None
                };
                let close_result = unsafe { CloseHandle(thread) };
                if let Some(error) = resume_error {
                    return Err(error);
                }
                if close_result == 0 {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            has_thread = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        Err(io::Error::other("suspended probe process has no discoverable thread"))
    })();
    unsafe { CloseHandle(snapshot) };
    result
}

#[cfg(test)]
fn wait_for_spawn_failure_control(descendant_pid_file: &Path) -> Result<(), String> {
    let ready_file = descendant_pid_file.with_extension("pid.ready");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready_file.is_file() {
        if Instant::now() >= deadline {
            return Err(format!(
                "descendant ready marker was not written: {}",
                ready_file.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn last_probe_pid_for_test() -> Option<u32> {
    match LAST_PROBE_PID.load(Ordering::Acquire) {
        0 => None,
        pid => Some(pid),
    }
}

#[cfg(test)]
fn defer_probe_child_for_test(child: Child) {
    let children = DEFERRED_PROBE_CHILDREN.get_or_init(|| Mutex::new(Vec::new()));
    let mut children = match children.lock() {
        Ok(children) => children,
        Err(poisoned) => poisoned.into_inner(),
    };
    children.push(child);
}

fn probe_debuggee_perl_with_options(
    binary: &Path,
    probe_budget: Duration,
    mut simulate_wait_error: bool,
    descendant_pid_file: Option<&Path>,
    cleanup_fault: CleanupFault,
) -> Result<DebuggeePerl, ProbeFailure> {
    let fail = |reason: String| ProbeFailure { reason, transient: false };
    // The workspace is explicitly closed after the probe body so recursive
    // removal errors remain observable. The pid-keyed prefix keeps concurrent
    // suites' workspaces distinguishable for hygiene proofs.
    let probe_prefix = format!("perl-lsp-dap-debuggee-probe-{}-", std::process::id());
    let probe_dir = tempfile::Builder::new()
        .prefix(&probe_prefix)
        .tempdir()
        .map_err(|e| fail(format!("cannot create probe dir: {e}")))?;
    let result = (|| -> Result<DebuggeePerl, ProbeFailure> {
        let script = probe_dir.path().join("pipe_probe.pl");
        fs::write(
            &script,
            "use strict;\nuse warnings;\nmy $x = 10;\nmy $y = $x + 5;\nprint \"$y\\n\";\n",
        )
        .map_err(|e| fail(format!("cannot write probe script: {e}")))?;

        let mut command = Command::new(binary);
        command
            .args(["-d", "--"])
            .arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("PERL5LIB")
            .env_remove("PERL5OPT")
            .env("LC_ALL", "C")
            .env("TZ", "UTC");
        if let Some(descendant_pid_file) = descendant_pid_file {
            command.env("PERL_LSP_DAP_TEST_DESCENDANT_PID_FILE", descendant_pid_file);
            command.env(
                "PERL_LSP_DAP_TEST_DESCENDANT_READY_FILE",
                format!("{}.ready", descendant_pid_file.display()),
            );
        }
        // A dedicated process group lets cleanup terminate descendants on Unix;
        // Windows uses a Job Object and taskkill's process-tree fallback. These
        // are the production-owned process-tree boundaries for the probe.
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            // Start suspended so no child code can create a pipe-inheriting
            // descendant before the process is inside the kill-on-close Job.
            command.creation_flags(CREATE_SUSPENDED_FLAG);
        }
        let mut child = command.spawn().map_err(|e| fail(format!("cannot spawn: {e}")))?;
        #[cfg(test)]
        // Record before Windows job assignment so the assignment-failure
        // control can observe and verify cleanup of the suspended child.
        LAST_PROBE_PID.store(child.id(), Ordering::Release);
        #[cfg(windows)]
        let _probe_job = if cleanup_fault.termination_failed() {
            // Leave the owned process tree live for the termination-failure
            // control. This makes the injected failure exercise the same
            // cleanup boundary as production, and the test harness performs
            // explicit cleanup after observing the bounded return.
            None
        } else {
            Some(
                match if cleanup_fault.assignment_failed() {
                    Err(io::Error::other("injected job assignment failure"))
                } else {
                    ProbeJob::assign(&child)
                } {
                    Ok(job) => job,
                    Err(error) => {
                        #[cfg(test)]
                        if cleanup_fault.assignment_failed() {
                            std::thread::sleep(Duration::from_secs(3));
                        }
                        let tree_kill = taskkill_process_tree(child.id());
                        let native_tree_kill = terminate_windows_process_tree(child.id());
                        let kill = child.kill();
                        let wait = wait_for_child_reap(&mut child, CLEANUP_REAP_BUDGET);
                        let mut cleanup = vec![format!(
                            "cannot assign probe process tree to a kill-on-close job: {error}"
                        )];
                        if !tree_kill.as_ref().is_ok_and(|status| status.success()) {
                            cleanup.push(format!(
                                "taskkill process-tree fallback failed: {}",
                                tree_kill.map_or_else(
                                    |error| error,
                                    |status| format!("non-success status: {status}")
                                )
                            ));
                        }
                        if let Err(error) = native_tree_kill {
                            cleanup.push(format!("native process-tree fallback failed: {error}"));
                        }
                        if let Err(error) = kill {
                            cleanup.push(format!("direct child kill failed: {error}"));
                        }
                        if let Err(error) = wait {
                            cleanup.push(format!("bounded child wait/reap failed: {error}"));
                        }
                        return Err(fail(cleanup.join("; ")));
                    }
                },
            )
        };
        #[cfg(windows)]
        if let Err(error) = resume_suspended_probe_process(&child) {
            let cleanup =
                terminate_probe_process_tree(&mut child, descendant_pid_file, cleanup_fault);
            return Err(fail(format!(
                "cannot resume probe process after assigning its ownership boundary: {error}{}",
                cleanup
                    .err()
                    .map_or_else(String::new, |error| format!("; cleanup failed: {error}"))
            )));
        }
        #[cfg(test)]
        if (cleanup_fault.thread_spawn_failed(ProbeThreadSpawnFailure::Writer)
            || cleanup_fault.thread_spawn_failed(ProbeThreadSpawnFailure::StdoutReader)
            || cleanup_fault.thread_spawn_failed(ProbeThreadSpawnFailure::StderrReader))
            && let Some(descendant_pid_file) = descendant_pid_file
            && let Err(error) = wait_for_spawn_failure_control(descendant_pid_file)
        {
            let cleanup =
                terminate_probe_process_tree(&mut child, Some(descendant_pid_file), cleanup_fault);
            return Err(fail(format!(
                "spawn-failure control did not start its descendant: {error}{}",
                cleanup
                    .err()
                    .map_or_else(String::new, |error| format!("; cleanup failed: {error}"))
            )));
        }

        let Some(stdout_pipe) = child.stdout.take() else {
            let cleanup =
                terminate_probe_process_tree(&mut child, descendant_pid_file, cleanup_fault);
            return Err(fail(format!(
                "stdout pipe unavailable{}",
                cleanup
                    .err()
                    .map_or_else(String::new, |error| format!("; cleanup failed: {error}"))
            )));
        };
        let Some(stderr_pipe) = child.stderr.take() else {
            let cleanup =
                terminate_probe_process_tree(&mut child, descendant_pid_file, cleanup_fault);
            return Err(fail(format!(
                "stderr pipe unavailable{}",
                cleanup
                    .err()
                    .map_or_else(String::new, |error| format!("; cleanup failed: {error}"))
            )));
        };

        // Feed the scripted debugger commands through the REAL pipe write end. The
        // payload is four bytes, so the writer cannot block on a full pipe buffer;
        // dropping `stdin` afterwards delivers EOF exactly like an editor closing
        // its side of the session.
        let stdin_pipe = child.stdin.take();
        let writer = match if cleanup_fault.thread_spawn_failed(ProbeThreadSpawnFailure::Writer) {
            Err(io::Error::other("injected probe stdin writer spawn failure"))
        } else {
            std::thread::Builder::new().name("perl-dap-probe-stdin".to_string()).spawn(move || {
                if let Some(mut stdin) = stdin_pipe {
                    let _ = stdin.write_all(b"c\nq\n");
                    let _ = stdin.flush();
                }
            })
        } {
            Ok(writer) => writer,
            Err(error) => {
                let cleanup =
                    terminate_probe_process_tree(&mut child, descendant_pid_file, cleanup_fault);
                return Err(fail(format!(
                    "cannot spawn probe stdin writer: {error}{}",
                    cleanup
                        .err()
                        .map_or_else(String::new, |error| { format!("; cleanup failed: {error}") })
                )));
            }
        };

        let stdout_chunks = match drain_pipe(
            stdout_pipe,
            cleanup_fault.thread_spawn_failed(ProbeThreadSpawnFailure::StdoutReader),
        ) {
            Ok(stdout_chunks) => stdout_chunks,
            Err(error) => {
                let cleanup =
                    terminate_probe_process_tree(&mut child, descendant_pid_file, cleanup_fault);
                let writer_result =
                    writer.join().map_err(|_| "probe stdin writer thread panicked".to_string());
                return Err(fail(format!(
                    "cannot spawn probe stdout reader: {error}{}",
                    cleanup_failure_suffix(cleanup, writer_result, Ok(()), Ok(()))
                )));
            }
        };
        let stderr_chunks = match drain_pipe(
            stderr_pipe,
            cleanup_fault.thread_spawn_failed(ProbeThreadSpawnFailure::StderrReader),
        ) {
            Ok(stderr_chunks) => stderr_chunks,
            Err(error) => {
                let cleanup =
                    terminate_probe_process_tree(&mut child, descendant_pid_file, cleanup_fault);
                let writer_result =
                    writer.join().map_err(|_| "probe stdin writer thread panicked".to_string());
                let stdout_result = join_pipe_reader(stdout_chunks);
                return Err(fail(format!(
                    "cannot spawn probe stderr reader: {error}{}",
                    cleanup_failure_suffix(cleanup, writer_result, stdout_result, Ok(()))
                )));
            }
        };

        // The injected wait error is test-only. Give a controlled descendant a
        // scheduling window to start before exercising that immediate error path;
        // otherwise the test could validate cleanup of a pre-spawn race instead of
        // cleanup of a live process tree holding inherited pipe handles.
        if simulate_wait_error {
            std::thread::sleep(Duration::from_secs(1));
        }

        let deadline = Instant::now() + probe_budget;
        let status = loop {
            let wait_result = if simulate_wait_error {
                simulate_wait_error = false;
                Err(io::Error::other("injected probe wait failure"))
            } else {
                child.try_wait()
            };
            match wait_result {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let tree_result = terminate_probe_process_tree(
                            &mut child,
                            descendant_pid_file,
                            cleanup_fault,
                        );
                        let writer_result = writer
                            .join()
                            .map_err(|_| "probe stdin writer thread panicked".to_string());
                        let stdout_joined = join_pipe_reader(stdout_chunks);
                        let stderr_joined = join_pipe_reader(stderr_chunks);
                        #[cfg(test)]
                        if cleanup_fault.termination_failed() {
                            defer_probe_child_for_test(child);
                        }
                        return Err(ProbeFailure {
                            reason: format!(
                                "no exit within {}s — perl5db cannot bootstrap over piped stdio{}",
                                probe_budget.as_secs(),
                                cleanup_failure_suffix(
                                    tree_result,
                                    writer_result,
                                    stdout_joined,
                                    stderr_joined
                                )
                            ),
                            transient: true,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    // A wait error leaves ownership with this function. Reap the
                    // child before returning so the probe cannot leak a live
                    // process, and join the small stdin writer after the child
                    // closes the pipe. The TempDir guard then removes the script
                    // workspace on this path as well.
                    let tree_result = terminate_probe_process_tree(
                        &mut child,
                        descendant_pid_file,
                        cleanup_fault,
                    );
                    let writer_result =
                        writer.join().map_err(|_| "probe stdin writer thread panicked".to_string());
                    let stdout_joined = join_pipe_reader(stdout_chunks);
                    let stderr_joined = join_pipe_reader(stderr_chunks);
                    let cleanup_suffix = cleanup_failure_suffix(
                        tree_result,
                        writer_result,
                        stdout_joined,
                        stderr_joined,
                    );
                    #[cfg(test)]
                    if cleanup_fault.termination_failed() {
                        defer_probe_child_for_test(child);
                    }
                    return Err(fail(format!("probe wait failed: {e}{cleanup_suffix}")));
                }
            }
        };
        let writer_result =
            writer.join().map_err(|_| "probe stdin writer thread panicked".to_string());

        // A successful parent can still leave descendants holding the inherited
        // pipe write ends. Close the probe's production-owned process-tree
        // boundary before joining readers; otherwise a descendant can make the
        // reader join unbounded even though the direct child exited successfully.
        let tree_result =
            terminate_probe_process_tree(&mut child, descendant_pid_file, cleanup_fault);
        #[cfg(windows)]
        drop(_probe_job);
        let stdout = collect_pipe_output(stdout_chunks);
        let stderr = collect_pipe_output(stderr_chunks);
        if tree_result.is_err() || writer_result.is_err() || stdout.is_err() || stderr.is_err() {
            let stdout_result = stdout;
            let stderr_result = stderr;
            return Err(fail(format!(
                "probe cleanup failed{}",
                cleanup_failure_suffix(tree_result, writer_result, stdout_result, stderr_result)
            )));
        }

        // The child has exited, so its pipe write ends are closing and the reader
        // threads reach EOF almost immediately; the bounded collector exists only
        // so a grandchild inheriting the write end cannot extend the probe past
        // its budget.
        let (stdout, stderr) = match (stdout, stderr) {
            (Ok(stdout), Ok(stderr)) => (stdout, stderr),
            (stdout, stderr) => {
                return Err(fail(format!(
                    "pipe reader cleanup failed (stdout={stdout:?}, stderr={stderr:?})"
                )));
            }
        };

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
    })();

    let close_result = if cleanup_fault.workspace_removal_failed() {
        // Move the directory away from the TempDir's recorded path so the
        // actual `TempDir::close` call takes its filesystem-error branch. The
        // displaced, process-owned artifact is intentionally left for the
        // hygiene test to identify and remove explicitly.
        let mut displaced = probe_dir.path().to_path_buf();
        displaced.set_extension("close-failure");
        match fs::rename(probe_dir.path(), &displaced) {
            Ok(()) => probe_dir.close(),
            Err(error) => Err(error),
        }
    } else {
        probe_dir.close()
    };

    match (result, close_result) {
        (Ok(debuggee), Ok(())) => Ok(debuggee),
        (Ok(_), Err(error)) => Err(fail(format!("probe workspace cleanup failed: {error}"))),
        (Err(failure), Ok(())) => Err(failure),
        (Err(mut failure), Err(error)) => {
            failure.reason.push_str(&format!("; probe workspace cleanup failed: {error}"));
            Err(failure)
        }
    }
}

fn cleanup_failure_suffix<T, U>(
    tree: Result<(), String>,
    writer: Result<(), String>,
    stdout: Result<T, String>,
    stderr: Result<U, String>,
) -> String {
    let failures = [tree.err(), writer.err(), stdout.err(), stderr.err()];
    let details: Vec<_> = failures.into_iter().flatten().collect();
    if details.is_empty() {
        String::new()
    } else {
        format!("; cleanup failed: {}", details.join("; "))
    }
}

const CLEANUP_COMMAND_BUDGET: Duration = Duration::from_millis(500);
const CLEANUP_REAP_BUDGET: Duration = Duration::from_secs(2);

/// Wait for a child without allowing a failed cleanup path to block forever.
fn wait_for_child_reap(child: &mut Child, budget: Duration) -> Result<(), String> {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => return Err(format!("child did not exit within {budget:?}")),
            Err(error) => return Err(format!("child reap check failed: {error}")),
        }
    }
}

/// Run a cleanup helper command with a bounded wait. A hung platform helper is
/// itself a cleanup failure; it must not prevent the probe from returning its
/// diagnostic.
fn run_cleanup_command(
    command: Command,
    budget: Duration,
) -> Result<std::process::ExitStatus, String> {
    run_cleanup_command_inner(command, budget, false).1
}

#[cfg(test)]
pub(crate) fn run_cleanup_command_for_test(
    command: Command,
    budget: Duration,
) -> Result<(u32, Result<std::process::ExitStatus, String>), String> {
    let (pid, result) = run_cleanup_command_inner(command, budget, true);
    match pid {
        Some(pid) => Ok((pid, result)),
        None => Err(format!("cleanup command did not spawn: {result:?}")),
    }
}

fn run_cleanup_command_inner(
    mut command: Command,
    budget: Duration,
    mut inject_wait_error: bool,
) -> (Option<u32>, Result<std::process::ExitStatus, String>) {
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return (None, Err(format!("cannot spawn cleanup command: {error}")));
        }
    };
    let pid = child.id();
    let deadline = Instant::now() + budget;
    loop {
        let wait_result = if inject_wait_error {
            inject_wait_error = false;
            Err(io::Error::other("injected cleanup command wait failure"))
        } else {
            child.try_wait()
        };
        match wait_result {
            Ok(Some(status)) => return (Some(pid), Ok(status)),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let kill_error = child.kill().err().map(|error| error.to_string());
                let reap = wait_for_child_reap(&mut child, CLEANUP_REAP_BUDGET);
                return (
                    Some(pid),
                    Err(format!(
                        "cleanup command timed out after {budget:?} (kill={kill_error:?}, reap={reap:?})"
                    )),
                );
            }
            Err(error) => {
                // A wait error does not release ownership of the helper child. Attempt
                // termination and bounded reap before returning the original failure.
                let kill_error = child.kill().err().map(|error| error.to_string());
                let reap = wait_for_child_reap(&mut child, CLEANUP_REAP_BUDGET);
                return (
                    Some(pid),
                    Err(format!(
                        "cleanup command wait failed: {error} (kill={kill_error:?}, reap={reap:?})"
                    )),
                );
            }
        }
    }
}

#[cfg(unix)]
fn process_group_exists(pid: u32) -> bool {
    let group = format!("-{pid}");
    run_cleanup_command(
        {
            let mut command = Command::new("kill");
            command.args(["-0", "--", &group]);
            command
        },
        CLEANUP_COMMAND_BUDGET,
    )
    .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn signal_process_group(pid: u32, signal: &str) -> Result<(), String> {
    let group = format!("-{pid}");
    let status = run_cleanup_command(
        {
            let mut command = Command::new("kill");
            command.args([signal, "--", &group]);
            command
        },
        CLEANUP_COMMAND_BUDGET,
    )?;
    if status.success() { Ok(()) } else { Err(format!("non-success status: {status}")) }
}

#[cfg(windows)]
fn taskkill_process_tree(pid: u32) -> Result<std::process::ExitStatus, String> {
    run_cleanup_command(
        {
            let mut command = Command::new("taskkill");
            command.args(["/PID", &pid.to_string(), "/T", "/F"]);
            command
        },
        CLEANUP_COMMAND_BUDGET,
    )
}

#[cfg(windows)]
fn terminate_windows_process_tree(root_pid: u32) -> Result<(), String> {
    use std::collections::HashSet;
    use std::mem::size_of;
    use winapi::shared::minwindef::DWORD;
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use winapi::um::winnt::PROCESS_TERMINATE;

    // SAFETY: The API receives only the documented process-snapshot flags and
    // does not dereference any caller-provided pointer.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!("cannot snapshot process tree: {}", io::Error::last_os_error()));
    }

    let result = (|| {
        // SAFETY: PROCESSENTRY32W is a plain Windows data structure whose
        // fields are initialized by Process32FirstW after dwSize is set.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = size_of::<PROCESSENTRY32W>() as DWORD;
        // SAFETY: snapshot is a valid process-snapshot handle and entry points
        // to the initialized structure owned by this stack frame.
        let first = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        if !first {
            return Err(format!("cannot enumerate process tree: {}", io::Error::last_os_error()));
        }

        let mut processes = Vec::new();
        loop {
            processes.push((entry.th32ProcessID, entry.th32ParentProcessID));
            // SAFETY: snapshot remains valid and entry is the same initialized
            // structure used by Process32FirstW.
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }

        let root_present = processes.iter().any(|&(pid, _)| pid == root_pid);
        let mut tree = if root_present { vec![root_pid] } else { Vec::new() };
        let mut frontier = vec![root_pid];
        let mut seen = HashSet::from([root_pid]);
        let mut index = 0;
        while index < frontier.len() {
            let parent = frontier[index];
            for &(pid, parent_pid) in &processes {
                if parent_pid == parent && seen.insert(pid) {
                    frontier.push(pid);
                    tree.push(pid);
                }
            }
            index += 1;
        }

        let mut failures = Vec::new();
        for pid in tree.into_iter().rev() {
            // SAFETY: pid came from the current process snapshot, and the API
            // takes a scalar PID plus documented access flags.
            let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
            if handle.is_null() {
                failures.push(format!("cannot open process {pid}: {}", io::Error::last_os_error()));
                continue;
            }
            // SAFETY: handle was returned by OpenProcess and is not used after
            // this termination call.
            let terminated = unsafe { TerminateProcess(handle, 1) } != 0;
            // SAFETY: handle is the valid process handle opened above.
            let close = unsafe { CloseHandle(handle) } != 0;
            if !terminated {
                failures.push(format!(
                    "cannot terminate process {pid}: {}",
                    io::Error::last_os_error()
                ));
            }
            if !close {
                failures.push(format!(
                    "cannot close process handle {pid}: {}",
                    io::Error::last_os_error()
                ));
            }
        }
        if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
    })();
    // SAFETY: snapshot is the valid handle created at the start of this
    // function and has not been closed on any path inside the closure.
    unsafe { CloseHandle(snapshot) };
    result
}

struct PipeDrain {
    receiver: Receiver<Vec<u8>>,
    thread: JoinHandle<Result<(), String>>,
    cancel: Arc<AtomicBool>,
}

trait ProbePipe: Read + Send + 'static {
    fn has_data(&self) -> Option<bool>;
}

impl ProbePipe for ChildStdout {
    fn has_data(&self) -> Option<bool> {
        probe_pipe_has_data(self)
    }
}

impl ProbePipe for ChildStderr {
    fn has_data(&self) -> Option<bool> {
        probe_pipe_has_data(self)
    }
}

/// Drain `pipe` to EOF on a background thread, forwarding chunks to the
/// returned receiver. The join handle remains owned by the probe so cleanup
/// cannot silently return while a detached reader is still blocked on a pipe.
fn drain_pipe<R>(pipe: R, inject_spawn_failure: bool) -> Result<PipeDrain, String>
where
    R: ProbePipe,
{
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    #[cfg(test)]
    ACTIVE_PROBE_READERS.fetch_add(1, Ordering::AcqRel);
    let thread = if inject_spawn_failure {
        Err(io::Error::other("injected probe pipe reader spawn failure"))
    } else {
        std::thread::Builder::new().name("perl-dap-probe-pipe".to_string()).spawn(
            move || -> Result<(), String> {
                #[cfg(test)]
                struct ReaderActivity;
                #[cfg(test)]
                impl Drop for ReaderActivity {
                    fn drop(&mut self) {
                        ACTIVE_PROBE_READERS.fetch_sub(1, Ordering::AcqRel);
                    }
                }
                #[cfg(test)]
                let _activity = ReaderActivity;
                let mut pipe = pipe;
                let mut buf = [0u8; 4096];
                loop {
                    if thread_cancel.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    match pipe.has_data() {
                        Some(true) => match pipe.read(&mut buf) {
                            Ok(0) => return Ok(()),
                            Err(error) => return Err(format!("pipe read failed: {error}")),
                            Ok(n) => {
                                if tx.send(buf[..n].to_vec()).is_err() {
                                    return Ok(());
                                }
                            }
                        },
                        Some(false) => {
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        None => return Err("pipe readiness probe failed".to_string()),
                    }
                }
            },
        )
    };
    let thread = match thread {
        Ok(thread) => thread,
        Err(error) => {
            #[cfg(test)]
            ACTIVE_PROBE_READERS.fetch_sub(1, Ordering::AcqRel);
            return Err(format!("cannot spawn probe pipe reader: {error}"));
        }
    };
    Ok(PipeDrain { receiver: rx, thread, cancel })
}

/// Collect drained chunks into a string, bounded well inside the probe budget.
fn collect_pipe_output(drain: PipeDrain) -> Result<String, String> {
    let mut bytes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    while let Ok(chunk) =
        drain.receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        bytes.extend_from_slice(&chunk);
    }
    join_pipe_reader(drain).map(|()| String::from_utf8_lossy(&bytes).into_owned())
}

/// Stop and join a pipe reader within a fixed budget. Windows anonymous pipes
/// are polled with `PeekNamedPipe`, so cancellation is observable even when an
/// inherited descendant has kept the write end open. Process-tree termination
/// happens before this helper is called; the cancellation budget is the final
/// guard against an unbounded reader join. Every production reader checks the
/// cancellation flag before polling and before each read, so termination closes
/// the pipe boundary before this bounded join can expire. Test instrumentation
/// verifies that no reader remains active after each cleanup scenario.
fn join_pipe_reader(drain: PipeDrain) -> Result<(), String> {
    drop(drain.receiver);
    drain.cancel.store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !drain.thread.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if drain.thread.is_finished() {
        let reader_result =
            drain.thread.join().map_err(|_| "pipe reader thread panicked".to_string())?;
        reader_result?;
        Ok(())
    } else {
        #[cfg(test)]
        {
            let readers = DEFERRED_PROBE_READERS.get_or_init(|| Mutex::new(Vec::new()));
            let mut readers = match readers.lock() {
                Ok(readers) => readers,
                Err(poisoned) => poisoned.into_inner(),
            };
            readers.push(drain.thread);
        }
        Err("pipe reader thread did not stop within 2s".to_string())
    }
}

#[cfg(unix)]
fn probe_pipe_has_data<T>(pipe: &T) -> Option<bool>
where
    T: std::os::fd::AsRawFd,
{
    use std::os::fd::AsRawFd;

    let mut descriptor = libc::pollfd { fd: pipe.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
    if result < 0 { None } else { Some(result > 0) }
}

#[cfg(all(not(unix), not(windows)))]
fn probe_pipe_has_data<T>(_pipe: &T) -> Option<bool> {
    Some(true)
}

#[cfg(windows)]
fn probe_pipe_has_data<R>(pipe: &R) -> Option<bool>
where
    R: std::os::windows::io::AsRawHandle,
{
    use std::ptr::null_mut;
    let mut available = 0u32;
    let result = unsafe {
        winapi::um::namedpipeapi::PeekNamedPipe(
            pipe.as_raw_handle() as winapi::shared::ntdef::HANDLE,
            null_mut(),
            0,
            null_mut(),
            &mut available,
            null_mut(),
        )
    };
    if result == 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::BrokenPipe { Some(true) } else { None }
    } else {
        Some(available > 0)
    }
}

/// Terminate the probe process tree within its production-owned boundary
/// before joining pipe readers. Unix process groups and Windows Job Objects do
/// not claim descendants that deliberately create a new session or escape a
/// Job Object.
///
/// Killing only the direct child is insufficient: a descendant can inherit a
/// stdout/stderr handle and keep a reader blocked after the parent exits. The
/// probe owns its process-group/Job Object boundary, so timeout and wait-error
/// cleanup closes that owned boundary before joining either reader. Descendants
/// that deliberately create a new session or escape a Job Object are outside
/// this helper's claim.
fn terminate_probe_process_tree(
    child: &mut Child,
    _descendant_pid_file: Option<&Path>,
    cleanup_fault: CleanupFault,
) -> Result<(), String> {
    let pid = child.id();
    let mut failures = Vec::new();
    let child_running = match child.try_wait() {
        Ok(Some(_)) => false,
        Ok(None) => true,
        Err(error) => {
            failures.push(format!("initial child reap check failed: {error}"));
            true
        }
    };
    #[cfg(windows)]
    {
        if child_running && cleanup_fault.termination_failed() {
            failures.push("injected owned process termination failure".to_string());
        } else if child_running {
            let taskkill = taskkill_process_tree(pid);
            if !taskkill.as_ref().is_ok_and(|status| status.success())
                && child.try_wait().ok().flatten().is_none()
            {
                failures.push(format!(
                    "taskkill failed for probe child {pid}: {}",
                    taskkill.map_or_else(
                        |error| error,
                        |status| format!("non-success status: {status}")
                    )
                ));
            }
        }
        if child_running
            && !cleanup_fault.termination_failed()
            && let Err(error) = terminate_windows_process_tree(pid)
        {
            failures.push(format!("native process-tree cleanup failed: {error}"));
        }
    }
    #[cfg(unix)]
    {
        let group_exists = process_group_exists(pid);
        if child_running && !group_exists {
            failures.push(format!("probe process group {pid} was not available"));
        }
        if group_exists && cleanup_fault.termination_failed() {
            failures.push("injected owned process-group termination failure".to_string());
        } else if group_exists {
            let term = signal_process_group(pid, "-TERM");
            if let Err(error) = term {
                failures.push(format!("SIGTERM failed for probe process group {pid}: {error}"));
            }
            let deadline = Instant::now() + Duration::from_millis(500);
            while !matches!(child.try_wait(), Ok(Some(_))) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let survived_term = process_group_exists(pid);
            #[cfg(test)]
            if survived_term {
                LAST_PROBE_USED_SIGKILL.store(true, Ordering::Release);
            }
            let kill = signal_process_group(pid, "-KILL");
            let group_remains = process_group_exists(pid);
            if let Err(error) = kill
                && group_remains
            {
                failures.push(format!("SIGKILL failed for probe process group {pid}: {error}"));
            }
        }
    }
    if child_running && cleanup_fault.termination_failed() {
        failures.push("injected direct child termination failure".to_string());
    } else if child_running && let Err(error) = child.kill() {
        match child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => failures.push(format!("direct child kill failed: {error}")),
            Err(wait_error) => failures.push(format!(
                "direct child kill failed: {error}; follow-up reap check failed: {wait_error}"
            )),
        }
    }
    if child_running && let Err(error) = wait_for_child_reap(child, CLEANUP_REAP_BUDGET) {
        failures.push(format!("bounded child wait/reap failed: {error}"));
    }
    if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
}

/// Explicit test-harness cleanup for the retained-child termination-failure
/// control. Production cleanup never uses this escape hatch: the fault
/// injection intentionally leaves the owned process tree live so the test can
/// prove the bounded error return before cleaning the exact recorded PID.
#[cfg(test)]
pub(crate) fn force_cleanup_probe_process_for_test(pid: u32) -> Result<(), String> {
    let mut failures = Vec::new();
    #[cfg(windows)]
    {
        let taskkill = taskkill_process_tree(pid);
        if !taskkill.as_ref().is_ok_and(|status| status.success())
            && let Err(native) = terminate_windows_process_tree(pid)
        {
            failures.push(format!(
                "test-harness process cleanup failed (taskkill={taskkill:?}; native={native})"
            ));
        }
    }
    #[cfg(unix)]
    {
        if let Err(error) = signal_process_group(pid, "-KILL") {
            failures.push(format!("test-harness process-group cleanup failed: {error}"));
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        failures.push("test-harness process cleanup is unsupported on this platform".to_string());
    }

    let deferred_child = DEFERRED_PROBE_CHILDREN.get().and_then(|children| {
        let mut children = match children.lock() {
            Ok(children) => children,
            Err(poisoned) => poisoned.into_inner(),
        };
        children.iter().position(|child| child.id() == pid).map(|index| children.remove(index))
    });
    if let Some(mut child) = deferred_child {
        if let Err(error) = wait_for_child_reap(&mut child, CLEANUP_REAP_BUDGET) {
            failures.push(format!("test-harness child reap failed: {error}"));
        }
    } else {
        failures.push(format!("no deferred probe child was recorded for PID {pid}"));
    }

    if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
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

fn resolve_debuggee_candidate(path: &Path, search_path: Option<&OsStr>) -> Result<PathBuf, String> {
    if path.is_absolute() || path.components().count() > 1 || path.starts_with(".") {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|error| {
                    format!("cannot resolve candidate relative to the test process: {error}")
                })?
                .join(path)
        };
        return fs::canonicalize(&absolute)
            .map_err(|error| format!("cannot canonicalize candidate {}: {error}", path.display()));
    }

    let search_path = search_path
        .ok_or_else(|| "PATH is unavailable while resolving a Perl candidate".to_string())?;
    for directory in std::env::split_paths(search_path) {
        let mut candidates = vec![directory.join(path)];
        #[cfg(windows)]
        if path.extension().is_none() {
            let pathext = std::env::var_os("PATHEXT")
                .unwrap_or_else(|| std::ffi::OsString::from(".COM;.EXE;.BAT;.CMD"));
            candidates.extend(
                pathext.to_string_lossy().split(';').filter(|extension| !extension.is_empty()).map(
                    |extension| {
                        directory.join(path).with_extension(extension.trim_start_matches('.'))
                    },
                ),
            );
        }
        for candidate in candidates {
            if let Ok(canonical) = fs::canonicalize(&candidate)
                && canonical.is_file()
            {
                return Ok(canonical);
            }
        }
    }
    Err(format!("candidate {} was not found on PATH", path.display()))
}

/// One uncached resolution sweep over every candidate interpreter.
fn resolve_debuggee_perl_uncached() -> DebuggeePerlResolution {
    let explicit = std::env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV).is_some();
    let mut diagnostics = Vec::new();
    let mut transient_failure = false;
    for raw_candidate in debuggee_perl_candidates() {
        let candidate = if explicit {
            match normalize_explicit_debuggee_pin(&raw_candidate) {
                Ok(candidate) => candidate,
                Err(reason) => {
                    diagnostics.push(format!(
                        "{DEBUGGEE_PERL_OVERRIDE_ENV} pin {}: {reason}",
                        raw_candidate.display()
                    ));
                    break;
                }
            }
        } else {
            match resolve_debuggee_candidate(&raw_candidate, std::env::var_os("PATH").as_deref()) {
                Ok(candidate) => candidate,
                Err(reason) => {
                    diagnostics.push(format!("{}: {reason}", raw_candidate.display()));
                    continue;
                }
            }
        };
        match probe_debuggee_perl(&candidate) {
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
#[expect(
    clippy::print_stderr,
    reason = "Typed integration-test skip diagnostics belong on stderr."
)]
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

/// Install an explicitly unbounded startup authority (#8656).
///
/// Workflow scenarios exercise debugging behavior, not the launch-authority
/// contract; without an installed authority every launch is refused.
pub fn install_unbounded_test_authority(adapter: &DebugAdapter) {
    use perl_dap::{
        LaunchAuthority, LaunchAuthoritySource, LaunchAuthorityStartup, UnboundedAcknowledgement,
    };
    let authority = LaunchAuthority::resolve(&LaunchAuthorityStartup {
        trusted_roots: Vec::new(),
        allow_unbounded: Some(UnboundedAcknowledgement::new(
            LaunchAuthoritySource::CommandLine,
            "test: unbounded session",
        )),
    })
    .expect("test authority resolution");
    adapter.set_launch_authority(authority);
}
