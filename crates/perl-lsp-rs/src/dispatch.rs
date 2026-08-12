//! Public JSON-RPC envelope compatibility facade.
//!
//! This module does **not** expose the server's routing engine. Runtime method
//! selection, lifecycle checks, cancellation, and response finalization are
//! internal implementation details owned under `runtime::dispatch` and reached
//! through [`crate::LspServer::handle_request`].
//!
//! The module remains public for one narrow compatibility purpose: downstream
//! code that historically imported JSON-RPC envelope types from
//! `perl_lsp::dispatch` can keep that import path while moving to the canonical
//! [`crate::protocol`] namespace. Both paths name the same types; no adapter or
//! alternate dispatch implementation is created here.
//!
//! New code should import these types from [`crate::protocol`] or from the
//! crate-root re-exports.

#[doc(inline)]
pub use crate::protocol::{JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse};

#[cfg(test)]
mod tests {
    use super::*;

    fn accepts_canonical_request(request: crate::protocol::JsonRpcRequest) -> JsonRpcRequest {
        request
    }

    fn accepts_canonical_response(response: crate::protocol::JsonRpcResponse) -> JsonRpcResponse {
        response
    }

    #[test]
    fn facade_reexports_canonical_envelope_types() {
        let request = crate::protocol::JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: Some(crate::protocol::JsonRpcId::Integer(1)),
            method: "initialize".to_string(),
            params: None,
        };
        let response = crate::protocol::JsonRpcResponse {
            jsonrpc: "2.0",
            id: Some(crate::protocol::JsonRpcId::Integer(1)),
            result: Some(serde_json::Value::Null),
            error: None,
        };

        let _: crate::protocol::JsonRpcRequest = accepts_canonical_request(request);
        let _: crate::protocol::JsonRpcResponse = accepts_canonical_response(response);
    }

    #[test]
    fn public_docs_do_not_point_to_stale_dispatch_paths() {
        let source = include_str!("dispatch.rs");
        for stale in [
            "server_impl/dispatch.rs",
            "crate::lsp::server_impl::dispatch",
            "Intentionally empty",
            "Request dispatch placeholder",
        ] {
            assert!(
                !source.contains(stale),
                "public dispatch documentation must not retain stale marker `{stale}`"
            );
        }
        assert!(source.contains("runtime::dispatch"));
        assert!(source.contains("crate::protocol"));
    }
}
