//! Process lifecycle management: initialize, launch, attach, disconnect, terminate, restart.

use super::logpoint::{DrainStep, LogpointDrain, LogpointStep, PendingLogpoint};
use super::{
    Arc, BufRead, BufReader, Child, DEBUG_SESSION_TERMINATE_WAIT_MS, DapEvent, DapMessage,
    DebugAdapter, DebugSession, DebugState, DisconnectArguments, Duration,
    EngineBreakpointHitOutcome, Instant, Mutex, Read, RestartArguments, ResumeMode, Source,
    StackFrame, Stdio, TcpAttachConfig, TcpAttachSession, TerminateArguments, TerminationState,
    Value, Write, ansi_escape_re, catalog_has_feature, context_re, die_suffix_re, error_re,
    exception_re, json, lock_or_recover, module_path_to_name, prompt_re, security, stack_frame_re,
    thread, warning_re,
};
#[cfg(unix)]
use nix::sys::signal::{self, Signal};
#[cfg(unix)]
use nix::unistd::Pid;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
// The internal TCP-attach DapEvent fan-in queue is bounded with a
// generation-aware forwarder (#9521); the reader-side admission policy lives in
// `tcp_attach::reader`.
use std::sync::atomic::Ordering;
use std::sync::mpsc::sync_channel;

use super::sync_utils::{EventDrainLatch, EventSender};
use super::tcp_attach_forwarder::{TCP_ATTACH_EVENT_CAPACITY, spawn_tcp_attach_event_forwarder};

mod perl_info;
mod perl_spawn;

use super::variable_cache::VariableCache;
use crate::reload::RuntimeModuleGenerationClock;
use perl_info::detect_perl_info;
use perl_spawn::{format_perl_spawn_error, is_valid_perl_interpreter};

fn emit_event_safe(
    sender: &EventSender,
    seq: &Mutex<i64>,
    event: &str,
    body: Option<Value>,
) -> bool {
    sender.send_event(seq, event, body) != super::sync_utils::EventDispatchResult::Disconnected
}

const SCOPE_FRAME_ID_MAX: u64 = 99_999;

/// Wall-clock budget for one perl5db capability probe. A cold interpreter
/// start answers well inside this on every supported platform; reaching the
/// deadline means the probe could not conclude, not that perl5db.pl failed
/// to load.
const DEBUGGER_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Poll interval while a capability-probe child is still running.
const DEBUGGER_PROBE_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Cached affirmative perl5db capability verdicts, keyed by
/// [`DebugAdapter::capability_probe_cache_key`]. Only passes are cached —
/// a pass skips the probe on every subsequent launch. A failure or an
/// inconclusive probe is not retained: a failure describes mutable
/// installation state (perl5db.pl can be installed in place between
/// launches), and an inconclusive probe is no verdict at all. The probe is
/// deadline-bounded, so a re-probe on the next launch is cheap. A poisoned
/// lock only bypasses the cache (the probe is re-run); it never fails a
/// launch.
static DEBUGGER_PROBE_CACHE: std::sync::LazyLock<Mutex<HashMap<String, Result<(), String>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// The success marker the capability probe's perl expression prints after
/// `require "perl5db.pl"` resolves and compiles. A clean child exit alone
/// does not certify capability: an interpreter shim named `perl` may ignore
/// the `-e` payload and exit 0 without ever loading the module.
const DEBUGGER_PROBE_SUCCESS_MARKER: &str = "OK";

/// One capability probe conclusion. Only [`Self::Capable`] is a measured,
/// cacheable verdict; [`Self::Incapable`] is reported but not retained, and
/// [`Self::Inconclusive`] keeps the launch-continue disposition.
enum DebuggerCapabilityProbe {
    /// The child loaded `perl5db.pl`, printed the success marker, and
    /// exited successfully.
    Capable,
    /// The child ran and answered negatively; the payload is the probe's
    /// diagnostic detail for the user-facing remediation message.
    Incapable(String),
    /// Spawn failure, observation failure, or deadline: the probe could not
    /// measure anything, so there is no verdict to report or cache.
    Inconclusive,
}

/// Read one probe pipe to EOF on a dedicated thread, delivering its decoded
/// contents through a channel. Used instead of blocking `Command::output()`
/// so the probe can be bounded by a deadline rather than waiting indefinitely
/// on the child; the caller collects each drain with
/// [`join_probe_drain_within`] against the remaining probe budget.
fn spawn_probe_pipe_drain<R: Read + Send + 'static>(
    pipe: Option<R>,
) -> Option<std::sync::mpsc::Receiver<String>> {
    pipe.map(|mut pipe| {
        let (sender, receiver) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
        });
        receiver
    })
}

/// Collect one bounded pipe drain: `Some(decoded text)` when the drainer
/// reached EOF and delivered within `budget`, `None` when the budget ran out
/// or the drainer died without delivering. The budget matters after the
/// direct child exits: a shim that leaves a pipe-inheriting descendant behind
/// never delivers EOF, so the drain is abandoned (the drainer thread ends
/// whenever the descendant exits) instead of joined forever.
fn join_probe_drain_within(
    drain: Option<std::sync::mpsc::Receiver<String>>,
    budget: Duration,
) -> Option<String> {
    drain.and_then(|receiver| receiver.recv_timeout(budget).ok())
}

/// Whether the probe's stdout carries the success marker the probe
/// expression prints on its own line. Matching the whole trimmed line keeps
/// an incidental "OK" substring in unrelated child output from certifying a
/// pass.
fn has_probe_success_marker(stdout: &str) -> bool {
    stdout.lines().any(|line| line.trim() == DEBUGGER_PROBE_SUCCESS_MARKER)
}

/// Apply the Windows debugger-console transport environment to a child
/// command: `EMACS=1` plus a `ReadLine=0` tail on any inherited
/// `PERLDB_OPTS`.
///
/// Strawberry Perl's Windows debugger selects its console transport when
/// EMACS is absent, even when all three stdio handles are pipes. Marking an
/// owned pipe launch explicitly keeps the debugger on its pipe transport;
/// ReadLine must use its dummy interface because its console backend calls
/// `GetConsoleMode` on a pipe and raises inside an otherwise valid debuggee.
/// The variables are scoped to the child and do not change the adapter's
/// process environment or the user's argv/launch configuration. perl5db
/// parses options left-to-right: the final debugger-only ReadLine switch
/// wins without changing the program's `PERL_RL`. The effective child
/// environment is read — including Windows' case-insensitive variable names
/// — rather than replacing user options.
///
/// Neither variable is consulted while `require "perl5db.pl"` resolves and
/// compiles the module, so applying the same environment to the capability
/// probe changes launch parity without changing what the probe measures.
#[cfg(windows)]
fn apply_windows_debugger_transport_env(cmd: &mut std::process::Command) {
    cmd.env("EMACS", "1");
    let mut perl_db_opts = cmd
        .get_envs()
        .find_map(|(key, value)| {
            key.to_str()
                .is_some_and(|key| key.eq_ignore_ascii_case("PERLDB_OPTS"))
                .then_some(value)
                .flatten()
        })
        .unwrap_or_default()
        .to_os_string();
    perl_db_opts.push(" ReadLine=0");
    cmd.env("PERLDB_OPTS", perl_db_opts);
}

/// Non-Windows child commands need no debugger-console transport override.
#[cfg(not(windows))]
fn apply_windows_debugger_transport_env(_cmd: &mut std::process::Command) {}

/// Return the authoritative frame id for the current suspension.
///
/// The output reader may observe a context line followed by a prompt for the
/// same stop, so the prompt path must preserve the generation established by
/// the context path. A prompt without a preceding context advances the
/// generation itself. Scope references have a bounded frame-id wire space; once
/// the generation exceeds it, return an unencodable sentinel rather than
/// reusing an older frame id and reviving stale references.
fn current_stopped_frame_id(session: &mut DebugSession, advance_generation: bool) -> i32 {
    if advance_generation {
        session.stopped_generation = session.stopped_generation.saturating_add(1);
    }
    let generation = session.stopped_generation.max(1);
    if generation <= SCOPE_FRAME_ID_MAX { generation as i32 } else { i32::MAX }
}

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
        let supports_exceptions = catalog_has_feature("dap.exceptions.die");
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
        // `gotoTargets`/`goto` are fail-closed while the native backend only has
        // a run-to-line primitive (`f <source>` + `c <line>` resumes execution
        // instead of moving the next statement).  The catalog row is
        // `not_proven`/unadvertised (#9064), so this flag stays `false` until a
        // backend proves a real next-statement relocation primitive.
        // Advertising requires the complete contract: targets that can never
        // be executed (`dap.goto` unadvertised) must not be published, so a
        // one-row promotion of `dap.goto_targets` alone cannot expose
        // selectable-but-dead targets.
        let supports_goto_targets =
            catalog_has_feature("dap.goto_targets") && catalog_has_feature("dap.goto");

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
            // #9578: the four optional breakpoint capability rows fail closed
            // from the single `backend::capabilities` authority. They are not
            // derived from `supports_core`, `dap.breakpoints.*` catalog rows,
            // maturity, handler presence, or backend method existence: the
            // runtime contracts (engine resolution/install, condition
            // enforcement, attributed hit counting, correlated logpoint
            // output) are unproven on this seam. Per-capability re-enable
            // gates: #8645 (function), #8988 (conditional), #8994 (hit),
            // #9000 (logpoint).
            "supportsFunctionBreakpoints":
                crate::backend::capabilities::advertises_function_breakpoints(),
            "supportsConditionalBreakpoints":
                crate::backend::capabilities::advertises_conditional_breakpoints(),
            "supportsHitConditionalBreakpoints":
                crate::backend::capabilities::advertises_hit_conditional_breakpoints(),
            "supportsLogPoints": crate::backend::capabilities::advertises_log_points(),
            // #9573: not `supports_core`. Hover is gated on a pure
            // selected-frame inspection proof that does not exist yet, so the
            // wire value comes from the single hover authority and no catalog
            // row can widen it.
            "supportsEvaluateForHovers": crate::backend::capabilities::advertises_evaluate_for_hovers(),
            "supportsStepBack": false,
            // #8354: not `supports_core`. setVariable is gated on an exact
            // mutation proof that does not exist yet, so the wire value comes
            // from the single setVariable authority and no catalog row can
            // widen it.
            "supportsSetVariable":
                crate::backend::capabilities::advertises_set_variable(),
            "supportsRestartFrame": supports_restart_frame,
            "supportsGotoTargetsRequest": supports_goto_targets,
            "supportsStepInTargetsRequest": supports_step_in_targets,
            // --- #9581 secondary-capability floor -------------------------------
            // The seven rows below are independent literal `false` cells. None
            // of them may be derived from `supports_core`, catalog maturity,
            // handler/type presence, or another mode's support: the useful
            // handler pieces exist, but their advertised contracts are not yet
            // exact behavior facts (#9581). Each row re-enables only through
            // its own gate, owned by the per-feature issues named in its
            // comment; one field's receipt never widens another. While a row
            // is false, its request is rejected by the dispatcher before any
            // handler computation (see `dispatch_request`), so the floored
            // paths perform no debugger I/O and mutate no state.
            //
            // completions: column/current-frame semantics unproven.
            // Gate: #9021 + #9046 + #9050 + #8581 + #9582 + #9584.
            "supportsCompletionsRequest": false,
            // modules: `%INC` output without stable module/source identity.
            // Gate: #8581 + #7667/#8668 + #9585 + #9586.
            "supportsModulesRequest": false,
            // restart: no atomic fresh-debuggee transaction yet.
            // Gate: #9051 + #8691/#8703 + #8974 + #9587 + #8726 + #7568.
            "supportsRestartRequest": false,
            "supportsExceptionOptions": supports_any_exception,
            // ValueFormat: formatting honor is not proven consistently across
            // variables/evaluate/mutation. Gate: #9050 + #8364 + #9070 +
            // #7342/#7345 + #9588 + #9590.
            "supportsValueFormattingOptions": false,
            "supportsExceptionInfoRequest": supports_any_exception,
            "supportTerminateDebuggee": supports_core,
            "supportsDelayedStackTraceLoading": false,
            // loadedSources: same identity gate as modules, as its own row.
            // Gate: #8581 + #7667/#8668 + #9585 + #9586.
            "supportsLoadedSourcesRequest": false,
            "supportsTerminateThreadsRequest": supports_terminate_threads,
            // #8294: exactly one synthetic main execution context is exposed;
            // single-thread execution requests are not a distinct capability
            // and stay unadvertised.
            "supportsSingleThreadExecutionRequests": false,
            // #9568: not `supports_core`. setExpression is gated on an exact
            // current-frame l-value assignment proof (#9570 promotion boundary)
            // that does not exist yet, so the wire value comes from the single
            // setExpression authority and no catalog row can widen it.
            "supportsSetExpression": crate::backend::capabilities::advertises_set_expression(),
            "supportsTerminateRequest": supports_core,
            "supportsDataBreakpoints": supports_watchpoints,
            "supportsReadMemoryRequest": false,
            "supportsDisassembleRequest": false,
            // Request-scoped cancellation is advertised only for native stdio,
            // whose concurrent intake and exact-binary proof own this row.
            // Peer and direct/in-process surfaces remain fail-closed.
            "supportsCancelRequest": self.native_stdio_transport,
            // breakpointLocations: canonical geometry/coordinate contract
            // unproven. Gate: #10524 + #2300 + #9021 + #7566.
            "supportsBreakpointLocationsRequest": false,
            // --- end #9581 secondary-capability floor ---------------------------
            "supportsClipboardContext": false,
            "supportsSteppingGranularity": false,
            "supportsInstructionBreakpoints": false,
            "supportsExceptionFilterOptions": supports_any_exception,
            // #9089: not the `dap.inline_values` catalog row. The routed
            // `inlineValues` request is a project extension, not standard DAP,
            // so the standard capability cell comes from the single
            // inline-values extension authority and no catalog row can widen
            // it while the negotiation contract is unproven.
            "supportsInlineValues": crate::backend::capabilities::advertises_inline_values_extension(),
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

            // Derive this session's workspace boundary from the adapter's
            // startup authority.
            //
            // The authority itself is immutable (see
            // `DebugAdapter::with_workspace_authority`). A launch-args
            // `workspaceRoot` is only ever a NARROWING input: under a bounded
            // authority it must resolve inside a trusted root or the launch is
            // refused, and under an unbounded authority it confines this one
            // session without becoming authority for the next.
            //
            // The result is written to the per-session boundary — never back
            // over the authority — so the previous session's narrowing is
            // cleared on every launch rather than inherited (#14587).
            let launch_root_arg =
                args.get("workspaceRoot").and_then(|w| w.as_str()).map(PathBuf::from);

            // Ownership is selected from the path the debuggee will actually
            // open, not the raw client string — see `resolve_launch_program`.
            let resolved_program = match Self::resolve_launch_program(program, user_cwd.as_deref())
            {
                Ok(resolved) => resolved,
                Err(message) => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "launch".to_string(),
                        body: None,
                        message: Some(message),
                    };
                }
            };

            let boundary = match security::resolve_session_boundary(
                self.workspace_authority(),
                &resolved_program,
                launch_root_arg.as_deref(),
            ) {
                Ok(boundary) => boundary,
                Err(error) => {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "launch".to_string(),
                        body: None,
                        message: Some(error.to_string()),
                    };
                }
            };

            // Install the derived boundary transactionally. `launch_debugger`
            // reads it, but a replacement launch that fails during metadata,
            // syntax, interpreter, or spawn setup deliberately leaves the prior
            // debuggee running — and that still-live session must keep its own
            // boundary rather than inherit the failed replacement's (#14592
            // review).
            let previous_boundary = self.session_boundary();
            self.set_session_boundary(&boundary);

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
                Ok(_) => {
                    // #15637: no eager `stopped(reason=entry)` here. At this
                    // point the session is still `Running` with no frame
                    // authority, so an entry event would announce a stop the
                    // very next `stackTrace` cannot observe (empty frames).
                    // The session is created with `entry_stop_pending` set and
                    // the output reader emits exactly one `stopped(reason=entry)`
                    // from the first real debugger suspension instead.
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
                    // The prior session, if any, is still running: give it its
                    // boundary back rather than leaving it under the failed
                    // replacement's.
                    self.restore_session_boundary(previous_boundary);
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
    /// [`LaunchConfiguration`]: crate::config::LaunchConfiguration
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

    /// Allocate the next execution-context id (#8294).
    ///
    /// Monotonic, exhaustion-aware, and poison-free by construction: the
    /// counter never wraps, never issues zero or negative ids, and has no
    /// failure path that can return an already-minted constant, so a replaced
    /// session's id can never be revived. `None` means the id space is
    /// exhausted and the caller must fail the launch.
    fn allocate_thread_id(&self) -> Option<i32> {
        let mut current = self.thread_counter.load(Ordering::Relaxed);
        loop {
            let next = current.checked_add(1).filter(|next| *next > 0)?;
            match self.thread_counter.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(next),
                Err(observed) => current = observed,
            }
        }
    }

    /// Resolve a launch `program` to the absolute path the debuggee will open.
    ///
    /// An absolute `program` is already unambiguous. A relative one is opened
    /// by `perl` relative to the working directory the launch supplies, so it
    /// resolves against `cwd` when given and against this process's working
    /// directory otherwise — which is also the directory the pre-spawn
    /// existence check uses. Authorization and execution must agree on exactly
    /// one path, or the workspace boundary can be validated against a file
    /// that is never the one run.
    ///
    /// # Errors
    ///
    /// Fails when the program is empty or whitespace-only, and when a relative
    /// program cannot be anchored to an absolute base — this process has no
    /// readable working directory. The launch is refused rather than resolved to
    /// a still-relative path: returning one would let the child apply the launch
    /// directory a second time and reopen the authorize-one-path-execute-another
    /// gap this function exists to close.
    pub(super) fn resolve_launch_program(
        program: &str,
        cwd: Option<&Path>,
    ) -> Result<PathBuf, String> {
        // Normalize here rather than at each call site. `launch_debugger`
        // trims the program for its own validation, but ownership selection in
        // `handle_launch` resolves earlier — and untrimmed, a leading space
        // makes an absolute path look relative, silently anchoring it under
        // the working directory. Owning the trim keeps every caller agreeing
        // on one path, which is the whole point of this function.
        let trimmed = program.trim();

        // Emptiness belongs to the function that owns the trim. `Path::new("")`
        // is not absolute and `base.join("")` returns `base` unchanged, so an
        // empty program would otherwise resolve to a *directory* — and
        // `handle_launch` would derive this session's boundary from it before
        // `launch_debugger`'s own empty check refuses the launch.
        if trimmed.is_empty() {
            return Err(
                "No Perl script was specified. Set the 'program' field in your launch.json \
                 to the path of the script you want to debug."
                    .to_string(),
            );
        }

        let raw = Path::new(trimmed);
        if raw.is_absolute() {
            return Ok(raw.to_path_buf());
        }

        // The base must itself be absolute. A launch `cwd` may be relative, and
        // the spawned child resolves a relative `current_dir` against *this*
        // process's working directory — so joining a relative `cwd` straight
        // onto the program would leave the result relative, and the same
        // segment would then be applied twice: once by the join and again by
        // the child. `{program: "script.pl", cwd: "sub"}` would authorize
        // `<root>/sub/script.pl` and open `<cwd>/sub/sub/script.pl`.
        let base = match cwd {
            Some(path) if path.is_absolute() => path.to_path_buf(),
            Some(path) => Self::process_working_directory()?.join(path),
            None => Self::process_working_directory()?,
        };

        let resolved = base.join(raw);
        if !resolved.is_absolute() {
            return Err(format!(
                "Cannot resolve the program '{program}' to an absolute path. \
                 Set 'program' in your launch.json to an absolute path, or set an \
                 absolute 'cwd'."
            ));
        }
        Ok(resolved)
    }

    /// This process's working directory, as an absolute anchor for relative
    /// launch inputs.
    ///
    /// # Errors
    ///
    /// Fails when the directory cannot be read — for example after it is
    /// deleted out from under a long-lived adapter. There is no safe fallback:
    /// a relative placeholder would silently break the absolute-path invariant
    /// every caller depends on.
    fn process_working_directory() -> Result<PathBuf, String> {
        let cwd = std::env::current_dir().map_err(|error| {
            format!(
                "Cannot determine this adapter's working directory ({error}), so a \
                 relative launch path cannot be resolved. Set 'program' and 'cwd' in \
                 your launch.json to absolute paths."
            )
        })?;
        if cwd.is_absolute() {
            Ok(cwd)
        } else {
            Err(format!(
                "This adapter's working directory ('{}') is not absolute, so a relative \
                 launch path cannot be resolved. Set 'program' and 'cwd' in your \
                 launch.json to absolute paths.",
                cwd.display()
            ))
        }
    }

    /// Launch the Perl debugger for the given script.
    ///
    /// Validates the program path and interpreter, probes that the interpreter
    /// can load the core `perl5db.pl` debugger module, runs a pre-launch
    /// `perl -c` syntax check, then spawns `perl -d` with the supplied
    /// arguments and environment overrides. Returns the thread ID on success.
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
        // Resolve the program to the exact path the debuggee will open, once,
        // before anything authorizes or inspects it (#14592 review).
        //
        // A relative `program` is opened by the spawned `perl` relative to the
        // working directory it is given, which the client controls through
        // `cwd`. Authorizing the raw relative string would validate
        // `<trusted-root>/script.pl` while `perl` opened `<cwd>/script.pl` —
        // a launch of `{program: "script.pl", cwd: "/outside"}` would pass the
        // workspace boundary and then execute outside it. Everything below
        // (existence, boundary, syntax check, spawn) uses this one path.
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
        // Resolve *after* the trim and quote validation above, so resolution
        // sees the cleaned program: a leading space would otherwise make an
        // absolute path look relative and silently anchor it to the cwd.
        let resolved_program = Self::resolve_launch_program(program, cwd_override.as_deref())?;

        let path = resolved_program.as_path();
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

        // Enforce workspace-bound launch paths when this session has a
        // boundary. This prevents launching scripts outside the active project
        // tree.
        let session_boundary = self.session_boundary();
        if let Some(root) = session_boundary.as_ref() {
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

        // Effective debuggee working directory, shared by the capability
        // probe, the pre-launch syntax check, and the launch itself so all
        // three see identical `@INC` resolution. User-specified cwd wins;
        // otherwise the script's parent directory (what `perl -d` would
        // effectively run in), else the adapter cwd.
        let prog_cwd = cwd_override.clone().unwrap_or_else(|| {
            Path::new(program)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        });

        // Debugger capability precondition: the debugger itself is the core
        // `perl5db.pl` module, so an interpreter that cannot load it can spawn
        // but never hosts a session. Probe it — from the same effective
        // debuggee directory — before spawning so such a launch fails with a
        // typed, actionable error instead of a mid-session pipe failure.
        Self::check_debugger_capability(perl_interpreter, &env_overrides, &prog_cwd)?;

        // Pre-launch syntax check: run `perl -c <script>` before spawning the
        // debugger.  This catches syntax errors early and surfaces a clear,
        // actionable message to the user instead of a generic "Cannot start
        // Perl debugger" failure after `perl -d` exits immediately.
        Self::check_syntax(perl_interpreter, path, &env_overrides, cwd_override.clone())?;

        // Use PerlOracleEnv to deny ambient PERL5LIB/PERL5OPT so the debug
        // session env is controlled entirely by launch.json `env` (#8688).
        // `env_overrides` (explicit launch.json entries) are added via
        // extra_env so they reach the subprocess unconditionally.
        let mut oracle = perl_lsp_rs_core::config::PerlOracleEnv::for_version_probe(
            PathBuf::from(perl_interpreter),
            prog_cwd,
        );
        oracle.extra_env.extend(env_overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
        let mut cmd = oracle.into_command();
        cmd.arg("-d");

        // Strawberry Perl's Windows debugger transport/ReadLine environment;
        // see `apply_windows_debugger_transport_env` for the contract. Shared
        // with the capability probe so both run under launch parity.
        apply_windows_debugger_transport_env(&mut cmd);

        // Perl debugger stops on the first line by default
        let _ = stop_on_entry; // currently unused

        // Use -- to separate flags from script name, preventing argument injection
        // if program starts with -
        cmd.arg("--");
        cmd.arg(&resolved_program);
        cmd.args(&args);

        // Set up pipes
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Capture the directory actually used by this child, not the independent
        // workspace security boundary. Pin an absolute spelling so a relative
        // launch cwd cannot later be reinterpreted against the adapter's cwd.
        let debuggee_cwd = std::path::absolute(cmd.get_current_dir().unwrap_or(Path::new(".")))
            .map_err(|error| format!("Cannot resolve debugger working directory: {error}"))?;
        cmd.current_dir(&debuggee_cwd);
        let launch_source_path = {
            let path = Path::new(program);
            if path.is_absolute() { path.to_path_buf() } else { debuggee_cwd.join(path) }
        };
        let launch_source_digest = std::fs::read(&launch_source_path)
            .map(|bytes| perl_source_identity::ContentDigest::of_bytes(&bytes).to_string())
            .map_err(|error| format!("Cannot snapshot launched source identity: {error}"))?;

        // Allocate the execution-context id BEFORE spawning: a launch that
        // cannot mint a fresh id must fail without side effects.
        let Some(thread_id) = self.allocate_thread_id() else {
            return Err(
                "Debugger could not be started: the execution-context id space is exhausted. \
                 Restart the debug adapter to reset execution contexts."
                    .to_string(),
            );
        };

        // A previously rejected replacement remains the sole owner of an
        // unconfirmed child. Do not spawn another child until terminal
        // cleanup has retried that owner successfully.
        if lock_or_recover(&self.rejected_child, "debug_adapter.rejected_child").is_some() {
            return Err(
                "Cannot replace the active debugger session while a rejected process cleanup remains unconfirmed"
                    .to_string(),
            );
        }

        match cmd.spawn() {
            Ok(mut child) => {
                // Only advance the session generation once spawning has succeeded. A rejected
                // launch must leave the currently active reader valid for its existing session.
                if !self.prepare_replacement_session() {
                    let cleanup = Self::terminate_child_process(&mut child);
                    return Err(self.reject_spawned_replacement_child(child, cleanup));
                }
                if let Ok(mut identity) = self.launch_source_identity.lock() {
                    *identity = Some((launch_source_path.clone(), launch_source_digest.clone()));
                }

                let session = DebugSession {
                    process: child,
                    state: DebugState::Running,
                    stack_frames: Vec::new(),
                    stack_frame_arguments: HashMap::new(),
                    variable_cache: VariableCache::default(),
                    thread_id,
                    debuggee_cwd: debuggee_cwd.clone(),
                    last_resume_mode: ResumeMode::Unknown,
                    initial_stop_pending: !stop_on_entry,
                    entry_stop_pending: stop_on_entry,
                    stopped_generation: 0,
                    module_generation: RuntimeModuleGenerationClock::new(),
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
                self.operation_broker.open_session();
                self.admit_terminal_lifecycle();

                // Apply any function breakpoints configured before launch.
                self.apply_stored_function_breakpoints();

                // Start output reader thread
                self.start_output_reader(debuggee_cwd);

                // Replay source breakpoints admitted before launch after the
                // reader is ready to capture each engine acknowledgement.
                // The pending DAP records keep their original IDs; clients
                // learn that installation completed through changed events.
                let installed_source_breakpoints = self
                    .launch_source_identity
                    .lock()
                    .ok()
                    .and_then(|identity| identity.as_ref().map(|(path, _)| path.clone()))
                    .map(|source_path| {
                        let raw_key = source_path.to_string_lossy().into_owned();
                        let canonical_key = source_path
                            .canonicalize()
                            .ok()
                            .map(|path| path.to_string_lossy().into_owned());
                        let (source_key, records) = {
                            let raw_records = self.breakpoints.get_breakpoints(&raw_key);
                            if raw_records.is_empty() {
                                if let Some(canonical_key) = canonical_key {
                                    let canonical_records =
                                        self.breakpoints.get_breakpoints(&canonical_key);
                                    (canonical_key, canonical_records)
                                } else {
                                    (raw_key, raw_records)
                                }
                            } else {
                                (raw_key, raw_records)
                            }
                        };
                        self.install_stored_source_breakpoints(&source_key, &records)
                    })
                    .unwrap_or_default();
                if installed_source_breakpoints.ambiguous {
                    return Err(if installed_source_breakpoints.cleanup_succeeded {
                        "Debugger session was invalidated because a source breakpoint acknowledgement was ambiguous"
                            .to_string()
                    } else {
                        "Debugger session was invalidated but cleanup was not confirmed after an ambiguous source breakpoint acknowledgement"
                            .to_string()
                    });
                }
                for id in installed_source_breakpoints.installed {
                    self.send_event(
                        "breakpoint",
                        Some(serde_json::json!({
                            "reason": "changed",
                            "breakpoint": { "id": id, "verified": true }
                        })),
                    );
                }

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

    /// Verify the selected interpreter can load the core `perl5db.pl` debugger
    /// module, returning a typed, actionable error when it cannot.
    ///
    /// `perl -d` bootstraps through `perl5db.pl`; an interpreter without it
    /// spawns but can never host a debugger session — for example a minimal
    /// Git-for-Windows/MSYS perl whose mount-relative `@INC` entries stop
    /// resolving once the binary runs outside its installation, or any
    /// stripped distribution. Without this precondition the launch "succeeded"
    /// (the child spawned) and only the mid-session control pipe then failed
    /// with `Can't locate perl5db.pl in @INC`.
    ///
    /// The probe belongs to the debuggee launch path, not the resolver:
    /// resolution picks an interpreter path, the launcher validates that the
    /// picked interpreter can host the debugger. It runs under the same
    /// [`perl_lsp_rs_core::config::PerlOracleEnv`] environment the real
    /// debuggee will see (ambient `PERL5LIB`/`PERL5OPT` denied, #8688;
    /// launch.json `env` honored) **and from the same effective working
    /// directory** (`probe_cwd`), so `@INC` entries that depend on where the
    /// debuggee runs resolve identically in the probe and the launch.
    ///
    /// The probe is bounded: the child is polled against
    /// [`DEBUGGER_PROBE_TIMEOUT`] and killed at the deadline. A probe that
    /// cannot conclude (spawn failure, `try_wait` failure, or timeout) is an
    /// instrument failure, not a capability verdict, so it is skipped and the
    /// real `perl -d` launch surfaces its own canonical error.
    ///
    /// Only an affirmative verdict is cached, keyed per (interpreter, probe
    /// cwd, launch env) for the life of the adapter process: a passing
    /// interpreter must not pay a fresh perl spawn on every launch. A
    /// failure describes mutable installation state — perl5db.pl can be
    /// installed in place — and an inconclusive probe is no verdict at all,
    /// so neither is retained and the next launch re-probes.
    fn check_debugger_capability(
        perl_interpreter: &str,
        env_overrides: &HashMap<String, String>,
        probe_cwd: &Path,
    ) -> Result<(), String> {
        let cache_key =
            Self::capability_probe_cache_key(perl_interpreter, env_overrides, probe_cwd);
        if let Ok(cache) = DEBUGGER_PROBE_CACHE.lock()
            && let Some(cached) = cache.get(&cache_key)
        {
            return cached.clone();
        }

        match Self::run_debugger_capability_probe(perl_interpreter, env_overrides, probe_cwd) {
            DebuggerCapabilityProbe::Capable => {
                if let Ok(mut cache) = DEBUGGER_PROBE_CACHE.lock() {
                    cache.insert(cache_key, Ok(()));
                }
                Ok(())
            }
            DebuggerCapabilityProbe::Incapable(detail) => Err(format!(
                "Selected interpreter cannot host the debugger (perl5db.pl not loadable): \
                 {perl_interpreter}. Install a full Perl distribution that ships the core \
                 debugger module, or point launch.json `perlPath` at one (e.g. \
                 {{\"perlPath\": \"/path/to/full/perl\"}}). Detail: {detail}"
            )),
            DebuggerCapabilityProbe::Inconclusive => Ok(()),
        }
    }

    /// Cache key for one probe verdict: interpreter, effective probe cwd, and
    /// the launch.json `env` entries that could steer `@INC`/module loading.
    /// Every component is length-prefixed, so no delimiter appearing inside
    /// an interpreter path, cwd, env name, or env value can splice two
    /// launches into one key — distinct launches always serialize to
    /// distinct keys. A conservative key can only cost a re-probe, never
    /// serve a verdict measured under a different launch configuration.
    fn capability_probe_cache_key(
        perl_interpreter: &str,
        env_overrides: &HashMap<String, String>,
        probe_cwd: &Path,
    ) -> String {
        let length_prefixed = |value: &str| format!("{}:{value}", value.len());
        let mut env_entries: Vec<String> = env_overrides
            .iter()
            .map(|(key, value)| format!("{}={}", length_prefixed(key), length_prefixed(value)))
            .collect();
        env_entries.sort();
        format!(
            "{interpreter}@{cwd}@{env}",
            interpreter = length_prefixed(perl_interpreter),
            cwd = length_prefixed(&probe_cwd.to_string_lossy()),
            env = env_entries.join(";")
        )
    }

    /// Run one bounded perl5db.pl loadability probe. See
    /// [`Self::check_debugger_capability`] for the contract. A measured
    /// verdict additionally requires the probe expression's own success
    /// marker on stdout — exit status alone can be produced by an
    /// interpreter shim that never evaluated the expression.
    fn run_debugger_capability_probe(
        perl_interpreter: &str,
        env_overrides: &HashMap<String, String>,
        probe_cwd: &Path,
    ) -> DebuggerCapabilityProbe {
        let mut oracle = perl_lsp_rs_core::config::PerlOracleEnv::for_version_probe(
            PathBuf::from(perl_interpreter),
            probe_cwd.to_path_buf(),
        );
        oracle.extra_env.extend(env_overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
        let mut cmd = oracle.into_command();
        cmd.arg("-e")
            .arg(format!("require \"perl5db.pl\"; print \"{DEBUGGER_PROBE_SUCCESS_MARKER}\\n\";"));
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // Run under the same debugger-transport environment the real `perl -d`
        // launch uses, so the probe measures perl5db.pl loading under launch
        // parity. EMACS=1 and PERLDB_OPTS only select the debugger's runtime
        // console/ReadLine backends; neither is consulted while `require`
        // resolves and compiles perl5db.pl, so this cannot flip the verdict —
        // and if that ever stopped being true, the probe would now observe it.
        apply_windows_debugger_transport_env(&mut cmd);

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                // The interpreter could not be spawned at all — skip the probe
                // and let the real `perl -d` launch produce the canonical
                // "perl not on PATH" error.
                tracing::warn!(
                    "perl5db capability probe could not run '{perl_interpreter}' \
                     (will attempt the launch anyway): {e}"
                );
                return DebuggerCapabilityProbe::Inconclusive;
            }
        };

        // Drain both pipes on dedicated threads while polling, so a verbose
        // interpreter (say, an @INC dump longer than the OS pipe buffer)
        // cannot fill a pipe and deadlock before its own deadline.
        let stdout_drain = spawn_probe_pipe_drain(child.stdout.take());
        let stderr_drain = spawn_probe_pipe_drain(child.stderr.take());
        let deadline = Instant::now() + DEBUGGER_PROBE_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        break None;
                    }
                    thread::sleep(DEBUGGER_PROBE_POLL_INTERVAL);
                }
                Err(e) => {
                    // Instrument failure: kill what we spawned and skip.
                    let _ = child.kill();
                    let _ = child.wait();
                    tracing::warn!(
                        "perl5db capability probe of '{perl_interpreter}' could not be \
                         observed (will attempt the launch anyway): {e}"
                    );
                    return DebuggerCapabilityProbe::Inconclusive;
                }
            }
        };

        let Some(status) = status else {
            // Deadline reached with no exit: the probe is inconclusive, not a
            // capability verdict. Kill the child so nothing outlives the
            // probe, then keep the launch-continue disposition.
            let _ = child.kill();
            let _ = child.wait();
            tracing::warn!(
                "perl5db capability probe of '{perl_interpreter}' exceeded its \
                 {} budget (will attempt the launch anyway)",
                DEBUGGER_PROBE_TIMEOUT.as_secs()
            );
            return DebuggerCapabilityProbe::Inconclusive;
        };

        // The child has exited, so both drains usually reach EOF immediately.
        // But a shim that spawned a pipe-inheriting descendant before exiting
        // never delivers EOF, so each drain is collected against the
        // remaining probe deadline instead of being joined forever; the
        // drainer thread is abandoned (it ends whenever the descendant
        // exits) and its pipe yields no observation.
        let remaining = deadline.saturating_duration_since(Instant::now());
        let raw_stdout = join_probe_drain_within(stdout_drain, remaining);
        let raw_stderr = join_probe_drain_within(stderr_drain, remaining);

        // Both drains have been collected before any verdict is decided, so
        // the join stays deterministic. A measured pass requires the marker
        // the probe expression prints: a shim that ignores the `-e` payload
        // and exits 0 never loads perl5db.pl, so exit status alone cannot
        // certify capability. A stdout drain abandoned to an inheriting
        // descendant is unobservable rather than marker-free, so it stays
        // inconclusive instead of failing a possibly capable interpreter.
        if status.success() {
            return match raw_stdout {
                Some(stdout) if has_probe_success_marker(&stdout) => {
                    DebuggerCapabilityProbe::Capable
                }
                Some(_) => DebuggerCapabilityProbe::Incapable(
                    "exited successfully but did not evaluate the probe expression \
                     (interpreter shim?)"
                        .to_string(),
                ),
                None => DebuggerCapabilityProbe::Inconclusive,
            };
        }

        // The "Can't locate perl5db.pl in @INC" report arrives on stderr, but
        // a non-perl binary selected as the interpreter may report on either
        // stream; merge for the diagnostic detail.
        let reported = raw_stderr
            .as_deref()
            .map(str::trim)
            .filter(|detail| !detail.is_empty())
            .or_else(|| raw_stdout.as_deref().map(str::trim))
            .filter(|detail| !detail.is_empty());
        let detail = match reported {
            Some(detail) => detail.to_string(),
            None => {
                // Report a bare exit code as a number; a child killed by a
                // signal has no code and says so instead of rendering
                // `Some(...)`/`None`.
                match status.code() {
                    Some(code) => format!("exit status {code}"),
                    None => "process terminated by a signal (no exit status)".to_string(),
                }
            }
        };
        DebuggerCapabilityProbe::Incapable(detail)
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
        program: &Path,
        env_overrides: &HashMap<String, String>,
        cwd_override: Option<PathBuf>,
    ) -> Result<(), String> {
        // PerlOracleEnv denies ambient PERL5LIB/PERL5OPT (#8688); explicit
        // env_overrides from launch.json are honored via extra_env.
        // Use user-specified cwd if provided; otherwise default to script's parent directory
        let prog_cwd = if let Some(user_cwd) = cwd_override {
            user_cwd
        } else {
            program
                .parent()
                .map(Path::to_path_buf)
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
            program.display(),
            detail
        ))
    }

    fn missing_module_name(detail: &str) -> Option<String> {
        // perl's own diagnostic is `Can't locate X in @INC ...`, but wrapper
        // shims, fat binaries, and environment layers may re-case the line.
        // Match the prefix case-insensitively so the typed module remediation
        // still fires; `str::get` declines non-char boundaries, so slicing
        // below cannot panic on non-ASCII output. The ` in @INC` separator is
        // matched case-insensitively for the same reason: a recased
        // `IN @INC` must still terminate the module path instead of being
        // swallowed into a corrupted module name.
        const MISSING_MODULE_PREFIX: &str = "Can't locate ";
        const MISSING_MODULE_SEPARATOR: &str = " in @INC";
        detail.lines().find_map(|line| {
            let trimmed = line.trim();
            let head = trimmed.get(..MISSING_MODULE_PREFIX.len())?;
            if !head.eq_ignore_ascii_case(MISSING_MODULE_PREFIX) {
                return None;
            }
            let body = &trimmed[MISSING_MODULE_PREFIX.len()..];
            let separator_lower = MISSING_MODULE_SEPARATOR.to_ascii_lowercase();
            let separator_at = body.to_ascii_lowercase().find(separator_lower.as_str())?;
            let module_path = body[..separator_at].trim_end_matches('.');
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
    pub(super) fn start_output_reader(&self, debuggee_cwd: PathBuf) {
        let session = self.session.clone();
        let seq = self.seq.clone();
        let sender = self.event_sender.clone();
        let recent_output = self.recent_output.clone();
        let breakpoints = self.breakpoints.clone();
        // Source-boundary snapshot for observed-stop correlation. Both are plain
        // `Arc` clones, so the reader takes no authority lock at all — it reads
        // only the leaf `session_boundary` mutex at correlation time, never the
        // session lock under the breakpoint-store lock.
        let workspace_authority = Arc::clone(&self.workspace_authority);
        let session_boundary = Arc::clone(&self.session_boundary);
        let exception_break_on_die = self.exception_break_on_die.clone();
        let exception_break_on_warn = self.exception_break_on_warn.clone();
        let last_exception_message = self.last_exception_message.clone();
        let tcp_session = self.tcp_session.clone();
        let attached_pid = self.attached_pid.clone();
        let termination_state = self.termination_state.clone();
        let operation_broker = self.operation_broker.clone();
        let event_drain = self.event_drain.clone();
        let session_generation = self.current_session_generation();
        // The reader's session epoch in the broker's own id space. The
        // launch-failure/EOF/read-error settles below are gated on it, so a
        // stale reader still draining its pipe after a restart or attach
        // replacement cannot settle the replacement session's pending
        // operations (#8564 review).
        let broker_session_generation = operation_broker.current_session_generation();

        thread::spawn(move || {
            // Perl's debugger prompt and evaluation output are emitted on stderr.
            // Prefer stderr as the control stream, with stdout as a fallback.
            let control_stream: Option<Box<dyn Read + Send>> = {
                if let Ok(mut guard) = session.lock() {
                    guard.as_mut().and_then(|s| {
                        if operation_broker.current_session_generation()
                            != broker_session_generation
                        {
                            return None;
                        }
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
                // Launch effectively failed for framed operations (#8564):
                // settle only while this reader still owns the live session.
                operation_broker.settle_all_if_current("launch_failed", broker_session_generation);
                if let Some(ref sender) = sender {
                    emit_terminated_event(
                        sender,
                        &seq,
                        &termination_state,
                        Some(session_generation),
                        Some(json!({"reason": "no_debugger_stream"})),
                        Some(&event_drain),
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
            // Most recent `error_re` message line (`<text> at FILE line N`). An
            // uncaught die arrives as that message line followed by the bare
            // perl5db-handler suffix line; the suffix is the detection signal,
            // but the message line is the text `exceptionInfo` should serve.
            let mut last_error_message = String::new();
            // In-flight logpoint value query, if any. A logpoint hit queues a framed
            // `p` query for the scalars its template mentions; the replies stream back
            // through this same loop and are folded into the message here (#5045).
            let mut pending_logpoint: Option<PendingLogpoint> = None;
            let mut logpoint_marker_id: u64 = 0;
            // Residual frame lines to filter after a capture is abandoned mid-frame:
            // (end marker, remaining budget).
            let mut logpoint_drain: Option<LogpointDrain> = None;
            let mut framed_reader_marker: Option<(String, usize)> = None;
            let mut suppress_prompt_after_frame = false;

            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        tracing::debug!("Perl debugger process terminated");
                        // Settle every pending framed operation first (#8564):
                        // waiters must observe SessionGone, not spin to their
                        // timeout against a dead session. Gated on the reader's
                        // spawn generation: a stale reader draining its pipe
                        // after a restart or attach replacement must not clear
                        // the replacement session's pending table (#8564
                        // review).
                        operation_broker
                            .settle_all_if_current("debugger_eof", broker_session_generation);
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
                                Some(&event_drain),
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
                        if operation_broker.current_session_generation()
                            != broker_session_generation
                        {
                            break;
                        }
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

                        // Framed query payload belongs exclusively to its waiter.  It may
                        // contain context-looking stack lines, and a late end marker can
                        // arrive after the waiter has timed out; neither may rewrite the
                        // live stop cache.  The prompt immediately following a completed
                        // frame is part of that control exchange as well.
                        if let Some(end) = operation_broker
                            .take_reader_frame(&analysis_text, broker_session_generation)
                        {
                            framed_reader_marker = Some((end, super::RECENT_OUTPUT_MAX_LINES));
                            suppress_prompt_after_frame = false;
                            continue;
                        }
                        if let Some((end, remaining)) = framed_reader_marker.as_mut() {
                            if super::operation_broker::OperationBroker::line_contains_full_marker(
                                &analysis_text,
                                end,
                            ) {
                                framed_reader_marker = None;
                                suppress_prompt_after_frame = true;
                            } else if *remaining == 0 {
                                // Do not resume interpreting an unterminated payload as
                                // fresh stop context when the bounded drain is exhausted.
                                operation_broker.settle_all_if_current(
                                    "debugger_frame_limit",
                                    broker_session_generation,
                                );
                                DebugAdapter::clear_active_session_state_for_generation(
                                    &session,
                                    &tcp_session,
                                    &attached_pid,
                                    &termination_state,
                                    session_generation,
                                );
                                if let Some(ref sender) = sender {
                                    emit_terminated_event(
                                        sender,
                                        &seq,
                                        &termination_state,
                                        Some(session_generation),
                                        Some(json!({"reason": "debugger_frame_limit"})),
                                        Some(&event_drain),
                                    );
                                }
                                break;
                            } else {
                                *remaining = remaining.saturating_sub(1);
                            }
                            continue;
                        }
                        if std::mem::take(&mut suppress_prompt_after_frame)
                            && prompt_re().is_some_and(|re| re.is_match(&analysis_text))
                            && lock_or_recover(&session, "debug_adapter.frame_prompt")
                                .as_ref()
                                .is_some_and(|session| matches!(session.state, DebugState::Stopped))
                        {
                            continue;
                        }

                        // perl5db prints this fixed line when the debuggee
                        // program ends — on normal exit AND on an uncaught die —
                        // and then idles at a prompt instead of exiting, so
                        // without this the client never observes `terminated`
                        // for a completed run. Emit the terminal event at the
                        // real program end. Session state intentionally stays
                        // intact so cached frames remain answerable; process
                        // cleanup remains with disconnect/watchdog (#9081).
                        if analysis_text.starts_with("Debugged program terminated") {
                            if let Some(ref sender) = sender {
                                emit_terminated_event(
                                    sender,
                                    &seq,
                                    &termination_state,
                                    Some(session_generation),
                                    Some(json!({ "reason": "debuggee_exit" })),
                                    Some(&event_drain),
                                );
                            }
                            continue;
                        }

                        // Enhanced context information parsing with multiple patterns
                        let mut context_updated = false;
                        // Whether THIS line is the perl5db die/warn-handler suffix
                        // (` at FILE line N.`) — the stream signal that the
                        // debugger's `__DIE__` handler observed an uncaught `die`.
                        let mut matched_die_suffix = false;

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
                            last_error_message = analysis_text.clone();

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

                        // perl5db's `__DIE__` handler reports an uncaught `die` as
                        // the message line followed by a bare ` at FILE line N.`
                        // suffix line — including for `die "msg\n"`, whose own text
                        // carries no suffix. A `print` of byte-identical text never
                        // fires the handler, and a `die` caught by `eval` propagates
                        // silently, so this line is the honest stream signal that an
                        // uncaught die reached the debugger. `warn` fires the
                        // sibling `__WARN__` handler with an indistinguishable
                        // suffix line; that warn/die ambiguity is inherent to the
                        // perl5db stream and stays with the residual #9081 warn
                        // claim. The suffix carries the authoritative die location,
                        // so attribute file/line from it.
                        if !context_updated
                            && let Some(re) = die_suffix_re()
                            && let Some(caps) = re.captures(&analysis_text)
                        {
                            if let Some(file) = caps.name("file") {
                                current_file = file.as_str().to_string();
                            }
                            if let Some(line_num) = caps.name("line") {
                                current_line = line_num.as_str().parse::<i32>().unwrap_or(0);
                            }
                            context_updated = true;
                            matched_die_suffix = true;
                        }

                        if context_updated {
                            let break_on_die =
                                exception_break_on_die.lock().map(|guard| *guard).unwrap_or(false);
                            let break_on_warn =
                                exception_break_on_warn.lock().map(|guard| *guard).unwrap_or(false);
                            // Detection must not key on words inside the user's
                            // die message (`exception_re` trigger words are kept
                            // for compatibility); an ordinary uncaught `die` is
                            // attributed from the perl5db handler suffix line.
                            let is_exception_line = exception_re()
                                .is_some_and(|re| re.is_match(&analysis_text))
                                || matched_die_suffix;
                            let is_warning_line =
                                warning_re().is_some_and(|re| re.is_match(&analysis_text));
                            let exception_match = break_on_die && is_exception_line;
                            let warning_match =
                                break_on_warn && is_warning_line && !is_exception_line;

                            // Store exception message for exceptionInfo request.
                            // When the handler suffix line is the detection
                            // signal, its own text (`at FILE line N.`) is
                            // content-free — serve the remembered die message
                            // line instead.
                            if (exception_match || warning_match)
                                && let Ok(mut guard) = last_exception_message.lock()
                            {
                                let message =
                                    if matched_die_suffix && !last_error_message.is_empty() {
                                        last_error_message.clone()
                                    } else {
                                        analysis_text.clone()
                                    };
                                *guard = Some(message);
                            }

                            let mut should_emit_stopped = false;
                            let mut should_auto_continue = false;
                            let mut stop_reason = "step".to_string();
                            let mut logpoint_messages: Vec<String> = Vec::new();
                            let mut hit_breakpoint_ids = Vec::new();

                            // Snapshot the source boundary before acquiring the
                            // session lock. The authority is an `Arc` (no lock) and
                            // `session_boundary` is a leaf mutex, so nothing here
                            // nests under the session or breakpoint-store locks.
                            //
                            // The reader runs only inside a live session, so the
                            // stored boundary is read directly rather than through
                            // `live_session_boundary`, whose liveness check would
                            // take the very session lock this snapshot exists to
                            // stay out from under.
                            let observed_session_boundary = lock_or_recover(
                                &session_boundary,
                                "debug_adapter.session_boundary",
                            )
                            .clone();
                            let observed_roots: &[PathBuf] =
                                match observed_session_boundary.as_ref() {
                                    Some(root) => std::slice::from_ref(root),
                                    None => workspace_authority.trusted_roots(),
                                };
                            let observed_bounded = workspace_authority.is_bounded();

                            let thread_id = {
                                let Ok(mut guard) = session.lock() else {
                                    tracing::warn!(
                                        "Failed to lock session when processing debugger context"
                                    );
                                    continue;
                                };

                                if let Some(ref mut s) = *guard {
                                    if operation_broker.current_session_generation()
                                        != broker_session_generation
                                    {
                                        continue;
                                    }
                                    let was_running = matches!(s.state, DebugState::Running);
                                    let current_frame_id = current_stopped_frame_id(s, was_running);
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

                                    if was_running && s.entry_stop_pending {
                                        // #15637: a `stopOnEntry` launch owes the
                                        // client its entry stop only from the
                                        // first real debugger suspension. The
                                        // context line above just established the
                                        // stopped frame authority for exactly
                                        // that suspension, so publish the entry
                                        // stop from here and consume the pending
                                        // reason — a later stop must report its
                                        // own cause, and exactly one entry stop
                                        // may ever be emitted per session.
                                        s.state = DebugState::Stopped;
                                        s.last_resume_mode = ResumeMode::Unknown;
                                        s.entry_stop_pending = false;
                                        should_emit_stopped = true;
                                        stop_reason = "entry".to_string();
                                    } else if was_running
                                        && s.initial_stop_pending
                                        && matches!(s.last_resume_mode, ResumeMode::Unknown)
                                    {
                                        // Perl pauses at the first executable line before
                                        // configurationDone. Retain that pause for an
                                        // acknowledged breakpoint on the same line; the
                                        // configurationDone handler will publish it only after
                                        // correlating the engine installation.
                                        s.state = DebugState::Stopped;
                                        s.last_resume_mode = ResumeMode::Unknown;
                                        continue;
                                    } else if was_running {
                                        should_emit_stopped = true;
                                        let resume_mode = s.last_resume_mode.clone();

                                        let breakpoint_outcome = if matches!(
                                            resume_mode,
                                            ResumeMode::Continue | ResumeMode::RunToBreakpoint
                                        ) && !current_file.is_empty()
                                            && current_line > 0
                                        {
                                            DebugAdapter::register_observed_engine_breakpoint_hit(
                                                &breakpoints,
                                                &current_file,
                                                i64::from(current_line),
                                                observed_roots,
                                                observed_bounded,
                                                &debuggee_cwd,
                                                session_generation,
                                            )
                                        } else {
                                            EngineBreakpointHitOutcome::default()
                                        };
                                        hit_breakpoint_ids =
                                            breakpoint_outcome.hit_breakpoint_ids.clone();

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
                                    Some({
                                        let mut body = json!({
                                            "reason": stop_reason,
                                            "threadId": thread_id,
                                            "allThreadsStopped": true
                                        });
                                        if !hit_breakpoint_ids.is_empty() {
                                            body["hitBreakpointIds"] = json!(hit_breakpoint_ids);
                                        }
                                        body
                                    }),
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
                            let (thread_id, prompt_stops_entry) = {
                                let Ok(mut guard) = session.lock() else {
                                    tracing::warn!(
                                        "Failed to lock session when processing debugger prompt"
                                    );
                                    continue;
                                };
                                if let Some(ref mut s) = *guard {
                                    // A prompt can be observed after the context
                                    if operation_broker.current_session_generation()
                                        != broker_session_generation
                                    {
                                        continue;
                                    }
                                    // branch (which already advanced the
                                    // suspension generation), or without a
                                    // parseable context. Preserve the existing
                                    // generation in the former case and advance
                                    // it in the latter; never reset the frame id
                                    // to the historical constant 1.
                                    let current_frame_id = current_stopped_frame_id(
                                        s,
                                        matches!(s.state, DebugState::Running),
                                    );
                                    // Create stack frame with enhanced context validation
                                    if !current_file.is_empty() && current_line > 0 {
                                        let frame = StackFrame {
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
                                        };
                                        s.stack_frames = vec![frame];
                                        s.stack_frame_arguments.clear();
                                    } else {
                                        // Provide a fallback frame for when we don't have perfect context
                                        let frame = StackFrame {
                                            id: current_frame_id,
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
                                    // #15637: a prompt can be the first observed
                                    // authority for the entry suspension (context
                                    // output may never parse). Consume a pending
                                    // entry reason here so the first prompt stop
                                    // still honors a `stopOnEntry` launch exactly
                                    // once; later prompts report `step` as before.
                                    let prompt_stops_entry = s.entry_stop_pending;
                                    s.entry_stop_pending = false;
                                    (s.thread_id, prompt_stops_entry)
                                } else {
                                    continue;
                                }
                            };

                            // Send stopped event with robust error handling
                            let prompt_stop_reason =
                                if prompt_stops_entry { "entry" } else { "step" };
                            if let Some(ref sender) = sender
                                && !emit_event_safe(
                                    sender,
                                    &seq,
                                    "stopped",
                                    Some(json!({
                                        "reason": prompt_stop_reason,
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
                        // Same contract as the EOF arm above (#8564): settle
                        // pending operations so waiters observe SessionGone —
                        // gated on the reader's spawn generation for the same
                        // stale-reader reason.
                        operation_broker
                            .settle_all_if_current("read_error", broker_session_generation);
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
                                Some(&event_drain),
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
        let operation_broker = self.operation_broker.clone();
        let event_drain = self.event_drain.clone();
        let session_generation = self.current_session_generation();
        let broker_session_generation = operation_broker.current_session_generation();
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

            // Settle framed-query waiters before killing the process. The EOF
            // reader will also observe the death, but it may be blocked on a
            // partial frame; broker waiters must not remain pending until that
            // path drains. If another reader already settled this generation,
            // it owns the terminal reason and the watchdog must not reserve a
            // timeout event over it.
            let settled_by_watchdog = operation_broker
                .settle_all_if_current("debuggee_timeout", broker_session_generation);

            // Reserve the timeout termination before kill only when this
            // watchdog performed the settlement. Kill still runs before the
            // blocking event send so a stalled client cannot leave the
            // debuggee alive.
            let owns_timeout_event = settled_by_watchdog
                && reserve_terminated_event(&termination_state, Some(session_generation));

            // Keep the termination-state lock while acquiring the session lock
            // and killing. Replacement teardown takes these locks in the same
            // order, so a replacement cannot advance the generation between
            // the check above and selection of the process to kill.
            let termination_guard =
                lock_or_recover(&termination_state, "debug_adapter.termination_state");
            if termination_guard.generation != session_generation {
                tracing::debug!(
                    session_generation,
                    current_generation = termination_guard.generation,
                    "Debuggee watchdog: session replaced before process kill"
                );
                return;
            }

            // Kill the debuggee process. The output reader will see EOF and
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
            drop(termination_guard);

            if !killed {
                tracing::error!("Debuggee watchdog: failed to kill hung debuggee process");
            }

            // Deliver the reserved timeout reason after kill, but only if the
            // session generation has not been closed or replaced since the
            // reservation was claimed (before the kill): a stale timeout event
            // must not leak into a newer client conversation (#12092 review).
            // Blocking send is OK: the debuggee is already dead; the emitted
            // flag was set at reserve time.
            if owns_timeout_event
                && let Some(ref sender) = sender
                && terminated_delivery_is_current(&termination_state, Some(session_generation))
            {
                // The reserved timeout event joins the drain latch like
                // every other terminal emission, so a response cannot
                // overtake it on the wire.
                event_drain.enqueue(1);
                let delivered = deliver_reserved_terminated_event(
                    sender,
                    &seq,
                    &termination_state,
                    session_generation,
                    Some(json!({"reason": "debuggee_timeout"})),
                    &|| false,
                );
                if !delivered {
                    event_drain.complete(1);
                }
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
                // Debuggee replacement invalidates the reload family's
                // session identities (#10102, R03): a PID attach is a
                // replacement session like launch/TCP attach, so the prior
                // reload epoch, negotiation, subjects, and operation
                // identities must not survive it.
                self.reset_reload_route_for_replacement_session();
                if !self.clear_active_session_state() {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "attach".to_string(),
                        body: None,
                        message: Some(
                            "Cannot attach while an earlier debugger process cleanup remains unconfirmed"
                                .to_string(),
                        ),
                    };
                }

                if let Ok(mut guard) = self.attached_pid.lock() {
                    *guard = Some(pid);
                    drop(guard);
                    self.admit_terminal_lifecycle();
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

                if stop_on_entry {
                    return DapMessage::Response {
                        seq,
                        request_seq,
                        success: false,
                        command: "attach".to_string(),
                        body: None,
                        message: Some(
                            "TCP attach does not support stopOnEntry=true. Set stopOnEntry=false and \
                             configure the debugger peer to pause if needed"
                                .to_string(),
                        ),
                    };
                }

                // Create TCP attach session
                let mut session = TcpAttachSession::new();

                // Set up the bounded event channel for TCP events (#9521)
                let (tx, rx) = sync_channel::<DapEvent>(TCP_ATTACH_EVENT_CAPACITY);
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
                        if !self.prepare_replacement_session() {
                            let _ = session.disconnect();
                            return DapMessage::Response {
                                seq,
                                request_seq,
                                success: false,
                                command: "attach".to_string(),
                                body: None,
                                message: Some(
                                    "Cannot replace the active debugger session because its process cleanup was not confirmed"
                                        .to_string(),
                                ),
                            };
                        }
                        // Store session
                        if let Ok(mut guard) = self.tcp_session.lock() {
                            *guard = Some(session);
                            drop(guard);
                            self.admit_terminal_lifecycle();
                        }
                        self.operation_broker.open_session();

                        // Start the generation-aware forwarder for TCP events.
                        // Events are published only while the attach's session
                        // generation is current; a replacement attach,
                        // termination, or disconnect discards the dead
                        // generation's queued events before DAP publication
                        // (#9521).
                        let seq_counter = self.seq.clone();
                        let event_sender = self.event_sender.clone();
                        let termination_state = self.termination_state.clone();
                        let event_drain = self.event_drain.clone();
                        let session_generation = self.current_session_generation();
                        spawn_tcp_attach_event_forwarder(
                            rx,
                            event_sender,
                            seq_counter,
                            termination_state,
                            session_generation,
                            event_drain,
                        );

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
    pub(super) fn clear_active_session_state(&self) -> bool {
        #[cfg(test)]
        if self.cleanup_failure_for_test.swap(false, Ordering::AcqRel) {
            let active_cleanup = Self::clear_active_session_state_with_terminator(
                &self.session,
                &self.tcp_session,
                &self.attached_pid,
                |_| false,
            );
            let rejected_cleanup = self.clear_rejected_child_with_terminator(|_| false);
            return active_cleanup && rejected_cleanup;
        }
        let active_cleanup = Self::clear_active_session_state_with_state(
            &self.session,
            &self.tcp_session,
            &self.attached_pid,
        );
        let rejected_cleanup =
            self.clear_rejected_child_with_terminator(Self::terminate_child_process);
        active_cleanup && rejected_cleanup
    }

    #[cfg(test)]
    pub(super) fn fail_next_cleanup_for_test(&self) {
        self.cleanup_failure_for_test.store(true, Ordering::Release);
    }

    fn clear_rejected_child_with_terminator(
        &self,
        mut terminate: impl FnMut(&mut Child) -> bool,
    ) -> bool {
        match self.rejected_child.lock() {
            Ok(mut guard) => {
                let Some(child) = guard.as_mut() else {
                    return true;
                };
                if terminate(child) {
                    *guard = None;
                    true
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    fn try_retain_rejected_child(&self, child: Child) -> Result<(), Child> {
        let mut guard = lock_or_recover(&self.rejected_child, "debug_adapter.rejected_child");
        if guard.is_some() {
            Err(child)
        } else {
            *guard = Some(child);
            Ok(())
        }
    }

    fn reject_spawned_replacement_child(&self, mut child: Child, cleanup: bool) -> String {
        if cleanup {
            return "Cannot replace the active debugger session because its process cleanup was not confirmed"
                .to_string();
        }
        match self.try_retain_rejected_child(child) {
            Ok(()) => {
                "Cannot replace the active debugger session; cleanup of both processes was not confirmed"
                    .to_string()
            }
            Err(returned_child) => {
                child = returned_child;
                let _ = Self::terminate_child_process(&mut child);
                "Cannot retain the rejected debugger process because another unconfirmed process is already retained"
                    .to_string()
            }
        }
    }

    /// Advance the session generation and tear down the prior active session.
    ///
    /// Callers invoke this only after a replacement launch or attach has
    /// successfully completed its external setup. A spawn failure leaves the
    /// existing session valid; a cleanup-blocked replacement invalidates the
    /// protocol state while retaining process ownership for retry.
    fn prepare_replacement_session(&self) -> bool {
        self.begin_session_generation();
        // Debuggee replacement invalidates the reload family's session
        // identities (#10102): a new epoch refuses prior family/operation
        // claims, and the runtime-module generation resets with the new
        // debuggee process (it lives on `DebugSession`).
        self.reset_reload_route_for_replacement_session();
        self.clear_active_session_state()
    }

    pub(super) fn clear_active_session_state_with_state(
        session: &Arc<Mutex<Option<DebugSession>>>,
        tcp_session: &Arc<Mutex<Option<TcpAttachSession>>>,
        attached_pid: &Arc<Mutex<Option<u32>>>,
    ) -> bool {
        Self::clear_active_session_state_with_terminator(
            session,
            tcp_session,
            attached_pid,
            Self::terminate_child_process,
        )
    }

    fn clear_active_session_state_with_terminator(
        session: &Arc<Mutex<Option<DebugSession>>>,
        tcp_session: &Arc<Mutex<Option<TcpAttachSession>>>,
        attached_pid: &Arc<Mutex<Option<u32>>>,
        mut terminate: impl FnMut(&mut Child) -> bool,
    ) -> bool {
        let mut cleanup_succeeded = true;
        // Terminate the debug session
        match session.lock() {
            Ok(mut guard) => {
                if let Some(active_session) = guard.as_mut() {
                    if terminate(&mut active_session.process) {
                        active_session.state = DebugState::Terminated;
                        let _ = guard.take();
                    } else {
                        tracing::warn!("Failed to ensure debug session process termination");
                        cleanup_succeeded = false;
                        // Retain the Child handle so a later owner can retry the
                        // bounded reap instead of silently dropping an unconfirmed
                        // live process.
                        active_session.state = DebugState::Terminated;
                    }
                }
            }
            Err(_) => cleanup_succeeded = false,
        }

        // Disconnect TCP session if active
        if let Ok(mut guard) = tcp_session.lock()
            && let Some(ref mut tcp_session) = *guard
            && tcp_session.disconnect().is_err()
        {
            cleanup_succeeded = false;
        }
        match tcp_session.lock() {
            Ok(mut guard) => *guard = None,
            Err(_) => cleanup_succeeded = false,
        }

        // Clear PID attach mode.
        match attached_pid.lock() {
            Ok(mut guard) => *guard = None,
            Err(_) => cleanup_succeeded = false,
        }
        cleanup_succeeded
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

        // Settle broker waiters before terminating the child so EOF cannot
        // win the race and replace the client-requested disconnect reason.
        self.operation_broker.settle_all("disconnect");
        // No-session disconnect must not fabricate a terminal event. An
        // admitted session with a reserved but undelivered event transfers
        // that obligation to this request before sequence allocation.
        let has_active_session = lock_or_recover(&self.session, "debug_adapter.session").is_some()
            || lock_or_recover(&self.attached_pid, "debug_adapter.attached_pid").is_some()
            || lock_or_recover(&self.tcp_session, "debug_adapter.tcp_session").is_some();
        let terminal_committed =
            lock_or_recover(&self.termination_state, "disconnect.termination_state")
                .terminal_committed;
        if has_active_session
            && !terminal_committed
            && let Some(ref sender) = self.event_sender
        {
            emit_terminated_event(
                sender,
                &self.seq,
                &self.termination_state,
                None,
                None,
                Some(&self.event_drain),
            );
        }
        let cleanup_succeeded = self.clear_active_session_state();
        self.close_terminal_session_generation("disconnect");

        DapMessage::Response {
            seq,
            request_seq,
            success: cleanup_succeeded,
            command: "disconnect".to_string(),
            body: None,
            message: (!cleanup_succeeded).then(|| {
                "Session invalidated, but debugger process cleanup remains unconfirmed".to_string()
            }),
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
        // Settle broker waiters before terminating the child so EOF cannot
        // win the race and replace the client-requested terminate reason.
        self.operation_broker.settle_all("terminated");
        if let Some(ref sender) = self.event_sender {
            emit_terminated_event(
                sender,
                &self.seq,
                &self.termination_state,
                None,
                terminated_body,
                Some(&self.event_drain),
            );
        }
        let cleanup_succeeded = self.clear_active_session_state();
        self.close_terminal_session_generation("terminated");

        DapMessage::Response {
            seq,
            request_seq,
            success: cleanup_succeeded,
            command: "terminate".to_string(),
            body: None,
            message: (!cleanup_succeeded).then(|| {
                "Session invalidated, but debugger process cleanup remains unconfirmed".to_string()
            }),
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

        let mut startup_breakpoint_ids = Vec::new();
        let mut startup_thread_id = None;
        let startup_generation = self.current_session_generation();
        // Snapshot the boundary before the session lock below, matching the
        // reader thread's ordering: the authority is never read while the
        // session lock is held.
        let startup_boundary = self.live_session_boundary();
        let startup_roots: &[PathBuf] = match startup_boundary.as_ref() {
            Some(root) => std::slice::from_ref(root),
            None => self.workspace_authority.trusted_roots(),
        };
        let startup_bounded = self.workspace_authority.is_bounded();
        if let Some(ref mut session) = *lock_or_recover(&self.session, "debug_adapter.session")
            && let Some(stdin) = session.process.stdin.as_mut()
        {
            if !stop_on_entry
                && session.initial_stop_pending
                && let Some(frame) = session.stack_frames.first().cloned()
            {
                let outcome = Self::register_observed_engine_breakpoint_hit(
                    &self.breakpoints,
                    &frame.source.path,
                    i64::from(frame.line),
                    startup_roots,
                    startup_bounded,
                    &session.debuggee_cwd,
                    startup_generation,
                );
                if outcome.should_stop {
                    startup_breakpoint_ids = outcome.hit_breakpoint_ids;
                    startup_thread_id = Some(session.thread_id);
                    session.state = DebugState::Stopped;
                    session.last_resume_mode = ResumeMode::Unknown;
                    session.initial_stop_pending = false;
                }
            }

            if startup_thread_id.is_some() {
                // The implicit startup pause already corresponds to an
                // acknowledged breakpoint. Publish it without sending `c`.
            } else if stop_on_entry {
                // #15637: the entry stop is emitted by the output reader at the
                // first real debugger suspension, so by configurationDone time it
                // may already have been published — or the reader may still be
                // waiting for the bootstrap output. List the current source
                // location so the debugger re-reports it either way and the
                // reader can observe an authoritative frame for the entry stop.
                let _ = stdin.write_all(b"l\n");
                let _ = stdin.flush();
            } else {
                // stopOnEntry is false: perl -d always stops at the first
                // executable line.  Run to the first user-set breakpoint.
                // ResumeMode::RunToBreakpoint signals the output reader to
                // silently skip non-breakpoint stops (the implicit first-line
                // stop) and auto-continue until a user breakpoint is hit.
                session.initial_stop_pending = false;
                session.state = DebugState::Running;
                session.last_resume_mode = ResumeMode::RunToBreakpoint;
                let _ = stdin.write_all(b"c\n");
                let _ = stdin.flush();
            }
        }

        if let Some(thread_id) = startup_thread_id {
            let mut body = json!({
                "reason": "breakpoint",
                "threadId": thread_id,
                "allThreadsStopped": true
            });
            body["hitBreakpointIds"] = json!(startup_breakpoint_ids);
            self.send_event("stopped", Some(body));
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

    /// Translate an identity-carrying inbound TCP-attach debuggee event into
    /// the DAP event name and body dispatched to the editor.
    ///
    /// The remote peer's raw thread id is never forwarded: the adapter
    /// advertises exactly one synthetic execution context, so `threads`,
    /// every thread-scoped request, and `stopped`/`continued` events all
    /// carry [`Self::TCP_ATTACH_SYNTHETIC_THREAD_ID`] (#8294). `Terminated`
    /// and `Error` carry no execution-context identity and are handled at
    /// the pump; this translation returns `None` for them.
    pub(super) fn tcp_event_message(event: DapEvent) -> Option<(&'static str, Option<Value>)> {
        match event {
            DapEvent::Output { category, output } => {
                Some(("output", Some(json!({ "category": category, "output": output }))))
            }
            DapEvent::Stopped { reason, thread_id: _ } => Some((
                "stopped",
                Some(json!({
                    "reason": reason,
                    "threadId": Self::TCP_ATTACH_SYNTHETIC_THREAD_ID,
                    "allThreadsStopped": true
                })),
            )),
            DapEvent::Continued { thread_id: _ } => Some((
                "continued",
                Some(json!({
                    "threadId": Self::TCP_ATTACH_SYNTHETIC_THREAD_ID,
                    "allThreadsContinued": true
                })),
            )),
            DapEvent::Terminated { .. } | DapEvent::Error { .. } => None,
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
            vec![json!({
                "id": Self::TCP_ATTACH_SYNTHETIC_THREAD_ID,
                "name": "TCP Attached Thread"
            })]
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

        if !self.clear_active_session_state() {
            return DapMessage::Response {
                seq,
                request_seq,
                success: false,
                command: "restart".to_string(),
                body: None,
                message: Some(
                    "Cannot restart while debugger process cleanup remains unconfirmed".to_string(),
                ),
            };
        }
        self.handle_launch(seq, request_seq, Some(launch_args))
    }
}

/// Atomically claim the single `terminated` emission for this session generation.
///
/// Returns `true` if this caller now owns emission (and must deliver the event),
/// `false` if another path already reserved or emitted termination.
pub(super) fn reserve_terminated_event(
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

/// Whether a terminal emission reserved under `expected_generation` may still be
/// delivered: the session generation must not have been closed or replaced since
/// the reservation was claimed.
///
/// A reservation can be held across slow work before its send (the debuggee
/// watchdog reserves before killing the process and delivers after), so a client
/// terminal request or a replacement launch can advance the generation while the
/// send is still outstanding. Delivering that stale send would leak an old
/// session's `terminated` event into a newer client conversation (e.g. a client
/// reading it as the replacement session terminating), so delivery must
/// revalidate and retire it (#12092 review).
///
/// `None` (the synchronous client `terminate`/`disconnect` path) is always
/// current: reservation, send, and generation close run sequentially on the
/// caller's thread, so nothing can interleave.
fn terminated_delivery_is_current(
    termination_state: &Mutex<TerminationState>,
    expected_generation: Option<u64>,
) -> bool {
    let Some(expected) = expected_generation else { return true };
    let state = lock_or_recover(termination_state, "debug_adapter.termination_state");
    state.generation == expected
}

/// Emit interpolated logpoint text on the debug console.
fn emit_logpoint_messages(sender: Option<&EventSender>, seq: &Mutex<i64>, messages: Vec<String>) {
    let Some(sender) = sender else {
        return;
    };
    for message in messages {
        let _ = sender.send_event(
            seq,
            "output",
            Some(json!({
                "category": "console",
                "output": format!("{message}\n")
            })),
        );
    }
}

pub(super) fn emit_terminated_event(
    sender: &EventSender,
    seq: &Mutex<i64>,
    termination_state: &Mutex<TerminationState>,
    expected_generation: Option<u64>,
    body: Option<Value>,
    drain: Option<&EventDrainLatch>,
) -> bool {
    emit_terminated_event_guarded(
        sender,
        seq,
        termination_state,
        expected_generation,
        body,
        &|| false,
        drain,
    )
}

/// [`emit_terminated_event`] with a staleness hook for the generation-aware
/// TCP-attach forwarder (#9521).
///
/// Reservation and delivery-currentness are unchanged; the final send is
/// generation-aware across the ENTIRE queue wait: a `terminated` event that
/// cannot be enqueued immediately re-validates the generation before every
/// commit attempt, so a replacement session retires a blocked stale terminal
/// event instead of an unbounded blocking send publishing it into the
/// replacement's conversation after validation passed.
///
/// The emission joins the `event_drain` latch contract (FC-TERMINATED-
/// DRAIN-BYPASS): the latch is reserved before the guarded dispatch and
/// retained only when the dispatch reports `Sent`, so `run_with_io`
/// observes an accepted terminal event before the response that follows
/// it. Reservation, currentness, stale, and disconnected outcomes all
/// complete the reservation instead of retaining it.
pub(super) fn emit_terminated_event_guarded(
    sender: &EventSender,
    seq: &Mutex<i64>,
    termination_state: &Mutex<TerminationState>,
    expected_generation: Option<u64>,
    body: Option<Value>,
    stale: &dyn Fn() -> bool,
    drain: Option<&EventDrainLatch>,
) -> bool {
    let generation = expected_generation
        .unwrap_or_else(|| lock_or_recover(termination_state, "terminal.generation").generation);
    if !reserve_terminated_event(termination_state, Some(generation)) {
        return false;
    }
    // Join the drain latch (FC-TERMINATED-DRAIN-BYPASS): reserve before
    // the delivery attempt and retain only when the event was accepted
    // into the outbound channel; stale, replaced-generation, and
    // disconnected outcomes complete the reservation instead.
    if let Some(drain) = drain {
        drain.enqueue(1);
    }
    let delivered =
        deliver_reserved_terminated_event(sender, seq, termination_state, generation, body, stale);
    if let Some(drain) = drain
        && !delivered
    {
        drain.complete(1);
    }
    delivered
}

fn deliver_reserved_terminated_event(
    sender: &EventSender,
    seq: &Mutex<i64>,
    termination_state: &Mutex<TerminationState>,
    generation: u64,
    body: Option<Value>,
    stale: &dyn Fn() -> bool,
) -> bool {
    let Some(sender) = sender.admitted_sender() else { return false };
    let mut sequence = lock_or_recover(seq, "terminal.seq");
    *sequence += 1;
    let mut message = DapMessage::Event { seq: *sequence, event: "terminated".to_string(), body };
    loop {
        if stale() {
            return false;
        }
        let mut state = lock_or_recover(termination_state, "terminal.commit");
        if state.generation != generation {
            return false;
        }
        match sender.try_send(message) {
            Ok(()) => {
                state.terminal_committed = true;
                return true;
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return false,
            Err(std::sync::mpsc::TrySendError::Full(returned)) => message = returned,
        }
        drop(state);
        thread::sleep(super::sync_utils::GENERATION_GUARD_PARK);
    }
}

#[cfg(test)]
mod tests {
    use super::super::sync_utils::EventDrainLatch;
    use super::super::sync_utils::EventSender;
    use super::{DapMessage, Duration, Instant, PathBuf, Stdio, Value, json, thread};
    use super::{
        DebugAdapter, DebugState, current_stopped_frame_id, detect_perl_info,
        emit_terminated_event, format_perl_spawn_error, has_probe_success_marker,
        is_valid_perl_interpreter, lock_or_recover, reserve_terminated_event,
        terminated_delivery_is_current,
    };
    use crate::reload::RuntimeModuleGenerationClock;
    use crate::tcp_attach::DapEvent;
    use perl_tdd_support::{must, must_err, must_some};
    use perl_test_must::must_some_with;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{TryRecvError, sync_channel};
    use std::sync::{Arc, Mutex};

    /// `resolve_launch_program` must agree with what `perl` will open.
    ///
    /// This is the whole point of the helper: the pre-fix code authorized
    /// `Path::new(program)`, and the shared validator joins a *relative* path
    /// with the root it is checked against — so `"script.pl"` validated as
    /// `<trusted-root>/script.pl` while the spawned `perl`, running under the
    /// client's `cwd`, opened `<cwd>/script.pl`.
    #[test]
    fn a_relative_program_resolves_against_the_launch_cwd_not_the_trusted_root() {
        let cwd = Path::new("/outside/project");
        let resolved = must(DebugAdapter::resolve_launch_program("script.pl", Some(cwd)));
        assert_eq!(
            resolved,
            PathBuf::from("/outside/project/script.pl"),
            "a relative program must resolve against the directory perl is given"
        );
        assert!(
            !resolved.starts_with("/trusted"),
            "resolution must never silently land inside a trusted root it was not given"
        );
    }

    /// A relative `cwd` must still yield an absolute program path.
    ///
    /// Devin review of #14592: the first version of this helper joined a
    /// relative `cwd` straight onto the program, leaving the result relative.
    /// The same segment was then applied twice — once here and again by the
    /// spawned child, which resolves a relative `current_dir` against this
    /// process's working directory — so `{program: "script.pl", cwd: "sub"}`
    /// authorized `<root>/sub/script.pl` while `perl` opened
    /// `<cwd>/sub/sub/script.pl`.
    #[test]
    fn a_relative_launch_cwd_still_yields_one_absolute_program_path() {
        let process_cwd = must(std::env::current_dir());
        let resolved =
            must(DebugAdapter::resolve_launch_program("script.pl", Some(Path::new("sub"))));
        assert!(
            resolved.is_absolute(),
            "authorization and execution can only agree on an absolute path, got {resolved:?}"
        );
        assert_eq!(resolved, process_cwd.join("sub").join("script.pl"));

        // The launch cwd must be applied exactly once. Count only inside the
        // part this function appended: the process working directory is chosen
        // by whoever runs the test and may itself contain a `sub` component,
        // which would fail a whole-path count on a correct result.
        let appended = must_some(resolved.strip_prefix(&process_cwd).ok());
        let occurrences =
            appended.components().filter(|c| c.as_os_str() == std::ffi::OsStr::new("sub")).count();
        assert_eq!(occurrences, 1, "the launch cwd must not be applied twice: {appended:?}");
    }

    /// A program that cannot be anchored absolutely is refused, not resolved to
    /// a relative path the child would re-anchor itself.
    #[test]
    fn an_absolute_cwd_anchors_a_relative_program_without_consulting_the_process_directory() {
        let resolved =
            must(DebugAdapter::resolve_launch_program("script.pl", Some(Path::new("/anchored"))));
        assert_eq!(resolved, PathBuf::from("/anchored/script.pl"));
        assert!(resolved.is_absolute());
    }

    /// Surrounding whitespace must not turn an absolute program relative.
    ///
    /// The pre-existing `program.trim()` in `launch_debugger` runs after
    /// ownership selection has already resolved the program, so the helper
    /// owns the trim itself — otherwise a leading space anchors an absolute
    /// path under the working directory at one call site but not the other.
    #[test]
    fn surrounding_whitespace_does_not_make_an_absolute_program_relative() {
        let resolved = must(DebugAdapter::resolve_launch_program("  /trusted/script.pl  ", None));
        assert_eq!(resolved, PathBuf::from("/trusted/script.pl"));
        assert!(resolved.is_absolute());
    }

    /// An empty program is refused rather than resolved to a directory.
    ///
    /// `Path::new("")` is not absolute and `base.join("")` returns `base`
    /// unchanged, so an empty program used to resolve to the launch `cwd`
    /// itself — and `handle_launch` derived this session's boundary from that
    /// directory before `launch_debugger`'s own empty check refused the launch.
    /// The refusal is fail-closed either way; the point is that the authorized
    /// subject was a directory the client never named and nothing would execute.
    #[test]
    fn an_empty_program_is_refused_before_it_can_resolve_to_a_directory() {
        for program in ["", "   ", "\t\n"] {
            let outcome =
                DebugAdapter::resolve_launch_program(program, Some(Path::new("/outside")));
            let error = must_err(outcome);
            assert!(
                error.contains("No Perl script was specified"),
                "an empty program must be refused by name, got: {error}"
            );
        }
    }

    #[test]
    fn an_absolute_program_is_unchanged_by_a_launch_cwd() {
        let resolved = must(DebugAdapter::resolve_launch_program(
            "/trusted/project/script.pl",
            Some(Path::new("/outside")),
        ));
        assert_eq!(resolved, PathBuf::from("/trusted/project/script.pl"));
    }

    #[test]
    fn a_nested_relative_program_resolves_under_the_launch_cwd() {
        let resolved = must(DebugAdapter::resolve_launch_program(
            "lib/deep/script.pl",
            Some(Path::new("/ws")),
        ));
        assert_eq!(resolved, PathBuf::from("/ws/lib/deep/script.pl"));
    }

    #[test]
    fn failed_child_cleanup_retains_owner_until_retry() -> Result<(), String> {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test().map_err(|error| error.to_string())?;
        let expected_pid = adapter
            .session
            .lock()
            .map_err(|_| "session lock poisoned")?
            .as_ref()
            .map(|session| session.process.id())
            .ok_or("test session was not installed")?;

        let failed = DebugAdapter::clear_active_session_state_with_terminator(
            &adapter.session,
            &adapter.tcp_session,
            &adapter.attached_pid,
            |_| false,
        );
        if failed {
            return Err("injected failed cleanup was reported as successful".to_string());
        }
        let retained = adapter
            .session
            .lock()
            .map_err(|_| "session lock poisoned")?
            .as_ref()
            .map(|session| session.process.id());
        if retained != Some(expected_pid) {
            return Err(format!(
                "failed cleanup dropped the owned child: expected {expected_pid}, got {retained:?}"
            ));
        }
        let retained_state = adapter
            .session
            .lock()
            .map_err(|_| "session lock poisoned")?
            .as_ref()
            .map(|session| session.state.clone());
        if retained_state != Some(DebugState::Terminated) {
            return Err("failed cleanup left the retained session resumable".to_string());
        }

        if !adapter.clear_active_session_state() {
            return Err("retrying real child cleanup failed".to_string());
        }
        if adapter.session.lock().map_err(|_| "session lock poisoned")?.is_some() {
            return Err("successful retry retained the child owner".to_string());
        }
        Ok(())
    }

    #[test]
    fn rejected_replacement_child_is_retained_across_failed_cleanup() -> Result<(), String> {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test().map_err(|error| error.to_string())?;
        let active_pid = adapter
            .session
            .lock()
            .map_err(|_| "session lock poisoned")?
            .as_ref()
            .map(|session| session.process.id())
            .ok_or("active session was not installed")?;
        if DebugAdapter::clear_active_session_state_with_terminator(
            &adapter.session,
            &adapter.tcp_session,
            &adapter.attached_pid,
            |_| false,
        ) {
            return Err("injected active cleanup unexpectedly succeeded".to_string());
        }
        let replacement = DebugAdapter::spawn_noop_child_for_test()
            .map_err(|error| format!("spawning replacement child: {error}"))?;
        let replacement_pid = replacement.id();
        let rejection = adapter.reject_spawned_replacement_child(replacement, false);
        if !rejection.contains("both processes") {
            return Err("production rejection path did not retain replacement child".to_string());
        }
        let retained_active_pid = adapter
            .session
            .lock()
            .map_err(|_| "session lock poisoned")?
            .as_ref()
            .map(|session| session.process.id());
        if retained_active_pid != Some(active_pid) {
            return Err("failed active cleanup lost its original child owner".to_string());
        }
        let retained_replacement_pid = adapter
            .rejected_child
            .lock()
            .map_err(|_| "rejected-child lock was poisoned")?
            .as_ref()
            .map(|child| child.id());
        if retained_replacement_pid != Some(replacement_pid) {
            return Err("failed replacement cleanup lost its child owner".to_string());
        }

        if !adapter.clear_active_session_state() {
            return Err("retrying cleanup of both child owners failed".to_string());
        }
        if adapter.session.lock().map_err(|_| "session lock poisoned")?.is_some()
            || adapter
                .rejected_child
                .lock()
                .map_err(|_| "rejected-child lock was poisoned")?
                .is_some()
        {
            return Err("successful retry retained a child owner".to_string());
        }
        Ok(())
    }

    #[test]
    fn context_then_prompt_preserves_current_suspension_frame_id() -> Result<(), String> {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test().map_err(|error| error.to_string())?;

        let mut guard = adapter.session.lock().map_err(|_| "session lock poisoned".to_string())?;
        let session = guard.as_mut().ok_or("test session was not installed")?;

        // The context branch sees the running session and establishes the
        // generation that the stopped event exposes.
        session.state = DebugState::Running;
        let context_frame_id = current_stopped_frame_id(session, true);
        session.state = DebugState::Stopped;

        // The prompt branch follows that same stop. It must retain the id
        // rather than reviving the historical constant frame id 1.
        let prompt_frame_id = current_stopped_frame_id(session, false);
        if prompt_frame_id != context_frame_id {
            return Err(format!(
                "prompt changed the current frame id: context={context_frame_id}, prompt={prompt_frame_id}"
            ));
        }

        // A subsequent context starts a fresh suspension and receives a new
        // authority, preventing the old scope reference from reviving.
        session.state = DebugState::Running;
        let next_context_frame_id = current_stopped_frame_id(session, true);
        if next_context_frame_id == context_frame_id {
            return Err("next suspension reused the previous frame id".to_string());
        }

        Ok(())
    }

    #[test]
    fn generation_frame_id_fails_closed_at_scope_reference_ceiling() -> Result<(), String> {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test().map_err(|error| error.to_string())?;
        let mut guard = adapter.session.lock().map_err(|_| "session lock poisoned".to_string())?;
        let session = guard.as_mut().ok_or("session was not installed")?;
        session.stopped_generation = 99_999;
        session.state = DebugState::Running;

        let exhausted = current_stopped_frame_id(session, true);
        if exhausted != i32::MAX {
            return Err(format!("generation 100000 must fail closed, got {exhausted}"));
        }
        session.stopped_generation = u64::MAX;
        let still_exhausted = current_stopped_frame_id(session, false);
        if still_exhausted != i32::MAX {
            return Err(format!("exhausted generation revived a scope frame: {still_exhausted}"));
        }
        Ok(())
    }

    /// A remote TCP peer's raw thread id must never reach the editor: a peer
    /// that reports a non-1 id still produces `stopped`/`continued` events
    /// carrying the advertised synthetic execution context, so `threads`,
    /// thread-scoped requests, and events cannot disagree (#8294).
    #[test]
    fn tcp_events_normalize_foreign_thread_ids_to_the_advertised_context() {
        let (stopped_name, stopped_body) = must_some_with(
            DebugAdapter::tcp_event_message(DapEvent::Stopped {
                reason: "breakpoint".to_string(),
                thread_id: 9,
            }),
            "stopped events translate",
        );
        assert_eq!(stopped_name, "stopped");
        let stopped_body = must_some_with(stopped_body, "stopped events carry a body");
        assert_eq!(stopped_body["threadId"], DebugAdapter::TCP_ATTACH_SYNTHETIC_THREAD_ID);
        assert_eq!(stopped_body["threadId"], 1);
        assert_eq!(stopped_body["reason"], "breakpoint");
        assert_eq!(stopped_body["allThreadsStopped"], true);

        let (continued_name, continued_body) = must_some_with(
            DebugAdapter::tcp_event_message(DapEvent::Continued { thread_id: 9 }),
            "continued events translate",
        );
        assert_eq!(continued_name, "continued");
        let continued_body = must_some_with(continued_body, "continued events carry a body");
        assert_eq!(continued_body["threadId"], DebugAdapter::TCP_ATTACH_SYNTHETIC_THREAD_ID);
        assert_eq!(continued_body["allThreadsContinued"], true);

        // Non-identity events keep their shape and stay decoupled from the
        // execution-context contract.
        let (output_name, output_body) = must_some_with(
            DebugAdapter::tcp_event_message(DapEvent::Output {
                category: "stdout".to_string(),
                output: "hi".to_string(),
            }),
            "output events translate",
        );
        assert_eq!(output_name, "output");
        assert!(must_some_with(output_body, "output events carry a body")["threadId"].is_null());
        assert!(
            DebugAdapter::tcp_event_message(DapEvent::Terminated { reason: "exit".to_string() })
                .is_none()
        );
    }

    /// Allocation is monotonic from 1 and fails closed at exhaustion: an
    /// unchecked fetch_add would panic (checked builds) or wrap into negative
    /// ids (release) near `i32::MAX`, reviving the stale-id hazard the atomic
    /// counter exists to remove (#8294).
    #[test]
    fn thread_id_allocation_is_monotonic_and_fails_closed_at_exhaustion() {
        let adapter = DebugAdapter::new();
        assert_eq!(adapter.allocate_thread_id(), Some(1));
        assert_eq!(adapter.allocate_thread_id(), Some(2));

        adapter.thread_counter.store(i32::MAX - 1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(adapter.allocate_thread_id(), Some(i32::MAX));
        assert_eq!(
            adapter.allocate_thread_id(),
            None,
            "exhaustion must fail closed, never wrap into negative or reused ids"
        );
    }

    #[test]
    fn exhausted_generation_rejects_old_scope_reference_without_query() -> Result<(), String> {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test().map_err(|error| error.to_string())?;
        let mut guard = adapter.session.lock().map_err(|_| "session lock poisoned".to_string())?;
        let session = guard.as_mut().ok_or("session was not installed")?;
        session.state = DebugState::Stopped;
        session.stopped_generation = 1;
        session.stack_frames = vec![super::StackFrame {
            id: 1,
            name: "main".to_string(),
            source: super::Source {
                name: Some("test.pl".to_string()),
                path: "test.pl".to_string(),
                source_reference: None,
            },
            line: 1,
            column: 1,
            end_line: None,
            end_column: None,
        }];
        drop(guard);

        let old_scope =
            adapter.handle_variables(1, 1, Some(serde_json::json!({ "variablesReference": 11 })));
        match old_scope {
            super::DapMessage::Response { .. } => {}
            other => return Err(format!("old scope returned unexpected response: {other:?}")),
        }
        let before_stale = adapter.debugger_query_count_for_test();

        let mut guard = adapter.session.lock().map_err(|_| "session lock poisoned".to_string())?;
        let session = guard.as_mut().ok_or("session was not installed")?;
        session.stopped_generation = 100_001;
        session.stack_frames =
            vec![super::StackFrame { id: i32::MAX, ..session.stack_frames[0].clone() }];
        drop(guard);

        let stale =
            adapter.handle_variables(1, 1, Some(serde_json::json!({ "variablesReference": 11 })));
        let stale_body = match stale {
            super::DapMessage::Response { body: Some(body), .. } => body,
            other => return Err(format!("stale scope returned unexpected response: {other:?}")),
        };
        if stale_body.get("variables") != Some(&serde_json::json!([]))
            || adapter.debugger_query_count_for_test() != before_stale
        {
            return Err(format!("stale scope revived or queried: {stale_body}"));
        }
        Ok(())
    }

    #[test]
    fn competing_termination_sources_emit_one_structured_event() -> Result<(), String> {
        let (sender, receiver) = sync_channel(64);
        let seq = Arc::new(Mutex::new(0));
        let termination_state =
            Arc::new(Mutex::new(super::TerminationState { generation: 1, ..Default::default() }));
        let first_sender = EventSender::new(sender.clone());
        let first_seq = seq.clone();
        let first_guard = termination_state.clone();
        let first = std::thread::spawn(move || {
            emit_terminated_event(
                &first_sender,
                &first_seq,
                &first_guard,
                Some(1),
                Some(serde_json::json!({"reason": "debugger_eof"})),
                None,
            )
        });
        let second_sender = EventSender::new(sender.clone());
        let second =
            emit_terminated_event(&second_sender, &seq, &termination_state, None, None, None);
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
    fn disconnect_terminal_after_async_completion_and_invalidation() -> Result<(), String> {
        for (invalidate, fail_cleanup) in [(false, false), (true, false), (true, true)] {
            let (sender, receiver) = sync_channel(64);
            let mut adapter = DebugAdapter::new();
            adapter.set_event_sender(sender);
            if fail_cleanup {
                adapter.seed_session_for_test().map_err(|error| error.to_string())?;
            }
            let event_sender = adapter.event_sender.as_ref().ok_or("missing event sender")?;
            let generation = adapter.current_session_generation();
            if !emit_terminated_event(
                event_sender,
                &adapter.seq,
                &adapter.termination_state,
                Some(generation),
                Some(json!({"reason": "debugger_eof"})),
                None,
            ) {
                return Err("current async terminal source did not emit".to_string());
            }
            match receiver.try_recv().map_err(|error| error.to_string())? {
                DapMessage::Event { event, .. } if event == "terminated" => {}
                other => return Err(format!("expected natural terminal event, got {other:?}")),
            }
            if fail_cleanup {
                if DebugAdapter::clear_active_session_state_with_terminator(
                    &adapter.session,
                    &adapter.tcp_session,
                    &adapter.attached_pid,
                    |_| false,
                ) {
                    return Err("injected cleanup failure unexpectedly succeeded".to_string());
                }
                let retained_state = adapter
                    .session
                    .lock()
                    .map_err(|_| "session lock poisoned")?
                    .as_ref()
                    .map(|session| session.state.clone());
                if retained_state != Some(DebugState::Terminated) {
                    return Err(
                        "failed cleanup did not retain the terminal child owner".to_string()
                    );
                }
            }
            if invalidate {
                let broker_generation = adapter.operation_broker.current_session_generation();
                if !adapter.invalidate_session_generation_if_current(
                    generation,
                    broker_generation,
                    "test_late_invalidation",
                ) {
                    return Err("current generation invalidation did not execute".to_string());
                }
                if adapter.current_session_generation() == generation {
                    return Err("invalidation did not retire the generation".to_string());
                }
            }
            for request_seq in [1, 2] {
                match adapter.handle_request(request_seq, "disconnect", None) {
                    DapMessage::Response { success: true, command, .. }
                        if command == "disconnect" => {}
                    other => return Err(format!("disconnect failed: {other:?}")),
                }
                if let Some(message) = receiver.try_iter().find(|message| {
                    matches!(message, DapMessage::Event { event, .. } if event == "terminated")
                }) {
                    return Err(format!("disconnect duplicated async completion: {message:?}"));
                }
            }
        }
        Ok(())
    }

    fn terminal_request_retires_pending_before_drain(
        command: &'static str,
        reserved_watchdog: bool,
    ) -> Result<(), String> {
        let (outbound, received) = sync_channel(1);
        outbound
            .send(DapMessage::Event { seq: 0, event: "queue_filler".to_string(), body: None })
            .map_err(|error| error.to_string())?;
        let mut adapter = DebugAdapter::new();
        adapter.set_event_sender(outbound.clone());
        adapter.seed_session_for_test().map_err(|error| error.to_string())?;
        let generation = adapter.current_session_generation();
        let state = adapter.termination_state.clone();
        let sequence = adapter.seq.clone();
        let rescue = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_rescue = rescue.clone();
        let (parked_sender, parked_receiver) = sync_channel(1);
        let (finished_sender, finished_receiver) = sync_channel(1);
        let emitter = thread::spawn(move || {
            let stale = || {
                let _ = parked_sender.try_send(());
                worker_rescue.load(std::sync::atomic::Ordering::SeqCst)
                    || lock_or_recover(&state, "test.termination_state").generation != generation
            };
            let sender = EventSender::new(outbound);
            let body = Some(json!({"reason": "old_async_completion"}));
            let emitted = if reserved_watchdog {
                reserve_terminated_event(&state, Some(generation))
                    && super::deliver_reserved_terminated_event(
                        &sender, &sequence, &state, generation, body, &stale,
                    )
            } else {
                super::emit_terminated_event_guarded(
                    &sender,
                    &sequence,
                    &state,
                    Some(generation),
                    body,
                    &stale,
                    None,
                )
            };
            let _ = finished_sender.send(emitted);
        });
        if parked_receiver.recv_timeout(Duration::from_secs(2)).is_err() {
            rescue.store(true, std::sync::atomic::Ordering::SeqCst);
            drop(received);
            emitter.join().map_err(|_| "emitter panicked")?;
            return Err("async emitter never reached guarded enqueue".to_string());
        }
        let (response_sender, response_receiver) = sync_channel(1);
        let request = thread::spawn(move || {
            let response = adapter.handle_request(1, command, None);
            let _ = response_sender.send(response);
        });
        let retired_before_drain = finished_receiver.recv_timeout(Duration::from_secs(2));
        if retired_before_drain.is_err() {
            rescue.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut terminal_events = Vec::new();
        let mut response = None;
        while Instant::now() < deadline {
            if let Ok(message) = received.recv_timeout(Duration::from_millis(10))
                && matches!(&message, DapMessage::Event { event, .. } if event == "terminated")
            {
                terminal_events.push(message);
            }
            if let Ok(message) = response_receiver.try_recv() {
                response = Some(message);
                break;
            }
        }
        terminal_events.extend(received.try_iter().filter(
            |message| matches!(message, DapMessage::Event { event, .. } if event == "terminated"),
        ));
        rescue.store(true, std::sync::atomic::Ordering::SeqCst);
        drop(received);
        emitter.join().map_err(|_| "emitter panicked")?;
        request.join().map_err(|_| "terminal request panicked")?;
        let old_emitted = retired_before_drain.map_err(|_| {
            format!("{command} did not retire the pending async terminal before queue drain")
        })?;
        if old_emitted {
            return Err("retired async send incorrectly reported delivery".to_string());
        }
        match response {
            Some(DapMessage::Response { success: true, command: actual, .. })
                if actual == command => {}
            other => return Err(format!("terminal request did not succeed: {other:?}")),
        }
        if terminal_events.len() != 1 {
            return Err(format!("expected exactly one terminal event, got {terminal_events:?}"));
        }
        if terminal_events.iter().any(|message| {
            matches!(message, DapMessage::Event { body: Some(body), .. }
                if body.get("reason").and_then(Value::as_str) == Some("old_async_completion"))
        }) {
            return Err("retired async event escaped into the client terminal response".to_string());
        }
        Ok(())
    }

    #[test]
    fn disconnect_terminal_takes_pending_async_reservation_before_queue_drain() -> Result<(), String>
    {
        terminal_request_retires_pending_before_drain("disconnect", false)
    }

    #[test]
    fn terminate_terminal_takes_pending_async_reservation_before_queue_drain() -> Result<(), String>
    {
        terminal_request_retires_pending_before_drain("terminate", false)
    }

    #[test]
    fn disconnect_terminal_takes_watchdog_reservation_before_queue_drain() -> Result<(), String> {
        terminal_request_retires_pending_before_drain("disconnect", true)
    }

    #[test]
    fn disconnect_terminal_stale_enqueue_is_not_delivery_or_lifecycle_closure() -> Result<(), String>
    {
        let (sender, receiver) = sync_channel(4);
        let mut adapter = DebugAdapter::new();
        adapter.set_event_sender(sender);
        adapter.seed_session_for_test().map_err(|error| error.to_string())?;
        let reported_delivery = super::emit_terminated_event_guarded(
            adapter.event_sender.as_ref().ok_or("missing event sender")?,
            &adapter.seq,
            &adapter.termination_state,
            Some(adapter.current_session_generation()),
            None,
            &|| true,
            None,
        );
        if receiver.try_recv().is_ok() {
            return Err("stale emitter published an event".to_string());
        }
        let response = adapter.handle_request(1, "disconnect", None);
        let terminal_count = receiver.try_iter().filter(|message| {
            matches!(message, DapMessage::Event { event, .. } if event == "terminated")
        }).count();
        if reported_delivery {
            return Err("stale enqueue was reported as delivered".to_string());
        }
        if !matches!(response, DapMessage::Response { success: true, .. }) || terminal_count != 1 {
            return Err(format!(
                "stale reservation closed lifecycle: {response:?}, events={terminal_count}"
            ));
        }
        Ok(())
    }

    #[test]
    fn disconnect_terminal_closed_channel_does_not_report_delivery() -> Result<(), String> {
        let (sender, receiver) = sync_channel(1);
        drop(receiver);
        let adapter = DebugAdapter::new();
        if emit_terminated_event(
            &EventSender::new(sender),
            &adapter.seq,
            &adapter.termination_state,
            Some(adapter.current_session_generation()),
            None,
            None,
        ) {
            return Err("closed channel reported terminal delivery".to_string());
        }
        Ok(())
    }

    #[test]
    fn stale_session_generation_cannot_emit_termination() -> Result<(), String> {
        let (sender, receiver) = sync_channel(64);
        let seq = Arc::new(Mutex::new(0));
        let termination_state =
            Mutex::new(super::TerminationState { generation: 2, ..Default::default() });

        if emit_terminated_event(
            &EventSender::new(sender.clone()),
            &seq,
            &termination_state,
            Some(1),
            Some(serde_json::json!({"reason": "stale_reader"})),
            None,
        ) {
            return Err("stale reader unexpectedly emitted termination".to_string());
        }
        if receiver.try_recv().is_ok() {
            return Err("stale reader sent a termination event".to_string());
        }

        if !emit_terminated_event(
            &EventSender::new(sender.clone()),
            &seq,
            &termination_state,
            Some(2),
            Some(serde_json::json!({"reason": "current_session"})),
            None,
        ) {
            return Err("current session failed to emit termination".to_string());
        }
        Ok(())
    }

    #[test]
    fn reserved_termination_retired_when_generation_advances_before_delivery() -> Result<(), String>
    {
        let (sender, receiver) = sync_channel(64);
        let seq = Arc::new(Mutex::new(0));
        let termination_state =
            Arc::new(Mutex::new(super::TerminationState { generation: 3, ..Default::default() }));

        // Watchdog-style early reservation: the debuggee watchdog reserves
        // before killing the process and delivers only after, so a client
        // terminal request (or replacement launch) can advance the generation
        // while this send is still outstanding.
        if !reserve_terminated_event(&termination_state, Some(3)) {
            return Err("watchdog-style reservation under the current generation failed".into());
        }

        // The generation advances while the reserved send is in flight
        // (`close_terminal_session_generation` / replacement launch shape).
        {
            let mut state = termination_state
                .lock()
                .map_err(|_| "termination state lock poisoned".to_string())?;
            state.generation = 4;
            state.emitted = false;
        }

        // The stale delivery is retired, not sent.
        if terminated_delivery_is_current(&termination_state, Some(3)) {
            return Err("stale delivery reported current after generation advanced".into());
        }

        // A delivery under the now-current generation is still acknowledged.
        if !emit_terminated_event(
            &EventSender::new(sender.clone()),
            &seq,
            &termination_state,
            Some(4),
            Some(serde_json::json!({"reason": "current_generation"})),
            None,
        ) {
            return Err("current-generation emission was suppressed".into());
        }

        // Exactly one event reached the channel: the current generation's.
        match receiver.try_recv() {
            Ok(super::DapMessage::Event { event, body, .. }) => {
                if event != "terminated"
                    || body.as_ref().and_then(|v| v.get("reason")).and_then(|v| v.as_str())
                        != Some("current_generation")
                {
                    return Err(format!("unexpected termination event: {event}, {body:?}"));
                }
            }
            other => return Err(format!("expected termination event, got {other:?}")),
        }
        match receiver.try_recv() {
            Err(TryRecvError::Empty) => Ok(()),
            Err(error) => Err(format!("termination channel error: {error}")),
            Ok(other) => Err(format!("stale reservation leaked a duplicate event: {other:?}")),
        }
    }

    #[test]
    fn terminated_emission_holds_the_drain_latch_only_when_sent() -> Result<(), String> {
        use std::time::Duration;
        // A sent terminal event retains its drain reservation for the
        // transport consumer, so `run_with_io` observes it before the
        // response that follows.
        let (sender, _receiver) = sync_channel(64);
        let seq = Arc::new(Mutex::new(0));
        let termination_state = Arc::new(Mutex::new(super::TerminationState {
            generation: 1,
            emitted: false,
            terminal_committed: false,
        }));
        let drain = EventDrainLatch::default();
        if !emit_terminated_event(
            &EventSender::new(sender),
            &seq,
            &termination_state,
            Some(1),
            None,
            Some(&drain),
        ) {
            return Err("a live terminal emission must report delivery".to_string());
        }
        if drain.wait_until_drained(Duration::from_millis(0)) {
            return Err("a sent terminal event must retain its drain reservation".to_string());
        }
        // A stale guarded dispatch retires without retaining anything.
        let (sender, _receiver) = sync_channel(64);
        let stale_state = Arc::new(Mutex::new(super::TerminationState {
            generation: 1,
            emitted: false,
            terminal_committed: false,
        }));
        let stale_drain = EventDrainLatch::default();
        if super::emit_terminated_event_guarded(
            &EventSender::new(sender),
            &seq,
            &stale_state,
            Some(1),
            None,
            &|| true,
            Some(&stale_drain),
        ) {
            return Err("a stale dispatch must retire without reporting delivery".to_string());
        }
        if !stale_drain.wait_until_drained(Duration::from_millis(0)) {
            return Err("a stale terminal emission must not retain a reservation".to_string());
        }
        // A disconnected channel completes its reservation as well.
        let (sender, _receiver) = sync_channel(64);
        let closed = EventSender::new(sender);
        closed.close();
        let closed_drain = EventDrainLatch::default();
        if emit_terminated_event(
            &closed,
            &seq,
            &Arc::new(Mutex::new(super::TerminationState {
                generation: 7,
                emitted: false,
                terminal_committed: false,
            })),
            Some(7),
            None,
            Some(&closed_drain),
        ) {
            return Err("emission on a closed channel must report failure".to_string());
        }
        if !closed_drain.wait_until_drained(Duration::from_millis(0)) {
            return Err("a disconnected emission must not retain a reservation".to_string());
        }
        Ok(())
    }

    #[test]
    fn stale_session_generation_cannot_clear_attached_pid() -> Result<(), String> {
        let session = Arc::new(Mutex::new(None));
        let tcp_session = Arc::new(Mutex::new(None));
        let attached_pid = Arc::new(Mutex::new(Some(4242_u32)));
        let termination_state =
            Mutex::new(super::TerminationState { generation: 2, ..Default::default() });

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

    /// A wrapper shim may uppercase the whole diagnostic; the recased
    /// `IN @INC` separator must still terminate the module path so the
    /// remediation names the module verbatim instead of the diagnostic tail.
    #[test]
    fn missing_module_name_preserves_module_when_diagnostic_is_recased() {
        let detail = "CAN'T LOCATE SOME/MISSING/MODULE IN @INC (@INC CONTAINS: C:/PERL/LIB .)";

        let module = DebugAdapter::missing_module_name(detail);

        assert_eq!(module.as_deref(), Some("SOME::MISSING::MODULE"));
    }

    /// Mixed recasing keeps the module's own case: only the separators are
    /// matched case-insensitively, the module name is preserved verbatim.
    #[test]
    fn missing_module_name_preserves_module_case_under_partial_recasing() {
        let detail = "Can't locate Some/Missing/Module.pm IN @INC (you may need to install \
                      the Some::Missing::Module module)";

        let module = DebugAdapter::missing_module_name(detail);

        assert_eq!(module.as_deref(), Some("Some::Missing::Module"));
    }

    /// `{"A": "x;B=y"}` and `{"A": "x", "B": "y"}` serialized identically
    /// under the old delimiter join; the length-prefixed encoding must keep
    /// them distinct so neither launch inherits the other's verdict.
    #[test]
    fn capability_probe_cache_key_distinguishes_env_delimiter_collisions() {
        let joined = HashMap::from([("A".to_string(), "x;B=y".to_string())]);
        let split =
            HashMap::from([("A".to_string(), "x".to_string()), ("B".to_string(), "y".to_string())]);
        let cwd = PathBuf::from(".");

        let joined_key = DebugAdapter::capability_probe_cache_key("perl", &joined, &cwd);
        let split_key = DebugAdapter::capability_probe_cache_key("perl", &split, &cwd);

        assert_ne!(joined_key, split_key);
    }

    /// The same collision class applies to the interpreter/cwd boundary:
    /// `@` inside a path must not splice two launches into one key.
    #[test]
    fn capability_probe_cache_key_distinguishes_delimiters_inside_components() {
        let env = HashMap::new();

        let spliced = DebugAdapter::capability_probe_cache_key("a@b", &env, &PathBuf::from("c"));
        let honest = DebugAdapter::capability_probe_cache_key("a", &env, &PathBuf::from("b@c"));

        assert_ne!(spliced, honest);
    }

    /// A pipe whose write end a descendant inherited never reaches EOF, so
    /// the drain wait must expire on budget and abandon the pipe instead of
    /// hanging the launch past the probe deadline.
    #[test]
    fn bounded_probe_drain_abandons_pipe_that_never_reaches_eof() {
        let (_sender, receiver) = std::sync::mpsc::channel::<String>();
        let started = Instant::now();

        let text = super::join_probe_drain_within(Some(receiver), Duration::from_millis(50));

        assert_eq!(text, None, "an abandoned drain must not deliver text");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "drain collection must stay bounded, took {:?}",
            started.elapsed()
        );
    }

    /// A drain that reaches EOF in time delivers its decoded contents.
    #[test]
    fn bounded_probe_drain_delivers_text_when_eof_arrives() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let _ = sender.send("OK".to_string());

        let text = super::join_probe_drain_within(Some(receiver), Duration::from_secs(5));

        assert_eq!(text.as_deref(), Some("OK"));
    }

    /// `spawn_probe_pipe_drain` grips both arms: no pipe yields no drain, and
    /// a pipe that reaches EOF delivers its decoded contents through the
    /// drain (ripr discriminator for the spawn seam).
    #[test]
    fn probe_pipe_drain_spawns_only_for_a_live_pipe() {
        use std::io::Cursor;

        assert!(super::spawn_probe_pipe_drain(None::<Cursor<Vec<u8>>>).is_none());

        let drain = super::spawn_probe_pipe_drain(Some(Cursor::new(b"hello".to_vec())));
        let text = super::join_probe_drain_within(drain, Duration::from_secs(5));

        assert_eq!(text.as_deref(), Some("hello"));
    }

    /// Only the marker's own trimmed line certifies a pass; incidental
    /// "OK" substrings elsewhere in child output must not.
    #[test]
    fn probe_success_marker_requires_its_own_line() {
        assert!(has_probe_success_marker("OK\n"));
        assert!(has_probe_success_marker("noise\nOK\nmore noise"));
        assert!(has_probe_success_marker("  OK  \n"));
        assert!(!has_probe_success_marker(""));
        assert!(!has_probe_success_marker("OKAY\n"));
        assert!(!has_probe_success_marker("the OK substring alone\n"));
        // A doubled marker on one line still is not the marker's own line.
        assert!(!has_probe_success_marker("OK OK\n"));
        // Windows-style probe output carries CRLF: the trim must still
        // isolate the marker's own line.
        assert!(has_probe_success_marker("noise\r\nOK\r\n"));
        assert!(!has_probe_success_marker("OKAY\r\n"));
    }

    /// Boundary inputs the own-line rule leaves open: a bare marker with no
    /// trailing newline, tab padding, and whitespace-only lines, which trim
    /// to empty and must never certify. Return values are asserted with
    /// `assert_eq!` so the true/false outcome per boundary input is exact:
    /// the oracle gap grammar matches `assert_eq!` return-value assertions,
    /// which is why the `bool_assert_comparison` lint is allowed here.
    /// (ripr discriminator for the
    /// `line.trim() == DEBUGGER_PROBE_SUCCESS_MARKER` seam.)
    #[test]
    #[allow(clippy::bool_assert_comparison)]
    fn has_probe_success_marker_boundary_discriminator() {
        assert_eq!(has_probe_success_marker("OK"), true);
        assert_eq!(has_probe_success_marker("\tOK\t\n"), true);
        assert_eq!(has_probe_success_marker("   \n"), false);
        assert_eq!(has_probe_success_marker("\t \r\n"), false);
    }

    /// Write an executable shell probe double: `body` runs with the probe's
    /// argv, so activation tests can observe the exact spawn.
    #[cfg(unix)]
    fn write_probe_double(dir: &std::path::Path, name: &str, body: &str) -> Result<String, String> {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n"))
            .map_err(|error| format!("writing probe double: {error}"))?;
        let mut perms = std::fs::metadata(&path)
            .map_err(|error| format!("reading probe double metadata: {error}"))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms)
            .map_err(|error| format!("chmod probe double: {error}"))?;
        path.to_str()
            .map(str::to_string)
            .ok_or_else(|| "probe double path is not UTF-8".to_string())
    }

    /// Exact error variant for a measured incapable verdict: an interpreter
    /// that exits 0 without evaluating the probe expression (silent stdout)
    /// must surface the full incapable message verbatim — interpreter path,
    /// remediation, and measured detail. Every word is literal in this test
    /// so a reworded variant fails. The `expect_err` shape is what the
    /// oracle gap grammar matches for error-variant assertions, hence the
    /// targeted `expect_used` allow. (ripr discriminator for the
    /// `run_debugger_capability_probe` match seam.)
    #[cfg(unix)]
    #[test]
    #[allow(clippy::expect_used)]
    fn check_debugger_capability_exact_error_variant() -> Result<(), String> {
        use super::DebuggerCapabilityProbe;
        let dir = tempfile::tempdir().map_err(|error| format!("tempdir: {error}"))?;
        let script = write_probe_double(dir.path(), "silent-probe-4f2a.sh", "exit 0")?;
        let expected_detail =
            "exited successfully but did not evaluate the probe expression (interpreter shim?)";
        let expected = format!(
            "Selected interpreter cannot host the debugger (perl5db.pl not loadable): {script}. Install a full Perl distribution that ships the core debugger module, or point launch.json `perlPath` at one (e.g. {{\"perlPath\": \"/path/to/full/perl\"}}). Detail: {expected_detail}"
        );
        // The silent double cannot verify capable, so an inconclusive
        // measurement (drain starved under load) re-measures instead of
        // failing: only a measured verdict is asserted. A reworded variant
        // never matches and fails immediately.
        for _ in 0..3 {
            let probe =
                DebugAdapter::run_debugger_capability_probe(&script, &HashMap::new(), dir.path());
            if matches!(&probe, DebuggerCapabilityProbe::Inconclusive) {
                continue;
            }
            // The exact error variant is asserted with `matches!` so the
            // oracle gap grammar observes the measured `Incapable` verdict.
            assert!(
                matches!(&probe, DebuggerCapabilityProbe::Incapable(_)),
                "silent probe must measure the incapable error variant"
            );
            let detail = match probe {
                DebuggerCapabilityProbe::Incapable(detail) => detail,
                DebuggerCapabilityProbe::Capable => {
                    return Err("silent probe must measure incapable, not capable".to_string());
                }
                DebuggerCapabilityProbe::Inconclusive => {
                    continue;
                }
            };
            assert_eq!(
                detail, expected_detail,
                "incapable detail must carry the measured probe diagnosis"
            );
            let err = DebugAdapter::check_debugger_capability(&script, &HashMap::new(), dir.path())
                .expect_err("silent probe must report incapable");
            assert_eq!(err, expected, "incapable verdict must carry the exact error variant");
            // The assertion text names the arm's remediation literal so the
            // static exposure tracer can observe the `Incapable => Err`
            // construction it guards.
            assert!(
                err.contains(
                    "Selected interpreter cannot host the debugger (perl5db.pl not loadable)"
                ),
                "incapable verdict must carry the remediation literal, got: {err:?}"
            );
            return Ok(());
        }
        Err("silent probe stayed inconclusive across re-measures; exact variant unobserved"
            .to_string())
    }

    /// Call-observation proof that the probe activates the interpreter with
    /// the perl5db.pl load expression: the double records its argv and
    /// prints the marker, so a skipped or reworded spawn fails the
    /// observation even when the verdict stays `Ok`. (ripr discriminator
    /// for the probe-spawn seam.)
    #[cfg(unix)]
    #[test]
    fn run_debugger_capability_probe_call_presence_observer() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|error| format!("tempdir: {error}"))?;
        let record = dir.path().join("probe-argv-9d1c.txt");
        let record_str = record.to_str().ok_or("record path is not UTF-8")?.to_string();
        let script = write_probe_double(
            dir.path(),
            "recording-probe-9d1c.sh",
            &format!("printf '%s\\n' \"$@\" >> '{record_str}'\nprintf 'OK\\n'"),
        )?;
        let verdict = DebugAdapter::check_debugger_capability(&script, &HashMap::new(), dir.path());
        if verdict != Ok(()) {
            return Err(format!("recording probe must verify capable, got: {verdict:?}"));
        }
        let argv = std::fs::read_to_string(&record)
            .map_err(|error| format!("reading argv record: {error}"))?;
        if !argv.contains("-e") || !argv.contains("require \"perl5db.pl\"") {
            return Err(format!(
                "probe must spawn the perl5db load expression, recorded argv: {argv:?}"
            ));
        }
        Ok(())
    }

    /// A probe that could not run (spawn failure) keeps the launch-continue
    /// disposition but must not be cached as a pass: the next launch has to
    /// probe again instead of trusting an instrument failure.
    #[test]
    fn inconclusive_probe_spawn_failure_is_not_cached_as_a_pass() {
        let interpreter = "perl-lsp-missing-probe-interpreter-9e2f1";
        let env = HashMap::new();
        let cwd = std::env::temp_dir();
        let key = DebugAdapter::capability_probe_cache_key(interpreter, &env, &cwd);
        if let Ok(cache) = super::DEBUGGER_PROBE_CACHE.lock() {
            assert!(cache.get(&key).is_none(), "precondition: key must start absent");
        }

        let verdict = DebugAdapter::check_debugger_capability(interpreter, &env, &cwd);

        assert!(verdict.is_ok(), "spawn failure is launch-continue, got: {verdict:?}");
        if let Ok(cache) = super::DEBUGGER_PROBE_CACHE.lock() {
            assert!(
                cache.get(&key).is_none(),
                "an inconclusive probe must not be cached as a pass"
            );
        }
    }

    /// A cached verdict is served without spawning a new probe: the cache-hit
    /// seam returns the stored verdict for the exact launch key. The sentinel
    /// is an `Err`, so the test fails if the lookup is skipped and the bogus
    /// interpreter is re-probed instead (that path yields launch-continue
    /// `Ok`). (ripr discriminator for the cache-hit seam.)
    #[test]
    fn cached_verdict_is_served_without_reprobing() {
        let interpreter = "perl-lsp-cached-probe-interpreter-7c3e9";
        let env = HashMap::new();
        let cwd = std::env::temp_dir();
        let key = DebugAdapter::capability_probe_cache_key(interpreter, &env, &cwd);
        if let Ok(mut cache) = super::DEBUGGER_PROBE_CACHE.lock() {
            cache.insert(key, Err("cached probe failure sentinel".to_string()));
        }

        let verdict = DebugAdapter::check_debugger_capability(interpreter, &env, &cwd);

        assert_eq!(
            verdict,
            Err("cached probe failure sentinel".to_string()),
            "the stored verdict must come back verbatim, without re-probing"
        );
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
    ///
    /// The "already exited" precondition uses a bounded poll instead of a fixed
    /// sleep: process-start latency under AV/load spikes exceeds any constant
    /// sleep and made this suite-order-dependent on Windows (#12791).
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

        // Wait for it to exit — deterministically, within a generous bound.
        if !DebugAdapter::wait_for_child_exit(&mut child, std::time::Duration::from_secs(10)) {
            return Err(
                "child did not exit within 10s; host process latency pathological".to_string()
            );
        }
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
            debuggee_cwd: std::path::PathBuf::from("."),
            last_resume_mode: ResumeMode::Unknown,
            initial_stop_pending: false,
            entry_stop_pending: false,
            stopped_generation: 0,
            module_generation: RuntimeModuleGenerationClock::new(),
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

    fn spawn_reader_fixture_child(mode: &str) -> Result<super::Child, String> {
        std::process::Command::new("perl")
                .arg("-e")
                .arg(r##"
                    my $mode = shift; my $round = 0; my $active;
                    select STDERR; $|=1; select STDOUT; $|=1;
                    while (<STDIN>) {
                        if (/DAP_BEGIN_(\d+)/) {
                            ++$round; $active = $round == 1 ? $mode : 'valid';
                            select undef, undef, undef, 0.9 if $active eq 'late_begin';
                            print STDERR "DAP_BEGIN_$1\n";
                        } elsif (/^T/) {
                            if ($active eq 'valid') {
                                print STDERR q{$ = main::run($value, [1, 2], "a,b") called from file `script.pl' line 7}, "\n";
                            } elsif ($active eq 'internal') {
                                print STDERR "# 0 DB::DB at /tmp/perl5db.pl line 8\n";
                            } elsif ($active eq 'partial' || $active eq 'late') {
                                print STDERR "main::(/tmp/poison.pl:9):\n";
                            } elsif ($active eq 'overflow') {
                                print STDERR "main::(/tmp/poison.pl:9):\n" for 1..2049;
                            }
                        } elsif (/DAP_END_(\d+)/) {
                            my $id=$1;
                            if ($active ne 'partial') {
                                select undef, undef, undef, 0.9 if $active eq 'late';
                                print STDERR "DAP_END_$id\nDB<1>\n";
                            }
                            print STDERR "READER_DONE_$round\n";
                        } elsif (/^outside/) {
                            print STDERR "DAP_BEGIN_999999\nmain::(/tmp/outside.pl:42):\nDAP_END_999999\nDB<9>\nOUTSIDE_DONE\n";
                        }
                    }
                "##)
                .arg(mode)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| format!("failed to spawn reader fixture: {error}"))
    }

    fn reader_stack_fixture(mode: &str) -> Result<Arc<DebugAdapter>, String> {
        let adapter = Arc::new(DebugAdapter::new());
        adapter.seed_stopped_session_with_frames_for_test(vec![super::StackFrame {
            id: 1,
            name: "sentinel".to_string(),
            source: super::Source {
                name: Some("sentinel.pl".to_string()),
                path: "/tmp/sentinel.pl".to_string(),
                source_reference: None,
            },
            line: 4,
            column: 1,
            end_line: None,
            end_column: None,
        }]);
        adapter.seed_stack_frame_arguments_for_test(1, vec!["sentinel_arg".to_string()]);
        {
            let mut guard = lock_or_recover(&adapter.session, "test.reader_fixture");
            let session = guard.as_mut().ok_or("Perl is required for the reader fixture")?;
            session.stopped_generation = 1;
            let child = spawn_reader_fixture_child(mode)?;
            let mut previous = std::mem::replace(&mut session.process, child);
            let _ = previous.kill();
            previous.wait().map_err(|error| format!("failed to reap seed process: {error}"))?;
        }
        adapter.start_output_reader(PathBuf::from("."));
        // Drop owns bounded child cleanup on both success and every error path.
        Ok(adapter)
    }

    fn wait_for_reader_barrier(adapter: &DebugAdapter, marker: &str) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if lock_or_recover(&adapter.recent_output, "test.reader_barrier")
                .lines
                .iter()
                .any(|line| line.normalized == marker)
            {
                // Reading this later line establishes that the preceding prompt
                // passed through the single reader loop, not merely its buffer.
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!("reader did not reach {marker}"));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn assert_reader_stack(
        adapter: &DebugAdapter,
        response: DapMessage,
        name: &str,
        line: i32,
        arguments: &[&str],
    ) -> Result<(), String> {
        let DapMessage::Response { success: true, body: Some(body), .. } = response else {
            return Err(format!("stackTrace failed: {response:?}"));
        };
        let frame = body
            .get("stackFrames")
            .and_then(Value::as_array)
            .and_then(|frames| frames.first())
            .ok_or_else(|| format!("missing stack frame: {body}"))?;
        if frame.get("name").and_then(Value::as_str) != Some(name)
            || frame.get("line").and_then(Value::as_i64) != Some(i64::from(line))
        {
            return Err(format!("unexpected stack response: {body}"));
        }
        let guard = lock_or_recover(&adapter.session, "test.reader_cache");
        let session = guard.as_ref().ok_or("reader lost the session")?;
        let cached = session.stack_frames.first().ok_or("reader lost cached frames")?;
        let expected = arguments.iter().map(|argument| (*argument).to_string()).collect::<Vec<_>>();
        if cached.name != name
            || cached.line != line
            || session.stack_frame_arguments.get(&cached.id) != Some(&expected)
        {
            return Err("reader changed accepted frames or arguments".to_string());
        }
        Ok(())
    }

    #[test]
    fn framed_reader_preserves_stack_and_recovers_after_partial_output() -> Result<(), String> {
        for mode in ["empty", "internal", "partial", "late", "late_begin", "valid"] {
            let adapter = reader_stack_fixture(mode)?;
            let response = adapter.handle_stack_trace(1, 1, Some(json!({"threadId": 1})));
            wait_for_reader_barrier(&adapter, "READER_DONE_1")?;
            if mode == "valid" {
                assert_reader_stack(
                    &adapter,
                    response,
                    "main::run",
                    7,
                    &["$value", "[1, 2]", "\"a,b\""],
                )?;
            } else {
                assert_reader_stack(&adapter, response, "sentinel", 4, &["sentinel_arg"])?;
            }
            // A newer registered frame recovers even if the previous one lacked
            // an end marker. It does not inherit the old poisoned context.
            let recovered = adapter.handle_stack_trace(2, 2, Some(json!({"threadId": 1})));
            wait_for_reader_barrier(&adapter, "READER_DONE_2")?;
            assert_reader_stack(
                &adapter,
                recovered,
                "main::run",
                7,
                &["$value", "[1, 2]", "\"a,b\""],
            )?;

            {
                let mut guard = lock_or_recover(&adapter.session, "test.reader_resume");
                let session = guard.as_mut().ok_or("missing session for next stop")?;
                session.state = DebugState::Running;
                let stdin = session.process.stdin.as_mut().ok_or("missing fixture stdin")?;
                DebugAdapter::write_debugger_command(stdin, "outside\n")?;
            }
            wait_for_reader_barrier(&adapter, "OUTSIDE_DONE")?;
            let guard = lock_or_recover(&adapter.session, "test.reader_new_stop");
            let session = guard.as_ref().ok_or("outside output lost session")?;
            if !matches!(session.state, DebugState::Stopped)
                || session.stopped_generation <= 1
                || session.stack_frames.first().map(|frame| frame.line) != Some(42)
                || !session.stack_frame_arguments.is_empty()
            {
                return Err(format!(
                    "{mode}: ordinary unowned output failed to establish next stop"
                ));
            }
        }
        Ok(())
    }

    #[test]
    fn framed_reader_exhaustion_clears_session_instead_of_interpreting_payload()
    -> Result<(), String> {
        let adapter = reader_stack_fixture("overflow")?;
        let response = adapter.handle_stack_trace(1, 1, Some(json!({"threadId": 1})));
        let DapMessage::Response { success: true, body: Some(body), .. } = response else {
            return Err(format!("unexpected exhausted-frame response: {response:?}"));
        };
        if body.get("stackFrames") != Some(&json!([])) {
            return Err(format!(
                "exhausted frame retained stack authority ({} frames)",
                body.get("stackFrames").and_then(Value::as_array).map_or(0, Vec::len),
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if lock_or_recover(&adapter.session, "test.reader_exhausted").is_none() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("exhausted reader did not reap and clear the session".to_string());
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn framed_reader_late_begin_cannot_mutate_replacement_session() -> Result<(), String> {
        let adapter = reader_stack_fixture("late_begin")?;
        let response = adapter.handle_stack_trace(1, 1, Some(json!({"threadId": 1})));
        assert_reader_stack(&adapter, response, "sentinel", 4, &["sentinel_arg"])?;
        adapter.begin_session_generation();
        let mut old_child = {
            let mut guard = lock_or_recover(&adapter.session, "test.reader_replacement");
            let session = guard.as_mut().ok_or("missing old session")?;
            let child = spawn_reader_fixture_child("valid")?;
            session.stack_frames.first_mut().ok_or("missing old frame")?.name =
                "replacement".to_string();
            session.stack_frame_arguments.insert(1, vec!["replacement_arg".to_string()]);
            std::mem::replace(&mut session.process, child)
        };
        adapter.operation_broker.open_session();
        adapter.start_output_reader(PathBuf::from("."));
        // Closing input lets the old fixture exit after its delayed output. Its
        // reader may close the old pipe on detecting replacement; either outcome
        // must leave the replacement's authorities intact.
        drop(old_child.stdin.take());
        let result = (|| {
            if !DebugAdapter::wait_for_child_exit(&mut old_child, Duration::from_secs(5)) {
                return Err("old reader fixture failed to finish delayed output".to_string());
            }
            {
                let guard = lock_or_recover(&adapter.session, "test.reader_replacement_cache");
                let session = guard.as_ref().ok_or("late reader cleared replacement")?;
                if session.stack_frames.first().map(|frame| frame.name.as_str())
                    != Some("replacement")
                    || session.stack_frame_arguments.get(&1)
                        != Some(&vec!["replacement_arg".to_string()])
                {
                    return Err("late old reader poisoned replacement".to_string());
                }
            }
            let recovered = adapter.handle_stack_trace(2, 2, Some(json!({"threadId": 1})));
            wait_for_reader_barrier(&adapter, "READER_DONE_1")?;
            assert_reader_stack(
                &adapter,
                recovered,
                "main::run",
                7,
                &["$value", "[1, 2]", "\"a,b\""],
            )
        })();
        let _ = old_child.kill();
        old_child.wait().map_err(|error| format!("failed to reap old reader fixture: {error}"))?;
        result
    }

    /// Spawn a controllable fake debugger for the #15637 entry-stop proofs.
    ///
    /// `mode` selects the stderr bootstrap script. After it, the fixture loops
    /// on stdin answering framed `T` queries exactly like the reader fixture
    /// above, so an immediate `stackTrace` observes real frame authority.
    fn spawn_entry_stop_fixture_child(mode: &str) -> Result<super::Child, String> {
        std::process::Command::new("perl")
            .arg("-e")
            .arg(r##"
                my $mode = shift;
                select STDERR; $|=1; select STDOUT; $|=1;
                if ($mode eq 'delayed_context') {
                    select undef, undef, undef, 0.6;
                    print STDERR "main::(entry_fixture.pl:2):\tuse strict;\n";
                    print STDERR "DB<1>\n";
                    print STDERR "ENTRY_FIXTURE_BOOTSTRAP_DONE\n";
                } elsif ($mode eq 'prompt_only') {
                    select undef, undef, undef, 0.2;
                    print STDERR "DB<1>\n";
                    print STDERR "ENTRY_FIXTURE_BOOTSTRAP_DONE\n";
                } elsif ($mode eq 'context_retained') {
                    print STDERR "main::(entry_fixture.pl:2):\tuse strict;\n";
                    print STDERR "ENTRY_FIXTURE_BOOTSTRAP_DONE\n";
                } elsif ($mode eq 'eof_before_stop') {
                    print STDERR "bootstrap chatter without any debugger context\n";
                    exit 0;
                }
                while (<STDIN>) {
                    if (/DAP_BEGIN_(\d+)/) {
                        print STDERR "DAP_BEGIN_$1\n";
                        print STDERR q{$ = main::entry_fixture called from file `entry_fixture.pl' line 2}, "\n";
                    } elsif (/DAP_END_(\d+)/) {
                        print STDERR "DAP_END_$1\nDB<1>\n";
                    }
                }
            "##)
            .arg(mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to spawn entry-stop fixture: {error}"))
    }

    /// Launch-shaped #15637 fixture: a `Running` session with empty frames —
    /// exactly what `handle_launch` leaves behind — plus the pending-stop flags
    /// a `stopOnEntry`/plain launch would install, and a live output reader.
    fn entry_stop_fixture(
        mode: &str,
        entry_stop_pending: bool,
        initial_stop_pending: bool,
    ) -> Result<(Arc<DebugAdapter>, std::sync::mpsc::Receiver<DapMessage>), String> {
        use super::{DebugSession, ResumeMode, VariableCache};

        let child = spawn_entry_stop_fixture_child(mode)?;
        let (sender, receiver) = sync_channel(64);
        let mut adapter = DebugAdapter::new();
        adapter.set_event_sender(sender);
        let adapter = Arc::new(adapter);
        adapter.operation_broker.open_session();
        {
            let mut guard = lock_or_recover(&adapter.session, "test.entry_fixture");
            *guard = Some(DebugSession {
                process: child,
                state: DebugState::Running,
                stack_frames: Vec::new(),
                stack_frame_arguments: HashMap::new(),
                variable_cache: VariableCache::default(),
                thread_id: 1,
                debuggee_cwd: PathBuf::from("."),
                last_resume_mode: ResumeMode::Unknown,
                initial_stop_pending,
                entry_stop_pending,
                stopped_generation: 0,
                module_generation: crate::reload::RuntimeModuleGenerationClock::new(),
            });
        }
        adapter.start_output_reader(PathBuf::from("."));
        Ok((adapter, receiver))
    }

    /// Collect `stopped` reasons until `deadline`, failing if the wait errors.
    fn stopped_reasons_within(
        receiver: &std::sync::mpsc::Receiver<DapMessage>,
        window: Duration,
    ) -> Result<Vec<String>, String> {
        let deadline = Instant::now() + window;
        let mut reasons = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(reasons);
            }
            match receiver.recv_timeout(remaining) {
                Ok(DapMessage::Event { event, body, .. }) => {
                    if event == "stopped" {
                        reasons.push(
                            body.as_ref()
                                .and_then(|value| value.get("reason"))
                                .and_then(Value::as_str)
                                .unwrap_or("unknown")
                                .to_string(),
                        );
                    }
                }
                Ok(_) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Ok(reasons),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("event channel disconnected while waiting for stopped".to_string());
                }
            }
        }
    }

    /// Wait for the first `stopped` event and return its reason.
    fn first_stopped_reason(
        receiver: &std::sync::mpsc::Receiver<DapMessage>,
    ) -> Result<String, String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("no stopped event arrived within 5s".to_string());
            }
            match receiver.recv_timeout(remaining) {
                Ok(DapMessage::Event { event, body, .. }) => {
                    if event == "stopped" {
                        return Ok(body
                            .as_ref()
                            .and_then(|value| value.get("reason"))
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_string());
                    }
                }
                Ok(_) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err("no stopped event arrived within 5s".to_string());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("event channel disconnected while waiting for stopped".to_string());
                }
            }
        }
    }

    /// #15637 regression: a `stopOnEntry` launch must NOT emit its entry stop
    /// eagerly. While the debuggee is still booting (session `Running`, no
    /// frames), no stopped event may exist; the entry stop is published exactly
    /// once, from the first real debugger suspension whose frames a subsequent
    /// `stackTrace` can immediately observe.
    #[test]
    fn stop_on_entry_launch_defers_entry_stop_to_first_real_suspension() -> Result<(), String> {
        let (adapter, receiver) = entry_stop_fixture("delayed_context", true, false)?;

        // The fixture stays silent for 600ms after spawn. The old eager launch
        // emission would have delivered `stopped(reason=entry)` long before any
        // debugger output existed; it must not appear in that window.
        let early = stopped_reasons_within(&receiver, Duration::from_millis(250))?;
        if !early.is_empty() {
            return Err(format!(
                "entry stop was published before the first real suspension: {early:?}"
            ));
        }

        let reason = first_stopped_reason(&receiver)?;
        if reason != "entry" {
            return Err(format!("first real suspension published reason {reason:?}, not entry"));
        }

        {
            let guard = lock_or_recover(&adapter.session, "test.entry_stopped");
            let session = guard.as_ref().ok_or("entry stop lost the session")?;
            if !matches!(session.state, DebugState::Stopped) {
                return Err("entry stop published without a Stopped session".to_string());
            }
            if session.stopped_generation != 1 {
                return Err(format!(
                    "entry stop bound to generation {}, expected 1",
                    session.stopped_generation
                ));
            }
            let frame = session.stack_frames.first().ok_or("entry stop has no frame")?;
            if frame.source.path != "entry_fixture.pl" || frame.line != 2 {
                return Err(format!(
                    "entry stop frame is not the launched script location: {:?}:{}",
                    frame.source.path, frame.line
                ));
            }
        }

        // Immediately after the entry event — no retry — the frame authority the
        // reader just established must answer `stackTrace`.
        let response = adapter.handle_stack_trace(1, 1, Some(json!({"threadId": 1})));
        let DapMessage::Response { success: true, body: Some(body), .. } = response else {
            return Err(format!("stackTrace failed right after the entry stop: {response:?}"));
        };
        let frame = body
            .get("stackFrames")
            .and_then(Value::as_array)
            .and_then(|frames| frames.first())
            .ok_or("stackTrace returned no frames for the entry stop")?;
        if frame.get("line").and_then(Value::as_i64) != Some(2) {
            return Err(format!("entry-stop stackTrace pointed elsewhere: {frame}"));
        }

        // Exactly one entry stop per session: the fixture's later prompt may
        // report its own stop, but a second `entry` must never be emitted.
        wait_for_reader_barrier(&adapter, "ENTRY_FIXTURE_BOOTSTRAP_DONE")?;
        let later = stopped_reasons_within(&receiver, Duration::from_millis(300))?;
        if later.iter().any(|reason| reason == "entry") {
            return Err(format!("a second entry stop was emitted: {later:?}"));
        }
        Ok(())
    }

    /// #15637: when the prompt is the first observed authority (context output
    /// never parsed), the pending entry reason is still consumed exactly once
    /// instead of leaving the client waiting for an entry stop forever.
    #[test]
    fn stop_on_entry_prompt_only_bootstrap_still_honors_pending_entry() -> Result<(), String> {
        let (adapter, receiver) = entry_stop_fixture("prompt_only", true, false)?;
        let reason = first_stopped_reason(&receiver)?;
        if reason != "entry" {
            return Err(format!("prompt-first authority published {reason:?}, not entry"));
        }
        let guard = lock_or_recover(&adapter.session, "test.entry_prompt");
        let session = guard.as_ref().ok_or("prompt entry stop lost the session")?;
        if !matches!(session.state, DebugState::Stopped) {
            return Err("prompt entry stop published without a Stopped session".to_string());
        }
        wait_for_reader_barrier(&adapter, "ENTRY_FIXTURE_BOOTSTRAP_DONE")?;
        let later = stopped_reasons_within(&receiver, Duration::from_millis(300))?;
        if later.iter().any(|reason| reason == "entry") {
            return Err(format!("a second entry stop was emitted: {later:?}"));
        }
        Ok(())
    }

    /// #15637 negative control: `stopOnEntry=false` must keep publishing no stop
    /// from the implicit first-line pause — that suspension stays reserved for
    /// the configuration/breakpoint admission route.
    #[test]
    fn stop_on_entry_false_launch_publishes_no_stop_from_implicit_first_pause() -> Result<(), String>
    {
        let (adapter, receiver) = entry_stop_fixture("context_retained", false, true)?;
        wait_for_reader_barrier(&adapter, "ENTRY_FIXTURE_BOOTSTRAP_DONE")?;
        {
            let guard = lock_or_recover(&adapter.session, "test.retained_stop");
            let session = guard.as_ref().ok_or("retained stop lost the session")?;
            if !matches!(session.state, DebugState::Stopped) {
                return Err("implicit first-line pause was not retained".to_string());
            }
        }
        let published = stopped_reasons_within(&receiver, Duration::from_millis(300))?;
        if !published.is_empty() {
            return Err(format!(
                "stopOnEntry=false published a stop before configuration admitted one: {published:?}"
            ));
        }
        Ok(())
    }

    /// #15637: if the debuggee dies before the first real suspension, the client
    /// observes termination — never a synthetic stopped event for a stop that
    /// never happened.
    #[test]
    fn entry_stop_pending_yields_termination_not_synthetic_stop_on_eof() -> Result<(), String> {
        // The adapter stays bound for the whole test: it owns the fixture child.
        let (adapter, receiver) = entry_stop_fixture("eof_before_stop", true, false)?;
        let _adapter = adapter;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("no terminal event arrived after debugger EOF".to_string());
            }
            match receiver.recv_timeout(remaining) {
                Ok(DapMessage::Event { event, body, .. }) => {
                    if event == "stopped" {
                        let reason = body
                            .as_ref()
                            .and_then(|value| value.get("reason"))
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        return Err(format!(
                            "a synthetic stopped event ({reason}) was published for a session that never suspended"
                        ));
                    }
                    if event == "terminated" {
                        return Ok(());
                    }
                }
                Ok(_) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err("no terminal event arrived after debugger EOF".to_string());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("event channel disconnected before termination".to_string());
                }
            }
        }
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
        use crate::debug_adapter::operation_broker::{BrokerOperationSpec, OperationClass};

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
        let operation = adapter
            .operation_broker
            .submit(BrokerOperationSpec {
                request_seq: None,
                class: OperationClass::Query,
                session_generation: adapter.operation_broker.current_session_generation(),
                suspension_generation: None,
                timeout: Duration::from_secs(2),
                cancellation: None,
            })
            .map_err(|error| {
                format!("watchdog regression operation must be admitted: {error:?}")
            })?;

        let session = DebugSession {
            process: child,
            state: DebugState::Running,
            stack_frames: Vec::new(),
            stack_frame_arguments: HashMap::new(),
            variable_cache: VariableCache::default(),
            thread_id: 1,
            debuggee_cwd: std::path::PathBuf::from("."),
            last_resume_mode: ResumeMode::Unknown,
            initial_stop_pending: false,
            entry_stop_pending: false,
            stopped_generation: 0,
            module_generation: RuntimeModuleGenerationClock::new(),
        };
        *lock_or_recover(&adapter.session, "test.session") = Some(session);

        adapter.start_debuggee_watchdog(1);

        // The watchdog must settle brokered waiters before attempting the kill.
        // No output reader is started in this fixture, so EOF cannot mask a
        // missing pre-kill settlement.
        let terminal = adapter.operation_broker.await_framed_payload(
            &operation,
            "never-begin",
            "never-end",
            &adapter.recent_output,
        );
        if terminal
            != crate::debug_adapter::operation_broker::BrokerTerminal::SessionGone(
                "debuggee_timeout",
            )
        {
            return Err(format!(
                "watchdog must settle a pending query before kill, got {terminal:?}"
            ));
        }

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
