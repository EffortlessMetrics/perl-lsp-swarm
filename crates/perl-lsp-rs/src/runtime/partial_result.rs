//! LSP partial-result streaming (`partialResultToken` + `$/progress`).
//!
//! Transport-only helper for standard methods that return JSON arrays. When the
//! client omits `partialResultToken`, the one-shot response is preserved.

use super::{JsonRpcError, LspServer, Value, json};

/// Chunk size aligned with workspace/symbol cooperative yield boundaries.
const PARTIAL_RESULT_CHUNK_SIZE: usize = 64;

impl LspServer {
    pub(crate) fn partial_result_token_from_params(params: Option<&Value>) -> Option<Value> {
        params.and_then(|p| p.get("partialResultToken")).filter(|token| !token.is_null()).cloned()
    }

    /// When `partialResultToken` is present, emit the array via `$/progress` and
    /// return a null final response. Otherwise return `result` unchanged.
    pub(crate) fn finish_lsp_partial_result_array(
        &self,
        params: Option<&Value>,
        result: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let Some(token) = Self::partial_result_token_from_params(params) else {
            return Ok(result);
        };

        if let Some(Value::Array(items)) = result {
            for chunk in items.chunks(PARTIAL_RESULT_CHUNK_SIZE) {
                let progress = json!({
                    "token": token,
                    "value": chunk,
                });
                if let Err(err) = self.notify("$/progress", progress) {
                    tracing::debug!(
                        error = %err,
                        "partial result notify failed; continuing with remaining chunks"
                    );
                }
            }
        }

        Ok(Some(json!(null)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn without_partial_result_token_preserves_one_shot_response()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let payload = Some(json!([{ "name": "test", "kind": 12 }]));
        let response = server.finish_lsp_partial_result_array(None, payload.clone())?;
        assert_eq!(response, payload);
        Ok(())
    }

    #[test]
    fn with_partial_result_token_returns_null_final_response()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let params = json!({ "partialResultToken": "partial-token" });
        let response = server.finish_lsp_partial_result_array(
            Some(&params),
            Some(json!([{ "name": "test", "kind": 12 }])),
        )?;
        assert_eq!(response, Some(json!(null)));
        Ok(())
    }
}
