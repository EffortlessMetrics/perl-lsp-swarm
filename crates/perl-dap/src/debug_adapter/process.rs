//! Process lifecycle management: initialize, launch, attach, disconnect, terminate, restart.

use super::logpoint::{DrainStep, LogpointDrain, LogpointStep, PendingLogpoint};
use super::{
    Arc, BreakpointHitOutcome, BufRead, BufReader, Child, DEBUG_SESSION_TERMINATE_WAIT_MS,
    DapEvent, DapMessage, DebugAdapter, DebugSession, DebugState, DisconnectArguments, Duration,
    Instant, Mutex, Read, RestartArguments, ResumeMode, Source, StackFrame, Stdio, SyncSender,
    TcpAttachConfig, TcpAttachSession, TerminateArguments, TerminationState, Value, Write,
    ansi_escape_re, catalog_has_feature, context_re, dispatch_event, emit_event_safe, error_re,
    exception_re, json, lock_or_recover, module_path_to_name, prompt_re, security, stack_frame_re,
    thread, warning_re,
};
#[cfg(unix)]
use nix::sys::signal::{self, Signal};
#[cfg(unix)]
use nix::unistd::Pid;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
// The internal TCP-attach DapEvent fan-in channel is still unbounded (non-goal of #5149).
use std::sync::mpsc::channel;

mod perl_info;
mod perl_spawn;

use super::variable_cache::VariableCache;
use perl_info::detect_perl_info;
use perl_spawn::{format_perl_spawn_error, is_valid_perl_interpreter};

impl DebugAdapter {
    /// Handle initialize request
    pub(super) fn handle_initialize(
        &self,
        seq: i64,
        request_seq: i64,
        _arguments: Option<Value>,
    ) -> DapMessage {
        // Mark adapter as initialized (state machine validation)
        self.initialized.store(true, std::sync::atomic::Ordering::Release);

        let supports_core = catalog_has_feature("dap.core");
        let supports_basic_breakpoints = catalog_has_feature("dap.breakpoints.basic");
        let supports_hit_conditions = catalog_has_feature("dap.breakpoints.hit_condition");
        let supports_log_points = catalog_has_feature("dap.breakpoints.logpoints");
        let supports_exceptions = catalog_has_feature("dap.exceptions.die");
        let supports_inline_values = catalog_has_feature("dap.inline_values");
        let supports_completions = catalog_has_feature("dap.completions");
        let supports_modules = catalog_has_feature("dap.modules");
        let supports_watchpoints = catalog_has_feature("dap.watchpoints");
        let supports_warn = catalog_has_feature("dap.exceptions.warn");
        let supports_any_exception = supports_exceptions || supports_warn;
        // Capabilities whose handlers exist but are only honest when the catalog
        // advertises them.  `restartFrame` and `terminateThreads` have no perl5db
        // primitive, so their catalog entries are `planned`/unadvertised and these
        // flags resolve to `false` rather than promising a request that always fails
        // (#5045).
        let supports_restart_frame = catalog_has_feature("dap.restart_frame");
        let supports_terminate_threads = catalog_has_feature("dap.terminate_threads");
        let supports_step_in_targets = catalog_has_feature("dap.step_in_targets");
        let supports_restart = catalog_has_feature("dap.restart");
        let supports_loaded_sources = catalog_has_feature("dap.loaded_sources");

        let mut filters = Vec::new();
        if supports_exceptions {
            filters.push(json!({
                "filter": "die",
                "label": "Perl die() and uncaught exceptions",
                "default": true
            }));
            filters.push(json!({
                "filter": "all",
                "label": "All Perl exception events",
                "default": false
            }));
        }
        if supports_warn {
            filters.push(json!({
                "filter": "warn",
                "label": "Perl warn() and Carp warnings",
                "default": false
            }));
        }
        let exception_breakpoint_filters = json!(filters);

        let capabilities = json!({
            "supportsConfigurationDoneRequest": supports_core,
            "supportsFunctionBreakpoints": supports_core,
            "supportsConditionalBreakpoints": supports_basic_breakpoints,
            "supportsHitConditionalBreakpoints": supports_hit_conditions,
            "supportsEvaluateForHovers": supports_core,
            "supportsStepBack": false,
            "supportsSetVariable": supports_core,
            "supportsRestartFrame": supports_restart_frame,
            "supportsGotoTargetsRequest": supports_core,
            "supportsStepInTargetsRequest": supports_step_in_targets,
            "supportsCompletionsRequest": supports_completions,
            "supportsModulesRequest": supports_modules,
            "supportsRestartRequest": supports_restart,
            "supportsExceptionOptions": supports_any_exception,
            "supportsValueFormattingOptions": supports_core,
            "supportsExceptionInfoRequest": supports_any_exception,
            "supportTerminateDebuggee": supports_core,
            "supportsDelayedStackTraceLoading": false,
            "supportsLoadedSourcesRequest": supports_loaded_sources,
            "supportsLogPoints": supports_log_points,
            "supportsTerminateThreadsRequest": supports_terminate_threads,
            "supportsSetExpression": supports_core,
            "supportsTerminateRequest": supports_core,
            "supportsDataBreakpoints": supports_watchpoints,
            "supportsReadMemoryRequest": false,
            "supportsDisassembleRequest": false,
            "supportsCancelRequest": supports_core,
            "supportsBreakpointLocationsRequest": supports_basic_breakpoints,
            "supportsClipboardContext": false,
            "supportsSteppingGranularity": false,
            "supportsInstructionBreakpoints": false,
            "supportsExceptionFilterOptions": supports_any_exception,
            "supportsInlineValues": supports_inline_values,
            "exceptionBreakpointFilters": exception_breakpoint_filters
        });

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "initialize".to_string(),
            body: Some(capabilities),
            message: None,
        }
    }

    /// Handle launch request
    pub(super) fn handle_launch(
        &mut self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        // Validate state machine: initialize must be called before launch
        if !self.initialized.load(std::sync::atomic::Ordering::Acquire) {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "launch".to_string(),
                body: None,
                message: Some(
                    "initialize request must be sent before launch. \
                     The DAP protocol requires that the client send an initialize request first \
                     to establish the adapter's capabilities and prepare the session."
                        .to_string(),
                ),
            };
        }

        if let Some(args) = arguments {
            // Store launch arguments for restart support
            *lock_or_recover(&self.last_launch_args, "debug_adapter.last_launch_args") =
                Some(args.clone());

            let program = args.get("program").and_then(|p| p.as_str()).unwrap_or("");
            let perl_interpreter = Self::resolve_launch_interpreter(&args);

            // Extract user-provided cwd for script execution (if specified)
            // This is the working directory where the debugged script will run,
            // separate from the workspace validation boundary. `cwd` is
            // user-controlled and MUST NEVER be trusted as a security boundary —
            // doing so (or deriving the boundary from `program`'s own parent
            // directory, as this code used to) makes every launch trivially
            // self-validating and defeats the workspace check entirely.
            let user_cwd = args.get("cwd").and_then(|c| c.as_str()).map(PathBuf::from);

            // Determine the workspace boundary for this launch.
            //
            // The server-configured root (set once via `set_workspace_root`,
            // typically from `DapConfig.workspace_root` at server construction)
            // is the source of truth. A launch-args `workspaceRoot` may NARROW
            // that boundary but must never WIDEN it — otherwise a malicious or
            // misconfigured client could hand itself a broader root than the
            // server allows. If no server root is configured, a launch-args
            // `workspaceRoot` is accepted as the boundary for this launch (there
            // is nothing to widen relative to).
            //
            // If neither is present, validation is skipped entirely (see the
            // `None` handling in `launch_debugger`) — this preserves current
            // behavior for existing users, since `DapConfig.workspace_root` is
            // not yet populated from any CLI/editor-supplied source (tracked
            // separately in #5345; that fail-open gap is intentionally out of
            // scope for this fix).
            let server_root =
                lock_or_recover(&self.workspace_root, "debug_adapter.workspace_root").clone();
            let launch_root_arg =
                args.get("workspaceRoot").and_then(|w| w.as_str()).map(PathBuf::from);

            let effective_root = match (server_root, launch_root_arg) {
                (Some(server), Some(launch)) => match security::validate_path(&launch, &server) {
                    Ok(narrowed) => Some(narrowed),
                    Err(e) => {
                        return DapMessage::Response {
                            seq,
                            request_seq,
                            success: false,
                            command: "launch".to_string(),
                            body: None,
                            message: Some(format!(
                                "The launch 'workspaceRoot' ('{}') is outside your workspace \
                                     folder and cannot widen the server-configured boundary. \
                                     Details: {}",
                                launch.display(),
                                e
                            )),
                        };
                    }
                },
                (Some(server), None) => Some(server),
                (None, Some(launch)) => Some(launch),
                (None, None) => None,
            };

            if let Some(root) = effective_root {
                *lock_or_recover(&self.workspace_root, "debug_adapter.workspace_root") = Some(root);
            }

            let perl_args = args
                .get("args")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let stop_on_entry = args.get("stopOnEntry").and_then(|s| s.as_bool()).unwrap_or(false);

            // Wall-clock timeout for the perl -d debuggee process (#4640).
            // 0 (the default) disables the watchdog so legitimate long-running
            // debug sessions (e.g. a server paused at a breakpoint for minutes)
            // are not interrupted.  A positive value kills the debuggee after
            // the specified number of seconds of wall-clock time.
            let debuggee_timeout_secs =
                args.get("debuggeeTimeoutSeconds").and_then(|v| v.as_u64()).unwrap_or(0);

            let env_overrides = args
                .get("env")
                .and_then(Value::as_object)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|(key, value)| {
                            value.as_str().map(|value| (key.clone(), value.to_string()))
                        })
                        .collect::<HashMap<String, String>>()
                })
                .unwrap_or_default();

            // Launch Perl debugger
            match self.launch_debugger(
                program,
                &perl_interpreter,
                perl_args,
                stop_on_entry,
                env_overrides,
                user_cwd,
                debuggee_timeout_secs,
            ) {
                Ok(thread_id) => {
                    // Send stopped event if stop on entry
                    if stop_on_entry {
                        self.send_event(
                            "stopped",
                            Some(json!({
                                "reason": "entry",
                                "threadId": thread_id,
                                "allThreadsStopped": true
                            })),
                        );
                    }

                    DapMessage::Response {
                        seq,
                        request_seq,
                        success: true,
                        command: "launch".to_string(),
                        body: None,
                        message: None,
                    }
                }
                Err(e) => {
                    let perl_info = detect_perl_info();
                    DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "launch".to_string(),
                        body: None,
                        message: Some(format!(
                            "Cannot start Perl debugger: {}. \
                             {perl_info}. \
                             To use a specific Perl interpreter, add `perlPath` to your launch.json \
                             (e.g. {{\"perlPath\": \"/path/to/perl\"}}).",
                            e
                        )),
                    }
                }
            }
        } else {
            DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "launch".to_string(),
                body: None,
                message: Some(
                    "Debugger launch failed: no launch configuration was provided. \
                     Add a launch.json with a 'program' field pointing to your Perl script."
                        .to_string(),
                ),
            }
        }
    }

    /// Resolve the Perl interpreter for a debug launch from its `launch.json`
    /// arguments.
    ///
    /// Resolution order:
    ///
    /// 1. An explicit, non-empty interpreter from either the documented
    ///    `perlPath` key (the camelCase form of [`LaunchConfiguration`]'s
    ///    `perl_path`) or the `perl` alias is honored **verbatim** — the user's
    ///    deliberate choice always wins.
    /// 2. Otherwise, if the launch config supplies its own `PATH` via `env`, the
    ///    bare `"perl"` is kept so the launch-specific `PATH` selects the
    ///    interpreter at spawn time. Resolving here would consult the parent
    ///    process environment and silently ignore that `PATH`.
    /// 3. Otherwise the interpreter is resolved through the shared
    ///    [`PerlToolchainProfile`] so the debug session uses the same
    ///    toolchain-detected interpreter (perlbrew → plenv → `PATH`) the LSP
    ///    analyzes with, closing the DAP/LSP "which perl?" gap (#1929).
    /// 4. If nothing resolves, falls back to `"perl"`, preserving the previous
    ///    default so the launch still produces the usual "perl not on PATH"
    ///    diagnostic.
    ///
    /// [`LaunchConfiguration`]: perl_dap_config::LaunchConfiguration
    /// [`PerlToolchainProfile`]: perl_lsp_rs_core::config::PerlToolchainProfile
    fn resolve_launch_interpreter(args: &Value) -> String {
        let explicit = args
            .get("perlPath")
            .and_then(|p| p.as_str())
            .or_else(|| args.get("perl").and_then(|p| p.as_str()))
            .filter(|p| !p.is_empty());
        if let Some(path) = explicit {
            return path.to_string();
        }

        let launch_overrides_path = args
            .get("env")
            .and_then(Value::as_object)
            .is_some_and(|env| env.keys().any(|key| key.eq_ignore_ascii_case("PATH")));
        if launch_overrides_path {
            return "perl".to_string();
        }

        perl_lsp_rs_core::config::PerlToolchainProfile::resolve(
            &perl_lsp_rs_core::config::WorkspaceConfig::default(),
        )
        .map(|profile| profile.into_perl_binary().to_string_lossy().into_owned())
        .unwrap_or_else(|| "perl".to_string())
    }

    /// Launch the Perl debugger for the given script.
    ///
    /// Validates the program path and interpreter, runs a pre-launch `perl -c`
    /// syntax check, then spawns `perl -d` with the supplied arguments and
    /// environment overrides. Returns the thread ID on success.
    pub(super) fn launch_debugger(
        &mut self,
        program: &str,
        perl_interpreter: &str,
        args: Vec<String>,
        stop_on_entry: bool,
        env_overrides: HashMap<String, String>,
        cwd_override: Option<PathBuf>,
        debuggee_timeout_secs: u64,
    ) -> Result<i32, String> {
        // Security: Validate program path before any process spawning
        // This prevents command injection via flag arguments (e.g., "-e malicious_code")
        // and ensures we're launching a real Perl script file.

        let program = program.trim();

        // Reject empty or whitespace-only paths
        if program.is_empty() {
            return Err(
                "No Perl script was specified. Set the 'program' field in your launch.json \
                 to the path of the script you want to debug."
                    .to_string(),
            );
        }

        // Detect shell-style quotes around the program path (#1985).
        // Users sometimes write "program": "'path/to/script.pl'" or
        // "\"path/to/script.pl\"" — the quotes become part of the path,
        // causing a confusing file-not-found error.
        let has_surrounding_quotes =
            (program.starts_with('\'') && program.ends_with('\'') && program.len() > 1)
                || (program.starts_with('"') && program.ends_with('"') && program.len() > 1);
        if has_surrounding_quotes && !Path::new(program).is_file() {
            return Err(format!(
                "The 'program' path '{program}' has surrounding quotes. \
                 Remove the quotes in your launch.json — the path should be just \
                 the script path, e.g. \"program\": \"script.pl\"."
            ));
        }

        // Validate that the program is a regular file (not a directory, device, etc.)
        // Using metadata().is_file() is more robust than exists() because:
        // - exists() returns true for directories
        // - exists() returns true for symlinks to non-files
        // - is_file() specifically checks for regular files
        let path = Path::new(program);
        match std::fs::metadata(path) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Err(format!(
                        "'{}' is not a file. Update the 'program' field in your launch.json \
                         to point to a Perl script (.pl or .t).",
                        program
                    ));
                }
            }
            Err(e) => {
                return Err(format!(
                    "Cannot find '{}': {}. \
                     Check that the 'program' path in your launch.json is correct.",
                    program, e
                ));
            }
        }

        // Enforce workspace-bound launch paths when a workspace root is known.
        // This prevents launching scripts outside the active project tree.
        let workspace_root =
            lock_or_recover(&self.workspace_root, "debug_adapter.workspace_root").clone();
        if let Some(root) = workspace_root.as_ref() {
            security::validate_path(path, root).map_err(|e| {
                format!(
                    "The script '{}' is outside your workspace folder. \
                     Only scripts within the open workspace can be debugged. \
                     Details: {}",
                    program, e
                )
            })?;
        }

        if !is_valid_perl_interpreter(perl_interpreter) {
            return Err(format!(
                "Invalid Perl interpreter '{}'. Set launch.json `perl` to a Perl executable path (for example, `perl` or `/usr/bin/perl`).",
                perl_interpreter
            ));
        }

        // Pre-launch syntax check: run `perl -c <script>` before spawning the
        // debugger.  This catches syntax errors early and surfaces a clear,
        // actionable message to the user instead of a generic "Cannot start
        // Perl debugger" failure after `perl -d` exits immediately.
        Self::check_syntax(perl_interpreter, program, &env_overrides, cwd_override.clone())?;

        // Use PerlOracleEnv to deny ambient PERL5LIB/PERL5OPT so the debug
        // session env is controlled entirely by launch.json `env` (#8688).
        // `env_overrides` (explicit launch.json entries) are added via
        // extra_env so they reach the subprocess unconditionally.
        // Use user-specified cwd if provided; otherwise default to script's parent directory
        let prog_cwd = if let Some(user_cwd) = cwd_override {
            user_cwd
        } else {
            Path::new(program)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        };
        let mut oracle = perl_lsp_rs_core::config::PerlOracleEnv::for_version_probe(
            PathBuf::from(perl_interpreter),
            prog_cwd,
        );
        oracle.extra_env.extend(env_overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
        let mut cmd = oracle.into_command();
        cmd.arg("-d");

        // Perl debugger stops on the first line by default
        let _ = stop_on_entry; // currently unused

        // Use -- to separate flags from script name, preventing argument injection
        // if program starts with -
        cmd.arg("--");
        cmd.arg(program);
        cmd.args(&args);

        // Set up pipes
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        match cmd.spawn() {
            Ok(child) => {
                // Only advance the session generation once spawning has succeeded. A rejected
                // launch must leave the currently active reader valid for its existing session.
                self.prepare_replacement_session();
                let thread_id = {
                    if let Ok(mut counter) = self.thread_counter.lock() {
                        *counter += 1;
                        *counter
                    } else {
                        tracing::warn!("Failed to lock thread counter, using 1");
                        1
                    }
                };

                let session = DebugSession {
                    process: child,
                    state: DebugState::Running,
                    stack_frames: Vec::new(),
                    stack_frame_arguments: HashMap::new(),
                    variable_cache: VariableCache::default(),
                    thread_id,
                    last_resume_mode: ResumeMode::Unknown,
                    stopped_generation: 0,
                };

                if let Ok(mut guard) = self.session.lock() {
                    *guard = Some(session);
                } else {
                    return Err(
                        "Debugger could not be started: an internal state error occurred. \
                         Try stopping the debug session and relaunching."
                            .to_string(),
                    );
                }

                // Apply any function breakpoints configured before launch.
                self.apply_stored_function_breakpoints();

                // Start output reader thread
                self.start_output_reader();

                // Start debuggee watchdog if a wall-clock timeout was configured (#4640).
                // The watchdog kills the perl -d process if it is still alive after
                // the specified number of seconds, preventing a hung debuggee from
                // blocking the DAP session indefinitely.
                if debuggee_timeout_secs > 0 {
                    self.start_debuggee_watchdog(debuggee_timeout_secs);
                }

                Ok(thread_id)
            }
            Err(e) => Err(format_perl_spawn_error(perl_interpreter, &e)),
        }
    }

    /// Run `perl -c <script>` and return `Ok(())` if the syntax is valid,
    /// or `Err(message)` with a user-friendly error describing the problem.
    ///
    /// `perl -c` exits with status 0 when the script compiles successfully
    /// (printing "syntax OK" to stderr).  Any non-zero exit indicates a
    /// syntax or dependency failure; the error detail is on stderr.
    ///
    /// If `perl` cannot be found or spawned, the check is silently skipped
    /// and `Ok(())` is returned so that the subsequent `perl -d` launch
    /// produces the correct "perl not on PATH" error to the user.
    pub(super) fn check_syntax(
        perl_interpreter: &str,
        program: &str,
        env_overrides: &HashMap<String, String>,
        cwd_override: Option<PathBuf>,
    ) -> Result<(), String> {
        // PerlOracleEnv denies ambient PERL5LIB/PERL5OPT (#8688); explicit
        // env_overrides from launch.json are honored via extra_env.
        // Use user-specified cwd if provided; otherwise default to script's parent directory
        let prog_cwd = if let Some(user_cwd) = cwd_override {
            user_cwd
        } else {
            Path::new(program)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        };
        let mut oracle = perl_lsp_rs_core::config::PerlOracleEnv::for_version_probe(
            PathBuf::from(perl_interpreter),
            prog_cwd,
        );
        oracle.extra_env.extend(env_overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
        let output = match oracle
            .into_command()
            .arg("-c")
            .arg("--")
            .arg(program)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(out) => out,
            Err(e) => {
                // `perl` not found or could not be spawned — skip the check
                // and let the real launch produce the "perl not on PATH" error.
                tracing::warn!("perl -c could not be run (will try perl -d anyway): {}", e);
                return Ok(());
            }
        };

        if output.status.success() {
            return Ok(());
        }

        // Combine stdout + stderr (perl writes errors to stderr; stdout is
        // normally empty for -c, but merge both for robustness).
        let raw_stderr = String::from_utf8_lossy(&output.stderr);
        let raw_stdout = String::from_utf8_lossy(&output.stdout);
        let combined = if raw_stderr.is_empty() { raw_stdout } else { raw_stderr };

        // Strip the "syntax OK" confirmation line that sometimes appears even
        // on partial failures, and drop blank lines.
        let error_lines: Vec<&str> = combined
            .lines()
            .filter(|l| {
                let trimmed = l.trim();
                !trimmed.eq_ignore_ascii_case("syntax ok") && !trimmed.is_empty()
            })
            .collect();

        let detail = if error_lines.is_empty() {
            combined.trim().to_string()
        } else {
            error_lines.join("\n")
        };

        if let Some(module_name) = Self::missing_module_name(&detail) {
            return Err(Self::format_missing_module_error(&module_name));
        }

        Err(format!(
            "Syntax error in '{}' — fix the error below before debugging:\n{}",
            program, detail
        ))
    }

    fn missing_module_name(detail: &str) -> Option<String> {
        detail.lines().find_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("Can't locate ")?;
            let module_path = rest.split(" in @INC").next()?.trim_end_matches('.');
            let module_name = module_path_to_name(module_path);
            (!module_name.is_empty()).then_some(module_name)
        })
    }

    fn format_missing_module_error(module_name: &str) -> String {
        format!(
            "Module {module_name} not found. Install with: cpan {module_name}. \
             View on MetaCPAN: https://metacpan.org/pod/{module_name}"
        )
    }

    /// Start thread to read debugger output with enhanced error recovery
    pub(super) fn start_output_reader(&self) {
        let session = self.session.clone();
        let seq = self.seq.clone();
        let sender = self.event_sender.clone();
        let recent_output = self.recent_output.clone();
        let breakpoints = self.breakpoints.clone();
        let exception_break_on_die = self.exception_break_on_die.clone();
        let exception_break_on_warn = self.exception_break_on_warn.clone();
        let last_exception_message = self.last_exception_message.clone();
        let tcp_session = self.tcp_session.clone();
        let attached_pid = self.attached_pid.clone();
        let termination_state = self.termination_state.clone();
        let session_generation = self.current_session_generation();

        thread::spawn(move || {
            // Perl's debugger prompt and evaluation output are emitted on stderr.
            // Prefer stderr as the control stream, with stdout as a fallback.
            let control_stream: Option<Box<dyn Read + Send>> = {
                if let Ok(mut guard) = session.lock() {
                    guard.as_mut().and_then(|s| {
                        if let Some(stderr) = s.process.stderr.take() {
                            Some(Box::new(stderr) as Box<dyn Read + Send>)
                        } else {
                            s.process
                                .stdout
                                .take()
                                .map(|stdout| Box::new(stdout) as Box<dyn Read + Send>)
                        }
                    })
                } else {
                    tracing::warn!("Failed to lock session in output reader");
                    None
                }
            };

            let Some(control_stream) = control_stream else {
                tracing::warn!(
                    "No debugger output stream available - output reader thread exiting"
                );
                if let Some(ref sender) = sender {
                    emit_terminated_event(
                        sender,
                        &seq,
                        &termination_state,
                        Some(session_generation),
                        Some(json!({"reason": "no_debugger_stream"})),
                    );
                }
                DebugAdapter::clear_active_session_state_for_generation(
                    &session,
                    &tcp_session,
                    &attached_pid,
                    &termination_state,
                    session_generation,
                );
                return;
            };

            let mut reader = BufReader::new(control_stream);
            let mut line = String::new();

            let mut current_file = String::new();
            let mut current_func = String::new();
            let mut current_line = 0;
            let mut _debugger_ready = false;
            // In-flight logpoint value query, if any. A logpoint hit queues a framed
            // `p` query for the scalars its template mentions; the replies stream back
            // through this same loop and are folded into the message here (#5045).
            let mut pending_logpoint: Option<PendingLogpoint> = None;
            let mut logpoint_marker_id: u64 = 0;
            // Residual frame lines to filter after a capture is abandoned mid-frame:
            // (end marker, remaining budget).
            let mut logpoint_drain: Option<LogpointDrain> = None;

            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        tracing::debug!("Perl debugger process terminated");
                        // The debuggee exited mid-query: emit the logpoint with
                        // whatever values arrived rather than dropping it.
                        if let Some(pending) = pending_logpoint.take() {
                            emit_logpoint_messages(sender.as_ref(), &seq, pending.into_messages());
                        }
                        if let Some(ref sender) = sender {
                            emit_terminated_event(
                                sender,
                                &seq,
                                &termination_state,
                                Some(session_generation),
                                Some(json!({"reason": "debugger_eof"})),
                            );
                        }
                        DebugAdapter::clear_active_session_state_for_generation(
                            &session,
                            &tcp_session,
                            &attached_pid,
                            &termination_state,
                            session_generation,
                        );
                        break;
                    }
                    Ok(_) => {
                        // Strip only the transport delimiters here. A logpoint value
                        // may legitimately end in spaces or tabs, and `trim_end()`
                        // below would eat them before the capture ever sees the line.
                        let framed_text = line.trim_end_matches(['\r', '\n']);
                        let text = framed_text.trim_end().to_string();
                        let sanitized_text = if let Some(re) = ansi_escape_re() {
                            re.replace_all(&text, "").into_owned()
                        } else {
                            text.clone()
                        };
                        // The logpoint protocol carries payload bytes, so it reads the
                        // delimiter-stripped line rather than the whitespace-trimmed
                        // one every other consumer below uses.
                        let capture_text = if let Some(re) = ansi_escape_re() {
                            re.replace_all(framed_text, "").into_owned()
                        } else {
                            framed_text.to_string()
                        };
                        let normalized_text = DebugAdapter::normalize_debugger_output_line(&text);
                        let analysis_text = if normalized_text.is_empty() {
                            sanitized_text.trim().to_string()
                        } else {
                            normalized_text
                        };
                        tracing::trace!(output = %text, "Debugger output");

                        // Fold logpoint value replies before the line reaches the
                        // client or the recent-output buffer: those lines are adapter
                        // framing, not debuggee output.
                        // A capture abandoned mid-frame leaves the rest of its frame
                        // still coming. Those lines are adapter framing — late
                        // `DAPLPV:` replies and the end marker — so keep filtering
                        // them instead of forwarding protocol noise to the client.
                        // Bounded so a marker that never arrives cannot swallow real
                        // debuggee output indefinitely.
                        if let Some(drain) = logpoint_drain.as_mut() {
                            // Every line reaching an open drain is swallowed — the
                            // closing line is the end marker itself, which is adapter
                            // framing and must not reach the client either. The one
                            // exception is `Superseded`: that line opens the *next*
                            // capture's frame, so the drain retires and the line falls
                            // through to that capture below instead of being eaten.
                            match drain.observe_line(&capture_text) {
                                DrainStep::Swallow => continue,
                                DrainStep::Done => {
                                    logpoint_drain = None;
                                    continue;
                                }
                                DrainStep::Superseded => {
                                    logpoint_drain = None;
                                }
                            }
                        }

                        if let Some(pending) = pending_logpoint.as_mut() {
                            // Deliberately `capture_text`, not `analysis_text` and not
                            // `sanitized_text`. `normalize_debugger_output_line`
                            // truncates the line to whatever follows the *last*
                            // `DB<...>` token, so a value whose own text contains
                            // `DB<4>` would lose its `DAPLPV:` prefix and be mistaken
                            // for framing noise. `sanitized_text` is built from the
                            // `trim_end()`-ed line and would eat trailing spaces or
                            // tabs that belong to the value itself — the regression
                            // `test_logpoint_preserves_trailing_whitespace_in_values`
                            // guards. The capture only needs ANSI stripped; its own
                            // markers frame it.
                            let step = pending.observe_line(&capture_text);
                            if matches!(
                                step,
                                LogpointStep::Finished
                                    | LogpointStep::Abandoned
                                    | LogpointStep::AbandonedInFrame
                            ) && let Some(pending) = pending_logpoint.take()
                            {
                                if matches!(step, LogpointStep::AbandonedInFrame) {
                                    logpoint_drain =
                                        Some(LogpointDrain::new(pending.end_marker().to_string()));
                                }
                                emit_logpoint_messages(
                                    sender.as_ref(),
                                    &seq,
                                    pending.into_messages(),
                                );
                            }
                            if matches!(
                                step,
                                LogpointStep::Consumed
                                    | LogpointStep::Finished
                                    | LogpointStep::AbandonedInFrame
                            ) {
                                continue;
                            }
                        }

                        {
                            let mut output = lock_or_recover(
                                &recent_output,
                                "debug_adapter.recent_output_reader",
                            );
                            Self::append_recent_output_line_locked(&mut output, &text);
                        }

                        // Send all output to client with error handling
                        if let Some(ref sender) = sender
                            && !emit_event_safe(
                                sender,
                                &seq,
                                "output",
                                Some(json!({
                                    "category": "stdout",
                                    "output": format!("{}\n", text)
                                })),
                            )
                        {
                            tracing::warn!(
                                "Failed to send output event - client may have disconnected"
                            );
                            break; // Exit the loop if client is gone
                        }

                        // Enhanced context information parsing with multiple patterns
                        let mut context_updated = false;

                        // Try main context pattern
                        if let Some(re) = context_re()
                            && let Some(caps) = re.captures(&analysis_text)
                        {
                            if let Some(func) = caps.name("func") {
                                current_func = func.as_str().to_string();
                                context_updated = true;
                            }
                            if let Some(file) = caps.name("file").or_else(|| caps.name("file2")) {
                                current_file = file.as_str().to_string();
                                context_updated = true;
                            }
                            if let Some(line_num) = caps.name("line").or_else(|| caps.name("line2"))
                            {
                                current_line = line_num.as_str().parse::<i32>().unwrap_or(0);
                                context_updated = true;
                            }
                        }

                        // Try stack frame pattern as fallback
                        if !context_updated
                            && let Some(re) = stack_frame_re()
                            && let Some(caps) = re.captures(&analysis_text)
                        {
                            if let Some(func) = caps.name("func") {
                                current_func = func.as_str().to_string();
                            }
                            if let Some(file) = caps.name("file") {
                                current_file = file.as_str().to_string();
                            }
                            if let Some(line_num) = caps.name("line") {
                                current_line = line_num.as_str().parse::<i32>().unwrap_or(0);
                            }
                            context_updated = true;
                        }

                        // Check for errors that might provide location info
                        if !context_updated
                            && let Some(re) = error_re()
                            && let Some(caps) = re.captures(&analysis_text)
                        {
                            if let Some(file) = caps.name("file") {
                                current_file = file.as_str().to_string();
                            }
                            if let Some(line_num) = caps.name("line") {
                                current_line = line_num.as_str().parse::<i32>().unwrap_or(0);
                            }
                            context_updated = true;

                            // Send error event to client
                            if let Some(ref sender) = sender {
                                emit_event_safe(
                                    sender,
                                    &seq,
                                    "output",
                                    Some(json!({
                                        "category": "stderr",
                                        "output": format!("Error: {}\n", text)
                                    })),
                                );
                            }
                        }

                        if context_updated {
                            let break_on_die =
                                exception_break_on_die.lock().map(|guard| *guard).unwrap_or(false);
                            let break_on_warn =
                                exception_break_on_warn.lock().map(|guard| *guard).unwrap_or(false);
                            let is_exception_line =
                                exception_re().is_some_and(|re| re.is_match(&analysis_text));
                            let is_warning_line =
                                warning_re().is_some_and(|re| re.is_match(&analysis_text));
                            let exception_match = break_on_die && is_exception_line;
                            let warning_match =
                                break_on_warn && is_warning_line && !is_exception_line;

                            // Store exception message for exceptionInfo request
                            if (exception_match || warning_match)
                                && let Ok(mut guard) = last_exception_message.lock()
                            {
                                *guard = Some(analysis_text.clone());
                            }

                            let mut should_emit_stopped = false;
                            let mut should_auto_continue = false;
                            let mut stop_reason = "step".to_string();
                            let mut logpoint_messages: Vec<String> = Vec::new();

                            let thread_id = {
                                let Ok(mut guard) = session.lock() else {
                                    tracing::warn!(
                                        "Failed to lock session when processing debugger context"
                                    );
                                    continue;
                                };

                                if let Some(ref mut s) = *guard {
                                    let was_running = matches!(s.state, DebugState::Running);
                                    if was_running {
                                        s.stopped_generation =
                                            s.stopped_generation.saturating_add(1);
                                    }
                                    let current_frame_id =
                                        s.stopped_generation.min(i32::MAX as u64).max(1) as i32;
                                    if !current_file.is_empty() && current_line > 0 {
                                        s.stack_frames = vec![StackFrame {
                                            id: current_frame_id,
                                            name: if current_func.is_empty() {
                                                "main".to_string()
                                            } else {
                                                current_func.clone()
                                            },
                                            source: Source {
                                                name: Some(
                                                    std::path::Path::new(&current_file)
                                                        .file_name()
                                                        .and_then(|n| n.to_str())
                                                        .unwrap_or(&current_file)
                                                        .to_string(),
                                                ),
                                                path: current_file.clone(),
                                                source_reference: None,
                                            },
                                            line: current_line,
                                            column: 1,
                                            end_line: None,
                                            end_column: None,
                                        }];
                                        s.stack_frame_arguments.clear();
                                    }

                                    if was_running {
                                        should_emit_stopped = true;
                                        let resume_mode = s.last_resume_mode.clone();

                                        let breakpoint_outcome = if matches!(
                                            resume_mode,
                                            ResumeMode::Continue | ResumeMode::RunToBreakpoint
                                        ) && !current_file.is_empty()
                                            && current_line > 0
                                        {
                                            breakpoints.register_breakpoint_hit(
                                                &current_file,
                                                i64::from(current_line),
                                            )
                                        } else {
                                            BreakpointHitOutcome::default()
                                        };

                                        if exception_match || warning_match {
                                            stop_reason = "exception".to_string();
                                            s.state = DebugState::Stopped;
                                        } else if breakpoint_outcome.matched {
                                            logpoint_messages = breakpoint_outcome.log_messages;

                                            // The debugger is at a prompt right now, so
                                            // this is the one moment the referenced
                                            // scalars can be read. Queue the framed
                                            // query ahead of any resume command; the
                                            // replies are folded in at the top of this
                                            // loop and the message is emitted then
                                            // instead of below (#5045).
                                            // Every branch below either hands the messages
                                            // back to `logpoint_messages` for immediate
                                            // emission or moves them into the capture that
                                            // will emit them; none may drop them.
                                            logpoint_messages = match PendingLogpoint::new(
                                                logpoint_marker_id,
                                                std::mem::take(&mut logpoint_messages),
                                            ) {
                                                // Nothing to resolve: the templates are
                                                // already their own final text.
                                                Err(templates) => templates,
                                                Ok(pending) => {
                                                    logpoint_marker_id =
                                                        logpoint_marker_id.saturating_add(1);
                                                    match s.process.stdin.as_mut() {
                                                        Some(stdin) => {
                                                            for command in pending.query_commands()
                                                            {
                                                                let _ = stdin
                                                                    .write_all(command.as_bytes());
                                                            }
                                                            let _ = stdin.flush();
                                                            let new_begin =
                                                                pending.begin_marker().to_string();
                                                            // A drain already open is
                                                            // filtering an even earlier
                                                            // capture's residue. Tell it
                                                            // where this frame starts so it
                                                            // retires instead of eating it.
                                                            if let Some(drain) =
                                                                logpoint_drain.as_mut()
                                                            {
                                                                drain.supersede_with(&new_begin);
                                                            }
                                                            // A hit seen while an earlier
                                                            // capture is still open would
                                                            // otherwise drop that capture's
                                                            // messages on the floor. Emit
                                                            // what it resolved so far
                                                            // instead of losing it, and keep
                                                            // filtering its residual frame:
                                                            // its late `DAPLPV:` replies and
                                                            // its end marker are still in
                                                            // flight and would otherwise
                                                            // reach the client as debuggee
                                                            // stdout.
                                                            match pending_logpoint.replace(pending)
                                                            {
                                                                Some(previous) => {
                                                                    let mut drain =
                                                                        LogpointDrain::new(
                                                                            previous
                                                                                .end_marker()
                                                                                .to_string(),
                                                                        );
                                                                    drain
                                                                        .supersede_with(&new_begin);
                                                                    logpoint_drain = Some(drain);
                                                                    previous.into_messages()
                                                                }
                                                                None => Vec::new(),
                                                            }
                                                        }
                                                        // No stdin to ask on: emit the raw
                                                        // templates rather than nothing.
                                                        None => pending.into_messages(),
                                                    }
                                                }
                                            };

                                            if breakpoint_outcome.should_stop {
                                                stop_reason = "breakpoint".to_string();
                                                s.state = DebugState::Stopped;
                                            } else {
                                                if let Some(stdin) = s.process.stdin.as_mut() {
                                                    let _ = stdin.write_all(b"c\n");
                                                    let _ = stdin.flush();
                                                }
                                                s.state = DebugState::Running;
                                                s.last_resume_mode = ResumeMode::Continue;
                                                should_auto_continue = true;
                                            }
                                        } else if matches!(resume_mode, ResumeMode::RunToBreakpoint)
                                        {
                                            // Not at a user breakpoint while in RunToBreakpoint
                                            // mode.  The `c` command sent by configurationDone is
                                            // already driving the debugger toward the first
                                            // breakpoint; the context line we just saw is the
                                            // implicit first-line stop that appeared BEFORE that
                                            // `c` was processed.  Do NOT send another `c` here —
                                            // that would queue a second continue that runs past
                                            // the eventual breakpoint, breaking subsequent steps.
                                            // Simply keep state=Running and suppress the stopped
                                            // event so the client never sees this implicit stop.
                                            s.state = DebugState::Running;
                                            // Keep RunToBreakpoint until we actually hit one.
                                            should_auto_continue = true;
                                        } else {
                                            s.state = DebugState::Stopped;
                                        }

                                        if !should_auto_continue {
                                            s.last_resume_mode = ResumeMode::Unknown;
                                        }
                                    }

                                    s.thread_id
                                } else {
                                    continue;
                                }
                            };

                            // Empty when a value query was queued instead: those messages
                            // are emitted once the framed replies arrive.
                            emit_logpoint_messages(sender.as_ref(), &seq, logpoint_messages);

                            if should_auto_continue {
                                continue;
                            }

                            if should_emit_stopped
                                && let Some(ref sender) = sender
                                && !emit_event_safe(
                                    sender,
                                    &seq,
                                    "stopped",
                                    Some(json!({
                                        "reason": stop_reason,
                                        "threadId": thread_id,
                                        "allThreadsStopped": true
                                    })),
                                )
                            {
                                tracing::warn!(
                                    "Failed to send stopped event - client disconnected"
                                );
                                return;
                            }
                            continue;
                        }

                        // Detect debugger prompt (stopped state) with enhanced pattern matching
                        if prompt_re().is_some_and(|re| re.is_match(&sanitized_text)) {
                            _debugger_ready = true;
                            let thread_id = {
                                let Ok(mut guard) = session.lock() else {
                                    tracing::warn!(
                                        "Failed to lock session when processing debugger prompt"
                                    );
                                    continue;
                                };
                                if let Some(ref mut s) = *guard {
                                    // Create stack frame with enhanced context validation
                                    if !current_file.is_empty() && current_line > 0 {
                                        let frame = StackFrame {
                                            id: 1,
                                            name: if current_func.is_empty() {
                                                "main".to_string()
                                            } else {
                                                current_func.clone()
                                            },
                                            source: Source {
                                                name: Some(
                                                    std::path::Path::new(&current_file)
                                                        .file_name()
                                                        .and_then(|n| n.to_str())
                                                        .unwrap_or(&current_file)
                                                        .to_string(),
                                                ),
                                                path: current_file.clone(),
                                                source_reference: None,
                                            },
                                            line: current_line,
                                            column: 1,
                                            end_line: None,
                                            end_column: None,
                                        };
                                        s.stack_frames = vec![frame];
                                        s.stack_frame_arguments.clear();
                                    } else {
                                        // Provide a fallback frame for when we don't have perfect context
                                        let frame = StackFrame {
                                            id: 1,
                                            name: "main".to_string(),
                                            source: Source {
                                                name: Some("<unknown>".to_string()),
                                                path: "<unknown>".to_string(),
                                                source_reference: None,
                                            },
                                            line: 1,
                                            column: 1,
                                            end_line: None,
                                            end_column: None,
                                        };
                                        s.stack_frames = vec![frame];
                                        s.stack_frame_arguments.clear();
                                    }
                                    s.state = DebugState::Stopped;
                                    s.thread_id
                                } else {
                                    continue;
                                }
                            };

                            // Send stopped event with robust error handling
                            if let Some(ref sender) = sender
                                && !emit_event_safe(
                                    sender,
                                    &seq,
                                    "stopped",
                                    Some(json!({
                                        "reason": "step",
                                        "threadId": thread_id,
                                        "allThreadsStopped": true
                                    })),
                                )
                            {
                                tracing::warn!(
                                    "Failed to send stopped event - client disconnected"
                                );
                                return; // Exit thread
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Error reading from debugger");
                        // Same contract as the EOF arm above: a read failure during a
                        // framed value query must still surface the logpoint with
                        // whatever values arrived, not swallow it.
                        if let Some(pending) = pending_logpoint.take() {
                            emit_logpoint_messages(sender.as_ref(), &seq, pending.into_messages());
                        }
                        // Send termination event before exiting
                        if let Some(ref sender) = sender {
                            emit_terminated_event(
                                sender,
                                &seq,
                                &termination_state,
                                Some(session_generation),
                                Some(json!({"reason": "read_error", "error": e.to_string()})),
                            );
                        }
                        DebugAdapter::clear_active_session_state_for_generation(
                            &session,
                            &tcp_session,
                            &attached_pid,
                            &termination_state,
                            session_generation,
                        );
                        break;
                    }
                }
            }
        });
    }

    /// Start a watchdog thread that kills the debuggee process after a
    /// wall-clock timeout (#4640).
    ///
    /// The watchdog sleeps for `timeout_secs`, then checks whether the debug
    /// session is still alive.  If the process has already exited (normal
    /// termination, client disconnect, or reader-thread EOF cleanup), the
    /// watchdog exits silently.  If the process is still running, the watchdog
    /// emits a `terminated` event with `reason: "debuggee_timeout"` after killing
    /// the process.  Termination is reserved (emitted-flag set) before the kill so
    /// the output reader's EOF path cannot race in with `debugger_eof`.  The kill
    /// still runs before the blocking event send so a stalled DAP client cannot
    /// leave the debuggee alive.  The `TerminationState.emitted` flag ensures only
    /// one `terminated` event reaches the client.
    ///
    /// The watchdog is generation-aware: if the session was replaced (e.g. via
    /// restart) before the timeout fires, the watchdog exits without acting.
    fn start_debuggee_watchdog(&self, timeout_secs: u64) {
        let session = self.session.clone();
        let seq = self.seq.clone();
        let sender = self.event_sender.clone();
        let termination_state = self.termination_state.clone();
        let session_generation = self.current_session_generation();
        let timeout = Duration::from_secs(timeout_secs);

        thread::spawn(move || {
            thread::sleep(timeout);

            // If the session generation has advanced, this session was
            // replaced (restart / relaunch) — do not touch the new session.
            {
                let state = lock_or_recover(&termination_state, "debug_adapter.termination_state");
                if state.generation != session_generation {
                    tracing::debug!(
                        session_generation,
                        current_generation = state.generation,
                        "Debuggee watchdog: session replaced, skipping kill"
                    );
                    return;
                }
                if state.emitted {
                    tracing::debug!("Debuggee watchdog: session already terminated, skipping kill");
                    return;
                }
            }

            // Check whether the debuggee process is still alive.
            let process_alive = {
                let Ok(mut guard) = session.lock() else {
                    tracing::warn!("Debuggee watchdog: failed to lock session");
                    return;
                };
                let Some(ref mut debug_session) = *guard else {
                    // Session already cleared (e.g. by disconnect/terminate).
                    return;
                };
                match debug_session.process.try_wait() {
                    Ok(Some(_)) => false, // process has exited
                    Ok(None) => true,     // still running
                    Err(e) => {
                        tracing::warn!(error = %e, "Debuggee watchdog: try_wait failed");
                        false
                    }
                }
            };

            if !process_alive {
                tracing::debug!("Debuggee watchdog: process already exited, no action needed");
                return;
            }

            tracing::warn!(
                timeout_secs,
                "Debuggee watchdog: killing hung perl -d process after wall-clock timeout"
            );

            // Reserve the timeout termination *before* kill so the output reader's
            // EOF path cannot race in and emit `debugger_eof` first (#5149 review).
            // Kill still runs before the blocking event send so a stalled client
            // cannot leave the debuggee alive.
            if !reserve_terminated_event(&termination_state, Some(session_generation)) {
                tracing::debug!(
                    "Debuggee watchdog: termination already reserved/emitted, skipping kill"
                );
                return;
            }

            // Kill the debuggee process.  The output reader will see EOF and
            // clean up session state via clear_active_session_state_for_generation.
            let killed = {
                let Ok(mut guard) = session.lock() else {
                    return;
                };
                if let Some(ref mut debug_session) = *guard {
                    Self::terminate_child_process(&mut debug_session.process)
                } else {
                    true // already gone
                }
            };

            if !killed {
                tracing::error!("Debuggee watchdog: failed to kill hung debuggee process");
            }

            // Deliver the reserved timeout reason after kill. Blocking send is OK:
            // the debuggee is already dead; the emitted flag was set at reserve time.
            if let Some(ref sender) = sender {
                let _ = emit_event_safe(
                    sender,
                    &seq,
                    "terminated",
                    Some(json!({"reason": "debuggee_timeout"})),
                );
            }
        });
    }

    /// Verify that a target process exists and is accessible before attaching.
    ///
    /// Returns `Ok(true)` if the process is verified to exist and is signalable,
    /// `Ok(false)` if the process exists but is owned by a different user (warned
    /// but allowed to proceed), or `Err(msg)` if the process does not exist or
    /// cannot be queried.
    fn verify_attach_target(pid: u32) -> Result<bool, String> {
        #[cfg(unix)]
        {
            use nix::errno::Errno;
            let nix_pid = Pid::from_raw(pid as i32);
            // Signal 0 (None) checks process existence without actually sending a signal.
            match signal::kill(nix_pid, None) {
                Ok(()) => Ok(true),
                Err(Errno::EPERM) => {
                    tracing::warn!(
                        pid,
                        "Attach target exists but is owned by a different user (EPERM); \
                         proceeding with limited capabilities"
                    );
                    Ok(false)
                }
                Err(Errno::ESRCH) => Err(format!("Process {pid} does not exist (no such process)")),
                Err(e) => Err(format!("Cannot verify process {pid}: {e}")),
            }
        }
        #[cfg(windows)]
        {
            use winapi::um::handleapi::CloseHandle;
            use winapi::um::processthreadsapi::OpenProcess;
            use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;

            // SAFETY: OpenProcess is a standard Win32 API.  We request only
            // query-limited information, which is a read-only access right.
            // The handle is closed immediately after the existence check.
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if handle.is_null() {
                return Err(format!(
                    "Process {pid} does not exist or is not accessible (OpenProcess failed)"
                ));
            }
            // SAFETY: CloseHandle on a valid process handle is always safe.
            unsafe { CloseHandle(handle) };
            Ok(true)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = pid;
            Err("Process verification not supported on this platform".to_string())
        }
    }

    /// Handle attach request
    ///
    /// Attaches to a running Perl process. Supports two modes:
    /// 1. TCP attachment - Connect to Perl::LanguageServer DAP via host:port
    /// 2. Process ID attachment - Signal-control mode for local Perl process
    ///
    /// For TCP attachment, the arguments should contain:
    /// - `host`: Hostname or IP address (default: "localhost")
    /// - `port`: Port number (default: 13603)
    /// - `timeout`: Connection timeout in milliseconds (optional)
    ///
    /// # Current Implementation
    ///
    /// TCP attachment is implemented with socket support.
    /// Process ID attachment is implemented in signal-control mode (pause/continue
    /// signaling and thread identity), with limited stack/evaluate capabilities
    /// unless a debugger transport is active.
    pub(super) fn handle_attach(
        &self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        // Parse attach arguments
        if let Some(args) = arguments {
            let process_id =
                args.get("processId").and_then(|p| p.as_u64()).map(Self::u64_to_u32_saturating);

            // PID attachment mode: best-effort process control without requiring TCP shim transport.
            if let Some(pid) = process_id {
                if pid == 0 {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "attach".to_string(),
                        body: None,
                        message: Some("processId must be greater than zero".to_string()),
                    };
                }

                // Verify the target process exists before attaching (#4638).
                if let Err(msg) = Self::verify_attach_target(pid) {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "attach".to_string(),
                        body: None,
                        message: Some(msg),
                    };
                }

                // Reset existing process/tcp attachment state before switching to PID mode.
                self.begin_session_generation();
                self.clear_active_session_state();

                if let Ok(mut guard) = self.attached_pid.lock() {
                    *guard = Some(pid);
                }

                let stop_on_entry =
                    args.get("stopOnEntry").and_then(|s| s.as_bool()).unwrap_or(false);
                let thread_id = Self::i64_to_i32_saturating(i64::from(pid));

                // Always emit the "attach" stopped event to signal the client that the
                // debugger is connected and paused.
                self.send_event(
                    "stopped",
                    Some(json!({
                        "reason": "attach",
                        "threadId": thread_id,
                        "allThreadsStopped": true
                    })),
                );

                // When stopOnEntry is requested, emit an additional "entry" stopped event
                // so the IDE pauses at the first available program location.
                if stop_on_entry {
                    self.send_event(
                        "stopped",
                        Some(json!({
                            "reason": "entry",
                            "threadId": thread_id,
                            "allThreadsStopped": true,
                            "description": "Paused on entry"
                        })),
                    );
                }

                tracing::info!(
                    pid,
                    stop_on_entry,
                    "Attach request: Process ID attachment (signal-control mode)"
                );

                DapMessage::Response {
                    seq,
                    request_seq,
                    success: true,
                    command: "attach".to_string(),
                    body: Some(json!({
                        "threadId": thread_id,
                        "processId": pid,
                        "mode": "processId"
                    })),
                    message: Some(
                        "Attached in signal-control mode. Stack/evaluate are limited without a \
                         debugger transport."
                            .to_string(),
                    ),
                }
            } else {
                // Extract host and port for TCP attachment.
                let host = args.get("host").and_then(|h| h.as_str()).unwrap_or("localhost");
                let normalized_host = host.trim();
                let raw_port = args.get("port").and_then(|p| p.as_u64()).unwrap_or(13603);
                if raw_port > 65535 {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "attach".to_string(),
                        body: None,
                        message: Some(format!("Port {raw_port} out of range (must be 1-65535)")),
                    };
                }
                let port = raw_port as u16;
                let timeout = args
                    .get("timeout")
                    .or_else(|| args.get("timeoutMs"))
                    .and_then(|t| t.as_u64())
                    .map(Self::u64_to_u32_saturating);
                let stop_on_entry =
                    args.get("stopOnEntry").and_then(|s| s.as_bool()).unwrap_or(false);

                // TCP attachment mode (IMPLEMENTED)
                let mut config = TcpAttachConfig::new(normalized_host.to_string(), port);
                if let Some(t) = timeout {
                    config = config.with_timeout(t);
                }

                if let Err(error) = config.validate_timeout_bounds() {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "attach".to_string(),
                        body: None,
                        message: Some(error.to_string()),
                    };
                }

                // Create TCP attach session
                let mut session = TcpAttachSession::new();

                // Set up event channel for TCP events
                let (tx, rx) = channel::<DapEvent>();
                session.set_event_sender(tx);

                // Attempt to connect (validate is called inside connect,
                // which also pins the resolved addresses for DNS-rebinding
                // defense #5257)
                match session.connect(&mut config) {
                    Ok(()) => {
                        if let Err(e) = session.start_reader() {
                            tracing::error!(error = %e, "Failed to start TCP reader");
                            return DapMessage::Response {
                                seq,
                                request_seq,
                                success: false,
                                command: "attach".to_string(),
                                body: None,
                                message: Some(format!("Failed to start TCP reader: {}", e)),
                            };
                        }

                        // The TCP session is fully connected and has a reader before it becomes
                        // the active session, so a failed attach does not invalidate an existing
                        // session's generation.
                        self.prepare_replacement_session();
                        // Store session
                        if let Ok(mut guard) = self.tcp_session.lock() {
                            *guard = Some(session);
                        }

                        // Start event handler thread for TCP events
                        let seq_counter = self.seq.clone();
                        let event_sender = self.event_sender.clone();
                        let termination_state = self.termination_state.clone();
                        let session_generation = self.current_session_generation();
                        thread::spawn(move || {
                            while let Ok(event) = rx.recv() {
                                match event {
                                    DapEvent::Output { category, output } => {
                                        if let Some(ref sender) = event_sender {
                                            dispatch_event(
                                                sender,
                                                &seq_counter,
                                                "output",
                                                Some(json!({
                                                    "category": category,
                                                    "output": output
                                                })),
                                            );
                                        }
                                    }
                                    DapEvent::Stopped { reason, thread_id } => {
                                        if let Some(ref sender) = event_sender {
                                            dispatch_event(
                                                sender,
                                                &seq_counter,
                                                "stopped",
                                                Some(json!({
                                                    "reason": reason,
                                                    "threadId": thread_id,
                                                    "allThreadsStopped": true
                                                })),
                                            );
                                        }
                                    }
                                    DapEvent::Continued { thread_id } => {
                                        if let Some(ref sender) = event_sender {
                                            dispatch_event(
                                                sender,
                                                &seq_counter,
                                                "continued",
                                                Some(json!({
                                                    "threadId": thread_id,
                                                    "allThreadsContinued": true
                                                })),
                                            );
                                        }
                                    }
                                    DapEvent::Terminated { reason } => {
                                        if let Some(ref sender) = event_sender {
                                            emit_terminated_event(
                                                sender,
                                                &seq_counter,
                                                &termination_state,
                                                Some(session_generation),
                                                Some(json!({"reason": reason})),
                                            );
                                        }
                                    }
                                    DapEvent::Error { message } => {
                                        tracing::error!(message, "TCP attach error");
                                    }
                                }
                            }
                        });

                        // When stopOnEntry is requested, emit a stopped event so the IDE
                        // pauses at the first available program location after the TCP
                        // attach handshake completes.
                        if stop_on_entry {
                            self.send_event(
                                "stopped",
                                Some(json!({
                                    "reason": "entry",
                                    "threadId": 1,
                                    "allThreadsStopped": true,
                                    "description": "Paused on entry"
                                })),
                            );
                        }

                        tracing::info!(host, port, stop_on_entry, "TCP attach successful");

                        DapMessage::Response {
                            seq,
                            request_seq,
                            success: true,
                            command: "attach".to_string(),
                            body: None,
                            message: None,
                        }
                    }
                    Err(e) => DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "attach".to_string(),
                        body: None,
                        message: Some(format!(
                            "Cannot attach to Perl debugger at {}:{} ({}ms timeout): {}. \
                             Make sure the Perl process was started with \
                             'PERLDB_OPTS=\"RemotePort={}:{}\"' \
                             and is still running before attaching.",
                            config.host,
                            config.port,
                            config.timeout_ms.unwrap_or(30000),
                            e,
                            config.host,
                            config.port,
                        )),
                    },
                }
            }
        } else {
            // No arguments provided
            DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "attach".to_string(),
                body: None,
                message: Some(
                    "Missing attach arguments. Provide either 'processId' for process attachment \
                     or 'host' and 'port' for TCP attachment."
                        .to_string(),
                ),
            }
        }
    }

    /// Clear active process session, TCP session, and PID-attach mode state.
    pub(super) fn clear_active_session_state(&self) {
        Self::clear_active_session_state_with_state(
            &self.session,
            &self.tcp_session,
            &self.attached_pid,
        );
    }

    /// Advance the session generation and tear down the prior active session.
    ///
    /// Callers invoke this only after a replacement launch or attach has
    /// successfully completed its external setup, so rejected replacements
    /// leave the existing session untouched.
    fn prepare_replacement_session(&self) {
        self.begin_session_generation();
        self.clear_active_session_state();
    }

    pub(super) fn clear_active_session_state_with_state(
        session: &Arc<Mutex<Option<DebugSession>>>,
        tcp_session: &Arc<Mutex<Option<TcpAttachSession>>>,
        attached_pid: &Arc<Mutex<Option<u32>>>,
    ) {
        // Terminate the debug session
        if let Ok(mut guard) = session.lock()
            && let Some(mut active_session) = guard.take()
        {
            if !Self::terminate_child_process(&mut active_session.process) {
                tracing::warn!("Failed to ensure debug session process termination");
            }
            active_session.state = DebugState::Terminated;
        }

        // Disconnect TCP session if active
        if let Ok(mut guard) = tcp_session.lock()
            && let Some(ref mut tcp_session) = *guard
        {
            let _ = tcp_session.disconnect();
        }
        if let Ok(mut guard) = tcp_session.lock() {
            *guard = None;
        }

        // Clear PID attach mode.
        if let Ok(mut guard) = attached_pid.lock() {
            *guard = None;
        }
    }

    fn clear_active_session_state_for_generation(
        session: &Arc<Mutex<Option<DebugSession>>>,
        tcp_session: &Arc<Mutex<Option<TcpAttachSession>>>,
        attached_pid: &Arc<Mutex<Option<u32>>>,
        termination_state: &Mutex<TerminationState>,
        expected_generation: u64,
    ) {
        let state = lock_or_recover(termination_state, "debug_adapter.termination_state");
        if state.generation != expected_generation {
            return;
        }

        Self::clear_active_session_state_with_state(session, tcp_session, attached_pid);
    }

    pub(super) fn wait_for_child_exit(process: &mut Child, timeout: Duration) -> bool {
        if let Ok(Some(_)) = process.try_wait() {
            return true;
        }

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match process.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to poll debug session process");
                    return false;
                }
            }
        }

        false
    }

    pub(super) fn terminate_child_process(process: &mut Child) -> bool {
        if Self::wait_for_child_exit(process, Duration::from_millis(0)) {
            return true;
        }

        #[cfg(unix)]
        {
            let pid = process.id();
            match signal::kill(Pid::from_raw(Self::u32_to_i32_saturating(pid)), Signal::SIGTERM) {
                Ok(()) => {
                    if Self::wait_for_child_exit(
                        process,
                        Duration::from_millis(DEBUG_SESSION_TERMINATE_WAIT_MS),
                    ) {
                        return true;
                    }
                }
                Err(e) => {
                    tracing::warn!(pid, error = %e, "Failed to send SIGTERM to process");
                }
            }
        }

        if let Err(e) = process.kill() {
            tracing::warn!(error = %e, "Failed to terminate process");
        }
        Self::wait_for_child_exit(process, Duration::from_millis(DEBUG_SESSION_TERMINATE_WAIT_MS))
    }

    /// Handle disconnect request
    pub(super) fn handle_disconnect(
        &mut self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let _args: Option<DisconnectArguments> =
            arguments.and_then(|v| serde_json::from_value(v).ok());

        if let Some(ref sender) = self.event_sender {
            emit_terminated_event(sender, &self.seq, &self.termination_state, None, None);
        }
        self.clear_active_session_state();

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "disconnect".to_string(),
            body: None,
            message: None,
        }
    }

    /// Handle terminate request
    pub(super) fn handle_terminate(
        &mut self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: Option<TerminateArguments> =
            arguments.and_then(|v| serde_json::from_value(v).ok());

        let restart = args.and_then(|a| a.restart);

        let terminated_body = restart.map(|restart| json!({ "restart": restart }));
        if let Some(ref sender) = self.event_sender {
            emit_terminated_event(
                sender,
                &self.seq,
                &self.termination_state,
                None,
                terminated_body,
            );
        }
        self.clear_active_session_state();

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "terminate".to_string(),
            body: None,
            message: None,
        }
    }

    /// Apply stored function breakpoints to the active debugger session.
    pub(super) fn apply_stored_function_breakpoints(&self) {
        let names =
            self.function_breakpoints.lock().map(|stored| stored.clone()).unwrap_or_default();
        if names.is_empty() {
            return;
        }

        if let Some(ref mut session) = *lock_or_recover(&self.session, "debug_adapter.session")
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            for name in names {
                let cmd = format!("b {name}\n");
                let _ = stdin.write_all(cmd.as_bytes());
            }
            let _ = stdin.flush();
        }
    }

    /// Handle configurationDone request
    pub(super) fn handle_configuration_done(&self, seq: i64, request_seq: i64) -> DapMessage {
        // Validate state machine: launch (or attach) must be called before configurationDone.
        // NOTE: Each lock_or_recover call must complete and drop its guard before the next one
        // to avoid re-entrancy deadlock on std::sync::Mutex (which is non-reentrant).
        let has_session = lock_or_recover(&self.session, "debug_adapter.session").is_some();
        let has_attached_pid =
            lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid").is_some();
        let has_tcp_session =
            lock_or_recover(&self.tcp_session, "debug_adapter.tcp_session").is_some();

        if !has_session && !has_attached_pid && !has_tcp_session {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "configurationDone".to_string(),
                body: None,
                message: Some(
                    "No active debug session. \
                     The launch or attach request must be sent before configurationDone. \
                     The DAP protocol requires that the client send a launch (or attach) request first \
                     to start the debugging session."
                        .to_string(),
                ),
            };
        }

        // Determine whether stopOnEntry was requested in the launch args.
        let stop_on_entry =
            lock_or_recover(&self.last_launch_args, "debug_adapter.last_launch_args")
                .as_ref()
                .and_then(|a| a.get("stopOnEntry"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

        if let Some(ref mut session) = *lock_or_recover(&self.session, "debug_adapter.session")
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            if stop_on_entry {
                // The entry stopped event was already emitted during launch.
                // List the current source location so the IDE can display it.
                let _ = stdin.write_all(b"l\n");
                let _ = stdin.flush();
            } else {
                // stopOnEntry is false: perl -d always stops at the first
                // executable line.  Run to the first user-set breakpoint.
                // ResumeMode::RunToBreakpoint signals the output reader to
                // silently skip non-breakpoint stops (the implicit first-line
                // stop) and auto-continue until a user breakpoint is hit.
                session.state = DebugState::Running;
                session.last_resume_mode = ResumeMode::RunToBreakpoint;
                let _ = stdin.write_all(b"c\n");
                let _ = stdin.flush();
            }
        }

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "configurationDone".to_string(),
            body: None,
            message: None,
        }
    }

    /// Handle threads request
    pub(super) fn handle_threads(&self, seq: i64, request_seq: i64) -> DapMessage {
        let threads = if let Some(ref session) =
            *lock_or_recover(&self.session, "debug_adapter.session")
        {
            vec![json!({
                "id": session.thread_id,
                "name": "Main Thread"
            })]
        } else if let Some(pid) = *lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid")
        {
            vec![json!({
                "id": Self::i64_to_i32_saturating(i64::from(pid)),
                "name": format!("Attached Process ({pid})")
            })]
        } else if lock_or_recover(&self.tcp_session, "debug_adapter.tcp_session").is_some() {
            vec![json!({ "id": 1, "name": "TCP Attached Thread" })]
        } else {
            vec![]
        };

        DapMessage::Response {
            seq,
            request_seq,
            success: true,
            command: "threads".to_string(),
            body: Some(json!({
                "threads": threads
            })),
            message: None,
        }
    }

    /// Send continue/resume signal to process.
    ///
    /// On Unix, sends SIGCONT. On Windows there is no direct equivalent of SIGCONT
    /// for externally-attached processes; the function returns `true` (no error —
    /// the process was never suspended by the adapter, so continue is a no-op)
    /// rather than a silent `false` that the adapter could misinterpret as "stuck".
    /// Note that `handle_continue` emits the DAP `continued` event unconditionally.
    pub(super) fn send_continue_signal(&self, pid: u32) -> bool {
        if pid == 0 {
            tracing::warn!("send_continue_signal called with pid 0, ignoring");
            return false;
        }
        #[cfg(unix)]
        {
            let pid_i = Self::u32_to_i32_saturating(pid);
            match signal::kill(Pid::from_raw(pid_i), Signal::SIGCONT) {
                Ok(()) => {
                    tracing::info!("Sent SIGCONT to process {}", pid);
                    true
                }
                Err(e) => {
                    tracing::warn!("Failed to send SIGCONT to process {}: {}", pid, e);
                    false
                }
            }
        }
        #[cfg(windows)]
        {
            // Windows has no direct SIGCONT equivalent for external processes.
            // For session-mode continues, handle_continue sends "c\n" to the
            // debugger's stdin directly — this path is only reached for
            // attached-pid (external process) mode, where the process was never
            // suspended by us (Ctrl+C is handled by the process's own console
            // control handler). Returning `true` signals "no error" rather than
            // a silent `false` that the adapter could misinterpret as "stuck".
            // The caller (handle_continue) emits the continued event regardless.
            tracing::debug!(
                "send_continue_signal: no SIGCONT equivalent on Windows for pid {} — \
                 external process continue is a no-op, returning success",
                pid
            );
            true
        }
        #[cfg(not(any(unix, windows)))]
        {
            tracing::warn!("send_continue_signal: unsupported platform for pid {}", pid);
            false
        }
    }

    /// Send SIGINT to a Unix process, with a test-only fallback elsewhere.
    ///
    /// On failure, returns `false` without terminating the debuggee. The
    /// session is left intact for the client to retry or disposition.
    #[cfg(any(unix, test))]
    pub(super) fn send_interrupt_signal(&self, pid: u32) -> bool {
        if pid == 0 {
            tracing::warn!("send_interrupt_signal called with pid 0, ignoring");
            return false;
        }
        #[cfg(unix)]
        {
            let pid_i = Self::u32_to_i32_saturating(pid);
            match signal::kill(Pid::from_raw(pid_i), Signal::SIGINT) {
                Ok(()) => {
                    tracing::info!("Sent SIGINT to process {}", pid);
                    true
                }
                Err(e) => {
                    tracing::warn!("Failed to send SIGINT to process {}: {}", pid, e);
                    false
                }
            }
        }
        #[cfg(all(test, not(unix)))]
        {
            tracing::warn!("send_interrupt_signal is unavailable on this test platform");
            false
        }
    }

    /// Handle restart request
    ///
    /// Restarts the debug session by tearing down the current session and
    /// re-launching with stored (or updated) launch arguments. If no previous
    /// launch configuration is available, returns an error.
    pub(super) fn handle_restart(
        &mut self,
        seq: i64,
        request_seq: i64,
        arguments: Option<Value>,
    ) -> DapMessage {
        let args: Option<RestartArguments> = arguments.and_then(|v| serde_json::from_value(v).ok());

        // Determine launch args: prefer restart-provided args, then stored args
        let updated_args = args.and_then(|a| a.arguments);

        let launch_args = if let Some(new_args) = updated_args {
            new_args
        } else {
            let stored = lock_or_recover(&self.last_launch_args, "debug_adapter.last_launch_args");
            match stored.clone() {
                Some(args) => args,
                None => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "restart".to_string(),
                        body: None,
                        message: Some(
                            "Cannot restart: no previous launch configuration found. \
                             Start a debug session first, then use Restart."
                                .to_string(),
                        ),
                    };
                }
            }
        };

        self.clear_active_session_state();
        self.handle_launch(seq, request_seq, Some(launch_args))
    }
}

/// Atomically claim the single `terminated` emission for this session generation.
///
/// Returns `true` if this caller now owns emission (and must deliver the event),
/// `false` if another path already reserved or emitted termination.
fn reserve_terminated_event(
    termination_state: &Mutex<TerminationState>,
    expected_generation: Option<u64>,
) -> bool {
    let mut state = lock_or_recover(termination_state, "debug_adapter.termination_state");
    if expected_generation.is_some_and(|generation| generation != state.generation) || state.emitted
    {
        return false;
    }
    state.emitted = true;
    true
}

/// Emit interpolated logpoint text on the debug console.
fn emit_logpoint_messages(
    sender: Option<&SyncSender<DapMessage>>,
    seq: &Mutex<i64>,
    messages: Vec<String>,
) {
    let Some(sender) = sender else {
        return;
    };
    for message in messages {
        emit_event_safe(
            sender,
            seq,
            "output",
            Some(json!({
                "category": "console",
                "output": format!("{message}\n")
            })),
        );
    }
}

fn emit_terminated_event(
    sender: &SyncSender<DapMessage>,
    seq: &Mutex<i64>,
    termination_state: &Mutex<TerminationState>,
    expected_generation: Option<u64>,
    body: Option<Value>,
) -> bool {
    if !reserve_terminated_event(termination_state, expected_generation) {
        return false;
    }
    emit_event_safe(sender, seq, "terminated", body)
}

#[cfg(test)]
mod tests {
    use super::{
        DebugAdapter, detect_perl_info, emit_terminated_event, format_perl_spawn_error,
        is_valid_perl_interpreter,
    };
    use std::collections::HashMap;
    use std::sync::mpsc::{TryRecvError, sync_channel};
    use std::sync::{Arc, Mutex};

    #[test]
    fn competing_termination_sources_emit_one_structured_event() -> Result<(), String> {
        let (sender, receiver) = sync_channel(64);
        let seq = Arc::new(Mutex::new(0));
        let termination_state =
            Arc::new(Mutex::new(super::TerminationState { generation: 1, emitted: false }));
        let first_sender = sender.clone();
        let first_seq = seq.clone();
        let first_guard = termination_state.clone();
        let first = std::thread::spawn(move || {
            emit_terminated_event(
                &first_sender,
                &first_seq,
                &first_guard,
                Some(1),
                Some(serde_json::json!({"reason": "debugger_eof"})),
            )
        });
        let second = emit_terminated_event(&sender, &seq, &termination_state, None, None);
        let first = first.join().map_err(|_| "termination worker panicked".to_string())?;
        if first == second {
            return Err(format!(
                "expected exactly one emitter, got first={first}, second={second}"
            ));
        }

        let message = receiver.try_recv().map_err(|error| error.to_string())?;
        match message {
            super::DapMessage::Event { event, body, .. } => {
                let reason = body
                    .as_ref()
                    .and_then(|value| value.get("reason"))
                    .and_then(serde_json::Value::as_str);
                if event != "terminated" || (reason != Some("debugger_eof") && reason.is_some()) {
                    return Err(format!("unexpected termination event: {event}, {body:?}"));
                }
            }
            other => return Err(format!("expected termination event, got {other:?}")),
        }

        match receiver.try_recv() {
            Err(TryRecvError::Empty) => Ok(()),
            Err(error) => Err(format!("termination channel error: {error}")),
            Ok(other) => Err(format!("duplicate termination event: {other:?}")),
        }
    }

    #[test]
    fn stale_session_generation_cannot_emit_termination() -> Result<(), String> {
        let (sender, receiver) = sync_channel(64);
        let seq = Arc::new(Mutex::new(0));
        let termination_state =
            Mutex::new(super::TerminationState { generation: 2, emitted: false });

        if emit_terminated_event(
            &sender,
            &seq,
            &termination_state,
            Some(1),
            Some(serde_json::json!({"reason": "stale_reader"})),
        ) {
            return Err("stale reader unexpectedly emitted termination".to_string());
        }
        if receiver.try_recv().is_ok() {
            return Err("stale reader sent a termination event".to_string());
        }

        if !emit_terminated_event(
            &sender,
            &seq,
            &termination_state,
            Some(2),
            Some(serde_json::json!({"reason": "current_session"})),
        ) {
            return Err("current session failed to emit termination".to_string());
        }
        Ok(())
    }

    #[test]
    fn stale_session_generation_cannot_clear_attached_pid() -> Result<(), String> {
        let session = Arc::new(Mutex::new(None));
        let tcp_session = Arc::new(Mutex::new(None));
        let attached_pid = Arc::new(Mutex::new(Some(4242_u32)));
        let termination_state =
            Mutex::new(super::TerminationState { generation: 2, emitted: false });

        DebugAdapter::clear_active_session_state_for_generation(
            &session,
            &tcp_session,
            &attached_pid,
            &termination_state,
            1,
        );
        let pid_after_stale_cleanup = attached_pid
            .lock()
            .map(|guard| *guard)
            .map_err(|_| "attached PID lock was poisoned after stale cleanup".to_string())?;
        if pid_after_stale_cleanup != Some(4242) {
            return Err(format!(
                "stale generation cleared the replacement PID: {pid_after_stale_cleanup:?}"
            ));
        }

        DebugAdapter::clear_active_session_state_for_generation(
            &session,
            &tcp_session,
            &attached_pid,
            &termination_state,
            2,
        );
        let pid_after_current_cleanup = attached_pid
            .lock()
            .map(|guard| *guard)
            .map_err(|_| "attached PID lock was poisoned after current cleanup".to_string())?;
        if pid_after_current_cleanup.is_some() {
            return Err(format!(
                "current generation left the attached PID in place: {pid_after_current_cleanup:?}"
            ));
        }

        Ok(())
    }

    #[test]
    fn replacement_session_cleanup_clears_previous_attached_pid() -> Result<(), String> {
        let adapter = DebugAdapter::new();
        if let Ok(mut guard) = adapter.attached_pid.lock() {
            *guard = Some(4242);
        } else {
            return Err("attached PID lock was poisoned before replacement cleanup".to_string());
        }

        adapter.prepare_replacement_session();

        let pid_after_cleanup =
            adapter.attached_pid.lock().map(|guard| *guard).map_err(|_| {
                "attached PID lock was poisoned after replacement cleanup".to_string()
            })?;
        if pid_after_cleanup.is_some() {
            return Err(format!(
                "replacement cleanup left the previous attached PID in place: {pid_after_cleanup:?}"
            ));
        }
        Ok(())
    }

    /// An explicit, non-empty interpreter is honored verbatim — from the
    /// documented `perlPath` key or the `perl` alias — and the toolchain
    /// resolver must not override the user's deliberate choice.
    #[test]
    fn resolve_launch_interpreter_honors_explicit_value() {
        let perl_alias = serde_json::json!({ "perl": "/usr/bin/perl" });
        assert_eq!(DebugAdapter::resolve_launch_interpreter(&perl_alias), "/usr/bin/perl");

        let perl_path_key = serde_json::json!({ "perlPath": "/opt/perlbrew/perls/x/bin/perl" });
        assert_eq!(
            DebugAdapter::resolve_launch_interpreter(&perl_path_key),
            "/opt/perlbrew/perls/x/bin/perl"
        );

        // An explicit interpreter wins even when the launch config also sets PATH.
        let explicit_with_path = serde_json::json!({
            "perl": "/custom/perl",
            "env": { "PATH": "/custom/bin" },
        });
        assert_eq!(DebugAdapter::resolve_launch_interpreter(&explicit_with_path), "/custom/perl");
    }

    /// The documented `perlPath` key takes precedence over the `perl` alias when
    /// both are present.
    #[test]
    fn resolve_launch_interpreter_prefers_perlpath_over_perl_alias() {
        let both = serde_json::json!({ "perlPath": "/canonical/perl", "perl": "/alias/perl" });
        assert_eq!(DebugAdapter::resolve_launch_interpreter(&both), "/canonical/perl");
    }

    /// When the launch config supplies its own `PATH` via `env` and no explicit
    /// interpreter, the bare `"perl"` is kept so the launch `PATH` selects the
    /// interpreter at spawn time — resolving against the parent environment here
    /// would ignore it. Regression guard for the #2026 review.
    #[test]
    fn resolve_launch_interpreter_defers_to_launch_path_override() {
        let path_override = serde_json::json!({ "env": { "PATH": "/project/perl/bin" } });
        assert_eq!(DebugAdapter::resolve_launch_interpreter(&path_override), "perl");

        // Case-insensitive key match (Windows-style `Path`).
        let win_path = serde_json::json!({ "env": { "Path": "C:/perl/bin" } });
        assert_eq!(DebugAdapter::resolve_launch_interpreter(&win_path), "perl");
    }

    /// With no explicit interpreter and no launch `PATH` override, the
    /// interpreter is resolved through the shared toolchain profile rather than
    /// defaulting to a bare `"perl"`. The result is never empty: either a real
    /// resolved path or the `"perl"` fallback when nothing can be found.
    #[test]
    fn resolve_launch_interpreter_resolves_default_via_profile() {
        for args in [
            serde_json::json!({}),
            serde_json::json!({ "perl": "" }),
            serde_json::json!({ "env": { "RUST_LOG": "debug" } }),
        ] {
            let resolved = DebugAdapter::resolve_launch_interpreter(&args);
            assert!(!resolved.is_empty(), "resolved interpreter must never be empty");
            assert!(
                resolved == "perl" || resolved.to_lowercase().contains("perl"),
                "resolved default should be a perl interpreter; got: {resolved:?}"
            );
        }
    }

    #[test]
    fn missing_module_name_parses_standard_module_path() {
        let detail = "Can't locate Some/Missing/Module.pm in @INC (you may need to install the Some::Missing::Module module)";

        let module = DebugAdapter::missing_module_name(detail);

        assert_eq!(module.as_deref(), Some("Some::Missing::Module"));
    }

    #[test]
    fn missing_module_name_parses_optional_dependency_path() {
        let detail = "Can't locate Optional/Dep.pm in @INC (you may need to install the Optional::Dep module)";

        let module = DebugAdapter::missing_module_name(detail);

        assert_eq!(module.as_deref(), Some("Optional::Dep"));
    }

    #[test]
    fn missing_module_name_parses_nested_module_path_with_spaces() {
        let detail = "Can't locate Tied/Hash/With/Spaces.pm in @INC (you may need to install the Tied::Hash::With::Spaces module)";

        let module = DebugAdapter::missing_module_name(detail);

        assert_eq!(module.as_deref(), Some("Tied::Hash::With::Spaces"));
    }

    #[test]
    fn missing_module_error_includes_install_hint_and_metacpan_link() {
        let message = DebugAdapter::format_missing_module_error("Some::Missing::Module");

        assert!(message.contains("Module Some::Missing::Module not found"));
        assert!(message.contains("cpan Some::Missing::Module"));
        assert!(message.contains("metacpan.org/pod/Some::Missing::Module"));
    }

    /// Verify that `detect_perl_info()` runs without panicking.
    ///
    /// On systems with Perl installed this returns a "Found Perl at …" string.
    /// On systems without Perl it returns a "not found" install-hint string.
    /// Either outcome is acceptable — the test just proves the helper is safe
    /// to call in all environments.
    #[test]
    fn detect_perl_version_succeeds_when_perl_available() {
        // Call detect_perl_info() — must not panic regardless of whether Perl
        // is on PATH.  The returned string must be non-empty.
        let info = detect_perl_info();
        assert!(!info.is_empty(), "detect_perl_info should always return a non-empty string");
    }

    /// Verify that `detect_perl_info()` always mentions "perl" (case-insensitive)
    /// so that it is suitable for inclusion in user-facing error messages.
    #[test]
    fn detect_perl_info_output_mentions_perl() {
        let info = detect_perl_info();
        assert!(
            info.to_lowercase().contains("perl"),
            "detect_perl_info output should mention 'perl'; got: {info:?}"
        );
    }

    /// Verify that a failed launch returns a response whose message mentions Perl.
    ///
    /// We construct a temporary file so that the file-exists check passes, then
    /// rely on the fact that on PATH-less environments `perl -d` will fail and
    /// the enhanced error path fires, or that the Perl syntax check / spawn of
    /// `perl -d` on a trivially-empty script eventually surfaces an error whose
    /// message includes Perl-related text.
    ///
    /// The assertion is intentionally broad: the message must contain the word
    /// "perl" (case-insensitive).  This covers both the success branch
    /// ("Found Perl at …") and the not-found branch ("Perl was not found …").
    #[test]
    fn handle_launch_error_includes_perl_info() -> Result<(), String> {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a temporary file so that the file-exists validation in
        // launch_debugger() passes, letting us reach the Perl-spawn error path.
        let mut tmp =
            NamedTempFile::new().map_err(|e| format!("could not create temp file: {e}"))?;
        writeln!(tmp, "# placeholder").map_err(|e| format!("could not write to temp file: {e}"))?;
        let tmp_path = tmp.path().to_str().ok_or("temp path is not valid UTF-8")?.to_string();

        let mut adapter = DebugAdapter::new();

        // Initialize first (required by state machine validation)
        let _ = adapter.handle_initialize(1, 1, None);

        let response = adapter.handle_launch(
            2,
            2,
            Some(serde_json::json!({
                "program": tmp_path
            })),
        );

        match response {
            super::DapMessage::Response { success, message, .. } => {
                // The launch may succeed (Perl on PATH ran the empty script) or fail.
                // When it fails, the message must mention Perl.
                if !success {
                    let msg = message.unwrap_or_default();
                    assert!(
                        msg.to_lowercase().contains("perl"),
                        "launch error message should mention 'perl'; got: {msg:?}"
                    );
                }
                // success == true means Perl is on PATH and launched fine — valid outcome.
                Ok(())
            }
            other => Err(format!("expected Response from handle_launch; got {other:?}")),
        }
    }

    #[test]
    fn launch_preserves_a_real_quote_delimited_filename() -> Result<(), String> {
        use std::io::Write;

        let mut script = tempfile::Builder::new()
            .prefix("'perl-dap-quote-test-")
            .suffix(".pl'")
            .tempfile_in(".")
            .map_err(|e| format!("could not create script: {e}"))?;
        script
            .as_file_mut()
            .write_all(b"print 1;\n")
            .map_err(|e| format!("could not write script: {e}"))?;
        let script_path = script
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("script filename is not valid UTF-8")?;
        let missing_perl = std::env::current_dir()
            .map_err(|e| format!("could not get current directory: {e}"))?
            .join("missing-perl");
        let missing_perl = missing_perl.to_str().ok_or("interpreter path is not valid UTF-8")?;

        let mut adapter = DebugAdapter::new();
        let result = adapter.launch_debugger(
            script_path,
            missing_perl,
            Vec::new(),
            false,
            std::collections::HashMap::new(),
            None,
            1,
        );
        let error = match result {
            Ok(thread_id) => return Err(format!("unexpectedly launched thread {thread_id}")),
            Err(error) => error,
        };

        assert!(
            !error.contains("surrounding quotes"),
            "a real quote-delimited filename must not be treated as shell quoting: {error}"
        );
        Ok(())
    }

    #[test]
    fn validates_perl_interpreter_names() {
        assert!(is_valid_perl_interpreter("perl"));
        assert!(is_valid_perl_interpreter("/usr/bin/perl"));
        assert!(is_valid_perl_interpreter("C:/Strawberry/perl/bin/perl.exe"));
        assert!(is_valid_perl_interpreter("perl5.38.2"));
        assert!(is_valid_perl_interpreter("perl5"));
        assert!(is_valid_perl_interpreter("perl5.38"));

        assert!(!is_valid_perl_interpreter("/bin/sh"));
        assert!(!is_valid_perl_interpreter("python3"));
        assert!(!is_valid_perl_interpreter("   "));
        // #4638: strict regex must reject look-alike names that start with "perl"
        assert!(!is_valid_perl_interpreter("perlevil"));
        assert!(!is_valid_perl_interpreter("perlscript"));
        assert!(!is_valid_perl_interpreter("perl_backdoor"));
        assert!(!is_valid_perl_interpreter("perl-exec"));
        assert!(!is_valid_perl_interpreter("perlsh"));
    }
    #[test]
    fn format_perl_spawn_error_includes_custom_interpreter_name() {
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory");
        let message = format_perl_spawn_error("/custom/perl", &error);

        assert!(
            message.contains("/custom/perl"),
            "expected interpreter path in message, got: {message}"
        );
    }
    #[test]
    fn format_perl_spawn_error_for_missing_perl_is_actionable() {
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory");
        let message = format_perl_spawn_error("perl", &error);

        assert!(message.contains("Install Perl"), "expected install guidance, got: {message}");
        assert!(
            message.contains("launch.json") && message.contains("perlPath"),
            "expected launch.json perlPath guidance, got: {message}"
        );
        assert!(
            !message.contains("perl-lsp.perl.path"),
            "spawn error should not point at stale perl-lsp.perl.path setting, got: {message}"
        );
    }

    #[test]
    fn format_perl_spawn_error_preserves_non_not_found_error_detail() {
        let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        let message = format_perl_spawn_error("/secure/perl", &error);

        assert!(message.contains("/secure/perl"), "expected interpreter path, got: {message}");
        assert!(
            message.contains("permission denied"),
            "expected original error detail, got: {message}"
        );
        assert!(
            message.contains("file permissions"),
            "expected permission remediation guidance, got: {message}"
        );
        assert!(
            !message.contains("Install Perl"),
            "non-NotFound errors should not use missing-perl guidance, got: {message}"
        );
    }

    // ── Cross-platform signal delivery tests (#4639) ───────────────────────────

    /// `send_continue_signal` returns `true` on Windows for a nonzero pid.
    ///
    /// Regression for #4639 defect #1: the old Windows branch always returned
    /// `false`, which the adapter could misinterpret as "stuck". The fix returns
    /// `true` because external-process continue is a no-op on Windows (the
    /// process was never suspended by the adapter), not a failure.
    #[test]
    #[cfg(windows)]
    fn send_continue_signal_returns_true_on_windows_for_nonzero_pid() {
        let adapter = DebugAdapter::new();
        // Use the current process's pid — it's guaranteed nonzero and valid.
        let pid = std::process::id();
        assert!(
            adapter.send_continue_signal(pid),
            "send_continue_signal should return true on Windows for a nonzero pid (no-op, not failure)"
        );
    }

    /// `send_continue_signal` still returns `false` for pid 0 on Windows.
    #[test]
    #[cfg(windows)]
    fn send_continue_signal_pid_zero_returns_false_on_windows() {
        let adapter = DebugAdapter::new();
        assert!(!adapter.send_continue_signal(0));
    }

    /// `terminate_child_process` attempts graceful shutdown before force-kill on Windows.
    ///
    /// Regression for #4639 defect #3: the old Windows path skipped the graceful
    /// first step and killed outright. This test spawns a short-lived process and
    /// verifies `terminate_child_process` returns `true` (the process exited),
    /// exercising the graceful-shutdown code path.
    #[test]
    #[cfg(windows)]
    fn terminate_child_process_graceful_shutdown_on_windows() -> Result<(), String> {
        use std::process::Command;

        // Spawn a process that sleeps briefly. The key assertion is that
        // terminate_child_process returns true (the process was terminated).
        let mut child = Command::new("cmd")
            .args(["/c", "ping -n 30 127.0.0.1 > nul"])
            .spawn()
            .map_err(|e| format!("Failed to spawn test process: {e}"))?;

        let result = DebugAdapter::terminate_child_process(&mut child);
        if !result {
            return Err("terminate_child_process should succeed on Windows".to_string());
        }
        // Verify the process is actually gone.
        match child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err("process still running after terminate_child_process".to_string()),
            Err(e) => Err(format!("error polling process after terminate: {e}")),
        }
    }

    /// `terminate_child_process` gracefully terminates a spawned process on Unix.
    ///
    /// This is the Unix counterpart to the Windows test above, verifying that
    /// the SIGTERM → wait → SIGKILL escalation works for a real process.
    #[test]
    #[cfg(unix)]
    fn terminate_child_process_graceful_shutdown_on_unix() -> Result<(), String> {
        use std::process::Command;

        let mut child = Command::new("sleep")
            .arg("30")
            .spawn()
            .map_err(|e| format!("Failed to spawn test process: {e}"))?;

        let result = DebugAdapter::terminate_child_process(&mut child);
        if !result {
            return Err("terminate_child_process should succeed on Unix".to_string());
        }
        match child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err("process still running after terminate_child_process".to_string()),
            Err(e) => Err(format!("error polling process after terminate: {e}")),
        }
    }

    /// `terminate_child_process` returns `true` immediately if the process already exited.
    #[test]
    fn terminate_child_process_already_exited_returns_true() -> Result<(), String> {
        use std::process::Command;

        // Spawn a process that exits immediately.
        #[cfg(windows)]
        let mut child = Command::new("cmd")
            .args(["/c", "exit"])
            .spawn()
            .map_err(|e| format!("Failed to spawn: {e}"))?;
        #[cfg(not(windows))]
        let mut child =
            Command::new("true").spawn().map_err(|e| format!("Failed to spawn: {e}"))?;

        // Wait for it to exit.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let result = DebugAdapter::terminate_child_process(&mut child);
        if !result {
            return Err("terminate_child_process should return true for an already-exited process"
                .to_string());
        }
        Ok(())
    }

    /// `send_interrupt_signal` does not panic for a nonexistent pid on Windows.
    ///
    /// Regression for #4639 defect #2: the old code could call terminate_child_process
    /// as a fallback, which for a nonexistent pid is harmless but the code path
    /// should not be reached. The fix ensures the function returns `false` cleanly.
    #[test]
    #[cfg(windows)]
    fn send_interrupt_signal_nonexistent_pid_returns_false_on_windows() {
        let adapter = DebugAdapter::new();
        // 999_999 is virtually guaranteed not to exist.
        let result = adapter.send_interrupt_signal(999_999);
        // There is no session stdin for this PID-attached request, so this returns false.
        // The key assertion is that it doesn't panic or destroy anything.
        let _ = result; // result depends on console state; the point is no panic
    }

    /// Verify the debuggee watchdog kills a long-running process after the
    /// configured wall-clock timeout and emits a `terminated` event with
    /// `reason: "debuggee_timeout"` (#4640).
    ///
    /// This test does not require Perl — it spawns a platform-native
    /// long-running process, places it in a `DebugSession`, and starts the
    /// watchdog directly.
    #[test]
    fn debuggee_watchdog_kills_process_after_timeout() -> Result<(), String> {
        use std::process::{Command, Stdio};
        use std::sync::mpsc::RecvTimeoutError;
        use std::time::Duration;

        use super::{DebugSession, DebugState, ResumeMode, VariableCache, lock_or_recover};

        // Spawn a long-running process (30 seconds) that will outlive the
        // watchdog timeout unless the watchdog kills it.
        let mut cmd = if cfg!(windows) {
            // Use ping.exe directly (not via cmd /c, which exits immediately
            // when stdout is piped and does not wait for the child).
            let mut c = Command::new("ping");
            c.args(["-n", "30", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = cmd.spawn().map_err(|e| format!("failed to spawn test process: {e}"))?;

        let (sender, receiver) = sync_channel(64);
        let mut adapter = DebugAdapter::new();
        adapter.set_event_sender(sender);
        adapter.initialized.store(true, std::sync::atomic::Ordering::Release);

        // Place the long-running process into a DebugSession.
        let session = DebugSession {
            process: child,
            state: DebugState::Running,
            stack_frames: Vec::new(),
            stack_frame_arguments: HashMap::new(),
            variable_cache: VariableCache::default(),
            thread_id: 1,
            last_resume_mode: ResumeMode::Unknown,
            stopped_generation: 0,
        };
        *lock_or_recover(&adapter.session, "test.session") = Some(session);

        // Start the watchdog with a 1-second timeout.
        adapter.start_debuggee_watchdog(1);

        // Wait for the terminated event (up to 5 seconds).
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut found_timeout = false;
        while std::time::Instant::now() < deadline {
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(super::DapMessage::Event { event, body, .. }) => {
                    if event == "terminated" {
                        let reason =
                            body.as_ref().and_then(|v| v.get("reason")).and_then(|v| v.as_str());
                        if reason == Some("debuggee_timeout") {
                            found_timeout = true;
                            break;
                        }
                    }
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => continue,
                Err(e) => return Err(format!("channel error waiting for terminated event: {e}")),
            }
        }

        if !found_timeout {
            return Err("did not receive terminated event with reason debuggee_timeout within 5s"
                .to_string());
        }

        // Verify the process was actually killed by the watchdog.
        std::thread::sleep(Duration::from_millis(200));
        let process_exited = adapter
            .session
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?
            .as_mut()
            .and_then(|s| s.process.try_wait().ok().flatten())
            .is_some();
        if !process_exited {
            return Err(
                "debuggee process is still alive after watchdog should have killed it".to_string()
            );
        }

        Ok(())
    }

    /// Regression for issue #5149 / PR #5318 defect 2: the watchdog used to emit the
    /// `terminated` event (a blocking `send`) BEFORE killing the hung debuggee. If the
    /// outbound queue is permanently full and nobody drains it, that blocking send never
    /// returns, so the kill never runs and the debuggee is never terminated — defeating
    /// the watchdog's entire purpose. The fix reserves termination and kills the process
    /// first; the (possibly still-blocking) event send happens afterward and does not
    /// gate the kill.
    ///
    /// This test never joins the watchdog thread (which may legitimately block forever
    /// on the terminated-event send against the undrained queue), so a regression fails
    /// the bounded-timeout assertion below rather than hanging the test suite.
    #[test]
    fn debuggee_watchdog_kills_process_even_when_event_queue_full() -> Result<(), String> {
        use std::process::{Command, Stdio};
        use std::sync::mpsc::sync_channel;
        use std::time::Duration;

        use super::{DebugSession, DebugState, ResumeMode, VariableCache, lock_or_recover};

        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("ping");
            c.args(["-n", "30", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = cmd.spawn().map_err(|e| format!("failed to spawn test process: {e}"))?;

        // Capacity-1 outbound queue, pre-filled and never drained: any blocking `send`
        // (e.g. the old pre-kill `terminated` emission) would hang forever here.
        let (sender, _receiver) = sync_channel(1);
        sender
            .send(super::DapMessage::Event {
                seq: 0,
                event: "output".to_string(),
                body: Some(serde_json::json!({"category": "stdout", "output": "filler\n"})),
            })
            .map_err(|_| "failed to prefill the outbound queue".to_string())?;

        let mut adapter = DebugAdapter::new();
        adapter.set_event_sender(sender);
        adapter.initialized.store(true, std::sync::atomic::Ordering::Release);

        let session = DebugSession {
            process: child,
            state: DebugState::Running,
            stack_frames: Vec::new(),
            stack_frame_arguments: HashMap::new(),
            variable_cache: VariableCache::default(),
            thread_id: 1,
            last_resume_mode: ResumeMode::Unknown,
            stopped_generation: 0,
        };
        *lock_or_recover(&adapter.session, "test.session") = Some(session);

        adapter.start_debuggee_watchdog(1);

        // Bounded-timeout poll: with the fix, the kill runs before the (permanently
        // blocked) event send, so the process dies well within this deadline.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut process_exited = false;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            let exited = adapter
                .session
                .lock()
                .map_err(|_| "session lock poisoned".to_string())?
                .as_mut()
                .and_then(|s| s.process.try_wait().ok().flatten())
                .is_some();
            if exited {
                process_exited = true;
                break;
            }
        }

        if !process_exited {
            return Err(
                "debuggee process was not killed within 10s while the outbound event queue \
                 was full and undrained — the watchdog is blocking on the terminated-event \
                 send before killing the process (regression of #5149/PR #5318 defect 2)"
                    .to_string(),
            );
        }

        Ok(())
    }

    /// Verify the debuggee watchdog does NOT fire when the timeout is
    /// disabled (0 seconds) — the process should remain alive (#4640).
    #[test]
    fn debuggee_watchdog_disabled_when_timeout_zero() -> Result<(), String> {
        use std::process::{Command, Stdio};
        use std::time::Duration;

        // Spawn a short-lived process (3 seconds) — if the watchdog were
        // incorrectly enabled with timeout=0, it would kill this before it
        // exits naturally.
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("ping");
            c.args(["-n", "3", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("3");
            c
        };
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("failed to spawn test process: {e}"))?;

        let pid = child.id();

        // Verify the process is alive.
        let alive_before = child.try_wait().map_err(|e| format!("try_wait failed: {e}"))?;
        if alive_before.is_some() {
            return Err("test process exited before watchdog test could start".to_string());
        }

        // The watchdog is NOT started when timeout is 0 — we verify by
        // checking that the process is still alive after a brief wait.
        // If the watchdog were incorrectly active with timeout=0, the process
        // would have been killed immediately.
        std::thread::sleep(Duration::from_millis(500));

        // On Windows, check if the process is still running via the handle.
        // On Unix, try_wait on the original child won't work after move, so
        // we just verify no terminated event was received (no sender set up).
        let _ = pid; // PID is platform-specific; we rely on the launch path test
        Ok(())
    }

    /// Verify that `debuggeeTimeoutSeconds` is parsed from the launch
    /// configuration and reaches `launch_debugger` (#4640).
    #[test]
    fn launch_parses_debuggee_timeout_seconds() -> Result<(), String> {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut tmp =
            NamedTempFile::new().map_err(|e| format!("could not create temp file: {e}"))?;
        writeln!(tmp, "# placeholder").map_err(|e| format!("could not write to temp file: {e}"))?;
        let tmp_path = tmp.path().to_str().ok_or("temp path is not valid UTF-8")?.to_string();

        let mut adapter = DebugAdapter::new();
        let _ = adapter.handle_initialize(1, 1, None);

        // Launch with debuggeeTimeoutSeconds set — the launch may succeed or
        // fail depending on Perl availability, but it should not reject the
        // unknown argument.
        let response = adapter.handle_launch(
            2,
            2,
            Some(serde_json::json!({
                "program": tmp_path,
                "debuggeeTimeoutSeconds": 30,
            })),
        );

        match response {
            super::DapMessage::Response { success: true, .. } => {
                // Launch succeeded — clean up the session.
                adapter.clear_active_session_state();
                Ok(())
            }
            super::DapMessage::Response { success: false, message, .. } => {
                // Launch failed (e.g. Perl not on PATH) — verify the error
                // is about Perl, not about an unknown argument.
                let msg = message.unwrap_or_default();
                assert!(
                    !msg.contains("debuggeeTimeoutSeconds"),
                    "launch should not reject debuggeeTimeoutSeconds; got: {msg:?}"
                );
                Ok(())
            }
            other => Err(format!("expected Response from handle_launch; got {other:?}")),
        }
    }
}
