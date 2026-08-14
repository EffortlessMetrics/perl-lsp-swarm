//! Dispatch cutover for the shared formatting outcome policy.
//!
//! The legacy routing table retains its formatting arms as a bounded rollback
//! seam, but live requests are intercepted here after preflight and before that
//! table so all four formatting surfaces receive the same request identity.

use super::super::{JsonRpcRequest, LspServer, Value};
use super::response::RoutedResponse;

pub(super) fn route(
    server: &LspServer,
    request: &JsonRpcRequest,
    id: Option<Value>,
    should_respond: bool,
) -> Option<RoutedResponse> {
    if !server.initialize_requested.load(std::sync::atomic::Ordering::Acquire)
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
    fn formatting_is_not_intercepted_outside_the_live_lifecycle() {
        let server = LspServer::new();
        let (request, id) = formatting_request();
        assert!(
            route(&server, &request, Some(id.clone()), true).is_none(),
            "formatting must not be intercepted before initialize"
        );

        server.initialize_requested.store(true, std::sync::atomic::Ordering::Release);
        server.shutdown_received.store(true, std::sync::atomic::Ordering::Release);
        assert!(
            route(&server, &request, Some(id), true).is_none(),
            "formatting must not be intercepted after shutdown"
        );
    }

    #[test]
    fn every_formatting_method_is_intercepted_after_initialize(
    ) -> Result<(), Box<dyn std::error::Error>> {
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
            server.initialize_requested.store(true, std::sync::atomic::Ordering::Release);
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
