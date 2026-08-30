//! Dispatch cutover for the shared formatting outcome policy.
//!
//! Live requests are intercepted here after preflight so all four formatting
//! surfaces reach one policy owner. The secondary routes are currently
//! withdrawn, and the policy returns the truthful method-not-advertised refusal;
//! restoration belongs to their named follow-up claims rather than to a
//! duplicate legacy handler.

use super::super::{JsonRpcRequest, LspServer, Value};
use super::response::RoutedResponse;

pub(super) fn route(
    server: &LspServer,
    request: &JsonRpcRequest,
    id: Option<Value>,
    should_respond: bool,
) -> Option<RoutedResponse> {
    // Fail-closed intercept gate on the single accepted-contract authority
    // (review 5061915323): formatting is intercepted only once initialize has
    // ACCEPTED the text-sync session contract. A consumed one-shot guard
    // without acceptance falls through here so the router's -32002 arm owns
    // the refusal; after shutdown the intercept must also stand down.
    if !server.initialization_accepted()
        || server.shutdown_received.load(std::sync::atomic::Ordering::Acquire)
    {
        return None;
    }

    let method = request.method.clone();
    let params = request.params.clone();
    let started = std::time::Instant::now();
    let result = match method.as_str() {
        "textDocument/formatting" => server.handle_formatting_policy(params, id.as_ref()),
        "textDocument/rangeFormatting" => {
            server.handle_range_formatting_policy(params, id.as_ref())
        }
        "textDocument/rangesFormatting" => {
            server.handle_ranges_formatting_policy(params, id.as_ref())
        }
        "textDocument/onTypeFormatting" => {
            server.handle_on_type_formatting_policy(params, id.as_ref())
        }
        _ => return None,
    };

    server.record_lsp_request_latency(&method, started);
    Some(RoutedResponse::Handler { id, method, should_respond, result })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::JsonRpcId;

    fn formatting_request() -> (JsonRpcRequest, Value) {
        let id = JsonRpcId::Integer(70820).to_value();
        (
            JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: JsonRpcId::from_value(&id),
                method: "textDocument/formatting".to_string(),
                params: None,
            },
            id,
        )
    }

    #[test]
    fn formatting_is_not_intercepted_outside_the_live_lifecycle()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let (request, id) = formatting_request();
        assert!(
            route(&server, &request, Some(id.clone()), true).is_none(),
            "formatting must not be intercepted before initialize"
        );

        // Real initialize so the accepted contract exists; the shutdown flag
        // alone must then stand the intercept down.
        server
            .handle_initialize(None)
            .map_err(|error| std::io::Error::other(format!("initialize failed: {error:?}")))?;
        server.shutdown_received.store(true, std::sync::atomic::Ordering::Release);
        assert!(
            route(&server, &request, Some(id), true).is_none(),
            "formatting must not be intercepted after shutdown"
        );
        Ok(())
    }

    /// Review 5061915323: the consumed-guard/failed-acceptance window (one-shot
    /// initialize guard set, no accepted text-sync session) must not be
    /// intercepted — the request falls through to the router's fail-closed
    /// -32002 arm instead of being served end-to-end.
    #[test]
    fn consumed_guard_without_accepted_contract_is_not_intercepted() {
        let server = LspServer::new();
        server.initialize_requested.store(true, std::sync::atomic::Ordering::Release);
        assert!(
            server.accepted_text_sync_session().is_none(),
            "constructed window state must have no accepted contract"
        );

        let (request, id) = formatting_request();
        assert!(
            route(&server, &request, Some(id), true).is_none(),
            "the window state must fall through to the router's -32002 arm"
        );
    }

    #[test]
    fn every_formatting_method_is_intercepted_after_initialize()
    -> Result<(), Box<dyn std::error::Error>> {
        for (offset, method) in [
            "textDocument/formatting",
            "textDocument/rangeFormatting",
            "textDocument/rangesFormatting",
            "textDocument/onTypeFormatting",
        ]
        .into_iter()
        .enumerate()
        {
            let server = LspServer::new();
            // Real initialize so the accepted contract admits the intercept.
            server
                .handle_initialize(None)
                .map_err(|error| std::io::Error::other(format!("initialize failed: {error:?}")))?;
            let id = JsonRpcId::Integer(70820 + offset as i64).to_value();
            let request = JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: JsonRpcId::from_value(&id),
                method: method.to_string(),
                params: None,
            };

            if route(&server, &request, Some(id), true).is_none() {
                return Err(std::io::Error::other(format!(
                    "{method} must route through the shared formatting policy"
                ))
                .into());
            }
        }

        Ok(())
    }
}
