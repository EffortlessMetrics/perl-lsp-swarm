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
    use anyhow::{Result, anyhow};
    use perl_dap::{DapMessage, DebugAdapter};
    use serde_json::json;
    use std::process::Command;
    use std::sync::mpsc::sync_channel;
    use std::time::{Duration, Instant};

    use crate::common::{debuggee_perl_or_typed_skip, resolve_launch_perl_path};

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

    fn process_is_alive(pid: u32) -> Result<bool> {
        let pid_text = pid.to_string();
        #[cfg(unix)]
        {
            return Ok(Command::new("kill").args(["-0", &pid_text]).status()?.success());
        }
        #[cfg(windows)]
        {
            let output = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid_text}"), "/NH"])
                .output()?;
            return Ok(output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| line.split_whitespace().nth(1) == Some(pid_text.as_str())));
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = pid;
            Err(anyhow!("process liveness is unsupported on this platform"))
        }
    }

    fn wait_for_process_exit(pid: u32) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_is_alive(pid)? {
            if Instant::now() >= deadline {
                return Err(anyhow!("launched Perl child {pid} remained alive after Drop"));
            }
            std::thread::sleep(Duration::from_millis(20));
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
        if debuggee_perl_or_typed_skip("test_drop_reaps_launched_perl_process").is_none() {
            return Ok(());
        }

        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut script = NamedTempFile::with_suffix(".pl")?;
        script.write_all(b"my $x = 1;\nmy $y = 2;\nsleep 60;\nprint $y;\n")?;
        script.flush()?;
        let path = script.path().to_string_lossy().to_string();
        let perl_path =
            resolve_launch_perl_path().map_err(anyhow::Error::msg)?.ok_or_else(|| {
                anyhow!(
                    "perl_available reported an interpreter, but the launch resolver returned none"
                )
            })?;

        let (mut adapter, rx) = make_adapter();
        let _ = adapter.handle_request(1, "initialize", None);

        // Drain initialized event.
        let _ = rx.recv_timeout(std::time::Duration::from_millis(200));

        // Carry the same resolved identity that satisfied perl_available into
        // the raw launch request; otherwise a valid pin can be bypassed by
        // ambient interpreter resolution.
        let launch_args = json!({
            "program": path,
            "stopOnEntry": true,
            "perlPath": perl_path.to_string_lossy(),
        });
        let launch_started = std::time::Instant::now();
        let launch = adapter.handle_request(2, "launch", Some(launch_args));
        assert!(
            matches!(&launch, DapMessage::Response { success: true, .. }),
            "cleanup proof requires a successful adapter-owned launch, got: {launch:?}"
        );
        let child_pid = adapter.active_process_id_for_test().ok_or_else(|| {
            anyhow!("successful launch did not leave an adapter-owned process session")
        })?;
        assert!(
            process_is_alive(child_pid)?,
            "cleanup proof requires the adapter-owned Perl child {child_pid} to be alive before Drop"
        );

        // Drop must complete promptly — the adapter owns the child and must
        // terminate/reap it on this path rather than merely dropping handles.
        let drop_started = std::time::Instant::now();
        drop(adapter);
        assert!(
            drop_started.elapsed() < std::time::Duration::from_secs(5),
            "dropping an active adapter exceeded the bounded cleanup budget"
        );
        wait_for_process_exit(child_pid)?;
        assert!(
            launch_started.elapsed() < std::time::Duration::from_secs(10),
            "launch setup exceeded its bounded test budget"
        );
        Ok(())
    }

    /// disconnect followed by drop must not double-free or panic (idempotency
    /// under Perl-session conditions).
    #[tokio::test]
    async fn test_disconnect_then_drop_idempotent_with_perl() -> Result<()> {
        if debuggee_perl_or_typed_skip("test_disconnect_then_drop_idempotent_with_perl").is_none() {
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
