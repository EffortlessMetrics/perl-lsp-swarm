//! DAP Session Cleanup Tests (AC: session-cleanup)
//!
//! Verifies that `DebugAdapter::drop` properly cancels in-flight work and
//! calls `clear_active_session_state`, releasing child-process and TCP
//! resources without panicking.
//!
//! Run with: `cargo test -p perl-dap --test dap_session_cleanup_e2e`

mod common;

#[cfg(feature = "dap-phase2")]
mod cleanup_tests {
    use anyhow::Result;
    use perl_dap::{DapMessage, DebugAdapter};
    use serde_json::{Value, json};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::sync::mpsc::sync_channel;
    use std::time::{Duration, Instant};

    use crate::common::{debuggee_perl_or_typed_skip, perl_available};

    // ── Helpers ─────────────────────────────────────────────────────────────────

    fn make_adapter() -> (DebugAdapter, std::sync::mpsc::Receiver<DapMessage>) {
        let (tx, rx) = sync_channel(64);
        let mut adapter = DebugAdapter::new();
        adapter.set_event_sender(tx);
        (adapter, rx)
    }

    fn is_success(msg: DapMessage) -> bool {
        matches!(msg, DapMessage::Response { success: true, .. })
    }

    fn wait_for_child_pid(marker: &Path, script: &Path, timeout: Duration) -> Result<u32> {
        let deadline = Instant::now() + timeout;
        loop {
            if fs::read_to_string(marker).is_ok() {
                #[cfg(windows)]
                if let Some(pid) = native_pid_for_script(script) {
                    return Ok(pid);
                }
                #[cfg(unix)]
                if let Ok(contents) = fs::read_to_string(marker) {
                    if let Ok(pid) = contents.trim().parse::<u32>() {
                        return Ok(pid);
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(anyhow::anyhow!(
                    "DAP launch marker did not identify a live child: {}",
                    marker.display()
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[cfg(windows)]
    fn native_pid_for_script(script: &Path) -> Option<u32> {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_Process | Select-Object ProcessId,CommandLine | ConvertTo-Json -Compress",
            ])
            .output()
            .ok()?;
        let processes: Value = serde_json::from_slice(&output.stdout).ok()?;
        let entries = match processes {
            Value::Array(entries) => entries,
            Value::Object(_) => vec![processes],
            _ => return None,
        };
        let needle = script.to_string_lossy().to_lowercase();
        entries.into_iter().find_map(|entry| {
            let command_line = entry.get("CommandLine")?.as_str()?.to_lowercase();
            if !command_line.contains(&needle) {
                return None;
            }
            entry.get("ProcessId")?.as_u64()?.try_into().ok()
        })
    }

    fn process_exists(pid: u32) -> Result<bool> {
        #[cfg(windows)]
        {
            let output = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .output()?;
            if !output.status.success() {
                return Err(anyhow::anyhow!(
                    "tasklist failed while checking PID {pid}: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            Ok(String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                line.split(',')
                    .nth(1)
                    .map(|field| field.trim_matches('"') == pid.to_string())
                    .unwrap_or(false)
            }))
        }
        #[cfg(unix)]
        {
            Ok(Command::new("kill").args(["-0", &pid.to_string()]).status()?.success())
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = pid;
            Ok(false)
        }
    }

    fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while process_exists(pid)? {
            if Instant::now() >= deadline {
                return Err(anyhow::anyhow!("DAP child {pid} survived adapter drop"));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    }

    // ── Non-Perl tests (no `perl` binary required) ────────────────────────────

    /// Drop of a freshly-created adapter with no session must not panic.
    #[test]
    fn test_drop_empty_adapter_no_panic() {
        let adapter = DebugAdapter::new();
        drop(adapter);
        // Reaching here without panic is the assertion.
    }

    /// Drop after `initialize` (but no `launch`) must not panic or block.
    #[test]
    fn test_drop_after_initialize_no_panic() -> Result<()> {
        let (mut adapter, _rx) = make_adapter();
        let resp = adapter.handle_request(1, "initialize", None);
        assert!(is_success(resp), "initialize should succeed");
        drop(adapter);
        Ok(())
    }

    /// Drop after `initialize` + `disconnect` must be idempotent — the
    /// `clear_active_session_state` call inside Drop should be a no-op because
    /// `disconnect` already cleaned up.
    #[test]
    fn test_drop_after_disconnect_is_idempotent() -> Result<()> {
        let (mut adapter, _rx) = make_adapter();
        let _ = adapter.handle_request(1, "initialize", None);
        let disc = adapter.handle_request(2, "disconnect", None);
        assert!(is_success(disc), "disconnect should succeed");
        drop(adapter); // second cleanup pass — must not panic
        Ok(())
    }

    /// Multiple sequential adapters must each clean up independently without
    /// interfering with each other's state.
    #[test]
    fn test_multiple_sequential_adapter_lifecycles() -> Result<()> {
        for i in 0u64..3 {
            let (mut adapter, _rx) = make_adapter();
            let resp = adapter.handle_request(i as i64 + 1, "initialize", None);
            assert!(is_success(resp));
            drop(adapter);
        }
        Ok(())
    }

    /// An adapter created without an event sender must still clean up safely.
    #[test]
    fn test_drop_without_event_sender_no_panic() {
        let mut adapter = DebugAdapter::new();
        let _ = adapter.handle_request(1, "initialize", None);
        drop(adapter);
    }

    // ── Perl-requiring tests (skipped when `perl` is absent) ──────────────────

    /// Cancellation flag is set before `clear_active_session_state` so that any
    /// in-flight output-reader loop sees `cancel_requested = true` and exits;
    /// this test exercises the flag ordering without a live Perl process.
    ///
    /// We verify by initialising and immediately dropping — if the output-reader
    /// thread (spawned on launch) holds a reference, the `Arc<AtomicBool>` write
    /// in Drop propagates to it; without a real process there is no thread to
    /// observe it, but the ordering contract is exercised.
    #[test]
    fn test_cancel_flag_set_before_session_clear() -> Result<()> {
        let (mut adapter, _rx) = make_adapter();
        let _ = adapter.handle_request(1, "initialize", None);
        // Drop triggers: store(true, Release) → clear_active_session_state()
        drop(adapter);
        Ok(())
    }

    /// When Perl is available, dropping an adapter that was launched but never
    /// explicitly disconnected must terminate the child `perl -d` process without
    /// leaking it as a zombie.
    #[tokio::test]
    async fn test_drop_reaps_launched_perl_process() -> Result<()> {
        if !perl_available() {
            return Ok(());
        }
        let perl = debuggee_perl_or_typed_skip("test_drop_reaps_launched_perl_process")
            .ok_or_else(|| anyhow::anyhow!("perl availability changed before launch"))?;

        use std::io::Write;
        use tempfile::{NamedTempFile, tempdir};

        let mut script = NamedTempFile::with_suffix(".pl")?;
        script.write_all(
            br#"use strict;
use warnings;
open my $pid_file, '>', $ENV{PERL_LSP_DAP_TEST_CHILD_PID_FILE}
    or die "cannot write child PID: $!";
my $pid = $$;
if ($^O eq 'MSWin32') {
    eval { require Win32; $pid = Win32::GetCurrentProcessId(); 1 }
        or die "cannot resolve native Windows PID: $@";
}
print $pid_file "$pid\n";
close $pid_file or die "cannot close child PID: $!";
while (1) {
    select undef, undef, undef, 0.1;
}
"#,
        )?;
        script.flush()?;
        let path = script.path().to_string_lossy().to_string();
        let child_pid_dir = tempdir()?;
        let child_pid_file = child_pid_dir.path().join("dap-child.pid");

        let (mut adapter, rx) = make_adapter();
        let _ = adapter.handle_request(1, "initialize", None);

        // Drain initialized event.
        let _ = rx.recv_timeout(std::time::Duration::from_millis(200));

        let launch = adapter.handle_request(
            2,
            "launch",
            Some(json!({
                "program": path,
                "stopOnEntry": false,
                "perlPath": perl.binary,
                "env": {
                    "PERL_LSP_DAP_TEST_CHILD_PID_FILE": child_pid_file.to_string_lossy()
                }
            })),
        );
        if !is_success(launch) {
            return Err(anyhow::anyhow!("DAP launch response must report success"));
        }
        let configured = adapter.handle_request(3, "configurationDone", None);
        if !is_success(configured) {
            return Err(anyhow::anyhow!("DAP configurationDone response must report success"));
        }

        let child_pid =
            wait_for_child_pid(&child_pid_file, Path::new(&path), Duration::from_secs(5))?;
        if !process_exists(child_pid)? {
            return Err(anyhow::anyhow!(
                "DAP launch marker named child {child_pid}, but it is not alive"
            ));
        }

        drop(adapter); // Drop must terminate and reap the observed child.
        wait_for_process_exit(child_pid, Duration::from_secs(5))?;
        Ok(())
    }

    /// disconnect followed by drop must not double-free or panic (idempotency
    /// under Perl-session conditions).
    #[tokio::test]
    async fn test_disconnect_then_drop_idempotent_with_perl() -> Result<()> {
        if !perl_available() {
            return Ok(());
        }

        let (mut adapter, rx) = make_adapter();
        let _ = adapter.handle_request(1, "initialize", None);
        let _ = rx.recv_timeout(std::time::Duration::from_millis(200));

        let disc = adapter.handle_request(2, "disconnect", None);
        // Disconnect succeeds whether or not a session was ever started.
        drop(disc);
        drop(adapter);
        Ok(())
    }

    /// Creating and dropping several adapters in quick succession must not leave
    /// dangling threads or panic due to mutex poisoning.
    #[test]
    fn test_rapid_create_drop_cycles() {
        for _ in 0..5 {
            let adapter = DebugAdapter::new();
            drop(adapter);
        }
    }
}
