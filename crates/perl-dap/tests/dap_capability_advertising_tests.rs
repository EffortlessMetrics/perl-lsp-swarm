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
//!
//! Exception (#9581): the seven secondary-capability rows are explicit `false`
//! wire cells in every mode until each field's own exact-behavior receipt
//! passes — they are deliberately NOT derived from catalog registration, so a
//! catalog row advertising one of these families cannot widen the wire claim.

#[cfg(feature = "dap-phase2")]
mod capability_tests {
    use anyhow::Result;
    use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
    use serde_json::Value;
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

    /// `stepInTargets` has a working handler, so it stays advertised — but it is now
    /// gated on the catalog rather than hardcoded, so the flag cannot drift from
    /// `features.toml`.
    #[tokio::test]
    async fn test_step_in_targets_is_advertised_and_answers_successfully() -> Result<()> {
        let mut adapter = create_test_adapter();
        let caps = initialize_capabilities(&mut adapter)?;

        assert_eq!(
            capability(&caps, "supportsStepInTargetsRequest")?,
            perl_dap::feature_catalog::has_feature("dap.step_in_targets"),
            "supportsStepInTargetsRequest must mirror the dap.step_in_targets catalog entry"
        );

        match adapter.handle_request(2, "stepInTargets", Some(serde_json::json!({"frameId": 1}))) {
            DapMessage::Response { success, command, .. } => {
                assert_eq!(command, "stepInTargets");
                assert!(success, "stepInTargets is advertised, so it must succeed");
            }
            other => anyhow::bail!("expected a response for stepInTargets, got {other:?}"),
        }
        Ok(())
    }

    /// Every capability that used to be hardcoded `true` in the initialize response
    /// must now mirror its feature-catalog entry, in both directions — except the
    /// #9581 secondary-capability floor rows, which are explicit `false` wire
    /// cells independent of their catalog registration until each field's own
    /// exact-behavior receipt passes (#9581).
    #[tokio::test]
    async fn test_previously_hardcoded_capabilities_mirror_the_catalog() -> Result<()> {
        let mut adapter = create_test_adapter();
        let caps = initialize_capabilities(&mut adapter)?;

        for (flag, feature_id) in [
            ("supportsRestartFrame", "dap.restart_frame"),
            ("supportsTerminateThreadsRequest", "dap.terminate_threads"),
            ("supportsStepInTargetsRequest", "dap.step_in_targets"),
        ] {
            assert_eq!(
                capability(&caps, flag)?,
                perl_dap::feature_catalog::has_feature(feature_id),
                "`{flag}` must mirror the `{feature_id}` catalog entry, not a hardcoded literal"
            );
        }

        // #9581 floor rows: explicit `false` regardless of catalog registration;
        // re-enable is per field through its own gate, never via the catalog.
        for flag in ["supportsRestartRequest", "supportsLoadedSourcesRequest"] {
            assert!(
                !capability(&caps, flag)?,
                "`{flag}` must stay false under the #9581 secondary-capability floor"
            );
        }
        Ok(())
    }
}
