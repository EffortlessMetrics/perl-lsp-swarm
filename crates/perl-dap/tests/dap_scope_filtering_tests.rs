//! Content-level tests for DAP scope admission and filtering.
//!
//! Perl-requiring tests skip silently (return Ok(())) rather than printing to
//! stderr, keeping this file clean under `clippy::print_stderr`.
//!
//! # What is tested
//!
//! The current contract admits scopes only for the exact current stopped frame.
//! That response contains Locals and, when captured, Arguments; Package and
//! Globals are intentionally omitted. Stale, unknown, running, terminated,
//! cleared-session, and no-session references are honest empty responses.
//!
//! | Scope     | admission                         | contract                         |
//! |-----------|-----------------------------------|----------------------------------|
//! | Locals    | exact current stopped frame      | captured lexical values          |
//! | Arguments | exact current stopped frame      | actually captured arguments only |
//! | Package   | never admitted by this contract  | omitted                           |
//! | Globals   | never admitted by this contract  | omitted                           |
//!
//! ## Test layers
//!
//! 1. **Protocol-shape tests** (no Perl required): codec arithmetic remains
//!    covered; handler admission and captured-value behavior are covered by
//!    focused perl-dap unit tests.
//!
//! 2. **Rejection tests** (no Perl required): unadmitted references return empty
//!    without querying the debugger or parsing unrelated history.
//!
//! 3. **Scope-filter unit tests** live beside the adapter implementation, where
//!    framed debugger output and query counters can be controlled directly.
//!
//! 4. **E2E content-level tests** (Perl required, skipped gracefully otherwise):
//!    retained cases document the current-frame contract; legacy multi-scope
//!    cases remain explicitly retired below.
//!
//! # Limitations
//!
//! Legacy live multi-scope tests remain ignored because Package/Globals and
//! arbitrary-frame admission are no longer part of this contract.

mod common;

use common::{DapWorkflowSession, perl_available, workflow_timeout};
use perl_dap::{DapMessage, DebugAdapter};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs::write;
use std::sync::mpsc::sync_channel;
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn wait_stopped_reports_termination_and_bounded_output_metadata() -> TestResult {
    let (sender, receiver) = sync_channel(64);
    for seq in 1..=10 {
        sender.send(DapMessage::Event {
            seq,
            event: "output".to_string(),
            body: Some(json!({
                "category": format!("category-{seq}-{}\nsecret", "x".repeat(80)),
                "output": "sensitive debugger output"
            })),
        })?;
    }
    sender.send(DapMessage::Event {
        seq: 11,
        event: "terminated".to_string(),
        body: Some(json!({"reason": "debugger_eof"})),
    })?;

    let session =
        DapWorkflowSession::with_receiver_for_test(receiver, std::time::Duration::from_secs(1));
    let error = session.wait_stopped().err().ok_or("wait_stopped unexpectedly succeeded")?;
    for required in [
        "adapter terminated (debugger_eof)",
        "output(category=category-10-",
        "bytes=25)",
        "terminated(reason=debugger_eof)",
    ] {
        if !error.contains(required) {
            return Err(format!("diagnostic missing {required:?}: {error}").into());
        }
    }
    if error.contains("sensitive debugger output") {
        return Err("diagnostic exposed debugger output content".into());
    }
    if error.contains("category-3-") || error.contains('\n') || error.len() > 1_000 {
        return Err(format!("diagnostic was not bounded and flattened: {error:?}").into());
    }
    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Extract the `variablesReference` value for a named scope from a `scopes`
/// response body.
fn scope_ref_by_name(body: &Value, name: &str) -> Option<i64> {
    body.get("scopes")
        .and_then(Value::as_array)?
        .iter()
        .find(|scope| scope.get("name").and_then(Value::as_str) == Some(name))
        .and_then(|scope| scope.get("variablesReference").and_then(Value::as_i64))
}

/// Collect all `name` field values from a `variables` response body.
fn var_names(body: &Value) -> HashSet<String> {
    body.get("variables")
        .and_then(Value::as_array)
        .map(|vars| {
            vars.iter()
                .filter_map(|v| v.get("name").and_then(Value::as_str))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Send a `variables` request to `adapter` and return the response body.
fn request_variables(adapter: &mut DebugAdapter, variables_reference: i64) -> Option<Value> {
    let resp = adapter.handle_request(
        1,
        "variables",
        Some(json!({ "variablesReference": variables_reference })),
    );
    match resp {
        DapMessage::Response { success: true, body, .. } => body,
        _ => None,
    }
}

// ── 1. Protocol shape and exact current-frame admission ─────────────────────

/// Legacy three-bucket response shape; retained only as a named historical
/// regression because handler admission now requires a stopped frame.
#[test]
#[ignore = "#10563: Retired: no-session handle_scopes cannot synthesize the old three-bucket response; admitted-frame coverage is in perl-dap unit tests."]
fn test_scopes_response_contains_three_named_buckets() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let resp = adapter.handle_request(1, "scopes", Some(json!({ "frameId": 1 })));
    let body = match resp {
        DapMessage::Response { success: true, body: Some(b), .. } => b,
        _ => return Err("scopes request failed or had no body".into()),
    };

    let scopes = body.get("scopes").and_then(Value::as_array).ok_or("missing scopes array")?;
    assert_eq!(scopes.len(), 3, "expected exactly 3 scopes (Locals/Package/Globals)");

    let names: Vec<&str> =
        scopes.iter().filter_map(|s| s.get("name").and_then(Value::as_str)).collect();
    assert!(names.contains(&"Locals"), "expected Locals scope, got: {names:?}");
    assert!(names.contains(&"Package"), "expected Package scope, got: {names:?}");
    assert!(names.contains(&"Globals"), "expected Globals scope, got: {names:?}");

    Ok(())
}

/// The Locals scope must carry presentationHint = "locals".
#[test]
#[ignore = "#10563: Retired: no-session handle_scopes cannot synthesize a Locals scope; admitted-frame coverage is in perl-dap unit tests."]
fn test_locals_scope_has_correct_presentation_hint() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let resp = adapter.handle_request(1, "scopes", Some(json!({ "frameId": 1 })));
    let body = match resp {
        DapMessage::Response { success: true, body: Some(b), .. } => b,
        _ => return Err("scopes request failed".into()),
    };

    let scopes = body.get("scopes").and_then(Value::as_array).ok_or("missing scopes array")?;
    let locals_scope = scopes
        .iter()
        .find(|s| s.get("name").and_then(Value::as_str) == Some("Locals"))
        .ok_or("Locals scope not found")?;

    let hint = locals_scope.get("presentationHint").and_then(Value::as_str).unwrap_or("");
    assert_eq!(hint, "locals", "Locals scope presentationHint must be 'locals', got '{hint}'");

    Ok(())
}

/// The Locals scope variablesReference must equal `frame_id * 10 + 1`.
/// This arithmetic is load-bearing: variables requests use it to identify
/// which scope type to query.
#[test]
#[ignore = "#10563: Retired: arbitrary frame ids are not admitted without an exact stopped frame."]
fn test_scope_reference_arithmetic_locals() -> TestResult {
    let frame_id: i64 = 3;
    let expected_locals_ref = frame_id * 10 + 1;

    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let resp = adapter.handle_request(1, "scopes", Some(json!({ "frameId": frame_id })));
    let body = match resp {
        DapMessage::Response { success: true, body: Some(b), .. } => b,
        _ => return Err("scopes request failed".into()),
    };

    let locals_ref = scope_ref_by_name(&body, "Locals").ok_or("Locals scope not found")?;
    assert_eq!(
        locals_ref, expected_locals_ref,
        "Locals ref for frame {frame_id} must be {expected_locals_ref}, got {locals_ref}"
    );

    Ok(())
}

/// The Package scope variablesReference must equal `frame_id * 10 + 2`.
#[test]
#[ignore = "#10563: Retired: Package is intentionally not advertised by the current-frame scope contract."]
fn test_scope_reference_arithmetic_package() -> TestResult {
    let frame_id: i64 = 3;
    let expected_package_ref = frame_id * 10 + 2;

    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let resp = adapter.handle_request(1, "scopes", Some(json!({ "frameId": frame_id })));
    let body = match resp {
        DapMessage::Response { success: true, body: Some(b), .. } => b,
        _ => return Err("scopes request failed".into()),
    };

    let package_ref = scope_ref_by_name(&body, "Package").ok_or("Package scope not found")?;
    assert_eq!(
        package_ref, expected_package_ref,
        "Package ref for frame {frame_id} must be {expected_package_ref}, got {package_ref}"
    );

    Ok(())
}

/// The Globals scope variablesReference must equal `frame_id * 10 + 3`.
#[test]
#[ignore = "#10563: Retired: Globals is intentionally not advertised by the current-frame scope contract."]
fn test_scope_reference_arithmetic_globals() -> TestResult {
    let frame_id: i64 = 3;
    let expected_globals_ref = frame_id * 10 + 3;

    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let resp = adapter.handle_request(1, "scopes", Some(json!({ "frameId": frame_id })));
    let body = match resp {
        DapMessage::Response { success: true, body: Some(b), .. } => b,
        _ => return Err("scopes request failed".into()),
    };

    let globals_ref = scope_ref_by_name(&body, "Globals").ok_or("Globals scope not found")?;
    assert_eq!(
        globals_ref, expected_globals_ref,
        "Globals ref for frame {frame_id} must be {expected_globals_ref}, got {globals_ref}"
    );

    Ok(())
}

/// All three scope variablesReferences must be distinct (no aliasing).
#[test]
#[ignore = "#10563: Retired: the old three-scope response is not produced for arbitrary frames."]
fn test_scope_references_are_distinct() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let resp = adapter.handle_request(1, "scopes", Some(json!({ "frameId": 2 })));
    let body = match resp {
        DapMessage::Response { success: true, body: Some(b), .. } => b,
        _ => return Err("scopes request failed".into()),
    };

    let scopes = body.get("scopes").and_then(Value::as_array).ok_or("missing scopes array")?;
    let refs: Vec<i64> =
        scopes.iter().filter_map(|s| s.get("variablesReference").and_then(Value::as_i64)).collect();

    assert_eq!(refs.len(), 3, "expected 3 scope references");
    let unique: HashSet<i64> = refs.iter().copied().collect();
    assert_eq!(unique.len(), 3, "scope variablesReferences must all be distinct: {refs:?}");

    Ok(())
}

/// Scope references must all be positive integers (non-zero per DAP spec).
#[test]
#[ignore = "#10563: Retired: no-session scopes are intentionally empty, so this assertion is vacuous; codec positivity is covered by var_ref tests."]
fn test_scope_references_are_positive() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let resp = adapter.handle_request(1, "scopes", Some(json!({ "frameId": 1 })));
    let body = match resp {
        DapMessage::Response { success: true, body: Some(b), .. } => b,
        _ => return Err("scopes request failed".into()),
    };

    let scopes = body.get("scopes").and_then(Value::as_array).ok_or("missing scopes array")?;
    for scope in scopes {
        let name = scope.get("name").and_then(Value::as_str).unwrap_or("<unknown>");
        let vars_ref = scope
            .get("variablesReference")
            .and_then(Value::as_i64)
            .ok_or(format!("scope '{name}' missing variablesReference"))?;
        assert!(vars_ref > 0, "scope '{name}' variablesReference must be > 0, got {vars_ref}");
    }

    Ok(())
}

// ── 2. Legacy fallback cases (retired) ───────────────────────────────────────

/// Historical fallback assertion retained only to document the retired shape.
#[test]
#[ignore = "#10563: Retired: reference 11 is an unadmitted no-session scope; zero-query rejection is covered by focused variables unit tests."]
fn test_fallback_locals_scope_contains_no_package_or_global_names() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    // frame_id=1 → locals_ref = 11
    let locals_ref: i64 = 11;
    let body = request_variables(&mut adapter, locals_ref)
        .ok_or("locals variables request returned no body")?;

    let names = var_names(&body);

    // Locals fallback must be empty — returning fake placeholders would be misleading.
    assert!(
        names.is_empty(),
        "#1006 regression: Locals fallback must return empty when B module is unavailable; \
         got variables: {names:?}"
    );

    Ok(())
}

/// Historical Globals fallback assertion retained only to document the retired
/// shape; Globals is no longer handler-admitted.
#[test]
#[ignore = "#10563: Retired: reference 13 is an unadmitted Globals scope; omission and zero-query rejection are covered by focused variables/frames tests."]
fn test_fallback_globals_scope_is_empty() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    // frame_id=1 → globals_ref = 13
    let globals_ref: i64 = 13;
    let body = request_variables(&mut adapter, globals_ref)
        .ok_or("globals variables request returned no body")?;

    let names = var_names(&body);

    assert!(
        names.is_empty(),
        "#7275 regression: Globals fallback must return empty without a live session; \
         got variables: {names:?}"
    );

    Ok(())
}

/// Historical three-way fallback overlap assertion; now vacuous by contract.
#[test]
#[ignore = "#10563: Retired: three-way empty-set overlap is vacuous once no-session references are rejected before fallback routing."]
fn test_fallback_scopes_have_no_overlapping_variable_names() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    // frame_id=1: locals=11, package=12, globals=13
    let locals_body =
        request_variables(&mut adapter, 11).ok_or("locals variables request failed")?;
    let package_body =
        request_variables(&mut adapter, 12).ok_or("package variables request failed")?;
    let globals_body =
        request_variables(&mut adapter, 13).ok_or("globals variables request failed")?;

    let locals_names = var_names(&locals_body);
    let package_names = var_names(&package_body);
    let globals_names = var_names(&globals_body);

    // Locals ∩ Package must be empty.
    let locals_package_overlap: HashSet<&String> =
        locals_names.intersection(&package_names).collect();
    assert!(
        locals_package_overlap.is_empty(),
        "cross-scope contamination: Locals and Package share variables: {locals_package_overlap:?}"
    );

    // Locals ∩ Globals must be empty.
    let locals_globals_overlap: HashSet<&String> =
        locals_names.intersection(&globals_names).collect();
    assert!(
        locals_globals_overlap.is_empty(),
        "cross-scope contamination: Locals and Globals share variables: {locals_globals_overlap:?}"
    );

    // Package ∩ Globals must be empty.
    let package_globals_overlap: HashSet<&String> =
        package_names.intersection(&globals_names).collect();
    assert!(
        package_globals_overlap.is_empty(),
        "cross-scope contamination: Package and Globals share variables: {package_globals_overlap:?}"
    );

    Ok(())
}

// ── 3. Scope-filter unit tests via handle_request + simulated output ──────────

/// An unadmitted Locals-shaped reference is rejected without a session.  This
/// deliberately proves the zero-query boundary rather than codec arithmetic.
#[test]
fn test_unadmitted_locals_reference_is_empty_without_session() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let locals_ref = 51;
    let body = request_variables(&mut adapter, locals_ref)
        .ok_or("unadmitted locals variables request failed")?;
    assert!(var_names(&body).is_empty());

    Ok(())
}

/// Package references are never admitted, including when their wire shape is
/// otherwise well-formed.
#[test]
fn test_unadmitted_package_reference_is_empty_without_session() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let package_ref = 52;
    let body = request_variables(&mut adapter, package_ref)
        .ok_or("unadmitted package variables request failed")?;
    assert!(var_names(&body).is_empty());

    Ok(())
}

/// Globals references are never admitted and cannot fall through to history.
#[test]
fn test_unadmitted_globals_reference_is_empty_without_session() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (tx, _rx) = sync_channel(64);
    adapter.set_event_sender(tx);

    let globals_ref = 53;
    let body = request_variables(&mut adapter, globals_ref)
        .ok_or("unadmitted globals variables request failed")?;
    assert!(var_names(&body).is_empty());

    Ok(())
}

/// The scope reference modulo-10 encoding remains self-consistent for the
/// protocol codec, even though Package/Globals are not handler-admitted.
#[test]
fn test_scope_type_encoding_is_self_consistent() -> TestResult {
    // For any frame_id, the three scope refs must have distinct moduli 1, 2, 3.
    for frame_id in [0i64, 1, 5, 10, 100] {
        let locals_ref = frame_id * 10 + 1;
        let package_ref = frame_id * 10 + 2;
        let globals_ref = frame_id * 10 + 3;

        assert_eq!(locals_ref % 10, 1, "frame {frame_id}: locals_ref mod 10 must be 1");
        assert_eq!(package_ref % 10, 2, "frame {frame_id}: package_ref mod 10 must be 2");
        assert_eq!(globals_ref % 10, 3, "frame {frame_id}: globals_ref mod 10 must be 3");

        // Refs must be distinct.
        assert_ne!(locals_ref, package_ref, "frame {frame_id}: locals_ref == package_ref");
        assert_ne!(locals_ref, globals_ref, "frame {frame_id}: locals_ref == globals_ref");
        assert_ne!(package_ref, globals_ref, "frame {frame_id}: package_ref == globals_ref");
    }

    Ok(())
}

// ── 4. E2E content-level scope filtering (requires live Perl) ─────────────────

/// Multi-scope Perl fixture: `my` lexicals, an `our` package variable, and
/// built-in globals.  When stopped inside `inner_sub`, only the inner sub's
/// own `my` variables should appear in the Locals scope.
///
/// Line layout:
///   1:  use strict;
///   2:  use warnings;
///   3:  (blank)
///   4:  our $pkg_counter = 0;
///   5:  my $outer_var = 42;
///   6:  (blank)
///   7:  sub inner_sub {
///   8:      my $inner_x = 10;
///   9:      my $inner_y = 20;   ← BP_INNER (breakpoint inside inner_sub)
///  10:      return $inner_x + $inner_y;
///  11:  }
///  12:  (blank)
///  13:  inner_sub();
fn multi_scope_script_content() -> &'static str {
    "use strict;\nuse warnings;\n\nour $pkg_counter = 0;\nmy $outer_var = 42;\n\nsub inner_sub {\n    my $inner_x = 10;\n    my $inner_y = 20;\n    return $inner_x + $inner_y;\n}\n\ninner_sub();\n"
}

const BP_INNER: u64 = 9; // my $inner_y = 20 — inside inner_sub

/// Execution stops inside the named sub, and its Locals scope must NOT contain
/// the outer `my $outer_var`, the `our $pkg_counter`, or built-in globals.
#[test]
#[ignore = "#10563: Retired: this legacy multi-scope E2E asserts omitted Package/Globals behavior; current-frame Locals/Arguments coverage is in perl-dap unit tests."]
fn test_e2e_named_sub_breakpoint_excludes_outer_and_global_vars() -> TestResult {
    if !perl_available() {
        return Ok(()); // skip: perl not available
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("scope_filter_locals.pl");
    write(&script, multi_scope_script_content())?;
    let script_str = script.to_str().ok_or("non-UTF-8 script path")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;

    session.launch(&script_str)?;
    let resolved_lines = session.set_breakpoints_checked(&script_str, &[BP_INNER])?;
    if resolved_lines != [BP_INNER as i64] {
        return Err(format!(
            "setBreakpoints resolved {resolved_lines:?}; expected line {BP_INNER}"
        )
        .into());
    }
    session.configuration_done()?;

    let stopped = session.wait_stopped()?;
    if stopped.reason != "breakpoint" {
        return Err(format!(
            "expected breakpoint stop inside inner_sub, got reason {:?}",
            stopped.reason
        )
        .into());
    }

    let (frame_id, _, _) = session.stack_trace(stopped.thread_id)?;
    let locals_ref = session.scopes_locals_ref(frame_id)?;
    let locals = session.variables(locals_ref)?;

    let local_names: HashSet<String> = locals
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str))
        .map(String::from)
        .collect();

    // We assert shape (non-empty, no package-qualified names, no globals) rather than
    // specific variable names, because whether the live debugger surfaces $inner_x/$inner_y
    // vs. the fallback $self/@_ depends on the parse path taken for this frame/session.
    // The cross-scope filtering properties are what matter here.
    assert!(!local_names.is_empty(), "locals scope must contain at least one variable");

    // All variable names in locals must NOT contain "::" (no package-qualified names).
    for name in &local_names {
        assert!(
            !name.contains("::"),
            "locals variable '{name}' must not be package-qualified (contains '::')"
        );
    }

    // Known global built-ins must NOT contaminate locals.
    let known_globals = ["%ENV", "@ARGV", "$_", "$!", "$@", "$/", "$|", "$0", "$^W"];
    for g in &known_globals {
        assert!(
            !local_names.contains(*g),
            "global built-in '{g}' must NOT appear in locals scope: {local_names:?}"
        );
    }

    assert!(
        !local_names.contains("$outer_var"),
        "outer-scope lexical '$outer_var' must NOT appear in named-sub Locals: {local_names:?}"
    );
    assert!(
        !local_names.contains("$pkg_counter"),
        "package variable '$pkg_counter' must NOT appear in named-sub Locals: {local_names:?}"
    );

    session.disconnect()?;
    Ok(())
}

/// Globals scope must contain recognised Perl built-in global variables.
/// It must NOT contain lexical `my` variables from the script.
#[test]
#[ignore = "#10563: globals enumeration returns nothing at a live breakpoint; the non-emptiness \
            and built-in-name assertions here passed only on the fabricated `$_` placeholder, \
            and the no-lexicals assertion is vacuous over an empty list. Un-ignore once \
            globals are genuinely enumerated (see issue #10162)"]
fn test_e2e_globals_scope_contains_builtin_globals_not_lexicals() -> TestResult {
    if !perl_available() {
        return Ok(()); // skip: perl not available
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("scope_filter_globals.pl");
    write(&script, multi_scope_script_content())?;
    let script_str = script.to_str().ok_or("non-UTF-8 script path")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;

    session.launch(&script_str)?;
    session.set_breakpoints(&script_str, &[BP_INNER])?;
    session.configuration_done()?;

    let stopped = session.wait_stopped()?;
    assert_eq!(stopped.reason, "breakpoint");

    let (frame_id, _, _) = session.stack_trace(stopped.thread_id)?;
    let globals_ref = session.scopes_globals_ref(frame_id)?;
    let globals = session.variables(globals_ref)?;

    assert!(!globals.is_empty(), "globals scope must contain at least one variable");

    let global_names: HashSet<String> = globals
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str))
        .map(String::from)
        .collect();

    // All variables in globals must be either known built-ins or magic `$^` variables.
    let known_globals: HashSet<&str> =
        ["%ENV", "@ARGV", "$_", "$!", "$@", "$/", "$|", "$0", "$^W"].iter().copied().collect();
    for name in &global_names {
        let is_known = known_globals.contains(name.as_str());
        let is_magic = name.starts_with("$^");
        assert!(
            is_known || is_magic,
            "globals scope contains '{name}' which is not a known global built-in"
        );
    }

    // Lexical variables from the script must NOT appear in globals.
    assert!(
        !global_names.contains("$inner_x"),
        "lexical '$inner_x' must NOT appear in globals scope: {global_names:?}"
    );
    assert!(
        !global_names.contains("$inner_y"),
        "lexical '$inner_y' must NOT appear in globals scope: {global_names:?}"
    );
    assert!(
        !global_names.contains("$outer_var"),
        "lexical '$outer_var' must NOT appear in globals scope: {global_names:?}"
    );

    session.disconnect()?;
    Ok(())
}

/// Outer-lexical variable `$outer_var` defined at file scope must NOT appear
/// in the Locals scope when execution is stopped inside `inner_sub`.
///
/// This tests closure/scope boundary: `perl -d`'s `V . .` command for the
/// current scope must not bleed outer lexicals into an inner sub's locals.
#[test]
#[ignore = "#10563: Retired: legacy cross-scope E2E expects the removed Package/Globals buckets."]
fn test_e2e_outer_lexical_does_not_leak_into_inner_sub_locals() -> TestResult {
    if !perl_available() {
        return Ok(()); // skip: perl not available
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("scope_filter_outer_leak.pl");
    write(&script, multi_scope_script_content())?;
    let script_str = script.to_str().ok_or("non-UTF-8 script path")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;

    session.launch(&script_str)?;
    session.set_breakpoints(&script_str, &[BP_INNER])?;
    session.configuration_done()?;

    let stopped = session.wait_stopped()?;
    assert_eq!(stopped.reason, "breakpoint");

    let (frame_id, _, _) = session.stack_trace(stopped.thread_id)?;
    let locals_ref = session.scopes_locals_ref(frame_id)?;
    let locals = session.variables(locals_ref)?;

    let local_names: HashSet<String> = locals
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str))
        .map(String::from)
        .collect();

    // `$outer_var` is defined at the outer file scope; it must NOT bleed into
    // the inner sub's Locals when stopped inside `inner_sub`.
    assert!(
        !local_names.contains("$outer_var"),
        "outer-scope lexical '$outer_var' must NOT appear in inner sub's Locals: {local_names:?}"
    );

    session.disconnect()?;
    Ok(())
}

/// No variable appears in multiple scopes simultaneously: Locals ∩ Package ∩
/// Globals must all be empty when stopped at a real breakpoint.
#[test]
#[ignore = "#10563: Retired: legacy cross-scope E2E expects the removed Package/Globals buckets."]
fn test_e2e_scopes_have_no_cross_contamination() -> TestResult {
    if !perl_available() {
        return Ok(()); // skip: perl not available
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("scope_filter_cross.pl");
    write(&script, multi_scope_script_content())?;
    let script_str = script.to_str().ok_or("non-UTF-8 script path")?.to_string();

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;

    session.launch(&script_str)?;
    session.set_breakpoints(&script_str, &[BP_INNER])?;
    session.configuration_done()?;

    let stopped = session.wait_stopped()?;
    assert_eq!(stopped.reason, "breakpoint");

    let (frame_id, _, _) = session.stack_trace(stopped.thread_id)?;

    let locals_ref = session.scopes_locals_ref(frame_id)?;
    let globals_ref = session.scopes_globals_ref(frame_id)?;

    let locals = session.variables(locals_ref)?;
    let globals = session.variables(globals_ref)?;

    let locals_names: HashSet<String> = locals
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str))
        .map(String::from)
        .collect();
    let globals_names: HashSet<String> = globals
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str))
        .map(String::from)
        .collect();

    // Locals ∩ Globals must be empty.
    let overlap: HashSet<&String> = locals_names.intersection(&globals_names).collect();
    assert!(
        overlap.is_empty(),
        "cross-scope contamination between Locals and Globals: {overlap:?}"
    );

    session.disconnect()?;
    Ok(())
}

// ── 5. Lexical variable correctness (issue #950 regression guard) ─────────────

/// The Locals scope at breakpoint line 6 (my $z = $x * $y) must return the
/// user script's own lexical variables `$x` (value "10") and `$y` (value "15"),
/// NOT the Perl debugger's internal DB-frame variables (`$self`, `@_`).
///
/// This is the regression guard for the bug where `handle_variables` for the
/// Locals scope returned DB-internal-frame placeholders instead of user lexicals.
/// Root cause: `V <frame_id> .` treats the numeric frame_id as a package name
/// (invalid) and the `fallback_scope_variables` lies by returning fake `$self`
/// and `@_` instead of returning empty / attempting real lexical enumeration.
///
/// Fixture (7 lines, BP_LEXICAL at line 6 = `my $z = $x * $y`):
///   Line 1: use strict;
///   Line 2: use warnings;
///   Line 3: (blank)
///   Line 4: my $x = 10;      <- already executed when stopped at line 6
///   Line 5: my $y = $x + 5;  <- already executed when stopped at line 6
///   Line 6: my $z = $x * $y; <- BP_LEXICAL (stopped HERE — $x and $y are set)
///   Line 7: print "$z\n";
#[test]
#[ignore = "#10563: Retired: legacy lexical E2E relies on old scope routing; current-frame Locals proof is in perl-dap unit tests."]
fn test_e2e_locals_scope_returns_user_lexicals_not_db_internals() -> TestResult {
    if !perl_available() {
        return Ok(()); // skip: perl not available
    }

    let workspace = tempdir()?;
    let script = workspace.path().join("scope_lexical_regression.pl");
    // Same 7-line fixture as dap_e2e_workflow_tests.rs; BP at line 6 so $x and $y are set.
    let content = "use strict;\nuse warnings;\n\nmy $x = 10;\nmy $y = $x + 5;\nmy $z = $x * $y;\nprint \"$z\\n\";\n";
    write(&script, content)?;
    let script_str = script.to_str().ok_or("non-UTF-8 script path")?.to_string();

    const BP_LEXICAL: u64 = 6; // my $z = $x * $y — $x and $y are already set here

    let timeout = workflow_timeout();
    let mut session = DapWorkflowSession::new(timeout)?;

    session.launch(&script_str)?;
    session.set_breakpoints(&script_str, &[BP_LEXICAL])?;
    session.configuration_done()?;

    let stopped = session.wait_stopped()?;
    assert_eq!(stopped.reason, "breakpoint", "must stop at breakpoint on line {BP_LEXICAL}");

    let (frame_id, _, _) = session.stack_trace(stopped.thread_id)?;
    let locals_ref = session.scopes_locals_ref(frame_id)?;
    let locals = session.variables(locals_ref)?;

    // Collect variable names from the Locals scope.
    let local_names: Vec<String> = locals
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str))
        .map(String::from)
        .collect();

    // PRIMARY ASSERTION (the bug): $x must appear by name in Locals.
    // Before the fix, locals contained ["$self", "@_"] (DB-internal frame placeholders).
    // After the fix, locals must contain the user's actual lexicals.
    assert!(
        local_names.iter().any(|n| n == "$x"),
        "Locals scope must contain '$x' (user lexical, value 10) — \
         got instead: {local_names:?}. \
         This indicates the adapter is returning DB-internal frame variables \
         instead of the user script's lexicals. Bug: V <frame_id> . treats the \
         numeric frame_id as a package name and falls back to fake placeholders."
    );

    // $y must also be present (it was assigned on line 5, before the breakpoint).
    assert!(
        local_names.iter().any(|n| n == "$y"),
        "Locals scope must contain '$y' (user lexical, value 15) — got: {local_names:?}"
    );

    // $x must have the correct value "10".
    let x_var = locals
        .iter()
        .find(|v| v.get("name").and_then(Value::as_str) == Some("$x"))
        .ok_or("$x not found in locals")?;
    let x_value =
        x_var.get("value").and_then(Value::as_str).ok_or("$x value missing or not a string")?;
    assert_eq!(
        x_value, "10",
        "$x value must be '10', got '{x_value}'. \
         Lexical variable inspection is not reading the correct value."
    );

    // Sanity guard: DB-internal fake placeholders must NOT appear.
    // `$self` and `@_` are the specific fake vars injected by fallback_scope_variables
    // for scope_type=1 when no real data is available.
    assert!(
        !local_names.iter().any(|n| n == "$self"),
        "Locals scope must NOT contain '$self' (DB-internal frame placeholder) — \
         got: {local_names:?}"
    );

    session.disconnect()?;
    Ok(())
}
