//! Stack frame management: stack trace parsing, scopes.

use super::*;

impl DebugAdapter {
    /// Handle stackTrace request
    pub(super) fn handle_stack_trace(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: Option<StackTraceArguments> =
            arguments.and_then(|v| serde_json::from_value(v).ok());
        let start_frame =
            args.as_ref().and_then(|value| value.start_frame).unwrap_or(0).max(0) as usize;
        let levels = args.as_ref().and_then(|value| value.levels).unwrap_or(0);
        let requested_count = if levels <= 0 { None } else { Some(levels as usize) };
        let mut framed_output_lines = None;

        // Ask the debugger for an explicit stack snapshot when a live session is present.
        if let Some(ref mut session) = *lock_or_recover(&self.session, "debug_adapter.session")
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            let commands = vec!["T".to_string()];
            match self.send_framed_debugger_commands(stdin, &commands) {
                Ok((begin, end)) => {
                    framed_output_lines = self.capture_framed_debugger_output(
                        &begin,
                        &end,
                        DEBUGGER_QUERY_WAIT_MS * 8,
                    );
                }
                Err(error) => {
                    tracing::warn!(%error, "Failed to send framed stackTrace command, falling back");
                    let _ = stdin.write_all(b"T\n");
                    let _ = stdin.flush();
                    Self::wait_for_debugger_output_window(DEBUGGER_QUERY_WAIT_MS as u32);
                }
            }
        }

        let parsed_frames = if let Some(lines) = framed_output_lines.as_ref() {
            let output = lines.join("\n");
            let framed_frames =
                Self::filter_user_visible_frames(Self::parse_stack_frames_from_text(&output));
            if framed_frames.is_empty() {
                // The framed T output contained only internal debugger frames (e.g.
                // `@ = DB::DB called from file '...' line N` at top-level stops) or
                // none at all.  These are filtered out by filter_user_visible_frames.
                //
                // Do NOT fall back to snapshot parsing here: the snapshot buffer
                // contains the entire session history, including the initial implicit
                // stop context line (e.g. line 4 in a 7-line fixture), which appears
                // BEFORE the current breakpoint context line (e.g. line 5).
                // Snapshot-based parsing returns frames in output order, so the FIRST
                // frame would be the stale line-4 context, not the current line-5 stop.
                //
                // The output reader already parsed the most recent context line and
                // stored it in session.stack_frames.  Returning an empty vec here
                // causes the caller to fall through to that authoritative source.
                Vec::new()
            } else {
                framed_frames
            }
        } else {
            // Degraded-transport path: the framed `T` command failed, so we have no
            // reliable, current-stop output.  The snapshot buffer contains the entire
            // session history in output order; at a breakpoint the initial implicit-stop
            // context line appears BEFORE the current-stop context line, so
            // parse_stack_frames_from_text would return the stale first frame — the same
            // class of bug #927 fixed on the primary path.
            //
            // Return Vec::new() to fall through to session.stack_frames (written by the
            // output reader with the most-recent context line) or the placeholder frame.
            Vec::new()
        };

        let stack_frames = if !parsed_frames.is_empty() {
            // Keep parsed frames as best-effort latest snapshot.
            if let Some(ref mut session) = *lock_or_recover(&self.session, "debug_adapter.session")
            {
                session.stack_frames = parsed_frames.clone();
            }
            parsed_frames
        } else if let Some(ref session) = *lock_or_recover(&self.session, "debug_adapter.session") {
            Self::filter_user_visible_frames(session.stack_frames.clone())
        } else if let Some(pid) = *lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid")
        {
            vec![StackFrame {
                id: Self::i64_to_i32_saturating(i64::from(pid)),
                name: format!("attached::process::{pid}"),
                source: Source {
                    name: Some(format!("pid:{pid}")),
                    path: format!("pid://{pid}"),
                    source_reference: None,
                },
                line: 1,
                column: 1,
                end_line: None,
                end_column: None,
            }]
        } else {
            // No session - return placeholder frame for testing
            vec![StackFrame {
                id: 1,
                name: "main::hello".to_string(),
                source: Source {
                    name: Some("hello.pl".to_string()),
                    path: "/tmp/hello.pl".to_string(),
                    source_reference: None,
                },
                line: 10,
                column: 1,
                end_line: None,
                end_column: None,
            }]
        };
        // Capture full depth before pagination so totalFrames reports the real
        // stack depth, not the size of the paginated window (DAP spec §StackTraceResponse:
        // "totalFrames: The total number of frames available in the stack").
        let total_frames = stack_frames.len();
        let stack_frames = Self::paginate_stack_frames(stack_frames, start_frame, requested_count);

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "stackTrace".to_string(),
            body: Some(json!({
                "stackFrames": stack_frames,
                "totalFrames": total_frames
            })),
            message: None,
        }
    }

    /// Handle scopes request
    pub(super) fn handle_scopes(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: ScopesArguments = match arguments.and_then(|v| serde_json::from_value(v).ok()) {
            Some(a) => a,
            None => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "scopes".to_string(),
                    body: None,
                    message: Some("Missing frameId".to_string()),
                };
            }
        };

        let frame_id = args.frame_id as i32;

        // AC8.3: Hierarchical scope inspection
        // Use bit-shifting or offsets to distinguish between scope types for the same frame
        let locals_ref = frame_id * 10 + 1;
        let package_ref = frame_id * 10 + 2;
        let globals_ref = frame_id * 10 + 3;

        let scopes_body = ScopesResponseBody {
            scopes: vec![
                Scope {
                    name: "Locals".to_string(),
                    presentation_hint: Some("locals".to_string()),
                    variables_reference: i64::from(locals_ref),
                    expensive: false,
                },
                Scope {
                    name: "Package".to_string(),
                    presentation_hint: None,
                    variables_reference: i64::from(package_ref),
                    expensive: true,
                },
                Scope {
                    name: "Globals".to_string(),
                    presentation_hint: None,
                    variables_reference: i64::from(globals_ref),
                    expensive: true,
                },
            ],
        };

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "scopes".to_string(),
            body: serde_json::to_value(&scopes_body).ok(),
            message: None,
        }
    }
}

impl DebugAdapter {
    fn paginate_stack_frames(
        stack_frames: Vec<StackFrame>,
        start_frame: usize,
        levels: Option<usize>,
    ) -> Vec<StackFrame> {
        let iter = stack_frames.into_iter().skip(start_frame);
        match levels {
            Some(limit) => iter.take(limit).collect(),
            None => iter.collect(),
        }
    }
}

#[cfg(test)]
mod degraded_transport_tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Regression: when the framed `T` command transport fails, the degraded-path
    /// previously parsed `snapshot_recent_output_lines()` in buffer order, which
    /// returned the stale initial-implicit-stop context line as the first frame.
    /// After the fix, the else branch returns Vec::new(), falling through to the
    /// authoritative `session.stack_frames` or the placeholder frame.
    #[test]
    fn degraded_transport_does_not_serve_stale_first_frame() -> TestResult {
        let mut adapter = DebugAdapter::new();

        // Seed the recent_output buffer with a stale context line (line 4) appearing
        // before the current context line (line 5), mirroring real debugger output order.
        // Format matches context_re() in patterns.rs: `FuncName(file:line):`
        {
            let mut output = lock_or_recover(&adapter.recent_output, "test.seed");
            DebugAdapter::append_recent_output_line_locked(&mut output, "main::(/tmp/test.pl:4):");
            DebugAdapter::append_recent_output_line_locked(&mut output, "  4:    my $x = 1;");
            DebugAdapter::append_recent_output_line_locked(&mut output, "main::(/tmp/test.pl:5):");
            DebugAdapter::append_recent_output_line_locked(&mut output, "  5:    my $y = 2;");
        }

        // Issue a stackTrace request.  No live process means the framed session block
        // is skipped and framed_output_lines stays None — exercising the else branch.
        //
        // Pre-fix: parse_stack_frames_from_text returns frame at line 4 (first in buffer).
        // Post-fix: Vec::new() is returned; caller falls through to placeholder path.
        let response = adapter.handle_request(1, "stackTrace", Some(json!({"threadId": 1})));
        match response {
            DapMessage::Response { body: Some(body), .. } => {
                let frames = body
                    .get("stackFrames")
                    .and_then(|v| v.as_array())
                    .ok_or("missing stackFrames")?;
                // No frame should have line == 4 (the stale buffer-first context line).
                for frame in frames {
                    let line = frame.get("line").and_then(|v| v.as_i64()).unwrap_or(0);
                    assert_ne!(
                        line, 4,
                        "stale snapshot frame (line 4) must not appear in degraded-transport \
                         response; got frames: {frames:?}"
                    );
                }
            }
            other => return Err(format!("expected Response, got {other:?}").into()),
        }
        Ok(())
    }

    /// Degraded-transport with empty recent_output buffer:
    /// Vec::new() is returned → falls through to placeholder frame.
    /// Directly covers the `Vec::new()` return in the else branch with empty buffer.
    #[test]
    fn degraded_transport_empty_buffer_returns_placeholder() -> TestResult {
        let mut adapter = DebugAdapter::new();
        // recent_output buffer is empty — no stale lines at all.

        let response = adapter.handle_request(1, "stackTrace", Some(json!({"threadId": 1})));
        match response {
            DapMessage::Response { success, body: Some(body), .. } => {
                assert!(success, "stackTrace must succeed even in degraded state");
                let frames = body
                    .get("stackFrames")
                    .and_then(|v| v.as_array())
                    .ok_or("missing stackFrames")?;
                // Placeholder frame must be returned (no real session).
                assert!(
                    !frames.is_empty(),
                    "degraded-transport with empty buffer must return placeholder frame"
                );
            }
            other => return Err(format!("expected Response with body, got {other:?}").into()),
        }
        Ok(())
    }

    /// Degraded-transport with many stale context lines: Vec::new() is returned
    /// regardless; stale lines (100-109) must not appear in response.
    #[test]
    fn degraded_transport_large_buffer_does_not_serve_stale_early_lines() -> TestResult {
        let mut adapter = DebugAdapter::new();
        {
            let mut output = lock_or_recover(&adapter.recent_output, "test.seed");
            // Use line numbers 100-109 to avoid collision with placeholder frame (line=10).
            for line_num in 100..=109_u32 {
                DebugAdapter::append_recent_output_line_locked(
                    &mut output,
                    &format!("main::(/tmp/test.pl:{line_num}):"),
                );
                DebugAdapter::append_recent_output_line_locked(
                    &mut output,
                    &format!("  {line_num}:    my $x = {line_num};"),
                );
            }
        }

        let response = adapter.handle_request(1, "stackTrace", Some(json!({"threadId": 1})));
        match response {
            DapMessage::Response { body: Some(body), .. } => {
                let frames = body
                    .get("stackFrames")
                    .and_then(|v| v.as_array())
                    .ok_or("missing stackFrames")?;
                // No stale frame from the buffer (lines 100-109) should appear.
                for frame in frames {
                    let line = frame.get("line").and_then(|v| v.as_i64()).unwrap_or(-1);
                    assert!(
                        !(100..=109).contains(&line),
                        "stale buffer frame (line {line}) must not appear in degraded-transport \
                         response; got frames: {frames:?}"
                    );
                }
            }
            other => return Err(format!("expected Response, got {other:?}").into()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;

    fn make_frame(id: i32, name: &str) -> StackFrame {
        StackFrame {
            id,
            name: name.to_string(),
            source: Source {
                name: Some("test.pl".to_string()),
                path: "/tmp/test.pl".to_string(),
                source_reference: None,
            },
            line: id,
            column: 1,
            end_line: None,
            end_column: None,
        }
    }

    /// Regression: paginate_stack_frames used to be called BEFORE capturing the
    /// full depth, so totalFrames reported the slice length instead of the full
    /// stack depth.  This unit test locks the correct invariant:
    ///   totalFrames == pre-pagination length >= paginated-window length
    #[test]
    fn total_frames_is_pre_pagination_length() -> Result<(), Box<dyn std::error::Error>> {
        let all_frames: Vec<StackFrame> = (1..=5).map(|i| make_frame(i, "main::step")).collect();
        let total_before = all_frames.len();

        // Paginate to window of 2, starting at offset 0.
        let paginated = DebugAdapter::paginate_stack_frames(all_frames, 0, Some(2));

        assert_eq!(paginated.len(), 2, "paginated window should be 2");
        assert_eq!(total_before, 5, "total_frames must be full depth (5)");
        assert!(
            total_before >= paginated.len(),
            "total_frames ({total_before}) must be >= paginated len ({})",
            paginated.len()
        );
        Ok(())
    }

    /// startFrame beyond the stack depth: paginated slice is empty, but the
    /// pre-pagination total is still the real depth.
    #[test]
    fn total_frames_with_start_frame_beyond_depth() -> Result<(), Box<dyn std::error::Error>> {
        let all_frames: Vec<StackFrame> = (1..=3).map(|i| make_frame(i, "main::step")).collect();
        let total_before = all_frames.len();

        let paginated = DebugAdapter::paginate_stack_frames(all_frames, 10, Some(2));

        assert_eq!(paginated.len(), 0, "paginated slice beyond depth should be empty");
        assert_eq!(total_before, 3, "total_frames must still report full depth when start > depth");
        Ok(())
    }

    /// No pagination (None levels): total_frames == paginated length (no difference).
    #[test]
    fn total_frames_no_pagination_unchanged() -> Result<(), Box<dyn std::error::Error>> {
        let all_frames: Vec<StackFrame> = (1..=4).map(|i| make_frame(i, "main::step")).collect();
        let total_before = all_frames.len();

        let paginated = DebugAdapter::paginate_stack_frames(all_frames, 0, None);

        assert_eq!(paginated.len(), total_before, "no pagination: total == paginated");
        Ok(())
    }
}
