//! Output handling: source retrieval, loaded sources, modules, inline values, exception info.

use super::{
    DEBUGGER_QUERY_WAIT_MS, DapMessage, DebugAdapter, ExceptionDetails, ExceptionInfoArguments,
    ExceptionInfoResponseBody, HashMap, InlineValuesArguments, InlineValuesResponseBody,
    LoadedSourcesResponseBody, Module, ModulesArguments, ModulesResponseBody, SourceArguments,
    SourceResponseBody, Value, collect_inline_values_with_runtime, extract_variable_names, inc_re,
    lock_or_recover, module_path_to_name,
};

impl DebugAdapter {
    /// Handle inlineValues request (custom)
    ///
    /// Queries the Perl debugger for runtime variable values and returns
    /// inline value hints with Perl-idiomatic formatting.
    ///
    /// #9089: the extension is fail-closed — the capability is advertised false
    /// and every unnegotiated request is refused at the gate below — until a
    /// versioned negotiation contract is proven. The remaining path stays so a
    /// future promotion flips advertisement and service together.
    pub(super) fn handle_inline_values(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let Some(args) = arguments else {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "inlineValues".to_string(),
                body: None,
                message: Some("Missing arguments".to_string()),
            };
        };

        let args: InlineValuesArguments = match serde_json::from_value(args) {
            Ok(parsed) => parsed,
            Err(e) => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "inlineValues".to_string(),
                    body: None,
                    message: Some(format!("Invalid arguments: {}", e)),
                };
            }
        };

        // #9089: the routed `inlineValues` request is a project extension, and
        // no versioned negotiation contract exists yet, so every client is an
        // unnegotiated client. Refuse here — before workspace path validation,
        // before any filesystem read, and before any debugger query — so the
        // extension cannot serve source-derived occurrences or runtime values
        // while it is disabled.
        //
        // The gate is deliberately input-independent: every request that passes
        // envelope validation receives the same deterministic refusal, whatever
        // its source or range, and no rejected request touches the filesystem,
        // the session, or the debugger.
        // Bound to the same authority `handle_initialize` advertises, so a
        // future promotion cannot leave the capability true while this still
        // refuses.
        if crate::backend::capabilities::refuse_inline_values_extension(
            crate::backend::capabilities::advertises_inline_values_extension(),
        ) {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "inlineValues".to_string(),
                body: None,
                message: Some(
                    crate::backend::capabilities::INLINE_VALUES_EXTENSION_UNSUPPORTED_MESSAGE
                        .to_string(),
                ),
            };
        }

        let Some(source_path) = args.source.path else {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "inlineValues".to_string(),
                body: None,
                message: Some("inlineValues requires source.path".to_string()),
            };
        };

        if args.start_line <= 0 || args.end_line <= 0 {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "inlineValues".to_string(),
                body: None,
                message: Some("inlineValues requires positive startLine/endLine".to_string()),
            };
        }

        let start_line = args.start_line.min(args.end_line);
        let end_line = args.end_line.max(args.start_line);
        // Validate path against workspace root to prevent path traversal.
        let validated_path = match self.validate_source_path(&source_path) {
            Ok(path) => path,
            Err(e) => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "inlineValues".to_string(),
                    body: None,
                    message: Some(e),
                };
            }
        };

        let content = match std::fs::read_to_string(&validated_path) {
            Ok(content) => content,
            Err(e) => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "inlineValues".to_string(),
                    body: None,
                    message: Some(format!("Failed to read source file: {}", e)),
                };
            }
        };

        // Query runtime variable values from the debugger
        let runtime_values =
            match self.query_inline_variable_values(&content, start_line, end_line, request_seq) {
                Ok(values) => values,
                Err(error) => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "inlineValues".to_string(),
                        body: None,
                        message: Some(format!("inlineValues query failed: {error}")),
                    };
                }
            };

        let inline_values = collect_inline_values_with_runtime(
            &content,
            start_line,
            end_line,
            runtime_values.as_ref(),
        );
        let body = InlineValuesResponseBody { inline_values };

        match serde_json::to_value(&body) {
            Ok(body) => DapMessage::Response {
                seq,
                request_seq,
                success: true,
                command: "inlineValues".to_string(),
                body: Some(body),
                message: None,
            },
            Err(e) => DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "inlineValues".to_string(),
                body: None,
                message: Some(format!("Failed to serialize inlineValues response: {}", e)),
            },
        }
    }

    /// Query runtime variable values for inline display.
    ///
    /// Extracts variable names from source, then queries the Perl debugger
    /// for each variable's current value.
    fn query_inline_variable_values(
        &self,
        source: &str,
        start_line: i64,
        end_line: i64,
        request_seq: i64,
    ) -> Result<Option<HashMap<String, String>>, String> {
        let var_names = extract_variable_names(source, start_line, end_line);
        if var_names.is_empty() {
            return Ok(None);
        }

        // Check for active debug session
        let has_session = lock_or_recover(&self.session, "debug_adapter.session").is_some();
        if !has_session {
            return Ok(None);
        }

        let request = self
            .operation_broker
            .register_request(
                request_seq,
                super::operation_broker::OperationClass::Inspection,
                std::time::Duration::from_millis(DEBUGGER_QUERY_WAIT_MS * 8),
            )
            .map_err(|error| format!("unable to register inlineValues: {}", error.as_str()))?;
        let cancellation = request.token().cloned();
        let expected_session_generation = request.session_generation();

        let mut values = HashMap::new();

        for var_name in &var_names {
            if cancellation.as_ref().is_some_and(|token| token.is_cancelled()) {
                let terminal = request.settle(super::operation_broker::BrokerTerminal::Cancelled);
                return Err(format!("inlineValues did not complete: {}", terminal.as_str()));
            }

            let sigil = var_name.chars().next().unwrap_or('$');
            let cmd = match sigil {
                '@' => format!("p scalar {}", var_name),
                '%' => format!("p scalar(keys {})", var_name),
                _ => format!("p {}", var_name),
            };

            let output_frame_markers = {
                let mut session_guard = lock_or_recover(&self.session, "debug_adapter.session");
                if let Some(ref mut session) = *session_guard {
                    if let Some(stdin) = session.process.stdin.as_mut() {
                        let commands = vec![cmd];
                        self.send_framed_debugger_query_bound_with_token(
                            stdin,
                            &commands,
                            DEBUGGER_QUERY_WAIT_MS,
                            None,
                            Some(expected_session_generation),
                            None,
                            cancellation.clone(),
                        )
                        .ok()
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            let result = output_frame_markers.and_then(|(operation, begin, end)| {
                self.capture_framed_debugger_output_for_operation(&operation, &begin, &end)
            });

            if result.is_none()
                && self.operation_broker.current_session_generation() != expected_session_generation
            {
                let terminal =
                    request.settle(super::operation_broker::BrokerTerminal::StaleGeneration);
                return Err(format!("inlineValues did not complete: {}", terminal.as_str()));
            }

            if let Some(lines) = result {
                let raw: String = lines.join(" ").trim().to_string();
                if !raw.is_empty() {
                    values.insert(var_name.clone(), raw);
                }
            }
        }

        let result = if values.is_empty() { None } else { Some(values) };
        let terminal = if cancellation.as_ref().is_some_and(|token| token.is_cancelled()) {
            super::operation_broker::BrokerTerminal::Cancelled
        } else {
            super::operation_broker::BrokerTerminal::Completed(Vec::new())
        };
        let terminal = request.settle(terminal);
        if !matches!(terminal, super::operation_broker::BrokerTerminal::Completed(_)) {
            return Err(format!("inlineValues did not complete: {}", terminal.as_str()));
        }
        Ok(result)
    }

    pub(super) fn handle_source(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: SourceArguments = match arguments.and_then(|v| serde_json::from_value(v).ok()) {
            Some(a) => a,
            None => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "source".to_string(),
                    body: None,
                    message: Some("Missing or invalid arguments".to_string()),
                };
            }
        };

        let path = match args.source.and_then(|s| s.path) {
            Some(p) => p,
            None => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "source".to_string(),
                    body: None,
                    message: Some("source.path is required".to_string()),
                };
            }
        };

        // Validate path against workspace root to prevent path traversal
        let validated_path = match self.validate_source_path(&path) {
            Ok(p) => p,
            Err(e) => {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "source".to_string(),
                    body: None,
                    message: Some(e),
                };
            }
        };

        match std::fs::read_to_string(&validated_path) {
            Ok(content) => {
                let body =
                    SourceResponseBody { content, mime_type: Some("text/x-perl".to_string()) };
                DapMessage::Response {
                    seq,
                    request_seq,
                    success: true,
                    command: "source".to_string(),
                    body: serde_json::to_value(&body).ok(),
                    message: None,
                }
            }
            Err(e) => DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "source".to_string(),
                body: None,
                message: Some(format!("Failed to read source file: {}", e)),
            },
        }
    }

    /// Handle exceptionInfo request
    ///
    /// Returns details about the most recent exception (die/croak) encountered
    /// during debugging. Reads from `self.last_exception_message` which is
    /// populated by the output reader when exception patterns are detected.
    pub(super) fn handle_exception_info(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let _args: Option<ExceptionInfoArguments> =
            arguments.and_then(|v| serde_json::from_value(v).ok());

        let stored_message =
            lock_or_recover(&self.last_exception_message, "debug_adapter.last_exception_message");
        let exception_text = stored_message.clone();
        drop(stored_message);

        let body = match exception_text {
            Some(ref message) => ExceptionInfoResponseBody {
                exception_id: "perl_exception".to_string(),
                description: Some(message.clone()),
                break_mode: "always".to_string(),
                details: Some(ExceptionDetails {
                    message: Some(message.clone()),
                    type_name: Some("die".to_string()),
                    stack_trace: None,
                }),
            },
            None => ExceptionInfoResponseBody {
                exception_id: "perl_exception".to_string(),
                description: Some("Unknown exception".to_string()),
                break_mode: "always".to_string(),
                details: None,
            },
        };

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "exceptionInfo".to_string(),
            body: serde_json::to_value(&body).ok(),
            message: None,
        }
    }

    /// Query `%INC` from the debugger and return parsed (module_key, abs_path) pairs.
    pub(super) fn query_inc_entries(&self) -> Vec<(String, String)> {
        self.query_inc_entries_with_token(None, None).unwrap_or_default()
    }

    fn query_inc_entries_with_token(
        &self,
        cancellation: Option<super::operation_broker::CancellationToken>,
        expected_session_generation: Option<super::operation_broker::SessionGeneration>,
    ) -> Result<Vec<(String, String)>, String> {
        let output_frame_markers = {
            let mut session_guard = lock_or_recover(&self.session, "debug_adapter.session");
            if let Some(ref mut session) = *session_guard {
                if let Some(stdin) = session.process.stdin.as_mut() {
                    let commands = vec!["x \\%INC".to_string()];
                    self.send_framed_debugger_query_bound_with_token(
                        stdin,
                        &commands,
                        DEBUGGER_QUERY_WAIT_MS * 8,
                        None,
                        expected_session_generation,
                        None,
                        cancellation.clone(),
                    )
                    .ok()
                } else {
                    None
                }
            } else {
                None
            }
        };
        // Session guard dropped — safe to read output.
        let lines = match output_frame_markers {
            Some((operation, begin, end)) => {
                match self.await_framed_debugger_output_for_operation(&operation, &begin, &end) {
                    super::operation_broker::BrokerTerminal::Completed(lines) => lines,
                    terminal => return Err(terminal.as_str().to_string()),
                }
            }
            None => return Err("session unavailable".to_string()),
        };

        let re = match inc_re() {
            Some(re) => re,
            None => return Err("%INC parser unavailable".to_string()),
        };

        let mut entries = Vec::new();
        for line in &lines {
            if cancellation.as_ref().is_some_and(|token| token.is_cancelled()) {
                return Err("cancelled".to_string());
            }
            if let Some(caps) = re.captures(line)
                && let (Some(key), Some(val)) = (caps.get(1), caps.get(2))
            {
                entries.push((key.as_str().to_string(), val.as_str().to_string()));
            }
        }
        Ok(entries)
    }

    /// Handle loadedSources request — returns all files loaded via `%INC`.
    pub(super) fn handle_loaded_sources(
        &self,
        seq: i64,
        request_seq: i64,
        _arguments: Option<Value>,
    ) -> DapMessage {
        let has_session = lock_or_recover(&self.session, "debug_adapter.session").is_some();

        let sources = if has_session {
            let operation = self.operation_broker.register_request(
                request_seq,
                super::operation_broker::OperationClass::Inspection,
                std::time::Duration::from_millis(DEBUGGER_QUERY_WAIT_MS * 8),
            );
            let Ok(operation) = operation else {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "loadedSources".to_string(),
                    body: None,
                    message: Some("Unable to register cancellation".to_string()),
                };
            };
            let sources = match self.query_inc_entries_with_token(
                operation.token().cloned(),
                Some(operation.session_generation()),
            ) {
                Ok(entries) => entries
                    .into_iter()
                    .map(|(key, path)| crate::protocol::Source {
                        name: Some(key),
                        path: Some(path),
                    })
                    .collect(),
                Err(error) => {
                    let _ = operation
                        .settle(super::operation_broker::BrokerTerminal::Rejected(error.clone()));
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "loadedSources".to_string(),
                        body: None,
                        message: Some(format!("loadedSources query failed: {error}")),
                    };
                }
            };
            let terminal = if operation.token().is_some_and(|token| token.is_cancelled()) {
                super::operation_broker::BrokerTerminal::Cancelled
            } else {
                super::operation_broker::BrokerTerminal::Completed(Vec::new())
            };
            let terminal = operation.settle(terminal);
            if !matches!(terminal, super::operation_broker::BrokerTerminal::Completed(_)) {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "loadedSources".to_string(),
                    body: None,
                    message: Some(format!("loadedSources did not complete: {}", terminal.as_str())),
                };
            }
            sources
        } else {
            Vec::new()
        };

        let body = LoadedSourcesResponseBody { sources };
        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "loadedSources".to_string(),
            body: serde_json::to_value(&body).ok(),
            message: None,
        }
    }

    /// Handle modules request — returns Perl modules from `%INC` with pagination.
    pub(super) fn handle_modules(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: Option<ModulesArguments> = arguments.and_then(|v| serde_json::from_value(v).ok());

        let start_module = args.as_ref().and_then(|a| a.start_module).unwrap_or(0).max(0) as usize;
        let module_count = args.as_ref().and_then(|a| a.module_count);

        let has_session = lock_or_recover(&self.session, "debug_adapter.session").is_some();

        let (all_entries, operation) = if has_session {
            let operation = match self.operation_broker.register_request(
                request_seq,
                super::operation_broker::OperationClass::Inspection,
                std::time::Duration::from_millis(DEBUGGER_QUERY_WAIT_MS * 8),
            ) {
                Ok(operation) => operation,
                Err(_) => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "modules".to_string(),
                        body: None,
                        message: Some("Unable to register cancellation".to_string()),
                    };
                }
            };
            let entries = match self.query_inc_entries_with_token(
                operation.token().cloned(),
                Some(operation.session_generation()),
            ) {
                Ok(entries) => entries,
                Err(error) => {
                    let _ =
                        operation.settle(super::operation_broker::BrokerTerminal::Rejected(error));
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "modules".to_string(),
                        body: None,
                        message: Some("modules query failed".to_string()),
                    };
                }
            };
            (entries, Some(operation))
        } else {
            (Vec::new(), None)
        };
        if let Some(operation) = operation {
            let terminal = if operation.token().is_some_and(|token| token.is_cancelled()) {
                super::operation_broker::BrokerTerminal::Cancelled
            } else {
                super::operation_broker::BrokerTerminal::Completed(Vec::new())
            };
            let terminal = operation.settle(terminal);
            if !matches!(terminal, super::operation_broker::BrokerTerminal::Completed(_)) {
                return DapMessage::Response {
                    seq,
                    request_seq,
                    success: false,
                    command: "modules".to_string(),
                    body: None,
                    message: Some(format!("modules did not complete: {}", terminal.as_str())),
                };
            }
        }

        let total = all_entries.len() as i64;
        let all_modules = modules_from_inc_entries(all_entries);

        // Apply pagination.
        let paginated: Vec<Module> = if let Some(count) = module_count {
            all_modules.into_iter().skip(start_module).take(count.max(0) as usize).collect()
        } else {
            all_modules.into_iter().skip(start_module).collect()
        };

        let body = ModulesResponseBody { modules: paginated, total_modules: Some(total) };

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "modules".to_string(),
            body: serde_json::to_value(&body).ok(),
            message: None,
        }
    }
}

fn modules_from_inc_entries(entries: Vec<(String, String)>) -> Vec<Module> {
    let mut modules: Vec<(String, String)> =
        entries.into_iter().map(|(key, path)| (module_path_to_name(&key), path)).collect();

    modules.sort_by(|(left_name, left_path), (right_name, right_path)| {
        left_name.cmp(right_name).then_with(|| left_path.cmp(right_path))
    });

    modules
        .into_iter()
        .enumerate()
        .map(|(idx, (name, path))| Module { id: idx.to_string(), name, path: Some(path) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::operation_broker::{BrokerTerminal, OperationClass};
    use super::super::variable_cache::VariableCache;
    use super::super::{DebugAdapter, DebugSession, DebugState, ResumeMode, lock_or_recover};
    use super::modules_from_inc_entries;
    use std::collections::HashMap;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn unknown_cancel_targets_leave_other_requests_and_executable_locations_intact() -> TestResult {
        let directory = tempfile::tempdir()?;
        let adapter = DebugAdapter::new();
        let live = adapter
            .operation_broker
            .register_request(200, OperationClass::Inspection, Duration::from_secs(1))
            .map_err(|error| format!("register live: {error:?}"))?;
        for arguments in [
            None,
            Some(serde_json::json!({"requestId": "invalid"})),
            Some(serde_json::json!({"progressId": "200"})),
            Some(serde_json::json!({"requestId": 9999, "progressId": "200"})),
        ] {
            match adapter.handle_cancel(1, 1, arguments) {
                super::DapMessage::Response { success: true, command, .. }
                    if command == "cancel" => {}
                other => return Err(format!("internal cancel acknowledgement: {other:?}").into()),
            }
            if live.token().ok_or("live token missing")?.is_cancelled() {
                return Err("unknown or invalid target cancelled unrelated request".into());
            }
        }
        let path = directory.path().join("executable.pl");
        std::fs::write(&path, "my $x = 1;\nprint $x;\n")?;
        match adapter.handle_breakpoint_locations(
            2,
            2,
            Some(serde_json::json!({"source": {"path": path}, "line": 1, "endLine": 2})),
        ) {
            super::DapMessage::Response {
                success: true, request_seq: 2, body: Some(body), ..
            } if body.get("breakpoints").and_then(serde_json::Value::as_array).is_some_and(
                |locations| {
                    locations.iter().any(|entry| {
                        entry.get("line").and_then(serde_json::Value::as_i64) == Some(1)
                    })
                },
            ) => {}
            other => {
                return Err(format!(
                    "executable locations were lost after unrelated cancel: {other:?}"
                )
                .into());
            }
        }
        Ok(())
    }

    // Real pipe/reader fixture, deliberately not a claim about installed Perl
    // debugger semantics. Log every command before replying, so a stale write
    // cannot hide behind its subsequently rejected response.
    fn install_query_peer(
        adapter: &DebugAdapter,
        log: &std::path::Path,
        exit_second: bool,
    ) -> TestResult {
        adapter.begin_session_generation();
        let old = lock_or_recover(&adapter.session, "test.session").take();
        if let Some(mut old) = old {
            let _ = old.process.kill();
            old.process.wait()?;
        }
        let script = r#"
use strict;
use warnings;
use IO::Handle;
$| = 1;
my ($path, $exit_second) = @ARGV;
open my $log, '>>', $path or die $!;
$log->autoflush(1);
print "READY:$path\n";
my $values = 0;
while (my $line = <STDIN>) {
    print {$log} $line;
    if ($line =~ /DAP_BEGIN_(\d+)/) { print "DAP_BEGIN_$1\n"; }
    elsif ($line =~ /DAP_END_(\d+)/) { print "DAP_END_$1\n"; }
    elsif ($line =~ /^x /) { print "'Fresh.pm' => '/fresh/Fresh.pm'\n"; }
    elsif ($line =~ /^p /) {
        $values++;
        exit 0 if $exit_second && $values == 2;
        print "42\n";
    }
}
"#;
        let process = Command::new("perl")
            .arg("-e")
            .arg(script)
            .arg(log)
            .arg(if exit_second { "1" } else { "0" })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        *lock_or_recover(&adapter.session, "test.session") = Some(DebugSession {
            process,
            state: DebugState::Stopped,
            stack_frames: Vec::new(),
            stack_frame_arguments: HashMap::new(),
            variable_cache: VariableCache::default(),
            thread_id: 1,
            debuggee_cwd: std::path::PathBuf::from("."),
            last_resume_mode: ResumeMode::Unknown,
            initial_stop_pending: false,
            stopped_generation: 1,
            module_generation: crate::reload::RuntimeModuleGenerationClock::new(),
        });
        adapter.operation_broker.open_session();
        adapter.start_output_reader(std::path::PathBuf::from("."));
        let ready = format!("READY:{}", log.display());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if lock_or_recover(&adapter.recent_output, "test.output")
                .lines
                .iter()
                .any(|line| line.normalized == ready)
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("query peer did not become ready".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn inc_stale_request_preflight_rejects_replacement_write_and_fresh_query_recovers() -> TestResult
    {
        let directory = tempfile::tempdir()?;
        let adapter = DebugAdapter::new();
        let old = adapter
            .operation_broker
            .register_request(71, OperationClass::Inspection, Duration::from_secs(1))
            .map_err(|error| format!("register old: {error:?}"))?;
        let log = directory.path().join("replacement.log");
        install_query_peer(&adapter, &log, false)?;
        if adapter
            .query_inc_entries_with_token(old.token().cloned(), Some(old.session_generation()))
            .is_ok()
        {
            return Err("old request was accepted by replacement session".into());
        }
        let fresh = adapter
            .operation_broker
            .register_request(72, OperationClass::Inspection, Duration::from_secs(1))
            .map_err(|error| format!("register fresh: {error:?}"))?;
        let entries = adapter.query_inc_entries_with_token(
            fresh.token().cloned(),
            Some(fresh.session_generation()),
        )?;
        if entries != vec![("Fresh.pm".to_string(), "/fresh/Fresh.pm".to_string())] {
            return Err(format!("fresh query did not recover: {entries:?}").into());
        }
        let commands = std::fs::read_to_string(log)?;
        if commands.lines().count() != 3 {
            return Err(format!("replacement received stale commands: {commands:?}").into());
        }
        if fresh.settle(BrokerTerminal::Completed(Vec::new()))
            != BrokerTerminal::Completed(Vec::new())
        {
            return Err("fresh request did not settle successfully".into());
        }
        Ok(())
    }

    #[test]
    fn inline_reader_eof_rejects_partial_values_and_replacement_recovers() -> TestResult {
        let directory = tempfile::tempdir()?;
        let adapter = DebugAdapter::new();
        let failed_log = directory.path().join("failed.log");
        install_query_peer(&adapter, &failed_log, true)?;
        let result = adapter.query_inline_variable_values("my $first; my $second;", 1, 1, 81);
        if result.is_ok() {
            return Err(format!("EOF accepted stale partial values: {result:?}").into());
        }
        let commands = std::fs::read_to_string(failed_log)?;
        if !commands.lines().any(|line| line == "p $second") {
            return Err("fixture never reached the second value after accepting the first".into());
        }
        install_query_peer(&adapter, &directory.path().join("recovered.log"), false)?;
        let values = adapter
            .query_inline_variable_values("my $fresh;", 1, 1, 82)?
            .ok_or("fresh inline request returned no values")?;
        if values.get("$fresh").map(String::as_str) != Some("42") {
            return Err(format!("fresh inline values did not recover: {values:?}").into());
        }
        Ok(())
    }

    #[test]
    fn modules_from_inc_entries_sorts_before_assigning_ids() -> TestResult {
        let modules = modules_from_inc_entries(vec![
            ("Zoo/Last.pm".to_string(), "/lib/Zoo/Last.pm".to_string()),
            ("App/Core.pm".to_string(), "/lib/App/Core.pm".to_string()),
            ("App/Core.pm".to_string(), "/vendor/App/Core.pm".to_string()),
        ]);

        let names: Vec<&str> = modules.iter().map(|module| module.name.as_str()).collect();
        assert_eq!(names, vec!["App::Core", "App::Core", "Zoo::Last"]);
        let first_path = modules
            .first()
            .and_then(|module| module.path.as_deref())
            .ok_or("expected first module to preserve sorted path")?;
        assert_eq!(first_path, "/lib/App/Core.pm");
        let ids: Vec<&str> = modules.iter().map(|module| module.id.as_str()).collect();
        assert_eq!(ids, vec!["0", "1", "2"]);

        Ok(())
    }
}
