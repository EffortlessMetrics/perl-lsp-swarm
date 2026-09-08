use super::{
    DapMessage, DebugAdapter, SetFunctionBreakpointsArguments, Value,
    is_valid_function_breakpoint_name, json, lock_or_recover,
};

impl DebugAdapter {
    /// Handle setFunctionBreakpoints request.
    ///
    /// Uses replace semantics and best-effort synchronization to the running debugger.
    ///
    /// #9578: while the capability is floored, every request shape — including
    /// malformed and empty-list shapes — receives the identical deterministic
    /// refusal before argument parsing, name resolution, registry mutation,
    /// stored-state replacement, or any debugger command. The admission gate
    /// reads the same single authority that advertises
    /// `supportsFunctionBreakpoints`, so advertisement and admission can never
    /// disagree in either promotion direction; the body below stays intact as
    /// the #8645 re-enable surface.
    pub(in crate::debug_adapter) fn handle_set_function_breakpoints(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        if !crate::backend::capabilities::advertises_function_breakpoints() {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "setFunctionBreakpoints".to_string(),
                body: None,
                message: Some(
                    crate::backend::capabilities::FUNCTION_BREAKPOINTS_UNSUPPORTED_MESSAGE
                        .to_string(),
                ),
            };
        }

        let args: SetFunctionBreakpointsArguments =
            match arguments.and_then(|v| serde_json::from_value(v).ok()) {
                Some(a) => a,
                None => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "setFunctionBreakpoints".to_string(),
                        body: None,
                        message: Some("Missing arguments".to_string()),
                    };
                }
            };

        let requested = args.breakpoints;

        let mut validated_names = Vec::with_capacity(requested.len());
        let mut response_breakpoints = Vec::with_capacity(requested.len());

        for entry in requested {
            let name = entry.name.trim().to_string();

            let id = {
                let mut next = lock_or_recover(
                    &self.next_function_breakpoint_id,
                    "debug_adapter.next_function_breakpoint_id",
                );
                let id = *next;
                *next += 1;
                id
            };

            let invalid_reason = if name.is_empty() {
                Some("Function breakpoint name is required".to_string())
            } else if name.contains('\n') || name.contains('\r') {
                Some("Function breakpoint name cannot contain newlines".to_string())
            } else if !is_valid_function_breakpoint_name(&name) {
                Some(format!(
                    "Invalid function breakpoint name `{name}` (expected package-qualified Perl symbol)"
                ))
            } else {
                None
            };

            if let Some(reason) = invalid_reason {
                response_breakpoints.push(json!({
                    "id": id,
                    "verified": false,
                    "message": reason
                }));
                continue;
            }

            validated_names.push(name.clone());
            response_breakpoints.push(json!({
                "id": id,
                "verified": true
            }));
        }

        // DAP replace semantics: overwrite existing function breakpoints.
        if let Ok(mut stored) = self.function_breakpoints.lock() {
            *stored = validated_names;
        }

        // Best-effort apply to currently running session as well.
        self.apply_stored_function_breakpoints();

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "setFunctionBreakpoints".to_string(),
            body: Some(json!({ "breakpoints": response_breakpoints })),
            message: None,
        }
    }
}
