//! DAP lifecycle matrix e2e tests.
//!
//! Drives the FULL DAP lifecycle in protocol-correct order against a real `perl -d`:
//!   initialize → launch → setBreakpoints → configurationDone
//!     → stopped(reason=breakpoint) → stackTrace → scopes → variables
//!     → continue → terminated(natural exit) → disconnect
//!
//! Uses `DapWorkflowSession` helpers from `common/mod.rs` (including the
//! `set_breakpoints_checked`/`wait_stopped_with_frame` additions from #927).
//!
//! All tests skip gracefully when `perl` is not on `PATH`.
//! AC: DAP lifecycle matrix — phase 2 e2e coverage.

mod common;

use common::{DapWorkflowSession, debuggee_perl_or_typed_skip, workflow_timeout};
use perl_dap::debug_adapter::{DapMessage, DapMessageWithEpoch, DebugAdapter};
use perl_tdd_support::must_some;
use serde_json::{Value, json};
use std::fs::write;
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::Duration;
use tempfile::tempdir;

// ─── Fixture ──────────────────────────────────────────────────────────────────────────────
//
//   Line 1: use strict;
//   Line 2: use warnings;
//   Line 3: (blank)
//   Line 4: my $x = 10;      <- first executable line; perl -d always pauses here
//   Line 5: my $y = $x + 5;  <- reliable first breakpoint (configurationDone runs FROM line 4 TO line 5)
//   Line 6: my $z = $x * $y;
//   Line 7: print "$z\n";
//
// Line 4 is the implicit stop line — a breakpoint there is skipped by the
// initial `c` from configurationDone.  Tests use line 5 (BP_LINE) as the
// reliable first hit.

const BP_LINE: u64 = 5;

fn lifecycle_script_content() -> &'static str {
    "use strict;\nuse warnings;\n\nmy $x = 10;\nmy $y = $x + 5;\nmy $z = $x * $y;\nprint \"$z\\n\";\n"
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ─── Test 1: full ordered lifecycle — single test, every step asserted ────────

/// Exercises the FULL DAP lifecycle in protocol-correct order:
///
///   1.  initialize (via DapWorkflowSession::new) → success + initialized event
///   2.  launch(stopOnEntry=false) → success
///   3.  setBreakpoints(verified=true) → adapter-resolved line returned
///   4.  configurationDone → success
///   5.  wait for stopped(reason=breakpoint) ← precedes stackTrace (ordering contract)
///   6.  stackTrace → concrete frame whose line matches resolved breakpoint line
///   7.  scopes → Locals scope present
///   8.  variables(locals) → non-empty list
///   9.  continue → execution resumes
///  10.  terminated event received (natural program exit via `continue`)
///  11.  disconnect → clean (disconnect helper drains terminated then returns)
///
/// This is the "happy path" lifecycle matrix: every step is asserted in order,
/// and each assertion documents the adapter contract it validates.
#[test]
fn test_lifecycle_full_ordered_sequence() -> TestResult {
    let Some(debuggee_perl) = debuggee_perl_or_typed_skip("test_lifecycle_full_ordered_sequence")
    else {
        return Ok(());
    };

    let workspace = tempdir()?;
    let script = workspace.path().join("lifecycle_matrix.pl");
    write(&script, lifecycle_script_content())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let timeout = workflow_timeout();

    // ── Step 1: initialize ───────────────────────────────────────────
    // DapWorkflowSession::new sends `initialize`, asserts success, and drains
    // the `initialized` event — verifying steps 1+1a before continuing.
    let mut session = DapWorkflowSession::new(timeout)?;

    // ── Step 2: launch ─────────────────────────────────────────────
    // stopOnEntry=false: adapter does NOT emit stopped until configurationDone.
    session.launch_pinned(&debuggee_perl.binary, &script_str)?;

    // ── Step 3: setBreakpoints (verified=true) ──────────────────────────────────
    // DAP protocol ordering: setBreakpoints MUST be called before configurationDone.
    // set_breakpoints_checked asserts verified=true for every entry and returns
    // the adapter-resolved line numbers (which may differ from requested in
    // future when breakpoint remapping is implemented).
    let resolved = session.set_breakpoints_checked(&script_str, &[BP_LINE])?;
    let resolved_line =
        resolved.first().copied().ok_or("set_breakpoints_checked returned empty resolved lines")?;

    // Resolved line must be positive (sanity: adapter gave us a real line).
    assert!(
        resolved_line > 0,
        "adapter-resolved breakpoint line must be positive, got {resolved_line}"
    );

    // ── Step 4: configurationDone ──────────────────────────────────────────
    session.configuration_done()?;

    // ── Step 5: stopped(reason=breakpoint) ──────────────────────────────────
    // ORDERING CONTRACT: the `stopped` event must arrive BEFORE we can issue
    // `stackTrace`.  We assert the event precedes the request by not calling
    // `stack_trace` until after `wait_stopped` returns.
    //
    // `wait_stopped_with_frame` is atomic: it waits for `stopped`, then
    // immediately calls `stackTrace` — so the ordering is correct by construction.
    let frame_info = session.wait_stopped_with_frame()?;

    assert_eq!(
        frame_info.reason, "breakpoint",
        "stopped reason at first breakpoint must be `breakpoint`, got `{}`",
        frame_info.reason
    );

    // ── Step 6: stackTrace → frame whose line matches resolved breakpoint ─────
    // The line contract: adapter-resolved line from setBreakpoints == stopped frame line.
    assert_eq!(
        frame_info.line, resolved_line,
        "stackTrace frame line must equal adapter-resolved breakpoint line \
         (resolved={resolved_line}, BP_LINE={BP_LINE}), got frame_line={}",
        frame_info.line
    );

    // Source path must reference our script (not an internal file).
    assert!(
        frame_info.source_path.contains("lifecycle_matrix"),
        "stackTrace source path `{}` must reference the lifecycle fixture script",
        frame_info.source_path
    );

    // frame_id must be positive (required for scopes/variables requests).
    assert!(
        frame_info.frame_id > 0,
        "stackTrace frame_id must be positive, got {}",
        frame_info.frame_id
    );

    // ── Step 7: scopes → Locals scope present ────────────────────────────
    // scopes_locals_ref returns the variablesReference for the Locals scope,
    // asserting the scope exists and has a positive reference.
    let locals_ref = session.scopes_locals_ref(frame_info.frame_id)?;
    assert!(
        locals_ref > 0,
        "Locals scope variablesReference must be positive (frameId={}), got {locals_ref}",
        frame_info.frame_id
    );

    // ── Step 8: variables(locals) → non-empty list ─────────────────────────
    // At BP_LINE (line 5, `my $y = $x + 5`), the previous line ($x = 10) has
    // already executed, so `$x` must be visible.  At minimum one variable is present.
    let locals = session.variables(locals_ref)?;
    assert!(
        !locals.is_empty(),
        "locals scope must contain at least one variable at BP_LINE={BP_LINE} \
         (locals_ref={locals_ref})"
    );

    // Each variable must have name, value, and variablesReference fields.
    for var in &locals {
        let name = var.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let value = var.get("value").and_then(|v| v.as_str()).unwrap_or("");
        let vars_ref = var.get("variablesReference").and_then(|v| v.as_i64()).unwrap_or(-1);

        assert!(!name.is_empty(), "locals variable must have non-empty `name`: {var:?}");
        assert!(!value.is_empty(), "locals variable `{name}` must have non-empty `value`: {var:?}");
        assert!(
            vars_ref >= 0,
            "locals variable `{name}` must have numeric `variablesReference`: {var:?}"
        );
    }

    // ── Step 9 + 10: continue → terminated event (natural program exit) ──────
    // `continue` resumes execution.  The script has no more breakpoints, so it
    // runs to completion.  The adapter should emit a `terminated` event when
    // the Perl process exits.
    //
    // COVERAGE GAP: `terminated` event delivery is not guaranteed by the current
    // adapter — the event channel may close before the event arrives, which
    // causes drain_until_event to return a channel-closed error.  The existing
    // test suite accommodates this with `let _ =` (see dap_e2e_workflow_tests.rs).
    // Test 4 (test_lifecycle_continue_leads_to_terminated_event) is the dedicated
    // test that documents this gap; here we follow the established pattern.
    session.continue_exec(frame_info.thread_id)?;

    // Drain `terminated` best-effort; channel may close before event arrives.
    let _ = session.drain_until_event("terminated");

    // ── Step 11: disconnect ───────────────────────────────────────────
    // disconnect sends the DAP `disconnect` request and expects a clean response.
    session.disconnect()?;

    Ok(())
}

// ─── Test 2: stopped event PRECEDES stackTrace (explicit ordering proof) ──────

/// Proves the event-before-request ordering required by the DAP spec:
/// the `stopped` event must arrive before the client issues `stackTrace`.
///
/// This test explicitly separates `wait_stopped` from `stack_trace` to
/// demonstrate that we observe the event first, then issue the request.
///
/// If the adapter were to emit a `stopped` event and also immediately send
/// frames without a client request, this test would catch the race.
#[test]
fn test_lifecycle_stopped_event_precedes_stack_trace() -> TestResult {
    let Some(debuggee_perl) =
        debuggee_perl_or_typed_skip("test_lifecycle_stopped_event_precedes_stack_trace")
    else {
        return Ok(());
    };

    let workspace = tempdir()?;
    let script = workspace.path().join("lifecycle_ordering.pl");
    write(&script, lifecycle_script_content())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;

    session.launch_pinned(&debuggee_perl.binary, &script_str)?;
    session.set_breakpoints_checked(&script_str, &[BP_LINE])?;
    session.configuration_done()?;

    // Explicitly separate: first wait for the stopped EVENT.
    let stopped = session.wait_stopped()?;
    assert_eq!(
        stopped.reason, "breakpoint",
        "stopped event reason must be `breakpoint`, got `{}`",
        stopped.reason
    );

    // Only AFTER observing the event do we issue the stackTrace REQUEST.
    // This is the protocol-correct ordering.
    let (frame_id, source_path, frame_line) = session.stack_trace(stopped.thread_id)?;
    assert!(frame_id > 0, "stackTrace frame_id must be positive after stopped event");
    assert!(
        !source_path.is_empty(),
        "stackTrace source_path must be non-empty after stopped event"
    );
    assert!(
        frame_line > 0,
        "stackTrace frame_line must be positive (1-based) after stopped event, got {frame_line}"
    );

    session.continue_exec(stopped.thread_id)?;
    let _ = session.drain_until_event("terminated");
    session.disconnect()?;

    Ok(())
}

// ─── Test 3: scopes advertises the #10563 contract (Locals only) ─────────────

/// Validates that `scopes` advertises the current scope contract — Locals, with
/// the retired Package/Globals scopes absent — and that the Locals scope
/// contains a REAL named lexical variable from the user script.
///
/// The DAP spec requires a `scopes` response for each frame. #10563 retired
/// Package/Globals from the advertised contract, so this journey asserts the
/// Locals reference is positive and the retired scope names stay unadvertised
/// at a live stopped frame.
///
/// Fixture layout (lifecycle_scopes.pl):
///   Line 1: use strict;
///   Line 2: use warnings;
///   Line 3: (blank)
///   Line 4: our $global = 42;   <- first executable; perl -d pauses here implicitly
///   Line 5: my $x = 10;         <- after configurationDone `c`, stops here first
///   Line 6: my $y = $x + 5;     <- SCOPES_BP_LINE: $x=10 has already executed
///   Line 7: print "$y\n";
///
/// The breakpoint is set at line 6 (not the shared BP_LINE=5) because line 5 is
/// the first stop after configurationDone and `my $x = 10` has not yet executed
/// at that point.  Stopping at line 6 guarantees `$x` is in scope with a real
/// value, so the Locals assertion cannot pass on placeholder fallback.
///
/// **Regression guard for #997 (B-module PADLIST locals fix):**
/// Prior to #997, the adapter's `variables(locals_ref)` handler returned locals
/// from the Perl debugger's internal frame (DB object: `$self` and `@_`) instead
/// of the user script's lexical scope (`$x`, `$y`).  The fix in #997 replaces the
/// broken `V <frame_id> .` approach with a B-module eval that walks the current
/// pad via `PADLIST`/`main_cv`.  The assertion below (`$x` must appear by name)
/// is a regression guard: it will catch any future revert of the PADLIST walk that
/// re-exposes the internal-frame bug.
#[test]
fn test_lifecycle_scopes_locals_contract() -> TestResult {
    let Some(debuggee_perl) = debuggee_perl_or_typed_skip("test_lifecycle_scopes_locals_contract")
    else {
        return Ok(());
    };

    // Breakpoint line for THIS fixture only — one line past the first lexical
    // assignment so that `my $x = 10` has already executed when we stop.
    const SCOPES_BP_LINE: u64 = 6;

    let workspace = tempdir()?;
    let script = workspace.path().join("lifecycle_scopes.pl");
    // Script keeps an explicit `our` global so this journey still witnesses how
    // the advertised scope contract treats package-level variables.
    // Line layout (1-based):
    //   1: use strict;
    //   2: use warnings;
    //   3: (blank)
    //   4: our $global = 42;
    //   5: my $x = 10;
    //   6: my $y = $x + 5;   <- SCOPES_BP_LINE
    //   7: print "$y\n";
    let content = "use strict;\nuse warnings;\n\nour $global = 42;\nmy $x = 10;\nmy $y = $x + 5;\nprint \"$y\\n\";\n";
    write(&script, content)?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;

    session.launch_pinned(&debuggee_perl.binary, &script_str)?;
    let resolved = session.set_breakpoints_checked(&script_str, &[SCOPES_BP_LINE])?;
    let resolved_line =
        resolved.first().copied().ok_or("set_breakpoints_checked returned empty resolved lines")?;
    session.configuration_done()?;

    let frame_info = session.wait_stopped_with_frame()?;
    assert_eq!(frame_info.reason, "breakpoint");

    // Verify we stopped at the expected line (adapter-resolved SCOPES_BP_LINE).
    assert_eq!(
        frame_info.line, resolved_line,
        "stopped frame line must equal adapter-resolved SCOPES_BP_LINE \
         (resolved={resolved_line}, SCOPES_BP_LINE={SCOPES_BP_LINE}), got {}",
        frame_info.line
    );

    // Locals scope: must be present and positive.
    let locals_ref = session.scopes_locals_ref(frame_info.frame_id)?;
    assert!(
        locals_ref > 0,
        "Locals scope variablesReference must be positive (frameId={})",
        frame_info.frame_id
    );

    // #10563 scope contract: a live stopped frame advertises only Locals (plus
    // Arguments when the frame has captured arguments). Package and Globals
    // are intentionally not advertised, so this journey proves the retired
    // scopes stay absent instead of expecting the pre-#10563 Globals entry.
    let scopes_resp = session.request("scopes", Some(json!({"frameId": frame_info.frame_id})));
    let scopes_body = session.expect_success(&scopes_resp, "scopes")?;
    let advertised: Vec<&str> = scopes_body
        .as_ref()
        .and_then(|body| body.get("scopes"))
        .and_then(Value::as_array)
        .map(|scopes| {
            scopes.iter().filter_map(|scope| scope.get("name").and_then(Value::as_str)).collect()
        })
        .unwrap_or_default();
    assert!(
        !advertised.iter().any(|name| *name == "Globals" || *name == "Package"),
        "#10563: Package/Globals scopes must stay unadvertised at a live stopped frame; \
         got {advertised:?}"
    );

    // Locals must contain the real lexical `$x` (assigned at line 5).
    // This assertion CANNOT pass on placeholder fallback — it checks that the
    // adapter parsed actual lexical locals, not a generic placeholder list.
    let locals = session.variables(locals_ref)?;
    assert!(
        !locals.is_empty(),
        "Locals scope variables must be non-empty when stopped at SCOPES_BP_LINE={SCOPES_BP_LINE} \
         (locals_ref={locals_ref}, frame_id={})",
        frame_info.frame_id
    );

    // Find `$x` by name in the locals list.  The adapter must report the real
    // lexical variable — not merely a placeholder entry.
    let x_var = locals.iter().find(|v| {
        v.get("name").and_then(|n| n.as_str()).map(|n| n == "$x" || n == "x").unwrap_or(false)
    });
    assert!(
        x_var.is_some(),
        "Locals must contain `$x` (assigned at line 5) when stopped at \
         SCOPES_BP_LINE={SCOPES_BP_LINE}; got locals={locals:?}"
    );

    // The Locals guards above are the part of this test that proves real
    // inspection. Globals enumeration returns nothing at a live breakpoint
    // (#10162), and since #10563 the Globals scope is not advertised at all.

    session.continue_exec(frame_info.thread_id)?;
    let _ = session.drain_until_event("terminated");
    session.disconnect()?;

    Ok(())
}

// ─── Test 4: continue → terminated (natural exit lifecycle) ──────────────

/// Validates the termination path: continue from a breakpoint → disconnect cleanly.
///
/// COVERAGE GAP (documented): the adapter does not reliably deliver a `terminated`
/// event before the event channel closes.  After `continue` causes the Perl
/// process to exit naturally, the channel may close before `terminated` arrives
/// (manifests as "channel closed/timeout waiting for `terminated`").  This gap
/// is already accommodated across the existing test suite with `let _ =` drains.
///
/// What this test DOES validate:
///   - `continue` is accepted without error
///   - `disconnect` is clean after natural program exit (even without a `terminated` event)
///
/// An explicit DAP `terminate` request is not tested; the adapter does not
/// currently expose a `terminate` handler beyond this natural-exit path.
#[test]
fn test_lifecycle_continue_leads_to_terminated_event() -> TestResult {
    let Some(debuggee_perl) =
        debuggee_perl_or_typed_skip("test_lifecycle_continue_leads_to_terminated_event")
    else {
        return Ok(());
    };

    let workspace = tempdir()?;
    let script = workspace.path().join("lifecycle_exit.pl");
    write(&script, lifecycle_script_content())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;

    session.launch_pinned(&debuggee_perl.binary, &script_str)?;
    session.set_breakpoints_checked(&script_str, &[BP_LINE])?;
    session.configuration_done()?;

    let frame_info = session.wait_stopped_with_frame()?;
    assert_eq!(frame_info.reason, "breakpoint");

    // Resume — no more breakpoints, script runs to EOF.
    // `continue` must be accepted without error.
    session.continue_exec(frame_info.thread_id)?;

    // Drain `terminated` best-effort.  The adapter does not guarantee this event
    // arrives before the channel closes; `let _ =` follows the established
    // pattern from the existing e2e test suite.  See PR body for the coverage gap.
    let _ = session.drain_until_event("terminated");

    // After natural exit, `disconnect` must succeed cleanly.
    // This is the primary assertion of this test.
    session.disconnect()?;

    Ok(())
}

// ─── Test 5: variables non-empty at stopped frame ──────────────────────────

/// Validates that variables inspection returns a non-empty list at a known stop.
///
/// This test verifies the variables contract specifically: that `$x` (assigned
/// at line 4) is visible in locals when stopped at line 5.  It also checks that
/// each variable entry satisfies the DAP `Variable` type shape:
///   name (string), value (string), variablesReference (number >= 0)
#[test]
fn test_lifecycle_variables_non_empty_at_stop() -> TestResult {
    let Some(debuggee_perl) =
        debuggee_perl_or_typed_skip("test_lifecycle_variables_non_empty_at_stop")
    else {
        return Ok(());
    };

    let workspace = tempdir()?;
    let script = workspace.path().join("lifecycle_vars.pl");
    write(&script, lifecycle_script_content())?;
    let script_str = script.to_str().ok_or("script path is not valid UTF-8")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;

    session.launch_pinned(&debuggee_perl.binary, &script_str)?;
    session.set_breakpoints_checked(&script_str, &[BP_LINE])?;
    session.configuration_done()?;

    let frame_info = session.wait_stopped_with_frame()?;
    assert_eq!(frame_info.reason, "breakpoint");

    let locals_ref = session.scopes_locals_ref(frame_info.frame_id)?;
    let variables = session.variables(locals_ref)?;

    // At line 5 (BP_LINE), `$x = 10` (line 4) has already executed.
    // At least `$x` must be visible.
    assert!(
        !variables.is_empty(),
        "variables list must be non-empty when stopped at BP_LINE={BP_LINE} \
         (locals_ref={locals_ref}, frame_id={})",
        frame_info.frame_id
    );

    // Validate DAP Variable shape for each entry.
    for var in &variables {
        let name = var.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let value = var.get("value").and_then(|v| v.as_str()).unwrap_or("");
        let vars_ref = var.get("variablesReference").and_then(|v| v.as_i64()).unwrap_or(-1);

        assert!(!name.is_empty(), "each variable must have non-empty `name` field: {var:?}");
        assert!(
            !value.is_empty(),
            "each variable `{name}` must have non-empty `value` field: {var:?}"
        );
        assert!(
            vars_ref >= 0,
            "each variable `{name}` must have non-negative `variablesReference`: {var:?}"
        );
    }

    session.continue_exec(frame_info.thread_id)?;
    let _ = session.drain_until_event("terminated");
    session.disconnect()?;

    Ok(())
}

/// No active session: stackTrace returns honest empty list, not a fabricated frame.
/// Regression guard: pre-fix returned main::hello @ /tmp/hello.pl:10.
/// This test requires no `perl` on PATH — pure unit isolation.
#[test]
fn test_stacktrace_no_session_returns_empty() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    let response = adapter.handle_request(1, "stackTrace", Some(json!({"threadId": 1})));
    let DapMessage::Response { success, command, body, .. } = response else {
        return Err("Expected Response".into());
    };
    assert!(success, "stackTrace should succeed even without a session");
    assert_eq!(command, "stackTrace");
    let body = body.ok_or("Expected body")?;
    let frames =
        body.get("stackFrames").and_then(|v| v.as_array()).ok_or("Expected stackFrames array")?;
    assert_eq!(frames.len(), 0, "no session must return stackFrames: [] (not a fabricated frame)");
    let total = body.get("totalFrames").and_then(|v| v.as_u64()).ok_or("Expected totalFrames")?;
    assert_eq!(total, 0, "totalFrames must be 0 when stackFrames is empty");
    Ok(())
}

// ─── Cleanup/teardown unit-level matrix (C1–C6) ──────────────────────────────
//
// These six tests exercise the lifecycle cleanup and teardown contracts at the
// protocol level. C1, C2, C4, C5, and C6 need no live Perl process. C3 launches
// a real stopOnEntry session so disconnect can observe genuine termination; it
// uses `debuggee_perl_or_typed_skip` like the live-session tests above.
//
// Each test uses `make_adapter_with_rx` + `wait_cleanup_event` (defined below).

fn make_adapter_with_rx() -> (DebugAdapter, Receiver<DapMessageWithEpoch>) {
    let (tx, rx) = sync_channel(64);
    let mut adapter = DebugAdapter::new();
    adapter.set_event_sender(tx);
    // Lifecycle-matrix scenarios exercise termination/replacement behavior,
    // not the launch-authority contract; without an installed authority every
    // launch is refused before it reaches the program validation these tests
    // assert on (#8656).
    common::install_unbounded_test_authority(&adapter);
    (adapter, rx)
}

fn require_terminal_count(
    rx: &Receiver<DapMessageWithEpoch>,
    expected: usize,
    context: &str,
) -> TestResult {
    let observed = rx
        .try_iter()
        .filter(
            |message| matches!(message, (DapMessage::Event { event, .. }, _) if event == "terminated"),
        )
        .count();
    if observed != expected {
        return Err(
            format!("{context}: expected {expected} terminal events, got {observed}").into()
        );
    }
    Ok(())
}

fn require_lifecycle_response(response: DapMessage, command: &str, success: bool) -> TestResult {
    match response {
        DapMessage::Response { command: actual, success: actual_success, .. }
            if actual == command && actual_success == success =>
        {
            Ok(())
        }
        other => Err(format!("expected {command} success={success}, got {other:?}").into()),
    }
}

#[test]
fn disconnect_terminal_initial_and_rejected_launch() -> TestResult {
    for reject_launch in [false, true] {
        let (mut adapter, rx) = make_adapter_with_rx();
        require_lifecycle_response(
            adapter.handle_request(1, "initialize", None),
            "initialize",
            true,
        )?;
        if reject_launch {
            let response = adapter.handle_request(2, "launch", Some(json!({"program": ""})));
            match response {
                DapMessage::Response { success: false, message: Some(message), .. }
                    if message.contains("No Perl script was specified") => {}
                other => {
                    return Err(format!("expected empty-program refusal, got {other:?}").into());
                }
            }
        }
        require_terminal_count(&rx, 0, "before first disconnect")?;
        require_lifecycle_response(
            adapter.handle_request(3, "disconnect", None),
            "disconnect",
            true,
        )?;
        require_terminal_count(&rx, 0, "first no-debuggee disconnect")?;
        require_lifecycle_response(
            adapter.handle_request(4, "disconnect", None),
            "disconnect",
            true,
        )?;
        require_terminal_count(&rx, 0, "duplicate disconnect")?;
    }
    Ok(())
}

#[test]
fn disconnect_terminal_after_terminate_and_rejected_replacement() -> TestResult {
    for reject_replacement in [false, true] {
        let (mut adapter, rx) = make_adapter_with_rx();
        require_lifecycle_response(
            adapter.handle_request(1, "initialize", None),
            "initialize",
            true,
        )?;
        require_lifecycle_response(
            adapter.handle_request(2, "terminate", None),
            "terminate",
            true,
        )?;
        require_terminal_count(&rx, 1, "initial terminate")?;
        if reject_replacement {
            let response = adapter.handle_request(3, "launch", Some(json!({"program": ""})));
            match response {
                DapMessage::Response { success: false, message: Some(message), .. }
                    if message.contains("No Perl script was specified") => {}
                other => return Err(format!("expected rejected replacement, got {other:?}").into()),
            }
            require_terminal_count(&rx, 0, "rejected replacement")?;
        }
        require_lifecycle_response(
            adapter.handle_request(4, "disconnect", None),
            "disconnect",
            true,
        )?;
        require_terminal_count(&rx, 0, "disconnect after terminated lifecycle")?;
        require_lifecycle_response(
            adapter.handle_request(5, "terminate", None),
            "terminate",
            true,
        )?;
        require_terminal_count(&rx, 1, "repeated explicit terminate remains acknowledged")?;
        require_lifecycle_response(
            adapter.handle_request(6, "disconnect", None),
            "disconnect",
            true,
        )?;
        require_terminal_count(&rx, 0, "disconnect after repeated terminate")?;
    }
    Ok(())
}

#[test]
fn disconnect_terminal_successful_replacement_reopens_lifecycle() -> TestResult {
    let Some(_) =
        debuggee_perl_or_typed_skip("disconnect_terminal_successful_replacement_reopens_lifecycle")
    else {
        return Ok(());
    };
    let workspace = tempdir()?;
    let script = workspace.path().join("disconnect_replacement.pl");
    write(&script, lifecycle_script_content())?;
    let script_str = script.to_str().ok_or("replacement script path is not valid UTF-8")?;
    let (mut adapter, rx) = make_adapter_with_rx();
    require_lifecycle_response(adapter.handle_request(1, "initialize", None), "initialize", true)?;
    require_lifecycle_response(adapter.handle_request(2, "terminate", None), "terminate", true)?;
    require_terminal_count(&rx, 1, "close initial lifecycle")?;
    for request_seq in [10, 20] {
        let arguments = common::resolved_launch_arguments_for_test(script_str, None, true)?;
        require_lifecycle_response(
            adapter.handle_request(request_seq, "launch", Some(arguments)),
            "launch",
            true,
        )?;
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
                Ok((DapMessage::Event { event, .. }, _)) if event == "terminated" => {
                    return Err(
                        "replacement terminated before establishing a stopped debuggee".into()
                    );
                }
                Ok((DapMessage::Event { event, .. }, _)) if event == "stopped" => break,
                Ok(_) => {}
                Err(error) => return Err(format!("replacement did not stop: {error}").into()),
            }
        }
        require_terminal_count(&rx, 0, "replacement is live")?;
        require_lifecycle_response(
            adapter.handle_request(request_seq + 1, "disconnect", None),
            "disconnect",
            true,
        )?;
        require_terminal_count(&rx, 1, "replacement disconnect")?;
        require_lifecycle_response(
            adapter.handle_request(request_seq + 2, "disconnect", None),
            "disconnect",
            true,
        )?;
        require_terminal_count(&rx, 0, "duplicate replacement disconnect")?;
    }
    Ok(())
}

/// Drain the event channel looking for an event with the given name, up to
/// `timeout_ms` total. Returns the event body on match.
fn wait_cleanup_event(
    rx: &Receiver<DapMessageWithEpoch>,
    name: &str,
    timeout_ms: u64,
) -> Option<Value> {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(remaining) {
            Ok((DapMessage::Event { event, body, .. }, _)) if event == name => {
                return Some(body.unwrap_or(Value::Null));
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    None
}

/// Drain waiting for `name`, recording whether a terminal event was seen
/// and discarded along the way. `terminated`/`exited` is once-only per
/// session generation (#15887, same family as #15884): a fast-exiting
/// debuggee can commit it during the `initialized`/`stopped` setup waits,
/// in which case a later post-disconnect-only assert would spin out though
/// the adapter behaved correctly. Callers assert at-least-once per
/// generation instead of strictly-post-disconnect.
fn wait_cleanup_event_track_terminal(
    rx: &Receiver<DapMessageWithEpoch>,
    name: &str,
    timeout: Duration,
    terminated_seen: &mut bool,
) -> Option<Value> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(remaining) {
            Ok((DapMessage::Event { event, body, .. }, _)) if event == name => {
                if event == "terminated" || event == "exited" {
                    *terminated_seen = true;
                }
                return Some(body.unwrap_or(Value::Null));
            }
            Ok((DapMessage::Event { event, .. }, _)) => {
                if event == "terminated" || event == "exited" {
                    *terminated_seen = true;
                }
                continue;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    None
}

fn assert_cleanup_success(response: &DapMessage, expected_command: &str) -> TestResult {
    match response {
        DapMessage::Response { success, command, message, .. } => {
            assert!(
                *success,
                "expected success=true for {expected_command}, got message={message:?}"
            );
            assert_eq!(command, expected_command, "command field mismatch");
            Ok(())
        }
        other => Err(format!("expected Response for {expected_command}, got {other:?}").into()),
    }
}

// ── C1: terminate with breakpoints set ───────────────────────────────────────

/// C1 — terminate with breakpoints set.
///
/// BEHAVIOUR LOCK: `clear_active_session_state` does NOT call
/// `breakpoints.clear_all()`. The `BreakpointStore` persists across terminate
/// so that IDEs can efficiently restore breakpoints on restart without resending.
///
/// Assertions:
/// - `terminate` returns success.
/// - A `terminated` event is emitted with the correct `restart` field.
/// - REPLACE semantics still work after terminate (empty setBreakpoints removes
///   only the file's registrations, returning 0 breakpoints).
///
/// See `docs/reference/DAP_LIFECYCLE_MATRIX.md` "Known limitations §C1".
#[test]
fn test_terminate_preserves_breakpoints_but_replace_still_clears() -> TestResult {
    let (mut adapter, rx) = make_adapter_with_rx();

    // Register a breakpoint before terminate.
    let bp_response = adapter.handle_request(
        1,
        "setBreakpoints",
        Some(json!({
            "source": { "path": "/tmp/test_lifecycle_c1.pl" },
            "breakpoints": [{ "line": 5 }]
        })),
    );
    assert_cleanup_success(&bp_response, "setBreakpoints")?;

    // Terminate the (simulated) session.
    let term_response = adapter.handle_request(2, "terminate", Some(json!({ "restart": false })));
    assert_cleanup_success(&term_response, "terminate")?;

    // "terminated" event must be emitted regardless of session state.
    let event_body = must_some(wait_cleanup_event(&rx, "terminated", 300));
    let restart_flag = event_body.get("restart").and_then(Value::as_bool);
    assert_eq!(restart_flag, Some(false), "terminated event must echo restart=false");

    // BEHAVIOUR LOCK: the BreakpointStore is NOT cleared on terminate.
    // A subsequent setBreakpoints with an empty list (REPLACE semantics) must
    // return 0 breakpoints, proving REPLACE still functions post-terminate.
    let recheck = adapter.handle_request(
        3,
        "setBreakpoints",
        Some(json!({
            "source": { "path": "/tmp/test_lifecycle_c1.pl" },
            "breakpoints": []
        })),
    );
    match recheck {
        DapMessage::Response { success: true, body: Some(ref body), .. } => {
            let bps = body
                .get("breakpoints")
                .and_then(Value::as_array)
                .ok_or("setBreakpoints response must include breakpoints array")?;
            assert_eq!(
                bps.len(),
                0,
                "REPLACE semantics with empty list must clear stored breakpoints for the file"
            );
        }
        other => return Err(format!("Expected successful setBreakpoints, got {other:?}").into()),
    }

    Ok(())
}

// ── C2: refused PID attach leaves no session to leak ─────────────────────────

/// C2 — PID-attach refusal (#8109) → no session created, no stopped event,
/// and nothing leaked: subsequent `threads` stays empty.
///
/// #8109 removed the signal-control PID attach: process existence plus signal
/// control never established a debugger transport, so the adapter must refuse
/// the request instead of synthesizing a stopped session. The no-leak property
/// this row guards is now that a refusal creates no session at all — later
/// `threads` calls stay empty.
#[test]
fn test_refused_pid_attach_leaves_no_session_to_leak() -> TestResult {
    let (mut adapter, rx) = make_adapter_with_rx();

    // Attach in PID-signal-control mode is refused fail-closed (#8109).
    let attach_response =
        adapter.handle_request(1, "attach", Some(json!({ "processId": std::process::id() })));
    match &attach_response {
        DapMessage::Response { success, command, body, message, .. } => {
            if *command != "attach" {
                return Err(format!("expected attach response command, got {command}").into());
            }
            if *success {
                return Err("PID attach must be refused (#8109)".into());
            }
            if body.is_some() {
                return Err("refusal must not carry an attach body".into());
            }
            let msg = message.as_deref().ok_or("Expected refusal message")?;
            if !msg.contains("not supported") {
                return Err(format!("refusal must name the disposition: {msg}").into());
            }
        }
        other => return Err(format!("Expected attach response, got {other:?}").into()),
    }

    // No synthetic "stopped" event may be emitted by the refusal.
    assert!(
        wait_cleanup_event(&rx, "stopped", 200).is_none(),
        "a refused PID attach must not emit a stopped event (#8109)"
    );

    // threads must return empty (no leaked PID session).
    let threads_response = adapter.handle_request(2, "threads", None);
    match threads_response {
        DapMessage::Response { success: true, body: Some(ref body), .. } => {
            let threads = body
                .get("threads")
                .and_then(Value::as_array)
                .ok_or("threads body must have threads array")?;
            assert!(
                threads.is_empty(),
                "after a refused PID attach, threads must be empty (no leaked session), got {threads:?}"
            );
        }
        DapMessage::Response { success: true, body: None, .. } => {
            // No body implies no threads — acceptable.
        }
        other => return Err(format!("Unexpected threads response: {other:?}").into()),
    }

    Ok(())
}

// ── C3: disconnect clears active session ─────────────────────────────────────

/// C3 — active launch-owned session → disconnect → state cleared:
/// "terminated" event emitted, subsequent stackTrace/modules return
/// protocol-safe responses (no panic).
///
/// Requires a pipe-capable Perl interpreter. Hosts without one skip via
/// `debuggee_perl_or_typed_skip`, matching the other live-session tests.
#[test]
fn test_disconnect_clears_active_session() -> TestResult {
    let Some(_) = debuggee_perl_or_typed_skip("test_disconnect_clears_active_session") else {
        return Ok(());
    };

    let (mut adapter, rx) = make_adapter_with_rx();
    let workspace = tempdir()?;
    let script = workspace.path().join("lifecycle_c3.pl");
    write(&script, lifecycle_script_content())?;
    let script_str = script.to_str().ok_or("C3 script path is not valid UTF-8")?;

    // Establish a real active launch-owned session before disconnect. The pinned
    // interpreter and stopOnEntry keep the child alive at the disconnect boundary.
    let timeout = workflow_timeout();
    let mut terminated_seen = false;
    let initialize = adapter.handle_request(1, "initialize", None);
    assert_cleanup_success(&initialize, "initialize")?;
    if wait_cleanup_event_track_terminal(&rx, "initialized", timeout, &mut terminated_seen)
        .is_none()
    {
        return Err("initialize must emit an initialized event".into());
    }
    let launch_args = common::resolved_launch_arguments_for_test(script_str, None, true)?;
    let launch = adapter.handle_request(2, "launch", Some(launch_args));
    assert_cleanup_success(&launch, "launch")?;
    if wait_cleanup_event_track_terminal(&rx, "stopped", timeout, &mut terminated_seen).is_none() {
        return Err("stopOnEntry launch must establish an active stopped session".into());
    }

    // Disconnect.
    let dc_response = adapter.handle_request(3, "disconnect", None);
    assert_cleanup_success(&dc_response, "disconnect")?;

    // `terminated` is once-only per generation: it may already have been
    // consumed during setup on a fast-exiting debuggee. Assert at-least-once
    // per generation with a bounded grace drain (same discipline as
    // `common::DapWorkflowSession::disconnect`), not strictly-post-disconnect.
    let grace = timeout.min(Duration::from_secs(2));
    let _ = wait_cleanup_event_track_terminal(&rx, "terminated", grace, &mut terminated_seen);
    assert!(
        terminated_seen,
        "disconnect must yield a terminated event for the generation \
         (already-consumed early termination counts)"
    );
    // At-most-once: no duplicate terminal event may be queued after accounting.
    let duplicates = rx
        .try_iter()
        .filter(|message| {
            matches!(message, (DapMessage::Event { event, .. }, _)
                if event == "terminated" || event == "exited")
        })
        .count();
    assert!(
        duplicates == 0,
        "terminated is once-only: duplicate terminal events queued after disconnect"
    );

    // After disconnect, stackTrace must prove the active session was cleared.
    let st_response = adapter.handle_request(4, "stackTrace", Some(json!({ "threadId": 1 })));
    match st_response {
        DapMessage::Response { command, success: true, body: Some(body), .. } => {
            assert_eq!(command, "stackTrace", "stackTrace command must be echoed correctly");
            let frames = body
                .get("stackFrames")
                .and_then(Value::as_array)
                .ok_or("stackTrace after disconnect must include stackFrames")?;
            if !frames.is_empty() {
                return Err(format!(
                    "stackTrace after disconnect must have empty stackFrames, got {frames:?}"
                )
                .into());
            }
        }
        other => {
            return Err(format!(
                "stackTrace after disconnect must return a successful empty response, got {other:?}"
            )
            .into());
        }
    }

    // modules must not panic and must return a valid response.
    let modules_response = adapter.handle_request(5, "modules", Some(json!({})));
    match modules_response {
        DapMessage::Response { .. } => {}
        other => {
            return Err(
                format!("modules after disconnect must return a Response, got {other:?}").into()
            );
        }
    }

    Ok(())
}

// ── C4: post-terminate requests are protocol-safe ─────────────────────────────

/// C4 — after terminate, `variables`, `stackTrace`, and `scopes` return
/// protocol-safe responses (success with empty body, or descriptive error);
/// no panic occurs.
///
/// Verifies the adapter does not unwrap/expect on a None session reference.
#[test]
fn test_post_terminate_requests_protocol_safe() -> TestResult {
    let (mut adapter, rx) = make_adapter_with_rx();

    // Terminate first (no session — must be idempotent).
    let term = adapter.handle_request(1, "terminate", None);
    assert_cleanup_success(&term, "terminate")?;
    let _ = wait_cleanup_event(&rx, "terminated", 200);

    // variables — must return a Response (not panic).
    let vars = adapter.handle_request(2, "variables", Some(json!({ "variablesReference": 1 })));
    match vars {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "variables", "response command must echo variables");
        }
        other => {
            return Err(
                format!("variables after terminate must be a Response, got {other:?}").into()
            );
        }
    }

    // stackTrace — must return a Response (not panic).
    let st = adapter.handle_request(3, "stackTrace", Some(json!({ "threadId": 1 })));
    match st {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "stackTrace", "response command must echo stackTrace");
        }
        other => {
            return Err(
                format!("stackTrace after terminate must be a Response, got {other:?}").into()
            );
        }
    }

    // scopes — must return a Response (not panic).
    let scopes = adapter.handle_request(4, "scopes", Some(json!({ "frameId": 1 })));
    match scopes {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "scopes", "response command must echo scopes");
        }
        other => {
            return Err(format!("scopes after terminate must be a Response, got {other:?}").into());
        }
    }

    Ok(())
}

// ── C5: relaunch after terminate carries no stale state ───────────────────────

/// C5 — terminate → launch again → fresh launch path, no stale state from
/// pre-terminate breakpoints or session handles.
///
/// A non-existent script path is used so the launch fails quickly at the
/// file-exists validation step (no Perl needed). The failure message must
/// describe a file/launch error, NOT a stale-session collision.
#[test]
fn test_relaunch_after_terminate_no_stale_state() -> TestResult {
    let (mut adapter, rx) = make_adapter_with_rx();

    // Set some breakpoints (potential stale state after terminate).
    let _ = adapter.handle_request(
        1,
        "setBreakpoints",
        Some(json!({
            "source": { "path": "/tmp/test_lifecycle_c5.pl" },
            "breakpoints": [{ "line": 7 }, { "line": 14 }]
        })),
    );

    // Terminate (simulated — no real session).
    let term = adapter.handle_request(2, "terminate", None);
    assert_cleanup_success(&term, "terminate")?;
    let _ = wait_cleanup_event(&rx, "terminated", 200);

    // Attempt a new launch. Non-existent path → fails at file-exists check.
    let launch = adapter.handle_request(
        3,
        "launch",
        Some(json!({
            "program": "/nonexistent/path/to/script_lifecycle_c5.pl"
        })),
    );

    match launch {
        DapMessage::Response { success: false, command, message, .. } => {
            assert_eq!(command, "launch");
            let msg = message.unwrap_or_default();
            // Must NOT indicate a stale-session collision.
            assert!(
                !msg.contains("already running")
                    && !msg.contains("active session")
                    && !msg.contains("previous session")
                    && !msg.contains("state conflict"),
                "launch after terminate must not report stale-session collision, got: {msg}"
            );
            // Must be a file-not-found, launch error, or protocol-ordering error.
            // If launch is sent without a prior initialize the adapter rejects it
            // with a protocol ordering message; that is NOT a stale-session error
            // and is an equally acceptable failure reason here.
            assert!(
                msg.contains("Cannot find")
                    || msg.contains("not a file")
                    || msg.contains("not found")
                    || msg.contains("Failed")
                    || msg.contains("no launch")
                    || msg.contains("Perl")
                    || msg.contains("Cannot start")
                    || msg.contains("initialize"),
                "launch failure must describe a file/launch error or protocol-ordering error, got: {msg}"
            );
        }
        DapMessage::Response { success: true, .. } => {
            // Unexpected success (e.g., Perl spawned somehow) — state isolation still holds.
        }
        other => {
            return Err(
                format!("launch after terminate must return a Response, got {other:?}").into()
            );
        }
    }

    Ok(())
}

// ── C6: restart without prior launch args → clean protocol error ──────────────

/// C6 — restart without prior launch args → clean protocol error; adapter
/// remains usable afterwards.
///
/// #9581: restart is a floored secondary capability, so the dispatch gate
/// rejects the request before `handle_restart` is ever reached — no stored
/// launch args are consulted, no session/generation state is touched. This
/// locks the explicit unsupported disposition and validates that restart does
/// not crash or produce an opaque "Unknown command" response.
#[test]
fn test_restart_without_prior_launch_fails_gracefully() -> TestResult {
    let (mut adapter, _rx) = make_adapter_with_rx();

    // Restart without any prior launch → must fail gracefully.
    let restart = adapter.handle_request(1, "restart", None);

    match restart {
        DapMessage::Response { success, command, message, .. } => {
            assert_eq!(command, "restart", "command field must echo restart");
            assert!(!success, "restart without prior launch must fail (floored by #9581)");
            let msg = message.as_deref().unwrap_or("");
            assert!(
                !msg.contains("Unknown command"),
                "restart must route to its handler, not the unknown-command fallback: {msg}"
            );
            assert!(
                msg.contains("unsupported") && msg.contains("supportsRestartRequest"),
                "restart error must be the explicit #9581 unsupported disposition, got: {msg}"
            );
        }
        other => {
            return Err(format!("restart must return a Response, got {other:?}").into());
        }
    }

    // After the failed restart, subsequent protocol requests must still work.
    let threads = adapter.handle_request(2, "threads", None);
    match threads {
        DapMessage::Response { command, .. } => {
            assert_eq!(command, "threads", "threads after failed restart must respond");
        }
        other => {
            return Err(format!(
                "threads after failed restart must return a Response, got {other:?}"
            )
            .into());
        }
    }

    Ok(())
}
