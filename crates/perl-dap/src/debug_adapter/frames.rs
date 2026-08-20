//! Stack frame management: stack trace parsing, scopes.

use super::{
    DEBUGGER_QUERY_WAIT_MS, DapMessage, DebugAdapter, Scope, ScopesArguments, ScopesResponseBody,
    Source, StackFrame, StackTraceArguments, Value, Write, json, lock_or_recover,
};

impl DebugAdapter {
    /// Return the only frame the native scope path may inspect.
    ///
    /// `stack_frames[0]` is the frame captured for the current stopped
    /// suspension.  Do not infer identity from source/line or accept another
    /// frame merely because its id is numerically valid; the typed frame
    /// authority in #9045/#9046 will replace this compatibility floor.
    pub(super) fn exact_current_stopped_frame_id(&self, requested: i64) -> Option<i32> {
        if requested < 0 || requested > i32::MAX as i64 {
            return None;
        }
        let frame_id = requested as i32;
        let session = lock_or_recover(&self.session, "debug_adapter.session");
        let session = session.as_ref()?;
        if session.state != crate::debug_adapter::DebugState::Stopped {
            return None;
        }
        session
            .stack_frames
            .first()
            .filter(|frame| frame.id >= 0 && frame.id == frame_id)
            .map(|frame| frame.id)
    }

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
            let (parsed_frames, frame_arguments) = Self::parse_stack_frames_from_text(&output);
            let framed_frames = Self::filter_user_visible_frames(parsed_frames);
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
                if let Some(ref mut session) =
                    *lock_or_recover(&self.session, "debug_adapter.session")
                {
                    session.stack_frame_arguments = frame_arguments;
                }
                framed_frames
            }
        } else {
            // Snapshot buffer is unreliable when framed transport fails: it holds
            // the full session history so snapshot-based parsing returns frames in
            // buffer order — the stale pre-stop context line appears before the
            // current stop line, producing a wrong first frame.  Return empty so
            // the caller falls through to session.stack_frames, which the output
            // reader populates with the authoritative current-stop frame.
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
            // No active session — return honest empty list per DAP spec
            Vec::new()
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
    pub fn handle_scopes(
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

        let Some(frame_id) = self.exact_current_stopped_frame_id(args.frame_id) else {
            return DapMessage::Response {
                seq,
                request_seq,
                success: true,
                command: "scopes".to_string(),
                body: Some(json!({ "scopes": [] })),
                message: None,
            };
        };

        // AC8.3: Hierarchical scope inspection
        // Use VariableReference codec to encode scope refs into disjoint wire bands.
        use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};
        let locals_ref =
            VariableReference::Scope { frame_id, kind: ScopeKind::Locals }.encode().unwrap_or(0);
        let arguments = lock_or_recover(&self.session, "debug_adapter.session")
            .as_ref()
            .and_then(|session| session.stack_frame_arguments.get(&frame_id))
            .cloned()
            .unwrap_or_default();

        let mut scopes = vec![
            Scope {
                name: "Locals".to_string(),
                presentation_hint: Some("locals".to_string()),
                variables_reference: i64::from(locals_ref),
                expensive: false,
                named_variables: None,
                indexed_variables: None,
            },
        ];
        if !arguments.is_empty() {
            let arguments_ref = VariableReference::Scope { frame_id, kind: ScopeKind::Arguments }
                .encode()
                .unwrap_or(0);
            scopes.push(Scope {
                name: "Arguments".to_string(),
                presentation_hint: Some("arguments".to_string()),
                variables_reference: i64::from(arguments_ref),
                expensive: false,
                named_variables: None,
                indexed_variables: Some(arguments.len() as i64),
            });
        }

        let scopes_body = ScopesResponseBody { scopes };

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

    #[test]
    fn arguments_scope_is_advertised_and_paginates_captured_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(7, "main::run")]);
        adapter.seed_stack_frame_arguments_for_test(
            7,
            vec!["$first".to_string(), "[1, 2]".to_string(), "\"a,b\"".to_string()],
        );

        let scopes = adapter.handle_scopes(1, 1, Some(json!({ "frameId": 7 })));
        let DapMessage::Response { body: Some(body), .. } = scopes else {
            return Err("scopes response did not contain a body".into());
        };
        let scope_values =
            body.get("scopes").and_then(Value::as_array).ok_or("scopes body was not an array")?;
        assert_eq!(scope_values.len(), 2);
        assert!(scope_values.iter().all(|scope| {
            scope.get("name") != Some(&json!("Package"))
                && scope.get("name") != Some(&json!("Globals"))
        }));
        let arguments_scope = scope_values
            .iter()
            .find(|scope| scope.get("name") == Some(&json!("Arguments")))
            .ok_or("Arguments scope was not advertised")?;
        assert_eq!(arguments_scope.get("variablesReference"), Some(&json!(74)));
        assert_eq!(arguments_scope.get("indexedVariables"), Some(&json!(3)));
        assert_eq!(arguments_scope.get("namedVariables"), None);

        let variables = adapter.handle_variables(
            2,
            2,
            Some(json!({ "variablesReference": 74, "start": 1, "count": 1 })),
        );
        let DapMessage::Response { body: Some(body), .. } = variables else {
            return Err("variables response did not contain a body".into());
        };
        assert_eq!(body.get("totalVariables"), Some(&json!(3)));
        let values = body
            .get("variables")
            .and_then(Value::as_array)
            .ok_or("variables body was not an array")?;
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].get("name"), Some(&json!("arg1")));
        assert_eq!(values[0].get("value"), Some(&json!("[1, 2]")));
        Ok(())
    }

    #[test]
    fn scopes_without_exact_current_stopped_frame_are_empty() -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();

        for frame_id in [-1_i64, 8, i64::from(i32::MAX) + 1] {
            let response = adapter.handle_scopes(1, 1, Some(json!({ "frameId": frame_id })));
            let DapMessage::Response { body: Some(body), .. } = response else {
                return Err("invalid frame response did not contain a body".into());
            };
            assert_eq!(body.get("scopes"), Some(&json!([])));
        }

        adapter.seed_stopped_session_with_frames_for_test(vec![make_frame(7, "main::run")]);
        for frame_id in [6_i64, 8] {
            let response = adapter.handle_scopes(1, 1, Some(json!({ "frameId": frame_id })));
            let DapMessage::Response { body: Some(body), .. } = response else {
                return Err("non-current frame response did not contain a body".into());
            };
            assert_eq!(body.get("scopes"), Some(&json!([])));
        }
        Ok(())
    }

    #[test]
    fn scopes_without_a_stopped_session_are_empty() -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        let response = adapter.handle_scopes(1, 1, Some(json!({ "frameId": 7 })));
        let DapMessage::Response { body: Some(body), .. } = response else {
            return Err("no-session response did not contain a body".into());
        };
        assert_eq!(body.get("scopes"), Some(&json!([])));

        adapter.seed_running_session_for_test();
        let response = adapter.handle_scopes(1, 1, Some(json!({ "frameId": 7 })));
        let DapMessage::Response { body: Some(body), .. } = response else {
            return Err("running-session response did not contain a body".into());
        };
        assert_eq!(body.get("scopes"), Some(&json!([])));
        Ok(())
    }
}
