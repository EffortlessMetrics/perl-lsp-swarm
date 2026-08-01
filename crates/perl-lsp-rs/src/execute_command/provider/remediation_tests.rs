//! Guards for the "no Perl interpreter" message emitted by
//! `perl.runTest` / `perl.checkSyntax` / the other Perl-spawning
//! `workspace/executeCommand` handlers.
//!
//! The defect class (#5034 → #5373 → #5376) is a remediation sentence that
//! names an interpreter-path setting the user cannot set. These tests assert
//! the absence in both spellings and pin the parts of the message that are
//! load-bearing, so a future reword cannot quietly drop them.

use super::ExecuteCommandProvider;
use std::path::Path;

fn message() -> String {
    ExecuteCommandProvider::unresolved_execute_command_perl_error(Path::new("/w/t/basic.t"))
}

/// Matched on the bare `perl.path` substring, which also covers the
/// `perl-lsp.perl.path` spelling. #5373's guards matched only the prefixed
/// form, which is why this site survived that fix.
#[test]
fn error_names_no_interpreter_path_setting() {
    let msg = message();

    assert!(
        !msg.contains("perl.path"),
        "must not point at the nonexistent perl.path setting, got: {msg}"
    );
    assert!(
        !msg.contains("Configure"),
        "must not tell the user to configure anything; no interpreter-path channel exists, \
         got: {msg}"
    );
}

/// `perlPath` in `launch.json` is a DAP-only key selecting the debuggee's
/// interpreter. It has no effect on `executeCommand` handlers, so naming it
/// would just be a different unactionable answer.
#[test]
fn error_does_not_send_users_to_dap_only_launch_json() {
    let msg = message();

    assert!(!msg.contains("launch.json"), "executeCommand does not read launch.json, got: {msg}");
    assert!(
        !msg.contains("perlPath"),
        "perlPath is a DAP-only key with no effect here, got: {msg}"
    );
}

#[test]
fn error_keeps_the_file_and_the_actionable_remediation() {
    let msg = message();

    assert!(msg.contains("/w/t/basic.t"), "must name the file the command was for, got: {msg}");
    assert!(msg.contains("Install Perl"), "must give the one remediation that works, got: {msg}");
    assert!(
        msg.contains("Reload Window"),
        "installing Perl alone does not refresh the server's inherited PATH, got: {msg}"
    );
}

/// The refusal is deliberate (perl-lsp will not run an arbitrary ambient
/// interpreter). Rewording the remediation must not drop the explanation, or
/// the message becomes "it didn't work" with no reason.
#[test]
fn error_still_states_that_ambient_fallback_is_refused() {
    let msg = message();

    assert!(
        msg.contains("does not fall back to an ambient interpreter"),
        "must explain why no interpreter was guessed, got: {msg}"
    );
}
