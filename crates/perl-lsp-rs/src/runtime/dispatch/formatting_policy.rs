//! Dispatch cutover for the shared formatting outcome policy.
//!
//! Slice 2 wires `textDocument/formatting` only. Range / multi-range / on-type
//! keep the legacy routing table until a follow-up slice reuses this path.

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

    if request.method != "textDocument/formatting" {
        return None;
    }

    let method = request.method.clone();
    let params = request.params.clone();
    let started = std::time::Instant::now();
    let result = server.handle_formatting_policy(params, id.as_ref());
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
    fn document_formatting_is_intercepted_after_initialize() {
        let server = LspServer::new();
        server.initialize_requested.store(true, std::sync::atomic::Ordering::Release);
        let (request, id) = formatting_request();
        assert!(
            route(&server, &request, Some(id), true).is_some(),
            "document formatting must route through the shared formatting policy"
        );
    }

    #[test]
    fn range_and_on_type_remain_on_legacy_dispatch_in_this_slice() {
        let server = LspServer::new();
        server.initialize_requested.store(true, std::sync::atomic::Ordering::Release);
        for method in [
            "textDocument/rangeFormatting",
            "textDocument/rangesFormatting",
            "textDocument/onTypeFormatting",
        ] {
            let id = JsonRpcId::Integer(1).to_value();
            let request = JsonRpcRequest {
                _jsonrpc: "2.0".to_string(),
                id: JsonRpcId::from_value(&id),
                method: method.to_string(),
                params: None,
            };
            assert!(
                route(&server, &request, Some(id), true).is_none(),
                "{method} must stay on the legacy table in the document-only slice"
            );
        }
    }

    #[test]
    fn handle_formatting_policy_call_presence_observer_routed_unadvertised()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        server.initialize_requested.store(true, std::sync::atomic::Ordering::Release);
        server.advertised_features.lock().formatting = false;
        server.advertised_feature_ids.lock().clear();

        let id = JsonRpcId::Integer(301).to_value();
        let request = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: JsonRpcId::from_value(&id),
            method: "textDocument/formatting".to_string(),
            params: None,
        };
        let routed = route(&server, &request, Some(id), true)
            .ok_or("formatting must be intercepted after initialize")?;
        let RoutedResponse::Handler { result, .. } = routed else {
            return Err("expected handler response".into());
        };
        let error = result.err().ok_or("expected method-not-advertised")?;
        assert_eq!(
            error.code, -32601,
            "input that reaches call self.ensure_surface_advertised(Surface::Document)"
        );
        Ok(())
    }

    #[test]
    fn handle_formatting_policy_call_presence_observer_routed_missing_params()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        server.initialize_requested.store(true, std::sync::atomic::Ordering::Release);
        server.advertised_features.lock().formatting = true;
        server
            .advertised_feature_ids
            .lock()
            .push(perl_lsp_rs_core::features::ids::LSP_FORMATTING);

        let id = JsonRpcId::Integer(302).to_value();
        let request = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: JsonRpcId::from_value(&id),
            method: "textDocument/formatting".to_string(),
            params: None,
        };
        let routed = route(&server, &request, Some(id), true)
            .ok_or("formatting must be intercepted after initialize")?;
        let RoutedResponse::Handler { result, .. } = routed else {
            return Err("expected handler response".into());
        };
        let error = result.err().ok_or("expected invalid params")?;
        assert_eq!(error.code, -32602);
        assert!(
            error.message.contains("Missing formatting parameters"),
            "input that reaches call params.ok_or_else(|| invalid_params(\"Missing formatting parameters\"))"
        );
        assert!(
            error.message.contains("Missing formatting parameters"),
            "input that reaches call invalid_params(\"Missing formatting parameters\")"
        );
        Ok(())
    }
}
