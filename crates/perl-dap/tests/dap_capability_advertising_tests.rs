//! DAP Capability Advertising Tests
//!
//! Capability advertising must be *honest*: a `supportsX` flag in the `initialize`
//! response is a promise that the corresponding request can actually succeed.
//!
//! These tests previously asserted the weaker rule "a handler is routed, therefore
//! advertise the capability" (#1663 Phase 0).  That rule let `restartFrame` and
//! `terminateThreads` be advertised as GA while their handlers returned
//! `success: false` on every single call (#5045).  The rule enforced here is:
//!
//! * a capability whose handler can never succeed must NOT be advertised, and
//! * a capability that is advertised must be backed by the feature catalog, so
//!   `features.toml` and the wire response can never drift apart.

#[cfg(feature = "dap-phase2")]
mod capability_tests {
    use anyhow::Result;
    use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
    use perl_dap::types::{Source, StackFrame};
    use serde_json::Value;
    use std::fs;
    use std::sync::mpsc::sync_channel;

    fn create_test_adapter() -> DebugAdapter {
        let (tx, _rx) = sync_channel(64);
        let mut adapter = DebugAdapter::new();
        adapter.set_event_sender(tx);
        adapter
    }

    fn extract_initialize_response(msg: DapMessage) -> Result<Value> {
        match msg {
            DapMessage::Response { success, command, body, .. } => {
                if command == "initialize" && success {
                    body.ok_or_else(|| anyhow::anyhow!("initialize response missing body"))
                } else {
                    anyhow::bail!("initialize response not successful")
                }
            }
            _ => anyhow::bail!("expected initialize response"),
        }
    }

    fn capability(caps: &Value, name: &str) -> Result<bool> {
        caps.get(name)
            .and_then(Value::as_bool)
            .ok_or_else(|| anyhow::anyhow!("missing boolean capability `{name}`"))
    }

    fn initialize_capabilities(adapter: &mut DebugAdapter) -> Result<Value> {
        extract_initialize_response(adapter.handle_request(1, "initialize", None))
    }

    /// A request that fails on every call must not be advertised as supported.
    ///
    /// `restartFrame` has no perl5db primitive: `handle_restart_frame` returns
    /// `success: false` unconditionally, so a client that trusts the capability
    /// would surface a "Restart Frame" action that can only ever error.
    #[tokio::test]
    async fn test_restart_frame_is_not_advertised_because_it_always_fails() -> Result<()> {
        let mut adapter = create_test_adapter();
        let caps = initialize_capabilities(&mut adapter)?;

        assert!(
            !capability(&caps, "supportsRestartFrame")?,
            "supportsRestartFrame must be false while handle_restart_frame always fails"
        );

        match adapter.handle_request(2, "restartFrame", Some(serde_json::json!({"frameId": 1}))) {
            DapMessage::Response { success, command, message, .. } => {
                assert_eq!(command, "restartFrame");
                assert!(
                    !success,
                    "restartFrame is expected to fail; if it now succeeds, advertise it"
                );
                assert!(
                    message.is_some_and(|m| !m.is_empty()),
                    "an unsupported request must explain why it failed"
                );
            }
            other => anyhow::bail!("expected a response for restartFrame, got {other:?}"),
        }
        Ok(())
    }

    /// Same rule for `terminateThreads`: Perl's debugger cannot target one thread.
    #[tokio::test]
    async fn test_terminate_threads_is_not_advertised_because_it_always_fails() -> Result<()> {
        let mut adapter = create_test_adapter();
        let caps = initialize_capabilities(&mut adapter)?;

        assert!(
            !capability(&caps, "supportsTerminateThreadsRequest")?,
            "supportsTerminateThreadsRequest must be false while handle_terminate_threads always fails"
        );

        match adapter.handle_request(
            2,
            "terminateThreads",
            Some(serde_json::json!({"threadIds": [1]})),
        ) {
            DapMessage::Response { success, command, message, .. } => {
                assert_eq!(command, "terminateThreads");
                assert!(
                    !success,
                    "terminateThreads is expected to fail; if it now succeeds, advertise it"
                );
                assert!(
                    message.is_some_and(|m| !m.is_empty()),
                    "an unsupported request must explain why it failed"
                );
            }
            other => anyhow::bail!("expected a response for terminateThreads, got {other:?}"),
        }
        Ok(())
    }

    /// #9069 fail-closed: `stepInTargets` mirrors the (now unadvertised)
    /// `dap.step_in_targets` catalog row, and every request is refused before
    /// any source read or target allocation because a client-selected target
    /// ID cannot influence the next native `stepIn`.
    #[tokio::test]
    async fn test_step_in_targets_is_not_advertised_and_fails_honestly() -> Result<()> {
        let mut adapter = create_test_adapter();
        let caps = initialize_capabilities(&mut adapter)?;

        assert_eq!(
            capability(&caps, "supportsStepInTargetsRequest")?,
            perl_dap::feature_catalog::has_feature("dap.step_in_targets"),
            "supportsStepInTargetsRequest must mirror the dap.step_in_targets catalog entry"
        );
        assert!(
            !capability(&caps, "supportsStepInTargetsRequest")?,
            "supportsStepInTargetsRequest must be false while targetId has no runtime effect (#9069)"
        );
        // Fail-closing stepInTargets must not disturb any other catalog row:
        // the data-breakpoint capability keeps mirroring its own (independently
        // fail-closed, #9091) watchpoints row in both directions.
        assert_eq!(
            capability(&caps, "supportsDataBreakpoints")?,
            perl_dap::feature_catalog::has_feature("dap.watchpoints"),
            "fail-closed targeted stepping must not alter the independent watchpoints row"
        );

        let dir = tempfile::tempdir()?;
        let script_path = dir.path().join("subroutine_calls.pl");
        fs::write(
            &script_path,
            "use strict;\nuse warnings;\nmy $x = abs(sqrt(length('hello')));\nprint $x;\n",
        )?;
        let source_path = script_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("temporary source path is not valid UTF-8"))?;
        adapter.seed_stopped_session_with_frames_for_test(vec![StackFrame::new(
            1,
            "main",
            Source::new(source_path),
            3,
        )]);

        match adapter.handle_request(2, "stepInTargets", Some(serde_json::json!({"frameId": 1}))) {
            DapMessage::Response { success, command, body, message, .. } => {
                assert_eq!(command, "stepInTargets");
                assert!(
                    !success,
                    "stepInTargets must fail while targeted stepping is unsupported (#9069)"
                );
                assert!(body.is_none(), "an unsupported stepInTargets must not publish target IDs");
                assert!(
                    message.is_some_and(|m| !m.is_empty()),
                    "an unsupported stepInTargets must explain why it failed"
                );
            }
            other => anyhow::bail!("expected a response for stepInTargets, got {other:?}"),
        }
        Ok(())
    }

    /// Every capability that used to be hardcoded `true` in the initialize response
    /// must now mirror its feature-catalog entry, in both directions.
    #[tokio::test]
    async fn test_previously_hardcoded_capabilities_mirror_the_catalog() -> Result<()> {
        let mut adapter = create_test_adapter();
        let caps = initialize_capabilities(&mut adapter)?;

        for (flag, feature_id) in [
            ("supportsRestartFrame", "dap.restart_frame"),
            ("supportsTerminateThreadsRequest", "dap.terminate_threads"),
            ("supportsStepInTargetsRequest", "dap.step_in_targets"),
            ("supportsRestartRequest", "dap.restart"),
            ("supportsLoadedSourcesRequest", "dap.loaded_sources"),
        ] {
            assert_eq!(
                capability(&caps, flag)?,
                perl_dap::feature_catalog::has_feature(feature_id),
                "`{flag}` must mirror the `{feature_id}` catalog entry, not a hardcoded literal"
            );
        }
        Ok(())
    }
}
