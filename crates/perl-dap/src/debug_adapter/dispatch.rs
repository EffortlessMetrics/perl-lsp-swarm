//! Request dispatching: handle_request, dispatch_request, response_succeeded_for_command.

use super::{DapMessage, DebugAdapter, Value};

/// How a dispatched request relates to the pinned upstream DAP schema.
///
/// The class travels with the executable row so the protocol-authority gate
/// classifies a request from the same table that routes it, rather than from
/// a separately maintained list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DapRequestClass {
    /// A request defined by the pinned upstream Debug Adapter Protocol schema.
    Standard,
    /// A project extension request upstream does not define.
    Extension,
}

/// Runtime frontends on which a request has an explicit handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DapRequestAvailability {
    /// The native adapter, external-peer connect, and mirror-listen adapters.
    AllFrontends,
    /// Only the native adapter; peer frontends apply their compatibility fallback.
    NativeOnly,
}

/// One executable production request row.
///
/// A row exists if and only if `dispatch_request` routes its wire command:
/// both are expanded from the same [`dap_request_table!`] invocation, so
/// neither can be edited without the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DapRequestRow {
    /// Stable identity for this row, independent of source ordering.
    pub(crate) row_id: &'static str,
    /// The DAP wire command this row routes.
    pub(crate) command: &'static str,
    /// Whether the row is standard DAP or a project extension.
    pub(crate) class: DapRequestClass,
    /// Which runtime-selected frontend owns an explicit route for this command.
    pub(crate) availability: DapRequestAvailability,
}

/// Map a table class token to its typed variant.
///
/// An unrecognised token is a compile error, so a row cannot enter the
/// inventory without an explicit reviewed classification.
macro_rules! dap_request_class {
    (standard) => {
        DapRequestClass::Standard
    };
    (extension) => {
        DapRequestClass::Extension
    };
}

macro_rules! dap_request_availability {
    (all_frontends) => {
        DapRequestAvailability::AllFrontends
    };
    (native_only) => {
        DapRequestAvailability::NativeOnly
    };
}

macro_rules! dap_request_peer_available {
    (all_frontends) => {
        true
    };
    (native_only) => {
        false
    };
}

/// Expand one row's handler call.
///
/// The arity marker in the table is matched literally; the `arguments`
/// binding is written by [`dap_request_table!`] itself so the expansion
/// stays hygienic.
macro_rules! dap_dispatch_call {
    ($adapter:expr, $handler:ident, $seq:expr, $request_seq:expr, $arguments:expr, (arguments)) => {
        $adapter.$handler($seq, $request_seq, $arguments)
    };
    ($adapter:expr, $handler:ident, $seq:expr, $request_seq:expr, $arguments:expr, ()) => {
        $adapter.$handler($seq, $request_seq)
    };
    // A typo or a new arity shape names its own mistake here, rather than
    // surfacing as an opaque "no rules expected this token" far from the row.
    ($adapter:expr, $handler:ident, $seq:expr, $request_seq:expr, $arguments:expr, $other:tt) => {
        compile_error!(concat!(
            "dap_request_table row for `",
            stringify!($handler),
            "` has arity marker `",
            stringify!($other),
            "`; it must be `(arguments)` or `()`"
        ))
    };
}

/// The adapter's one executable request authority.
///
/// Each row expands into three places at once: the typed inventory row, the
/// derived `SUPPORTED_COMMANDS` list, and the `dispatch_request` match arm
/// that actually routes the request. Adding, removing or renaming a request
/// is therefore a single edit, and a row can never name a handler that does
/// not exist.
macro_rules! dap_request_table {
    (
        $( $class:ident $availability:ident $variant:ident $command:literal
            => $handler:ident $arity:tt ),* $(,)?
    ) => {
        /// Stable typed command identity shared by all production frontends.
        pub(crate) enum DapRequestRoute {
            $($variant),*
        }

        /// Every executable production request row, in table order.
        pub(crate) const DAP_REQUEST_ROWS: &[DapRequestRow] = &[
            $(
                DapRequestRow {
                    row_id: concat!("dap.request.", $command),
                    command: $command,
                    class: dap_request_class!($class),
                    availability: dap_request_availability!($availability),
                },
            )*
        ];

        /// The adapter's closed DAP request-command list, derived from
        /// [`DAP_REQUEST_ROWS`] and consumed by the reload contract's
        /// collision check (#10097) and the unknown-command suggester.
        pub(crate) const SUPPORTED_COMMANDS: [&str; DAP_REQUEST_ROWS.len()] =
            [$($command),*];

        impl DapRequestRoute {
            pub(crate) fn from_command(wire_command: &str) -> Option<Self> {
                match wire_command {
                    $($command => Some(Self::$variant),)*
                    _ => None,
                }
            }

            pub(crate) const fn available_in_peer_frontends(&self) -> bool {
                match self {
                    $(Self::$variant => dap_request_peer_available!($availability),)*
                }
            }
        }

        impl DebugAdapter {
            pub(super) fn dispatch_request(
                &mut self,
                request_seq: i64,
                command: &str,
                arguments: Option<Value>,
            ) -> DapMessage {
                let seq = self.next_seq();

                match command {
                    $(
                        $command => dap_dispatch_call!(
                            self, $handler, seq, request_seq, arguments, $arity
                        ),
                    )*
                    _ => DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: command.to_string(),
                        body: None,
                        message: Some(Self::unknown_command_message(command)),
                    },
                }
            }
        }
    };
}

dap_request_table! {
    standard all_frontends Initialize "initialize" => handle_initialize(arguments),
    standard all_frontends Launch "launch" => handle_launch(arguments),
    standard all_frontends Attach "attach" => handle_attach(arguments),
    standard all_frontends Disconnect "disconnect" => handle_disconnect(arguments),
    standard all_frontends Terminate "terminate" => handle_terminate(arguments),
    standard all_frontends SetBreakpoints "setBreakpoints" => handle_set_breakpoints(arguments),
    standard all_frontends SetFunctionBreakpoints "setFunctionBreakpoints" => handle_set_function_breakpoints(arguments),
    standard native_only SetExceptionBreakpoints "setExceptionBreakpoints" => handle_set_exception_breakpoints(arguments),
    standard all_frontends ConfigurationDone "configurationDone" => handle_configuration_done(),
    standard all_frontends Threads "threads" => handle_threads(),
    standard all_frontends StackTrace "stackTrace" => handle_stack_trace(arguments),
    standard all_frontends Scopes "scopes" => handle_scopes(arguments),
    standard all_frontends Variables "variables" => handle_variables(arguments),
    standard native_only SetVariable "setVariable" => handle_set_variable(arguments),
    standard all_frontends Continue "continue" => handle_continue(arguments),
    standard all_frontends Next "next" => handle_next(arguments),
    standard all_frontends StepIn "stepIn" => handle_step_in(arguments),
    standard all_frontends StepOut "stepOut" => handle_step_out(arguments),
    standard all_frontends Pause "pause" => handle_pause(arguments),
    standard all_frontends Evaluate "evaluate" => handle_evaluate(arguments),
    extension native_only InlineValues "inlineValues" => handle_inline_values(arguments),
    standard all_frontends BreakpointLocations "breakpointLocations" => handle_breakpoint_locations(arguments),
    standard native_only Source "source" => handle_source(arguments),
    standard native_only LoadedSources "loadedSources" => handle_loaded_sources(arguments),
    standard native_only Modules "modules" => handle_modules(arguments),
    standard native_only Completions "completions" => handle_completions(arguments),
    standard native_only ExceptionInfo "exceptionInfo" => handle_exception_info(arguments),
    standard native_only Restart "restart" => handle_restart(arguments),
    standard native_only SetExpression "setExpression" => handle_set_expression(arguments),
    standard native_only DataBreakpointInfo "dataBreakpointInfo" => handle_data_breakpoint_info(arguments),
    standard native_only SetDataBreakpoints "setDataBreakpoints" => handle_set_data_breakpoints(arguments),
    standard native_only Cancel "cancel" => handle_cancel(arguments),
    standard native_only StepInTargets "stepInTargets" => handle_step_in_targets(arguments),
    standard native_only GotoTargets "gotoTargets" => handle_goto_targets(arguments),
    standard native_only Goto "goto" => handle_goto(arguments),
    standard native_only RestartFrame "restartFrame" => handle_restart_frame(arguments),
    standard native_only TerminateThreads "terminateThreads" => handle_terminate_threads(arguments),
}

/// Whether `command` is one of the DAP request names this adapter
/// dispatches.
pub(crate) fn is_supported_dap_command(command: &str) -> bool {
    SUPPORTED_COMMANDS.contains(&command)
}

impl DebugAdapter {
    /// The #9581 secondary-capability floor, applied at the sanctioned request
    /// seams ([`Self::handle_request`], [`Self::handle_request_mock`], and the
    /// stdio transport loop in [`super::transport`]).
    ///
    /// The floor's authority — which requests are floored, and the exact
    /// disposition text — lives solely in `backend/capabilities.rs`; this is
    /// only its application point. It sits *outside* the generated
    /// `dispatch_request` body deliberately: that body must stay the fixed,
    /// table-owned shape the protocol-authority gate pins
    /// (`scripts/ci/dap_authority_common.py`), with no branch reachable around
    /// the request table.
    ///
    /// Returns `Some(refusal)` when the request is floored. The refusal is
    /// constructed before any handler runs, so the floored path performs no
    /// debugger I/O, process action, or session/peer state mutation, and a
    /// missing session can never masquerade as a successful empty result; the
    /// only adapter change is response framing (one `seq` allocation).
    pub(super) fn secondary_capability_floor_response(
        &mut self,
        request_seq: i64,
        command: &str,
        arguments: Option<&Value>,
    ) -> Option<DapMessage> {
        if let Some(message) =
            crate::backend::capabilities::capability_floor_message(command, arguments)
        {
            let seq = self.next_seq();
            return Some(DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: command.to_string(),
                body: None,
                message: Some(message),
            });
        }
        None
    }

    /// Dispatch a DAP request and return the response message.
    ///
    /// Emits the `initialized` event automatically when an `initialize` request
    /// succeeds. This mirrors the behavior expected by DAP-compliant clients.
    pub fn handle_request(
        &mut self,
        request_seq: i64,
        command: &str,
        arguments: Option<Value>,
    ) -> DapMessage {
        tracing::debug!(command, arguments = ?arguments, "DAP request");

        // #9581 secondary-capability floor, ahead of the table-owned dispatch:
        // a floored request is refused before any handler can run.
        let response = match self.secondary_capability_floor_response(
            request_seq,
            command,
            arguments.as_ref(),
        ) {
            Some(floored) => floored,
            None => self.dispatch_request(request_seq, command, arguments),
        };

        // Preserve existing direct-call behavior for tests and in-memory usage.
        if command == "initialize" && Self::response_succeeded_for_command(&response, "initialize")
        {
            self.send_event("initialized", None);
        }

        response
    }

    /// Handle a DAP request (mock version for testing)
    pub fn handle_request_mock(
        &mut self,
        request_seq: i64,
        command: &str,
        arguments: Option<Value>,
    ) -> DapMessage {
        tracing::debug!(command, arguments = ?arguments, "DAP request (mock)");

        // #9581 secondary-capability floor, ahead of the table-owned dispatch:
        // the mock surface must not route floored requests either.
        let response = match self.secondary_capability_floor_response(
            request_seq,
            command,
            arguments.as_ref(),
        ) {
            Some(floored) => floored,
            None => self.dispatch_request(request_seq, command, arguments),
        };
        if command == "initialize" && Self::response_succeeded_for_command(&response, "initialize")
        {
            self.send_event("initialized", None);
        }
        response
    }

    fn unknown_command_message(command: &str) -> String {
        if let Some(suggestion) = Self::suggested_command(command) {
            format!("Unknown command: {command}. Did you mean '{suggestion}'?")
        } else {
            format!("Unknown command: {command}")
        }
    }

    fn suggested_command(command: &str) -> Option<&'static str> {
        if let Some(case_suggestion) =
            SUPPORTED_COMMANDS.iter().copied().find(|known| known.eq_ignore_ascii_case(command))
        {
            return Some(case_suggestion);
        }

        SUPPORTED_COMMANDS
            .iter()
            .copied()
            .filter_map(|known| {
                let distance = Self::command_edit_distance(command, known);
                (distance <= Self::suggestion_threshold(command, known))
                    .then_some((known, distance))
            })
            .min_by_key(|(known, distance)| (*distance, known.len()))
            .map(|(known, _)| known)
    }

    fn suggestion_threshold(command: &str, known: &str) -> usize {
        command.len().max(known.len()).saturating_div(4).clamp(1, 4)
    }

    fn command_edit_distance(left: &str, right: &str) -> usize {
        let left = left.to_ascii_lowercase();
        let right = right.to_ascii_lowercase();
        let left_bytes = left.as_bytes();
        let right_bytes = right.as_bytes();

        let mut previous: Vec<usize> = (0..=right_bytes.len()).collect();
        let mut current = vec![0; right_bytes.len() + 1];

        for (left_index, left_byte) in left_bytes.iter().enumerate() {
            current[0] = left_index + 1;

            for (right_index, right_byte) in right_bytes.iter().enumerate() {
                let substitution_cost = usize::from(left_byte != right_byte);
                let deletion = previous[right_index + 1] + 1;
                let insertion = current[right_index] + 1;
                let substitution = previous[right_index] + substitution_cost;
                current[right_index + 1] = deletion.min(insertion).min(substitution);
            }

            std::mem::swap(&mut previous, &mut current);
        }

        previous[right_bytes.len()]
    }

    pub(super) fn response_succeeded_for_command(
        response: &DapMessage,
        expected_command: &str,
    ) -> bool {
        matches!(
            response,
            DapMessage::Response {
                success: true,
                command,
                ..
            } if command == expected_command
        )
    }
}

#[cfg(test)]
mod request_inventory_tests {
    use super::{DAP_REQUEST_ROWS, DapRequestClass, SUPPORTED_COMMANDS, is_supported_dap_command};

    /// The landed wire surface. A row added or removed without a reviewed
    /// change to this list is a visible protocol change, not a refactor.
    const LANDED_REQUESTS: [&str; 37] = [
        "initialize",
        "launch",
        "attach",
        "disconnect",
        "terminate",
        "setBreakpoints",
        "setFunctionBreakpoints",
        "setExceptionBreakpoints",
        "configurationDone",
        "threads",
        "stackTrace",
        "scopes",
        "variables",
        "setVariable",
        "continue",
        "next",
        "stepIn",
        "stepOut",
        "pause",
        "evaluate",
        "inlineValues",
        "breakpointLocations",
        "source",
        "loadedSources",
        "modules",
        "completions",
        "exceptionInfo",
        "restart",
        "setExpression",
        "dataBreakpointInfo",
        "setDataBreakpoints",
        "cancel",
        "stepInTargets",
        "gotoTargets",
        "goto",
        "restartFrame",
        "terminateThreads",
    ];

    #[test]
    fn the_request_surface_is_unchanged_by_the_table_migration() {
        assert_eq!(SUPPORTED_COMMANDS, LANDED_REQUESTS);
    }

    #[test]
    fn supported_commands_is_derived_from_the_executable_rows() {
        let from_rows: Vec<&str> = DAP_REQUEST_ROWS.iter().map(|row| row.command).collect();
        assert_eq!(
            from_rows,
            SUPPORTED_COMMANDS.to_vec(),
            "SUPPORTED_COMMANDS must stay a projection of the routed rows"
        );
    }

    #[test]
    fn every_row_reports_as_supported() {
        for row in DAP_REQUEST_ROWS {
            assert!(
                is_supported_dap_command(row.command),
                "routed request {} is not reported as supported",
                row.command
            );
        }
    }

    #[test]
    fn a_command_with_no_row_is_not_supported() {
        // Opposite-direction control: the predicate must not accept a name
        // merely because it looks like a DAP request.
        for absent in ["sneak", "Initialize", "readMemory", ""] {
            assert!(
                !is_supported_dap_command(absent),
                "{absent:?} has no routed row and must not be reported as supported"
            );
        }
    }

    #[test]
    fn rows_have_unique_commands_and_row_ids() {
        let mut commands: Vec<&str> = DAP_REQUEST_ROWS.iter().map(|row| row.command).collect();
        commands.sort_unstable();
        let before = commands.len();
        commands.dedup();
        assert_eq!(before, commands.len(), "duplicate wire command in the request table");

        let mut ids: Vec<&str> = DAP_REQUEST_ROWS.iter().map(|row| row.row_id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate row id in the request table");
    }

    #[test]
    fn row_ids_are_namespaced_and_derived_from_the_wire_command() {
        for row in DAP_REQUEST_ROWS {
            assert_eq!(
                row.row_id,
                format!("dap.request.{}", row.command),
                "row id must be derivable from the wire command so it is stable across reordering"
            );
        }
    }

    #[test]
    fn inline_values_is_the_only_project_extension_row() {
        let extensions: Vec<&str> = DAP_REQUEST_ROWS
            .iter()
            .filter(|row| row.class == DapRequestClass::Extension)
            .map(|row| row.command)
            .collect();
        assert_eq!(
            extensions,
            vec!["inlineValues"],
            "the extension classification must match .ci/dap/protocol-authority.json"
        );
    }
}
