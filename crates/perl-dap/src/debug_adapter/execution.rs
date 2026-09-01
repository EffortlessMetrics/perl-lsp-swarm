//! Execution control: continue, next, step in, step out, pause, goto, cancel.

use super::{
    AstBreakpointValidator, BreakpointValidator, ContinueResponseBody, DapMessage, DebugAdapter,
    DebugState, GotoArguments, GotoTarget, GotoTargetsArguments, GotoTargetsResponseBody, Ordering,
    ResumeMode, StepInTarget, StepInTargetsArguments, StepInTargetsResponseBody, Value, Write,
    json, lock_or_recover,
};
use regex::Regex;
use std::sync::LazyLock;

static STEP_IN_TARGET_CALL_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(\w[\w:]*)\s*\(").ok());

impl DebugAdapter {
    /// Synthetic execution-context id exposed for TCP-attach sessions, which
    /// have no locally spawned debuggee process to derive an identity from
    /// (#8294). Mirrored by `handle_threads` so `threads` and every
    /// thread-scoped request agree on the live id.
    pub(super) const TCP_ATTACH_SYNTHETIC_THREAD_ID: i32 = 1;

    /// Build a protocol-safe guidance message for execution-control requests
    /// that arrive when neither a process session nor an attached pid is active.
    fn no_active_debug_session_message(action: &str) -> String {
        format!(
            "Cannot {action} because no Perl debug session is active. \
             Start a launch or attach request first, wait for the debug session \
             to start, then retry {action}."
        )
    }

    /// The live synthetic execution context id, if any (#8294).
    ///
    /// The adapter exposes exactly one synthetic main execution context per
    /// active session: the launch-allocated id, the attached PID identity, or
    /// the TCP-attach constant. Runtime context discovery is not proven, so
    /// there is never more than one live id.
    pub(super) fn live_synthetic_thread_id(&self) -> Option<i32> {
        if let Some(ref session) = *lock_or_recover(&self.session, "debug_adapter.session") {
            Some(session.thread_id)
        } else if let Some(pid) = *lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid")
        {
            Some(Self::i64_to_i32_saturating(i64::from(pid)))
        } else if lock_or_recover(&self.tcp_session, "debug_adapter.tcp_session").is_some() {
            Some(DebugAdapter::TCP_ATTACH_SYNTHETIC_THREAD_ID)
        } else {
            None
        }
    }

    /// Typed rejection for a thread-scoped request that does not name the live
    /// execution context (#8294). Emitted before any backend command is sent
    /// or any session state is mutated.
    fn thread_identity_rejection(
        command: &str,
        seq: i64,
        request_seq: i64,
        detail: String,
    ) -> DapMessage {
        DapMessage::Response {
            seq,
            request_seq,
            success: false,
            command: command.to_string(),
            body: None,
            message: Some(format!(
                "Cannot {command} because the request does not name the live execution \
                 context: {detail}. Re-request `threads` to obtain the current \
                 synthetic execution context id."
            )),
        }
    }

    /// Validate a thread-scoped request's `threadId` against the one live
    /// synthetic execution context (#8294).
    ///
    /// - `Ok(None)` — no execution context is live at all; the caller applies
    ///   its own (pre-#8294) no-session behavior, so no-session responses are
    ///   unchanged.
    /// - `Ok(Some(live))` — the request names the live context id.
    /// - `Err(rejection)` — a context is live but the request supplies a
    ///   missing, negative, out-of-range, unknown, or stale `threadId`. The
    ///   rejection must be returned before any backend command is sent or any
    ///   session state is mutated.
    ///
    /// Validation and the subsequent backend action acquire the session lock
    /// separately, so the validate-then-act pairing rests on requests being
    /// dispatched serially through the single request loop: no replacement
    /// session can interleave. If dispatch ever becomes concurrent, the lock
    /// must span validation and action together.
    pub(super) fn validated_live_thread_id(
        &self,
        command: &str,
        seq: i64,
        request_seq: i64,
        requested: Option<i64>,
    ) -> Result<Option<i32>, DapMessage> {
        let Some(live) = self.live_synthetic_thread_id() else {
            return Ok(None);
        };
        let requested = match requested {
            None => {
                return Err(Self::thread_identity_rejection(
                    command,
                    seq,
                    request_seq,
                    format!("missing `threadId`; the live synthetic execution context is {live}"),
                ));
            }
            Some(id) if id < 0 => {
                return Err(Self::thread_identity_rejection(
                    command,
                    seq,
                    request_seq,
                    format!("negative `threadId` {id} is not a valid execution context"),
                ));
            }
            Some(id) if id > i64::from(i32::MAX) => {
                return Err(Self::thread_identity_rejection(
                    command,
                    seq,
                    request_seq,
                    format!("`threadId` {id} is out of range"),
                ));
            }
            Some(id) => id as i32,
        };
        if requested != live {
            return Err(Self::thread_identity_rejection(
                command,
                seq,
                request_seq,
                format!(
                    "unknown or stale `threadId` {requested}; the live synthetic \
                     execution context is {live}"
                ),
            ));
        }
        Ok(Some(live))
    }

    /// Extract `threadId` from raw request arguments, if present.
    fn requested_thread_id(arguments: Option<&Value>) -> Option<i64> {
        arguments.and_then(|v| v.get("threadId")).and_then(Value::as_i64)
    }

    /// Handle continue request
    pub(super) fn handle_continue(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let thread_arg = Self::requested_thread_id(arguments.as_ref());
        let live_thread =
            match self.validated_live_thread_id("continue", seq, request_seq, thread_arg) {
                Ok(id) => id,
                Err(rejection) => return rejection,
            };
        let mut thread_id = live_thread.unwrap_or(1);

        let has_session = if let Some(ref mut session) =
            *lock_or_recover(&self.session, "debug_adapter.session")
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            let _ = stdin.write_all(b"c\n");
            let _ = stdin.flush();
            session.state = DebugState::Running;
            session.last_resume_mode = ResumeMode::Continue;
            session.variable_cache.clear();
            session.stack_frames.clear();
            thread_id = session.thread_id;
            true
        } else if let Some(pid) = *lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid")
        {
            let _ = self.send_continue_signal(pid);
            thread_id = Self::i64_to_i32_saturating(i64::from(pid));
            true
        } else {
            false
        };

        if !has_session {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "continue".to_string(),
                body: None,
                message: Some(Self::no_active_debug_session_message("continue")),
            };
        }

        // AC9.4: Proper DAP event emission: continued
        self.send_event(
            "continued",
            Some(json!({
                "threadId": thread_id,
                "allThreadsContinued": true
            })),
        );

        let continue_body = ContinueResponseBody { all_threads_continued: true };

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "continue".to_string(),
            body: serde_json::to_value(&continue_body).ok(),
            message: None,
        }
    }

    /// Handle next request
    pub(super) fn handle_next(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        if let Err(rejection) = self.validated_live_thread_id(
            "next",
            seq,
            request_seq,
            Self::requested_thread_id(arguments.as_ref()),
        ) {
            return rejection;
        }
        let has_session = if let Some(ref mut session) =
            *lock_or_recover(&self.session, "debug_adapter.session")
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            let _ = stdin.write_all(b"n\n");
            let _ = stdin.flush();
            session.state = DebugState::Running;
            session.last_resume_mode = ResumeMode::Next;
            session.variable_cache.clear();
            session.stack_frames.clear();
            let t_id = session.thread_id;
            self.send_event(
                "continued",
                Some(json!({
                    "threadId": t_id,
                    "allThreadsContinued": true
                })),
            );
            true
        } else {
            false
        };

        if !has_session {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "next".to_string(),
                body: None,
                message: Some(Self::no_active_debug_session_message("next")),
            };
        }

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "next".to_string(),
            body: None,
            message: None,
        }
    }

    /// Handle stepIn request
    pub(super) fn handle_step_in(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        if let Err(rejection) = self.validated_live_thread_id(
            "stepIn",
            seq,
            request_seq,
            Self::requested_thread_id(arguments.as_ref()),
        ) {
            return rejection;
        }
        // #9069: while targeted stepping is unsupported, a supplied
        // `targetId` must never silently look honored — the whole-`s`
        // operation would step without the requested target. Refuse before
        // any debugger I/O: no `s` write, no resume-state change, no cache
        // clear, no `continued` event.
        if let Some(args) = arguments.as_ref()
            && args.get("targetId").is_some()
        {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "stepIn".to_string(),
                body: None,
                message: Some(
                    "`stepIn targetId` is not supported: targeted stepping is \
                     unavailable, so the request is refused instead of stepping \
                     without the requested target"
                        .to_string(),
                ),
            };
        }
        let has_session = if let Some(ref mut session) =
            *lock_or_recover(&self.session, "debug_adapter.session")
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            let _ = stdin.write_all(b"s\n");
            let _ = stdin.flush();
            session.state = DebugState::Running;
            session.last_resume_mode = ResumeMode::StepIn;
            session.variable_cache.clear();
            session.stack_frames.clear();
            let t_id = session.thread_id;
            self.send_event(
                "continued",
                Some(json!({
                    "threadId": t_id,
                    "allThreadsContinued": true
                })),
            );
            true
        } else {
            false
        };

        if !has_session {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "stepIn".to_string(),
                body: None,
                message: Some(Self::no_active_debug_session_message("stepIn")),
            };
        }

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "stepIn".to_string(),
            body: None,
            message: None,
        }
    }

    /// Handle stepOut request
    pub(super) fn handle_step_out(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        if let Err(rejection) = self.validated_live_thread_id(
            "stepOut",
            seq,
            request_seq,
            Self::requested_thread_id(arguments.as_ref()),
        ) {
            return rejection;
        }
        let has_session = if let Some(ref mut session) =
            *lock_or_recover(&self.session, "debug_adapter.session")
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            let _ = stdin.write_all(b"r\n");
            let _ = stdin.flush();
            session.state = DebugState::Running;
            session.last_resume_mode = ResumeMode::StepOut;
            session.variable_cache.clear();
            session.stack_frames.clear();
            let t_id = session.thread_id;
            self.send_event(
                "continued",
                Some(json!({
                    "threadId": t_id,
                    "allThreadsContinued": true
                })),
            );
            true
        } else {
            false
        };

        if !has_session {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "stepOut".to_string(),
                body: None,
                message: Some(Self::no_active_debug_session_message("stepOut")),
            };
        }

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "stepOut".to_string(),
            body: None,
            message: None,
        }
    }

    /// Handle pause request
    pub(super) fn handle_pause(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        // Identity first (#8294): a request that does not name the live
        // execution context is rejected before any signal is delivered or any
        // session state (variable cache, stack frames) is mutated. When no
        // context is live at all, the validator returns the same actionable
        // no-session guidance as before.
        if let Err(rejection) = self.validated_live_thread_id(
            "pause",
            seq,
            request_seq,
            Self::requested_thread_id(arguments.as_ref()),
        ) {
            return rejection;
        }

        // Check session presence first: "no session" and "signal failed" are distinct errors.
        // "No session" gets the actionable guidance message; "signal failed" gets the signal
        // failure message (the session exists, the interrupt delivery failed).
        let session_present = lock_or_recover(&self.session, "debug_adapter.session").is_some()
            || lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid").is_some();

        if !session_present {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "pause".to_string(),
                body: None,
                message: Some(Self::no_active_debug_session_message("pause")),
            };
        }

        // Deliver pause while the launched-session guard is held when possible:
        // - Windows: write interrupt to debugger stdin under this lock. Do not
        //   call `send_interrupt_signal` here — it re-locks `self.session` and
        //   deadlocks on the non-reentrant mutex.
        // - Unix: SIGINT via `send_interrupt_signal` does not re-lock session,
        //   so keep the guard across the call to avoid a cleanup race between
        //   cache clear and signal delivery.
        let (signal_sent, failure_message) = {
            let mut session_guard = lock_or_recover(&self.session, "debug_adapter.session");
            if let Some(ref mut session) = *session_guard {
                session.variable_cache.clear();
                session.stack_frames.clear();
                #[cfg(windows)]
                {
                    let sent = match session.process.stdin.as_mut() {
                        Some(stdin) => match stdin.write_all(b"\x03\n") {
                            Ok(()) => {
                                let _ = stdin.flush();
                                true
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to send interrupt via stdin: {}. \
                                     Pause delivery failed — session left intact.",
                                    e
                                );
                                false
                            }
                        },
                        None => {
                            tracing::warn!("No stdin handle for launched session pause");
                            false
                        }
                    };
                    (sent, "Failed to pause debugger")
                }
                #[cfg(unix)]
                {
                    let pid = session.process.id();
                    (self.send_interrupt_signal(pid), "Failed to pause debugger")
                }
                #[cfg(not(any(unix, windows)))]
                {
                    (false, "Failed to pause debugger")
                }
            } else {
                drop(session_guard);
                if let Some(pid) =
                    *lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid")
                {
                    #[cfg(windows)]
                    {
                        let _ = pid;
                        (false, "Pause is unsupported for PID-attached sessions on Windows")
                    }
                    #[cfg(unix)]
                    {
                        (self.send_interrupt_signal(pid), "Failed to pause debugger")
                    }
                } else {
                    (false, "Failed to pause debugger")
                }
            }
        };

        DapMessage::Response {
            seq,
            request_seq,
            success: signal_sent,
            command: "pause".to_string(),
            body: None,
            message: if signal_sent { None } else { Some(failure_message.to_string()) },
        }
    }

    pub(super) fn handle_goto_targets(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: GotoTargetsArguments =
            match arguments.and_then(|v| serde_json::from_value(v).ok()) {
                Some(a) => a,
                None => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "gotoTargets".to_string(),
                        body: None,
                        message: Some("Missing or invalid arguments".to_string()),
                    };
                }
            };

        let source_path = match args.source.path {
            Some(ref p) => p.clone(),
            None => {
                let body = GotoTargetsResponseBody { targets: Vec::new() };
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: true,
                    command: "gotoTargets".to_string(),
                    body: serde_json::to_value(&body).ok(),
                    message: None,
                };
            }
        };

        // Validate path against workspace root to prevent path traversal
        let validated_path = match self.validate_source_path(&source_path) {
            Ok(p) => p,
            Err(e) => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "gotoTargets".to_string(),
                    body: None,
                    message: Some(e),
                };
            }
        };

        let content = match std::fs::read_to_string(&validated_path) {
            Ok(c) => c,
            Err(_) => {
                let body = GotoTargetsResponseBody { targets: Vec::new() };
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: true,
                    command: "gotoTargets".to_string(),
                    body: serde_json::to_value(&body).ok(),
                    message: None,
                };
            }
        };

        // Clear stale goto target mappings and build fresh ones
        let mut goto_map = lock_or_recover(&self.goto_targets, "debug_adapter.goto_targets");
        goto_map.clear();
        let mut id_counter =
            lock_or_recover(&self.next_goto_target_id, "debug_adapter.next_goto_target_id");

        let mut targets = Vec::new();
        let search_start = (args.line - 5).max(1);
        let search_end = args.line + 5;

        if let Ok(validator) = AstBreakpointValidator::new(&content) {
            for line in search_start..=search_end {
                if self.cancel_requested.load(Ordering::Acquire) {
                    self.cancel_requested.store(false, Ordering::Release);
                    break;
                }
                if validator.is_executable_line(line) {
                    let id = *id_counter;
                    *id_counter += 1;
                    goto_map.insert(id, (source_path.clone(), line));
                    targets.push(GotoTarget {
                        id,
                        label: format!("Line {}", line),
                        line,
                        column: None,
                        end_line: None,
                        end_column: None,
                    });
                }
            }
        }
        drop(goto_map);
        drop(id_counter);

        let body = GotoTargetsResponseBody { targets };
        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "gotoTargets".to_string(),
            body: serde_json::to_value(&body).ok(),
            message: None,
        }
    }

    pub(super) fn handle_goto(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: GotoArguments = match arguments.and_then(|v| serde_json::from_value(v).ok()) {
            Some(a) => a,
            None => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "goto".to_string(),
                    body: None,
                    message: Some("Missing or invalid arguments".to_string()),
                };
            }
        };

        // Identity before target lookup or any debugger command (#8294).
        if let Err(rejection) =
            self.validated_live_thread_id("goto", seq, request_seq, Some(args.thread_id))
        {
            return rejection;
        }

        // Look up the goto target from our stored mapping
        let target_info = {
            let mut goto_map = lock_or_recover(&self.goto_targets, "debug_adapter.goto_targets");
            goto_map.remove(&args.target_id)
        };
        let (target_path, target_line) = match target_info {
            Some(info) => info,
            None => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "goto".to_string(),
                    body: None,
                    message: Some(format!("Unknown goto target id {}", args.target_id)),
                };
            }
        };

        if let Some(ref mut session) = *lock_or_recover(&self.session, "debug_adapter.session")
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            // Set debugger file context for cross-file goto
            let file_cmd = format!("f {}\n", target_path);
            let _ = stdin.write_all(file_cmd.as_bytes());
            let _ = stdin.flush();
            let goto_cmd = format!("c {}\n", target_line);
            let _ = stdin.write_all(goto_cmd.as_bytes());
            let _ = stdin.flush();
            session.state = DebugState::Running;
            session.last_resume_mode = ResumeMode::Goto;
            session.variable_cache.clear();
            session.stack_frames.clear();
            let t_id = session.thread_id;

            self.send_event(
                "continued",
                Some(json!({
                    "threadId": t_id,
                    "allThreadsContinued": true
                })),
            );

            DapMessage::Response {
                seq,
                request_seq,
                success: true,
                command: "goto".to_string(),
                body: None,
                message: None,
            }
        } else {
            DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "goto".to_string(),
                body: None,
                message: Some("No active debug session".to_string()),
            }
        }
    }

    pub(super) fn handle_step_in_targets(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: StepInTargetsArguments =
            match arguments.and_then(|v| serde_json::from_value(v).ok()) {
                Some(a) => a,
                None => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "stepInTargets".to_string(),
                        body: None,
                        message: Some("Missing or invalid arguments".to_string()),
                    };
                }
            };

        let mut targets = Vec::new();

        // Extract the frame source path while session lock is held, then release.
        let frame_info = {
            let session_guard = lock_or_recover(&self.session, "debug_adapter.session");
            if let Some(ref session) = *session_guard {
                session
                    .stack_frames
                    .iter()
                    .find(|f| i64::from(f.id) == args.frame_id)
                    .map(|frame| (frame.source.path.clone(), frame.line))
            } else {
                None
            }
        };

        if let Some((source_path, frame_line)) = frame_info {
            // Defense-in-depth: validate even internal session paths
            if let Ok(validated_path) = self.validate_source_path(&source_path)
                && let Ok(content) = std::fs::read_to_string(&validated_path)
            {
                let line_idx = frame_line.max(0) as usize;
                if let Some(source_line) = content.lines().nth(line_idx.saturating_sub(1)) {
                    // Find function call patterns
                    if let Some(call_re) = STEP_IN_TARGET_CALL_RE.as_ref() {
                        for (idx, cap) in call_re.captures_iter(source_line).enumerate() {
                            if let Some(name) = cap.get(1) {
                                targets.push(StepInTarget {
                                    id: idx as i64,
                                    label: name.as_str().to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        let body = StepInTargetsResponseBody { targets };
        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "stepInTargets".to_string(),
            body: serde_json::to_value(&body).ok(),
            message: None,
        }
    }

    pub(super) fn handle_cancel(
        &self,
        seq: i64,
        request_seq: i64,
        _arguments: Option<Value>,
    ) -> DapMessage {
        // cancel_requested field will be added by the integration task
        self.cancel_requested.store(true, Ordering::Release);
        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "cancel".to_string(),
            body: None,
            message: None,
        }
    }

    pub(super) fn handle_restart_frame(
        &self,
        seq: i64,
        request_seq: i64,
        _arguments: Option<Value>,
    ) -> DapMessage {
        DapMessage::Response {
            seq,
            request_seq,
            success: false,
            command: "restartFrame".to_string(),
            body: None,
            message: Some(
                "Perl does not support restarting execution from a specific stack frame"
                    .to_string(),
            ),
        }
    }

    pub(super) fn handle_terminate_threads(
        &self,
        seq: i64,
        request_seq: i64,
        _arguments: Option<Value>,
    ) -> DapMessage {
        DapMessage::Response {
            seq,
            request_seq,
            success: false,
            command: "terminateThreads".to_string(),
            body: None,
            message: Some(
                "Perl threading model does not support targeted thread termination from the debugger"
                    .to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod thread_identity_tests {
    // Test assertions favor `unwrap()`/`panic!` over propagating errors;
    // the workspace-wide deny is a production-code rule.
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::super::session::DebugSession;
    use super::super::variable_cache::VariableCache;
    use super::*;
    use std::collections::HashMap;
    use std::sync::mpsc::sync_channel;

    fn replacement_session(thread_id: i32) -> DebugSession {
        let child =
            DebugAdapter::spawn_noop_child_for_test().unwrap_or_else(|_| panic!("noop child"));
        DebugSession {
            process: child,
            state: DebugState::Stopped,
            stack_frames: Vec::new(),
            stack_frame_arguments: HashMap::new(),
            variable_cache: VariableCache::default(),
            thread_id,
            last_resume_mode: ResumeMode::Unknown,
            stopped_generation: 0,
        }
    }

    fn response_of(message: DapMessage, command: &str) -> (bool, String) {
        match message {
            DapMessage::Response { success, command: actual, message, .. } => {
                assert_eq!(actual, command);
                (success, message.unwrap_or_default())
            }
            other => panic!("expected {command} response, got {other:?}"),
        }
    }

    /// #8294: with a live session, a thread-scoped request without `threadId`
    /// is rejected before any backend command is sent, and the session's
    /// stopped state, stack frames, and variable cache are untouched.
    #[test]
    fn missing_thread_id_is_rejected_without_state_mutation() {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test().unwrap();
        adapter.inject_stack_frames_for_test(vec![crate::types::StackFrame::new(
            7,
            "held_frame".to_string(),
            crate::types::Source::new("/held/file.pl"),
            5,
        )]);
        if let Some(ref mut session) = *lock_or_recover(&adapter.session, "t.missing") {
            session.state = DebugState::Stopped;
        }

        let (success, message) = response_of(adapter.handle_continue(1, 1, None), "continue");
        assert!(!success, "missing threadId must be rejected");
        assert!(message.contains("missing `threadId`"), "got: {message}");

        let session = lock_or_recover(&adapter.session, "t.missing.after");
        let session = session.as_ref().unwrap();
        assert_eq!(
            session.state,
            DebugState::Stopped,
            "rejected request must not resume the session"
        );
        assert_eq!(session.stack_frames.len(), 1, "rejected request must not clear stack frames");
    }

    /// #8294: unknown and negative ids are identity rejections, distinct from
    /// the no-session guidance.
    #[test]
    fn unknown_and_negative_thread_ids_are_rejected() {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test().unwrap();

        let (success, message) =
            response_of(adapter.handle_next(1, 1, Some(json!({"threadId": 99}))), "next");
        assert!(!success);
        assert!(message.contains("unknown or stale `threadId` 99"), "got: {message}");

        let (success, message) =
            response_of(adapter.handle_step_in(1, 1, Some(json!({"threadId": -5}))), "stepIn");
        assert!(!success);
        assert!(message.contains("negative `threadId` -5"), "got: {message}");

        // No-session path keeps the pre-existing guidance, not an identity error.
        let bare = DebugAdapter::new();
        let (success, message) =
            response_of(bare.handle_step_out(1, 1, Some(json!({"threadId": 99}))), "stepOut");
        assert!(!success);
        assert!(
            message.contains("no Perl debug session is active"),
            "no-session guidance must win when nothing is live: {message}"
        );
    }

    /// #8294: the live context id is accepted and resumes the session.
    #[test]
    fn valid_live_thread_id_is_accepted() {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test().unwrap();

        let (success, message) =
            response_of(adapter.handle_next(1, 1, Some(json!({"threadId": 1}))), "next");
        assert!(success, "live id must be accepted: {message}");
        let session = lock_or_recover(&adapter.session, "t.valid");
        assert_eq!(session.as_ref().unwrap().state, DebugState::Running);
    }

    /// #8294: a replacement session allocates a fresh context id, so the
    /// previous id becomes stale and must be rejected while the new id works.
    #[test]
    fn stale_thread_id_after_replacement_session_is_rejected() {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test().unwrap();
        // Simulate a replacement launch allocating the next context id.
        *lock_or_recover(&adapter.session, "t.stale") = Some(replacement_session(2));

        let (success, message) =
            response_of(adapter.handle_continue(1, 1, Some(json!({"threadId": 1}))), "continue");
        assert!(!success, "stale id must be rejected after replacement");
        assert!(message.contains("unknown or stale `threadId` 1"), "got: {message}");

        let (success, message) =
            response_of(adapter.handle_continue(1, 1, Some(json!({"threadId": 2}))), "continue");
        assert!(success, "replacement id must be accepted: {message}");
    }

    /// #8294: the attached-PID path exposes the PID as its synthetic context
    /// id; `threads` agrees with the validated live id and foreign ids are
    /// rejected before any signal path is reached.
    #[test]
    fn attached_pid_context_identity_is_strict() {
        let adapter = DebugAdapter::new();
        adapter.seed_attached_pid_for_test(4321);

        let response = adapter.handle_threads(1, 1);
        match response {
            DapMessage::Response { success: true, body: Some(body), .. } => {
                assert_eq!(body["threads"][0]["id"], 4321);
            }
            other => panic!("expected threads response, got {other:?}"),
        }

        assert_eq!(
            adapter.validated_live_thread_id("pause", 1, 1, Some(4321)).unwrap(),
            Some(4321),
            "attached pid identity must validate"
        );
        let rejection = adapter
            .validated_live_thread_id("pause", 1, 1, Some(9999))
            .err()
            .unwrap_or_else(|| panic!("foreign id must be rejected while a pid context is live"));
        match rejection {
            DapMessage::Response { success: false, message: Some(m), .. } => {
                assert!(m.contains("unknown or stale `threadId` 9999"), "got: {m}");
            }
            other => panic!("expected typed rejection, got {other:?}"),
        }
    }

    /// #8294: the TCP-attach path exposes the same constant context id from
    /// `threads` and the request validator.
    #[test]
    fn tcp_context_identity_matches_threads_response() {
        let adapter = DebugAdapter::new();
        {
            let mut guard = lock_or_recover(&adapter.tcp_session, "t.tcp");
            *guard = Some(crate::tcp_attach::TcpAttachSession::new());
        }

        let response = adapter.handle_threads(1, 1);
        match response {
            DapMessage::Response { success: true, body: Some(body), .. } => {
                assert_eq!(body["threads"][0]["id"], DebugAdapter::TCP_ATTACH_SYNTHETIC_THREAD_ID);
            }
            other => panic!("expected threads response, got {other:?}"),
        }
        assert_eq!(
            adapter
                .validated_live_thread_id(
                    "stackTrace",
                    1,
                    1,
                    Some(i64::from(DebugAdapter::TCP_ATTACH_SYNTHETIC_THREAD_ID))
                )
                .unwrap(),
            Some(DebugAdapter::TCP_ATTACH_SYNTHETIC_THREAD_ID)
        );
        assert!(
            adapter.validated_live_thread_id("stackTrace", 1, 1, Some(2)).is_err(),
            "foreign id must be rejected on the TCP path"
        );
    }

    /// #8294: emitted continued events carry the same live context id the
    /// validator accepts, so clients cannot observe a divergent identity.
    #[test]
    fn continued_event_carries_live_context_id() {
        let mut adapter = DebugAdapter::new();
        let (tx, rx) = sync_channel(8);
        adapter.set_event_sender(tx);
        adapter.seed_session_for_test().unwrap();

        let (success, _) =
            response_of(adapter.handle_continue(1, 1, Some(json!({"threadId": 1}))), "continue");
        assert!(success);
        match rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap() {
            DapMessage::Event { event, body: Some(body), .. } => {
                assert_eq!(event, "continued");
                assert_eq!(body["threadId"], 1, "event id must equal the live context id");
            }
            other => panic!("expected continued event, got {other:?}"),
        }
    }

    /// #8294: initialize keeps `supportsSingleThreadExecutionRequests` false
    /// and `terminateThreads` unadvertised.
    #[test]
    fn initialize_capabilities_stay_identity_honest() {
        let adapter = DebugAdapter::new();
        let response = adapter.handle_initialize(1, 1, None);
        match response {
            DapMessage::Response { success: true, body: Some(body), .. } => {
                assert_eq!(
                    body["supportsSingleThreadExecutionRequests"], false,
                    "single-context execution must never be advertised"
                );
                let terminate_threads = body
                    .get("supportsTerminateThreadsRequest")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                assert!(!terminate_threads, "terminateThreads must stay unadvertised");
            }
            other => panic!("expected initialize response, got {other:?}"),
        }
    }
}
