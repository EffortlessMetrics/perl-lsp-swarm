use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn test_evaluate_rejects_newlines() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Malicious expression with newline
    let args = json!({
        "expression": "1\nprint 'hacked'"
    });

    let response = adapter.handle_request(1, "evaluate", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Evaluate should fail for expression with newlines");
            let msg = message.ok_or("Should have error message")?;
            assert_eq!(
                msg, "Expression cannot contain newlines",
                "Should reject newlines explicitly"
            );
        }
        _ => return Err("Expected Response".into()),
    }
    Ok(())
}

#[test]
fn test_evaluate_detects_unsafe_backticks() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Expression with backticks (shell execution)
    let args = json!({
        "expression": "`ls -la`",
        "allowSideEffects": false
    });

    let response = adapter.handle_request(1, "evaluate", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Evaluate should fail for backticks in safe mode");
            let msg = message.ok_or("Should have error message")?;
            assert!(
                msg.contains("Safe evaluation mode: backticks"),
                "Should specifically mention backticks"
            );
        }
        _ => return Err("Expected Response".into()),
    }
    Ok(())
}

#[test]
fn test_evaluate_detects_unsafe_qx() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Expression with qx (shell execution)
    let args = json!({
        "expression": "qx(ls -la)",
        "allowSideEffects": false
    });

    let response = adapter.handle_request(1, "evaluate", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Evaluate should fail for qx in safe mode");
            let msg = message.ok_or("Should have error message")?;
            assert!(
                msg.contains("Safe evaluation mode: potentially mutating operation 'qx'"),
                "Should specifically mention qx"
            );
        }
        _ => return Err("Expected Response".into()),
    }
    Ok(())
}

#[test]
fn test_evaluate_defaults_to_safe_mode_without_side_effects_flag() -> TestResult {
    let mut adapter = DebugAdapter::new();

    let args = json!({
        "expression": "system('ls')"
    });

    let response = adapter.handle_request(1, "evaluate", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(
                !success,
                "Evaluate should fail for dangerous ops when allowSideEffects is omitted"
            );
            let msg = message.ok_or("Should have error message")?;
            assert!(
                msg.contains("Safe evaluation mode"),
                "Should use the safe-mode validator by default"
            );
        }
        _ => return Err("Expected Response".into()),
    }

    Ok(())
}

#[test]
fn test_evaluate_rejects_carriage_returns() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Malicious expression with carriage return
    let args = json!({
        "expression": "1\rprint 'hacked'"
    });

    let response = adapter.handle_request(1, "evaluate", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Evaluate should fail for expression with carriage returns");
            let msg = message.ok_or("Should have error message")?;
            assert_eq!(
                msg, "Expression cannot contain newlines",
                "Should reject newlines explicitly"
            );
        }
        _ => return Err("Expected Response".into()),
    }
    Ok(())
}

/// Comprehensive test for all unsafe operations that must be blocked in safe evaluation mode.
/// These operations can cause:
/// - Code execution (eval, require, do)
/// - Process control issues (kill, exit, dump, fork, alarm, sleep, wait, waitpid)
/// - I/O side effects (print, say, printf, sysread, syswrite)
/// - Filesystem modification (chroot, truncate, symlink, link)
/// - Network operations (socket, connect, bind, listen, accept, send, recv)
/// - Arbitrary code via tie mechanism (tie, untie)
#[test]
fn test_evaluate_blocks_dangerous_operations() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // Map of operation -> example expression that uses it
    // Coverage: all categories of dangerous operations
    let dangerous_ops = [
        // Code execution
        ("eval", "eval('1+1')"),
        ("require", "require 'File.pm'"),
        ("do", "do 'script.pl'"),
        // Process control
        ("kill", "kill 9, $$"),
        ("exit", "exit(0)"),
        ("dump", "dump"),
        ("fork", "fork"),
        ("alarm", "alarm(60)"),
        ("sleep", "sleep(1)"),
        ("wait", "wait"),
        ("waitpid", "waitpid(-1, 0)"),
        // I/O
        ("print", "print 'side effect'"),
        ("say", "say 'side effect'"),
        ("printf", "printf '%s', 'effect'"),
        ("sysread", "sysread(FH, $buf, 100)"),
        ("syswrite", "syswrite(FH, 'data')"),
        // Filesystem
        ("chroot", "chroot('/tmp')"),
        ("truncate", "truncate('file', 0)"),
        ("symlink", "symlink('old', 'new')"),
        ("link", "link('old', 'new')"),
        // Tie mechanism
        ("tie", "tie %hash, 'Tie::Hash'"),
        ("untie", "untie %hash"),
        // Network
        ("socket", "socket(S, PF_INET, SOCK_STREAM, 0)"),
        ("connect", "connect(S, $addr)"),
        ("bind", "bind(S, $addr)"),
        ("listen", "listen(S, 5)"),
        ("accept", "accept(C, S)"),
        ("send", "send(S, 'data', 0)"),
        ("recv", "recv(S, $buf, 100, 0)"),
    ];

    let mut failures = Vec::new();

    for (op_name, expression) in dangerous_ops {
        let args = json!({
            "expression": expression,
            "allowSideEffects": false
        });

        let response = adapter.handle_request(1, "evaluate", Some(args));

        match response {
            DapMessage::Response { success, message, .. } => {
                let msg = message.unwrap_or_default();
                let expected_pattern = format!("potentially mutating operation '{}'", op_name);

                if !success && msg.contains(&expected_pattern) {
                    // Blocked correctly
                } else {
                    failures.push(format!(
                        "Operation '{}' (expr: '{}') was NOT blocked. success={}, msg={}",
                        op_name, expression, success, msg
                    ));
                }
            }
            _ => failures.push(format!("Operation '{}': expected Response, got Event", op_name)),
        }
    }

    if !failures.is_empty() {
        return Err(format!(
            "The following dangerous operations were NOT blocked in safe mode:\n{}",
            failures.join("\n")
        )
        .into());
    }

    Ok(())
}

/// Test that dangerous operations ARE allowed when allowSideEffects is true
/// in the explicit `repl` context.
///
/// #9385 narrowed `allowSideEffects` to the interactive REPL, so this test now
/// states the context it always implicitly meant. The companion tests below
/// prove every other context refuses the same expressions.
#[test]
fn test_evaluate_allows_dangerous_ops_with_side_effects_enabled() -> TestResult {
    let mut adapter = DebugAdapter::new();

    // These should NOT be blocked when allowSideEffects is true
    // (they may still fail for other reasons like not being in a debug session)
    let ops_to_test = ["eval('1')", "print 'test'", "system('ls')"];

    for expression in ops_to_test {
        let args = json!({
            "expression": expression,
            "context": "repl",
            "allowSideEffects": true
        });

        let response = adapter.handle_request(1, "evaluate", Some(args));

        if let DapMessage::Response { message, .. } = response {
            let msg = message.unwrap_or_default();
            // Should NOT be blocked by safe mode validation
            assert!(
                !msg.contains("Safe evaluation mode"),
                "Operation '{}' should NOT be blocked when allowSideEffects=true, but got: {}",
                expression,
                msg
            );
        }
        // Events are fine, just checking we don't get safe-mode rejection
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// #9385: side-effectful evaluation is confined to the explicit REPL boundary.
//
// The custom `allowSideEffects` flag must not widen a read-oriented evaluate
// context into arbitrary Perl execution. Watch expressions re-evaluate on every
// stop and hovers fire from mouse movement, so admitting side effects there
// would let passive inspection mutate the debuggee.
// ---------------------------------------------------------------------------

/// Expressions that mutate state, spawn processes, or execute code.
const SIDE_EFFECTFUL_EXPRESSIONS: [&str; 4] =
    ["system('ls')", "eval('1')", "print 'test'", "$x = 1"];

/// Every evaluate context that must never carry side-effect authority.
const NON_REPL_CONTEXTS: [&str; 5] =
    ["watch", "hover", "variables", "clipboard", "totally-unknown"];

fn refusal_message(response: DapMessage) -> String {
    match response {
        DapMessage::Response { command, success, message, .. } => {
            assert_eq!(command, "evaluate");
            assert!(!success, "a refused evaluate must not report success");
            message.unwrap_or_default()
        }
        other => panic!("expected Response, got {other:?}"),
    }
}

#[test]
fn side_effects_are_refused_in_every_non_repl_context() -> TestResult {
    let mut adapter = DebugAdapter::new();

    for context in NON_REPL_CONTEXTS {
        for expression in SIDE_EFFECTFUL_EXPRESSIONS {
            let response = adapter.handle_request(
                1,
                "evaluate",
                Some(json!({
                    "expression": expression,
                    "context": context,
                    "allowSideEffects": true
                })),
            );

            let message = refusal_message(response);
            assert!(
                message.contains("only honored for the 'repl' evaluation context"),
                "context {context:?} must refuse '{expression}' with the trust-boundary \
                 message, but got: {message}"
            );
        }
    }

    Ok(())
}

#[test]
fn side_effects_are_refused_when_the_context_is_absent() -> TestResult {
    // Fail-closed default: `context` is optional in the DAP schema, so a client
    // that omits it must not inherit REPL authority.
    let mut adapter = DebugAdapter::new();

    for expression in SIDE_EFFECTFUL_EXPRESSIONS {
        let response = adapter.handle_request(
            1,
            "evaluate",
            Some(json!({ "expression": expression, "allowSideEffects": true })),
        );

        let message = refusal_message(response);
        assert!(
            message.contains("only honored for the 'repl' evaluation context"),
            "an absent context must refuse '{expression}', but got: {message}"
        );
    }

    Ok(())
}

#[test]
fn repl_context_is_matched_exactly_and_not_by_case_or_prefix() -> TestResult {
    // Negative control: the boundary is an exact label match, so a client
    // cannot reach execution authority with a near-miss label.
    let mut adapter = DebugAdapter::new();

    for context in ["REPL", "Repl", "repl ", "repl-console", "myrepl"] {
        let response = adapter.handle_request(
            1,
            "evaluate",
            Some(json!({
                "expression": "system('ls')",
                "context": context,
                "allowSideEffects": true
            })),
        );

        let message = refusal_message(response);
        assert!(
            message.contains("only honored for the 'repl' evaluation context"),
            "near-miss context {context:?} must not be treated as the REPL, got: {message}"
        );
    }

    Ok(())
}

#[test]
fn read_oriented_contexts_still_evaluate_safe_expressions() -> TestResult {
    // The boundary must confine side effects without breaking ordinary
    // inspection: a safe watch/hover expression is still admitted for
    // evaluation (it fails later only because no debug session is active).
    let mut adapter = DebugAdapter::new();

    for context in NON_REPL_CONTEXTS {
        let response = adapter.handle_request(
            1,
            "evaluate",
            Some(json!({ "expression": "$my_scalar", "context": context })),
        );

        if let DapMessage::Response { message, .. } = response {
            let message = message.unwrap_or_default();
            assert!(
                !message.contains("only honored for the 'repl' evaluation context")
                    && !message.contains("Safe evaluation mode"),
                "safe expression in context {context:?} must not be refused, got: {message}"
            );
        }
    }

    Ok(())
}

#[test]
fn screening_still_blocks_dangerous_expressions_without_the_flag() -> TestResult {
    // The pre-existing admission control must survive the new boundary: a
    // dangerous expression with no `allowSideEffects` is still screened, and is
    // refused for being dangerous rather than for the context.
    let mut adapter = DebugAdapter::new();

    for context in ["repl", "watch"] {
        let response = adapter.handle_request(
            1,
            "evaluate",
            Some(json!({ "expression": "system('ls')", "context": context })),
        );

        let message = refusal_message(response);
        assert!(
            !message.contains("only honored for the 'repl' evaluation context"),
            "context {context:?} without the flag must be screened, not refused for context"
        );
        assert!(
            !message.is_empty(),
            "context {context:?} must refuse a dangerous screened expression"
        );
    }

    Ok(())
}

#[test]
fn inspection_refusals_do_not_prescribe_a_retry_that_is_always_refused() -> TestResult {
    // Regression control for a defect this boundary introduced. The safe-eval
    // validators append "(use allowSideEffects: true)" to every refusal. Before
    // #9385 that was actionable from any context; now the flag is refused
    // outside `repl`, so a watch or hover caller who follows the advice hits a
    // second, different refusal. An error that prescribes a guaranteed failure
    // is worse than one that stays silent.
    let mut adapter = DebugAdapter::new();

    for context in ["watch", "hover", "variables", "clipboard", "totally-unknown"] {
        let response = adapter.handle_request(
            1,
            "evaluate",
            Some(json!({ "expression": "$x = 42", "context": context })),
        );

        let message = refusal_message(response);
        assert!(
            !message.contains("allowSideEffects"),
            "context {context:?} cannot set the flag, so its refusal must not prescribe it: \
             {message}"
        );
        assert!(
            message.contains("debug console"),
            "context {context:?} must be told where side effects are actually available: \
             {message}"
        );
        assert!(
            message.contains("assignment operator"),
            "retargeting the hint must not erase why the expression was refused: {message}"
        );
    }

    // The console keeps the advice it can actually act on.
    let response = adapter.handle_request(
        1,
        "evaluate",
        Some(json!({ "expression": "$x = 42", "context": "repl" })),
    );
    let message = refusal_message(response);
    assert!(
        message.contains("allowSideEffects"),
        "the REPL caller can set the flag, so the actionable hint must survive: {message}"
    );

    Ok(())
}

#[test]
fn repl_side_effects_are_refused_when_trusted_repl_is_disabled() -> TestResult {
    // Product policy can withdraw the REPL execution surface entirely, and the
    // refusal happens before any debugger command is constructed.
    let mut adapter =
        DebugAdapter::new().with_repl_trust(perl_dap::eval::ReplTrustPolicy::ReplDisabled);

    let response = adapter.handle_request(
        1,
        "evaluate",
        Some(json!({
            "expression": "system('ls')",
            "context": "repl",
            "allowSideEffects": true
        })),
    );

    let message = refusal_message(response);
    assert!(
        message.contains("disabled by policy"),
        "REPL-disabled policy must refuse before debugger write, got: {message}"
    );

    Ok(())
}
