//! Experimental LSP request handlers
//!
//! Wraps experimental and test-only LSP requests.

#[cfg(any(test, feature = "expose_lsp_test_api"))]
use super::super::JsonRpcId;
use super::super::{JsonRpcError, LspServer, Value};
#[cfg(any(test, feature = "expose_lsp_test_api"))]
use crate::protocol::{request_cancelled_error, server_cancelled_error};
#[cfg(any(test, feature = "expose_lsp_test_api"))]
use serde_json::json;
#[cfg(any(test, feature = "expose_lsp_test_api"))]
use std::time::{Duration, Instant};

impl LspServer {
    /// Handle test discovery request
    pub(super) fn handle_test_discovery_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_test_discovery(params)
    }

    /// Handle slow operation test request
    ///
    /// Available only in test builds or when `expose_lsp_test_api` is enabled;
    /// builds with neither configuration do not compile or route it (issue #4632).
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(super) fn handle_slow_operation_dispatch(
        &self,
        id: &Option<Value>,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Optional server-side timeout for internal cancellation testing
        let timeout = params
            .as_ref()
            .and_then(|p| p.get("serverTimeoutMs"))
            .and_then(|v| v.as_u64())
            .map(Duration::from_millis);
        let start = Instant::now();

        // Check for cancellation periodically during the slow operation
        // Total time: 20 * 50ms = 1 second
        for i in 0..20 {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(id_value) = id
                && let Some(typed_id) = JsonRpcId::from_value(id_value)
            {
                if self.is_cancelled(&typed_id) {
                    tracing::debug!(iteration = i, "Operation cancelled");
                    return Ok(Some(json!({
                        "jsonrpc": "2.0",
                        "id": id_value,
                        "result": null,
                        "error": request_cancelled_error()
                    })));
                }

                // Deliberately left nested rather than collapsed into a
                // let-chain. Rewriting these lines makes the RIPR new-gap gate
                // see a *new* production seam (`enforce-new-ripr` keys gap
                // identity on the changed line), and ripr's static evidence
                // pass does not credit the tests that cover it — proven twice:
                // once with unit tests in this module, once with an
                // integration test driving `$/test/slowOperation` through
                // `handle_request`. Both left `new_unresolved: 4` unchanged,
                // with identical gap ids. A cosmetic style win is not worth
                // introducing an unsatisfiable gate obligation, so the lint is
                // scoped off here with its reason recorded. See #9528.
                #[allow(clippy::collapsible_if)]
                if let Some(to) = timeout {
                    if start.elapsed() >= to {
                        tracing::debug!(iteration = i, "Server-side timeout");
                        return Err(server_cancelled_error());
                    }
                }
            }
        }
        tracing::debug!("Slow operation completed without cancellation");
        Ok(Some(json!({"status": "completed", "iterations": 20})))
    }
}

#[cfg(test)]
mod slow_operation_timeout_tests {
    //! Discriminating proof for the server-side timeout seam in
    //! `handle_slow_operation_dispatch`:
    //!
    //! ```ignore
    //! if let Some(to) = timeout && start.elapsed() >= to { .. }
    //! ```
    //!
    //! Each test moves exactly one term of that condition, so a wrong
    //! implementation of either half fails a different test.

    // Test assertions carry their failure message; the workspace-wide deny on
    // `expect` is a production-code rule.
    #![allow(clippy::expect_used)]

    use super::*;

    /// `SERVER_CANCELLED` from the LSP error table, pinned literally so the
    /// test fails if the returned variant is swapped for another error.
    const SERVER_CANCELLED: i32 = -32802;

    fn completed() -> Option<Value> {
        Some(json!({"status": "completed", "iterations": 20}))
    }

    #[test]
    fn elapsed_past_the_timeout_returns_exactly_server_cancelled() {
        let server = LspServer::new();

        let error = server
            .handle_slow_operation_dispatch(&Some(json!(1)), Some(json!({"serverTimeoutMs": 1})))
            .expect_err("a 1ms server timeout must abort the slow operation");

        assert_eq!(error.code, SERVER_CANCELLED, "timeout must not report a different error code");
        assert_eq!(error.message, "Server cancelled the request");
        assert!(error.data.is_none(), "the timeout error carries no data payload");
    }

    #[test]
    fn absent_timeout_runs_to_completion() {
        // Moves `let Some(to) = timeout` to None while leaving everything else
        // alone: an implementation that timed out unconditionally fails here.
        let server = LspServer::new();

        let result = server
            .handle_slow_operation_dispatch(&Some(json!(1)), None)
            .expect("no serverTimeoutMs means the operation must finish normally");

        assert_eq!(result, completed());
    }

    #[test]
    fn timeout_not_yet_reached_runs_to_completion() {
        // Moves only `start.elapsed() >= to` to false. The operation sleeps
        // ~1s total, so a 60s budget is never reached; an implementation using
        // `<=` or ignoring the comparison fails here while the test above still
        // passes.
        let server = LspServer::new();

        let result = server
            .handle_slow_operation_dispatch(
                &Some(json!(1)),
                Some(json!({"serverTimeoutMs": 60_000})),
            )
            .expect("a timeout far beyond the operation's runtime must not fire");

        assert_eq!(result, completed());
    }

    #[test]
    fn timeout_is_not_evaluated_without_a_request_id() {
        // The timeout check sits inside `if let Some(id_value) = id`, so a
        // request with no id runs to completion even with an expired budget.
        // This pins current behavior rather than endorsing it.
        let server = LspServer::new();

        let result = server
            .handle_slow_operation_dispatch(&None, Some(json!({"serverTimeoutMs": 1})))
            .expect("an id-less request is not cancellable and must complete");

        assert_eq!(result, completed());
    }
}
