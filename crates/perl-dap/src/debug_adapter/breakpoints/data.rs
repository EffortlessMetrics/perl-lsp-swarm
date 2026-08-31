use super::{
    DapMessage, DataBreakpointInfoArguments, DataBreakpointInfoResponseBody, DebugAdapter,
    SetDataBreakpointsArguments, SetDataBreakpointsResponseBody, Value, is_valid_set_variable_name,
    lock_or_recover,
};

fn context_qualified_watchpoint_refusal(
    args: &DataBreakpointInfoArguments,
) -> Option<&'static str> {
    if args.variables_reference.is_some() {
        return Some(
            "Cannot validate a variablesReference-qualified data breakpoint yet; \
             no dataId was created for an unproven variable container",
        );
    }
    if args.frame_id.is_some() {
        return Some(
            "Cannot validate a frameId-qualified data breakpoint yet; \
             no dataId was created for an unproven stopped frame",
        );
    }
    None
}

impl DebugAdapter {
    /// Handle dataBreakpointInfo request — check if a variable can be watched.
    pub(in crate::debug_adapter) fn handle_data_breakpoint_info(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: DataBreakpointInfoArguments =
            match arguments.and_then(|v| serde_json::from_value(v).ok()) {
                Some(a) => a,
                None => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "dataBreakpointInfo".to_string(),
                        body: None,
                        message: Some("Missing arguments".to_string()),
                    };
                }
            };

        let body = if !is_valid_set_variable_name(&args.name) {
            DataBreakpointInfoResponseBody {
                data_id: None,
                description: "Cannot watch this expression".to_string(),
                access_types: None,
            }
        } else if let Some(description) = context_qualified_watchpoint_refusal(&args) {
            // A context-qualified dataId is scoped to the referenced container or
            // suspended frame. The current native path cannot yet prove either
            // identity, so returning the bare Perl name would create a plausible
            // but cross-frame/cross-generation identifier. Fail closed until
            // #2374's stopped-generation lookup and installation receipt exist.
            DataBreakpointInfoResponseBody {
                data_id: None,
                description: description.to_string(),
                access_types: None,
            }
        } else {
            // #9091 fail-closed: native data breakpoints are unsupported until a
            // watchpoint identity, backend install acknowledgement, and hit
            // attribution can be proven. A syntactically valid Perl name is not a
            // watchpoint identity, so no persistent dataId is minted — not even a
            // context-free one that a client could mistake for a session-stable ID.
            DataBreakpointInfoResponseBody {
                data_id: None,
                description:
                    "Native data breakpoints are unsupported: no proven watchpoint identity, \
                     install, or hit attribution exists, so no dataId is created (#9091)"
                        .to_string(),
                access_types: None,
            }
        };

        // DAP makes `dataId` required-but-nullable. The shared response type
        // predates that distinction and omits `None`, so repair this one wire
        // boundary explicitly rather than allowing an unavailable target to
        // serialize as a schema-invalid missing property.
        let body = serde_json::to_value(&body).ok().map(|mut value| {
            if let Value::Object(fields) = &mut value {
                fields.entry("dataId".to_string()).or_insert(Value::Null);
            }
            value
        });

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "dataBreakpointInfo".to_string(),
            body,
            message: None,
        }
    }

    /// Handle setDataBreakpoints request — set watchpoints via Perl debugger `w` command.
    pub(in crate::debug_adapter) fn handle_set_data_breakpoints(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: SetDataBreakpointsArguments =
            match arguments.and_then(|v| serde_json::from_value(v).ok()) {
                Some(a) => a,
                None => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "setDataBreakpoints".to_string(),
                        body: None,
                        message: Some("Missing arguments".to_string()),
                    };
                }
            };

        // #9091 fail-closed: native data breakpoints are unsupported, so this
        // request performs zero debugger mutation (no `W *`, no `w <name>`),
        // stores no watchpoint registry state that could imply installation,
        // and reports one unverified entry per input in request order. An
        // empty replacement is naturally a no-op. No watchpoint state exists
        // to survive restart or session boundaries.
        {
            // The legacy registry slot is retained but is now permanently
            // empty: no supported native watchpoint state exists (#9091).
            let mut store =
                lock_or_recover(&self.data_breakpoints, "debug_adapter.data_breakpoints");
            store.clear();
        }

        let response_breakpoints: Vec<crate::protocol::Breakpoint> = args
            .breakpoints
            .iter()
            .enumerate()
            .map(|(idx, _bp)| crate::protocol::Breakpoint {
                id: (idx as i64) + 1,
                verified: false,
                line: 0,
                column: None,
                message: Some(
                    "Native data breakpoints are unsupported: no watchpoint was \
                     installed and no debugger state was changed (#9091)"
                        .to_string(),
                ),
            })
            .collect();

        let body = SetDataBreakpointsResponseBody { breakpoints: response_breakpoints };

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "setDataBreakpoints".to_string(),
            body: serde_json::to_value(&body).ok(),
            message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(
        variables_reference: Option<i64>,
        frame_id: Option<i64>,
    ) -> DataBreakpointInfoArguments {
        DataBreakpointInfoArguments { name: "$value".to_string(), variables_reference, frame_id }
    }

    #[test]
    fn variables_reference_takes_precedence_over_frame_id() {
        let reason = context_qualified_watchpoint_refusal(&args(Some(11), Some(7)));
        assert!(reason.is_some_and(|value| value.contains("variablesReference")));
    }

    #[test]
    fn frame_id_is_context_qualified_without_a_container() {
        let reason = context_qualified_watchpoint_refusal(&args(None, Some(7)));
        assert!(reason.is_some_and(|value| value.contains("frameId")));
    }

    #[test]
    fn context_free_request_retains_compatibility_path() {
        assert!(context_qualified_watchpoint_refusal(&args(None, None)).is_none());
    }
}
