//! Exercise the editor-visible stderr boundary, including the documented opt-in.

use std::process::{Command, Stdio};

fn stderr_for(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_perllsp"))
        .args(args)
        .env_remove("RUST_LOG")
        .env_remove("PERL_LSP_LOG")
        .env_remove("PERL_LSP_LOG_FILE")
        .env_remove("PERL_LSP_QUIET")
        .stdin(Stdio::null())
        .output()
        .expect("start perllsp");
    assert!(
        output.status.success(),
        "server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

    let quiet = Command::new(env!("CARGO_BIN_EXE_perllsp"))
        .args(["--stdio"])
        .env_remove("RUST_LOG")
        .env_remove("PERL_LSP_LOG")
        .env_remove("PERL_LSP_LOG_FILE")
        .env("PERL_LSP_QUIET", "1")
        .stdin(Stdio::null())
        .output()
        .expect("start quiet perllsp");
    assert!(quiet.status.success());
    assert!(
        quiet.stderr.is_empty(),
        "quiet default session wrote stderr: {}",
        String::from_utf8_lossy(&quiet.stderr)
    );
}
