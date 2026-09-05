use super::{DapMessage, DebugAdapter, HashMap, Value, Write, json};

impl DebugAdapter {
    /// Handle setBreakpoints request
    ///
    /// #9578: entries carrying floored optional fields (`condition`,
    /// `hitCondition`, `logMessage`) are rejected per item with a deterministic
    /// field-specific message naming the floored capability and its re-enable
    /// gate. A rejected entry is never silently stripped into an unconditional
    /// breakpoint, never converted into an ordinary stopping breakpoint, and
    /// never counted or simulated locally. Plain entries keep their
    /// independent replace-semantics contract; the response preserves one
    /// breakpoint per input in request order. A request whose entries are all
    /// rejected performs no store replacement, no session synchronization, and
    /// no other state mutation.
    pub(in crate::debug_adapter) fn handle_set_breakpoints(
        &mut self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let Some(args_value) = arguments else {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setBreakpoints".to_string(),
                body: None,
                message: Some("Missing arguments".to_string()),
            };
        };

        let parsed: crate::protocol::SetBreakpointsArguments =
            match serde_json::from_value(args_value) {
                Ok(a) => a,
                Err(e) => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "setBreakpoints".to_string(),
                        body: None,
                        message: Some(format!("Invalid arguments: {}", e)),
                    };
                }
            };

        // #9578: partition floored optional-field entries out of the request
        // before any store or session work. Each rejection is gated on its own
        // authority, so promotion flips advertisement and admission together
        // and one capability's receipt can never widen another. Unsupported
        // combinations reject on every still-floored offending field.
        let input_len = parsed.breakpoints.as_ref().map_or(0, Vec::len);
        let mut rejected: Vec<(usize, i64, Option<i64>, String)> = Vec::new();
        let mut plain_entries: Vec<crate::protocol::SourceBreakpoint> = Vec::new();
        let mut plain_slots: Vec<(usize, i64)> = Vec::new();
        for (index, entry) in parsed.breakpoints.iter().flatten().enumerate() {
            let mut reasons: Vec<&'static str> = Vec::new();
            if entry.condition.is_some()
                && !crate::backend::capabilities::advertises_conditional_breakpoints()
            {
                reasons.push(crate::backend::capabilities::CONDITION_UNSUPPORTED_MESSAGE);
            }
            if entry.hit_condition.is_some()
                && !crate::backend::capabilities::advertises_hit_conditional_breakpoints()
            {
                reasons.push(crate::backend::capabilities::HIT_CONDITION_UNSUPPORTED_MESSAGE);
            }
            if entry.log_message.is_some() && !crate::backend::capabilities::advertises_log_points()
            {
                reasons.push(crate::backend::capabilities::LOG_MESSAGE_UNSUPPORTED_MESSAGE);
            }
            if reasons.is_empty() {
                plain_slots.push((index, entry.line));
                plain_entries.push(entry.clone());
            } else {
                rejected.push((index, entry.line, entry.column, reasons.join(" ")));
            }
        }

        // A request whose entries are all rejected must not replace desired
        // state, synchronize the session, or clear unrelated current
        // breakpoints; the store and engine stay untouched.
        let all_entries_rejected = input_len > 0 && plain_entries.is_empty();

        let args = if input_len > 0 {
            crate::protocol::SetBreakpointsArguments {
                source: parsed.source,
                breakpoints: Some(plain_entries),
                source_modified: parsed.source_modified,
            }
        } else {
            parsed
        };

        // Snapshot old breakpoints for this file before replacing them,
        // so we can clear only per-file breakpoints instead of global `B *`.
        let old_breakpoints = if all_entries_rejected {
            Vec::new()
        } else if let Some(ref source_path) = args.source.path {
            self.breakpoints.get_breakpoints(source_path)
        } else {
            Vec::new()
        };

        // AC7: AST-based breakpoint validation via BreakpointStore
        let verified_breakpoints =
            if all_entries_rejected { Vec::new() } else { self.breakpoints.set_breakpoints(&args) };
        let new_breakpoint_records = if all_entries_rejected {
            Vec::new()
        } else if let Some(ref source_path) = args.source.path {
            self.breakpoints.get_breakpoints(source_path)
        } else {
            Vec::new()
        };
        let condition_by_id: HashMap<i64, Option<String>> = new_breakpoint_records
            .into_iter()
            .map(|record| (record.id, record.condition))
            .collect();

        // If a session is active, also sync the breakpoints to the Perl debugger
        if !all_entries_rejected
            && let Ok(mut guard) = self.session.lock()
            && let Some(ref mut session) = *guard
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            let mut command_batch = String::new();

            // Clear only the old breakpoints for this specific file
            for old_bp in &old_breakpoints {
                if old_bp.verified {
                    command_batch.push_str(&format!("B {}\n", old_bp.line));
                }
            }

            // Set new breakpoints that were successfully verified
            for bp in &verified_breakpoints {
                if bp.verified {
                    // Retrieve original condition (if present) from records produced by this call.
                    let cmd = if let Some(Some(cond)) = condition_by_id.get(&bp.id) {
                        format!("b {} {}\n", bp.line, cond)
                    } else {
                        format!("b {}\n", bp.line)
                    };
                    command_batch.push_str(&cmd);
                }
            }

            if !command_batch.is_empty() {
                let _ = stdin.write_all(command_batch.as_bytes());
                let _ = stdin.flush();
            }
        }

        // Keep function breakpoints active after line-breakpoint synchronization.
        if !all_entries_rejected {
            self.apply_stored_function_breakpoints();
        }

        // One response breakpoint per input, in request order (#9578): a
        // rejected optional-field entry occupies its own slot as unverified
        // with the exact per-field reason instead of shifting every later
        // entry onto the wrong requested line. A plain slot whose store result
        // never materializes (no source path to validate against) still
        // occupies its position as unverified, so the response keeps exactly
        // one entry per input.
        let mut plain_results = verified_breakpoints.into_iter();
        let mut plain_slots = plain_slots.into_iter().peekable();
        let mut rejected_results = rejected.into_iter().peekable();
        let mut body_breakpoints: Vec<Value> = Vec::with_capacity(input_len);
        for index in 0..input_len {
            if let Some((_, line, column, message)) =
                rejected_results.next_if(|(i, ..)| *i == index)
            {
                let mut entry = json!({
                    "verified": false,
                    "line": line,
                    "message": message,
                });
                if let Some(column) = column {
                    entry["column"] = json!(column);
                }
                body_breakpoints.push(entry);
            } else if let Some(bp) = plain_results.next() {
                body_breakpoints.push(
                    serde_json::to_value(bp).unwrap_or_else(|_| json!({ "verified": false })),
                );
            } else if let Some((_, line)) = plain_slots.next_if(|(i, _)| *i == index) {
                body_breakpoints.push(json!({ "verified": false, "line": line }));
            }
        }

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "setBreakpoints".to_string(),
            body: Some(json!({
                "breakpoints": body_breakpoints
            })),
            message: None,
        }
    }
}
