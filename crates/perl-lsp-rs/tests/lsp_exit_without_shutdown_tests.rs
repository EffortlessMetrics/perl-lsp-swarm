//! Abnormal protocol exit must stay visible on stderr (#17439 part a, #17337).
//!
//! Production `handle_exit_dispatch` calls `process::exit(1)` when `exit` arrives
//! without a prior `shutdown`, after writing a direct stderr reason that survives
//! the default `warn` tracing filter. The in-process harness cannot send bare
//! `exit` without terminating the test binary; this test drives the shipped
//! `perllsp --stdio` process instead.

mod common;

use common::{initialize_lsp, send_notification, start_lsp_server};
use serde_json::json;
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn exit_without_shutdown_reports_reason_on_stderr() -> TestResult {
    let server = start_lsp_server();
    let initialized = initialize_lsp(&server);
    if initialized.get("error").is_some() {
        return Err(format!("initialize failed: {initialized:?}").into());
    }

    send_notification(&server, json!({"jsonrpc":"2.0","method":"exit","params":null}));

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        let status = {
            let mut process = match server.process.lock() {
                Ok(process) => process,
                Err(poisoned) => poisoned.into_inner(),
            };
            process.try_wait()?
        };
        if status.is_some() || Instant::now() >= deadline {
            break status;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let status = status.ok_or("server child did not exit after bare exit notification")?;
    if status.success() {
        return Err(format!("exit without shutdown must not succeed: {status}").into());
    }
    let code = status.code().ok_or("server exited without status code")?;
    if code != 1 {
        return Err(format!("expected exit code 1, got {code}; {}", server.stderr_tail()).into());
    }

    let stderr = server.stderr_tail();
    let reason = "perl-lsp: exiting with code 1: `exit` notification received without a prior `shutdown` request";
    if !stderr.contains(reason) {
        return Err(format!("expected abnormal-exit reason on stderr; got: {stderr}").into());
    }
    Ok(())
}
