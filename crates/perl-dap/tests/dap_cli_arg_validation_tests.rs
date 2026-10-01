//! Discriminating proof for the external-peer CLI argument validation (#16556).
//!
//! These tests spawn the packaged `perl-dap` binary. They fail if a malformed
//! `--external-peer` spec surfaces as the transport's raw "invalid socket
//! address" jargon instead of a format-naming startup error, or if an
//! explicit-but-unparseable `--external-peer-listen` port silently binds an
//! ephemeral port instead of failing startup.
//!
//! The one-shot emit flags' unreadable-program behavior is owned by
//! `dap_one_shot_unreadable_program_16553.rs` (#16553, landed by #16702) and is
//! deliberately not duplicated here.

use anyhow::{Context, Result, anyhow};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// The listen session intentionally waits for a peer, so bound the wait.
const LISTEN_PROBE_ALIVE: Duration = Duration::from_secs(3);
/// Generous enough for a loaded host; still fails a genuine hang.
const CLI_TIMEOUT: Duration = Duration::from_secs(30);

fn dap_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perl-dap"))
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn drain_pipe<R: Read + Send + 'static>(reader: Option<R>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut reader) = reader {
            let _ = reader.read_to_string(&mut buf);
        }
        buf
    })
}

/// Run the CLI to completion with piped output. Fails if the process is still
/// running after `CLI_TIMEOUT` (a malformed argument must never hang).
fn run_cli(args: &[&str]) -> Result<(ExitStatus, String, String)> {
    let mut child = Command::new(dap_binary())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn perl-dap")?;
    let stdout = drain_pipe(child.stdout.take());
    let stderr = drain_pipe(child.stderr.take());
    let status = wait_for_exit(&mut child, CLI_TIMEOUT)?
        .ok_or_else(|| {
            let _ = child.kill();
            let _ = child.wait();
            anyhow!("perl-dap {args:?} still running after {CLI_TIMEOUT:?}; a malformed argument must fail startup, not wait")
        })?;
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    Ok((status, stdout, stderr))
}

fn assert_nonzero(status: &ExitStatus, stdout: &str, stderr: &str, why: &str) -> Result<()> {
    if status.success() {
        return Err(anyhow!("{why}: must exit nonzero. stdout={stdout:?} stderr={stderr:?}"));
    }
    Ok(())
}

#[test]
fn external_peer_malformed_spec_fails_at_startup_naming_host_port_format() -> Result<()> {
    let (status, stdout, stderr) =
        run_cli(&["--external-peer", "no-colon-thing", "--log-level", "error"])?;
    assert_nonzero(
        &status,
        &stdout,
        &stderr,
        "a malformed --external-peer spec must fail startup",
    )?;
    let combined = format!("{stdout}\n{stderr}");
    for expected in ["HOST:PORT", "no-colon-thing"] {
        if !combined.contains(expected) {
            return Err(anyhow!(
                "the error must name the expected format and the offending spec ({expected}); stdout={stdout:?} stderr={stderr:?}"
            ));
        }
    }
    if combined.contains("invalid socket address") {
        return Err(anyhow!(
            "the raw transport jargon must not surface for a malformed spec; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if combined.contains("Starting DAP server on stdio") {
        return Err(anyhow!(
            "a malformed spec must not start any DAP session; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    Ok(())
}

#[test]
fn external_peer_listen_unparseable_port_fails_at_startup_instead_of_binding_ephemeral()
-> Result<()> {
    let (status, stdout, stderr) =
        run_cli(&["--external-peer-listen", "127.0.0.1:notaport", "--log-level", "error"])?;
    assert_nonzero(
        &status,
        &stdout,
        &stderr,
        "an explicit-but-unparseable listen port must fail startup",
    )?;
    let combined = format!("{stdout}\n{stderr}");
    if !combined.contains("HOST[:PORT]") {
        return Err(anyhow!(
            "the error must name the expected HOST[:PORT] format; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    if combined.contains("PERL_DAP_PEER") {
        return Err(anyhow!(
            "no peer env contract may be advertised when the port is unparseable; stdout={stdout:?} stderr={stderr:?}"
        ));
    }
    Ok(())
}

#[test]
fn external_peer_listen_bare_host_still_binds_an_ephemeral_loopback_port() -> Result<()> {
    // Keep stdin open: the editor-owned stdio session settles on EOF, so a
    // null stdin would end the listen session before the probe observes it.
    let mut child = Command::new(dap_binary())
        .args(["--external-peer-listen", "127.0.0.1", "--log-level", "info"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn perl-dap --external-peer-listen")?;
    let stderr = drain_pipe(child.stderr.take());
    // Give the listen session time to bind and log its env contract.
    thread::sleep(LISTEN_PROBE_ALIVE);
    let settled = child.try_wait()?;
    let stderr = if settled.is_some() {
        let output = stderr.join().unwrap_or_default();
        let _ = child.kill();
        let _ = child.wait();
        return Err(anyhow!(
            "the bare-host ephemeral fallback must keep listening for a peer; status={settled:?} stderr={output:?}"
        ));
    } else {
        let _ = child.kill();
        let _ = child.wait();
        stderr.join().unwrap_or_default()
    };
    for expected in ["PERL_DAP_PEER", "127.0.0.1:"] {
        if !stderr.contains(expected) {
            return Err(anyhow!(
                "the ephemeral fallback must still advertise the peer env contract ({expected}); stderr={stderr:?}"
            ));
        }
    }
    Ok(())
}
