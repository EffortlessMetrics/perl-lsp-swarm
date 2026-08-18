//! Public JSON-RPC envelope compatibility facade.
//!
//! This module does **not** expose the server's routing engine. Runtime method
//! selection, lifecycle checks, cancellation, and response finalization are
//! internal implementation details owned under `runtime::dispatch` and reached
//! through [`crate::LspServer::handle_request`].
//!
//! The module is public as an envelope-only facade for the existing
//! `perl_lsp::dispatch` path while the canonical definitions remain in
//! [`crate::protocol`]. Both paths name the same types; no adapter, behavior,
//! or alternate dispatch implementation is created here.
//!
//! New code should normally import these types from [`crate::protocol`] or from
//! the crate-root re-exports.

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
    fn architecture_docs_define_only_the_envelope_facade() {
        let architecture_docs = include_str!("lib.rs");
        for stale_or_overclaim in [
            "server_impl/dispatch.rs",
            "crate::lsp::server_impl::dispatch",
            "Intentionally empty",
            "Request dispatch placeholder",
            "historically imported",
            "Request routing and method dispatch logic",
        ] {
            assert!(
                !architecture_docs.contains(stale_or_overclaim),
                "architecture documentation must not retain stale or unproved marker \
                 `{stale_or_overclaim}`"
            );
        }
        assert!(architecture_docs.contains("Public JSON-RPC envelope compatibility facade"));
        assert!(architecture_docs.contains("runtime::dispatch"));
    }
}
