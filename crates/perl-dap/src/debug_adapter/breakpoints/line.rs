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

        let mut args = if input_len > 0 {
            crate::protocol::SetBreakpointsArguments {
                source: parsed.source,
                breakpoints: Some(plain_entries),
                source_modified: parsed.source_modified,
            }
        } else {
            parsed
        };

        // #14593: the store reads source before validating breakpoint lines.
        // Admit the path before any store access, including empty replacements.
        // Consume the returned path: checking a workspace-relative spelling but
        // reading that spelling against the process cwd would authorize one
        // file and read another. All-rejected requests retain their no-I/O path.
        if !all_entries_rejected
            && let Some(source_path) = args.source.path.as_deref()
        {
            let admitted_path = self.validate_source_path(source_path).and_then(|path| {
                path.into_os_string().into_string().map_err(|_| {
                    "Path validation failed: resolved source path is not valid UTF-8".to_string()
                })
            });
            match admitted_path {
                Ok(path) => args.source.path = Some(path),
                Err(message) => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "setBreakpoints".to_string(),
                        body: Some(json!({ "breakpoints": [] })),
                        message: Some(message),
                    };
                }
            }
        }

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

#[cfg(test)]
mod source_boundary_tests {
    use super::{DapMessage, DebugAdapter, Value, json};
    use std::error::Error;
    use std::fs;
    use std::path::Path;

    fn bounded_adapter(root: &Path) -> Result<DebugAdapter, Box<dyn Error>> {
        let adapter = DebugAdapter::new();
        adapter.set_workspace_root(root.canonicalize()?);
        Ok(adapter)
    }

    fn source_text(path: &Path) -> Result<&str, Box<dyn Error>> {
        path.to_str().ok_or_else(|| "test fixture path is not UTF-8".into())
    }

    fn request(adapter: &mut DebugAdapter, path: &str, entries: Value) -> DapMessage {
        adapter.handle_set_breakpoints(
            2,
            1,
            Some(json!({ "source": { "path": path }, "breakpoints": entries })),
        )
    }

    fn successful_body(response: DapMessage) -> Result<Value, Box<dyn Error>> {
        match response {
            DapMessage::Response { success: true, body: Some(body), .. } => Ok(body),
            other => Err(format!("expected successful breakpoint response, got {other:?}").into()),
        }
    }

    fn assert_refused(response: DapMessage) -> Result<(), Box<dyn Error>> {
        match response {
            DapMessage::Response { success, command, body, message, .. } => {
                assert!(!success, "a source outside the configured boundary must be refused");
                assert_eq!(command, "setBreakpoints");
                assert_eq!(body, Some(json!({ "breakpoints": [] })));
                assert!(message.is_some_and(|text| !text.is_empty()), "refusal needs a reason");
                Ok(())
            }
            other => Err(format!("expected refused breakpoint response, got {other:?}").into()),
        }
    }

    #[test]
    fn contained_source_and_relative_alias_share_one_store_key() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("boundary_fixture.pl");
        fs::write(&source, "my $value = 1;\nprint $value;\n")?;
        let canonical = source.canonicalize()?;
        let canonical_key = source_text(&canonical)?;
        let mut adapter = bounded_adapter(root.path())?;

        let absolute = successful_body(request(&mut adapter, canonical_key, json!([{ "line": 1 }])))?;
        assert_eq!(absolute["breakpoints"].as_array().map(Vec::len), Some(1));
        assert_eq!(absolute["breakpoints"][0]["verified"], json!(true));

        let relative = successful_body(request(
            &mut adapter,
            "boundary_fixture.pl",
            json!([{ "line": 2 }]),
        ))?;
        assert_eq!(relative["breakpoints"].as_array().map(Vec::len), Some(1));
        assert_eq!(relative["breakpoints"][0]["verified"], json!(true));
        let records = adapter.breakpoints.get_breakpoints(canonical_key);
        assert_eq!(records.len(), 1, "relative request replaces rather than duplicates the file");
        assert_eq!(records.first().map(|record| record.line), Some(2));
        assert!(adapter.breakpoints.get_breakpoints("boundary_fixture.pl").is_empty());

        successful_body(request(&mut adapter, "boundary_fixture.pl", json!([])))?;
        assert!(adapter.breakpoints.get_breakpoints(canonical_key).is_empty());
        Ok(())
    }

    #[test]
    fn outside_source_is_refused_before_launch() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let source = outside.path().join("outside.pl");
        fs::write(&source, "print 'fixture';\n")?;
        let mut adapter = bounded_adapter(root.path())?;
        assert_refused(request(&mut adapter, source_text(&source)?, json!([{ "line": 1 }])))?;
        assert!(adapter.breakpoints.get_breakpoints(source_text(&source)?).is_empty());
        Ok(())
    }

    #[test]
    fn prefix_collision_and_parent_traversal_are_refused() -> Result<(), Box<dyn Error>> {
        let parent = tempfile::tempdir()?;
        let root = parent.path().join("workspace");
        let sibling = parent.path().join("workspace-other");
        fs::create_dir(&root)?;
        fs::create_dir(&sibling)?;
        let source = sibling.join("outside.pl");
        fs::write(&source, "print 'fixture';\n")?;
        let mut adapter = bounded_adapter(&root)?;
        assert_refused(request(&mut adapter, source_text(&source)?, json!([{ "line": 1 }])))?;
        assert_refused(request(
            &mut adapter,
            "../workspace-other/outside.pl",
            json!([{ "line": 1 }]),
        ))?;
        Ok(())
    }

    #[test]
    fn rejected_replacement_preserves_existing_records() -> Result<(), Box<dyn Error>> {
        let parent = tempfile::tempdir()?;
        let root = parent.path().join("workspace");
        fs::create_dir(&root)?;
        let source = parent.path().join("outside.pl");
        fs::write(&source, "print 'fixture';\n")?;
        let canonical = source.canonicalize()?;
        let key = source_text(&canonical)?;
        let mut adapter = DebugAdapter::new();
        successful_body(request(&mut adapter, key, json!([{ "line": 1 }])))?;
        let before = adapter.breakpoints.get_breakpoints(key);
        assert_eq!(before.len(), 1);
        adapter.set_workspace_root(root.canonicalize()?);

        assert_refused(request(&mut adapter, key, json!([])))?;
        let after = adapter.breakpoints.get_breakpoints(key);
        assert_eq!(after.len(), before.len(), "refused empty replacement must not clear the store");
        assert_eq!(after.first().map(|record| record.id), before.first().map(|record| record.id));
        Ok(())
    }

    #[test]
    fn uncreated_contained_source_remains_an_admitted_request() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("not_created_yet.pl");
        let mut adapter = bounded_adapter(root.path())?;
        let body = successful_body(request(
            &mut adapter,
            source_text(&source)?,
            json!([{ "line": 1 }]),
        ))?;
        assert_eq!(body["breakpoints"].as_array().map(Vec::len), Some(1));
        assert!(!source.exists(), "breakpoint admission must not create source files");
        Ok(())
    }

    #[test]
    fn unconfigured_adapter_still_accepts_an_ordinary_absolute_source()
    -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("ordinary.pl");
        fs::write(&source, "print 'fixture';\n")?;
        let mut adapter = DebugAdapter::new();
        let body = successful_body(request(
            &mut adapter,
            source_text(&source)?,
            json!([{ "line": 1 }]),
        ))?;
        assert_eq!(body["breakpoints"].as_array().map(Vec::len), Some(1));
        assert_eq!(body["breakpoints"][0]["verified"], json!(true));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_outside_source_is_refused() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let source = outside.path().join("outside.pl");
        fs::write(&source, "print 'fixture';\n")?;
        let link = root.path().join("linked.pl");
        std::os::unix::fs::symlink(&source, &link)?;
        let mut adapter = bounded_adapter(root.path())?;
        assert_refused(request(&mut adapter, source_text(&link)?, json!([{ "line": 1 }])))?;
        assert!(adapter.breakpoints.get_breakpoints(source_text(&link)?).is_empty());
        Ok(())
    }
}
