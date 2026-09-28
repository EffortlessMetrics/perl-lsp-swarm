//! Exercise the editor-visible stderr boundary, including the documented opt-in.

use std::process::{Command, Stdio};

/// Spawn `perllsp` with the log surface reduced to its documented defaults.
///
/// Color-forcing runner environments (`FORCE_COLOR`, `CLICOLOR_FORCE`,
/// `TERM_PROGRAM=WarpTerminal`) make `should_use_ansi_stderr` enable ANSI on a
/// piped stderr, so the child's own INFO token would carry escape codes and the
/// plain-text assertions below would read the wrong boundary. The variables are
/// removed from the child, matching the launcher's own ANSI tests.
fn spawn_perllsp(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_perllsp"));
    command
        .args(args)
        .env_remove("RUST_LOG")
        .env_remove("PERL_LSP_LOG")
        .env_remove("PERL_LSP_LOG_FILE")
        .env_remove("PERL_LSP_QUIET")
        .env_remove("FORCE_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("TERM_PROGRAM")
        .stdin(Stdio::null());
    command
}

fn stderr_for(args: &[&str]) -> String {
    let output = spawn_perllsp(args).output().expect("start perllsp");
    assert!(output.status.success(), "server failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stderr).expect("server stderr is UTF-8")
}

#[test]
fn stderr_info_logging_requires_opt_in() {
    let default = stderr_for(&["--stdio"]);
    assert!(!default.contains(" INFO "), "default session leaked INFO logs: {default}");
    assert!(
        !default.contains("Workspace indexing receipt"),
        "default session leaked telemetry: {default}"
    );

    let explicit = stderr_for(&["--stdio", "--log"]);
    assert!(explicit.contains(" INFO "), "--log did not enable INFO logs: {explicit}");

    let quiet = spawn_perllsp(&["--stdio"])
        .env("PERL_LSP_QUIET", "1")
        .output()
        .expect("start quiet perllsp");
    assert!(quiet.status.success());
    assert!(
        quiet.stderr.is_empty(),
        "quiet default session wrote stderr: {}",
        String::from_utf8_lossy(&quiet.stderr)
    );
}
