//! AST explorer handler for the `perl/showAst` custom request.
//!
//! The VSCode extension's `perl-lsp.showParserAst` command sends this request
//! with `{ uri: "file:///path/to/file.pl" }` and expects either:
//!
//! - A JSON string containing the S-expression representation of the AST, or
//! - A JSON `null` value when the document has not been parsed yet (e.g. on
//!   `didOpen` before the async parse completes), or
//! - A JSON-RPC error when the document is not open in the server.
//!
//! # Response format
//!
//! The S-expression is produced by `perl_ast::Node::to_sexp()` and looks like:
//!
//! ```text
//! (source_file (use_statement ...) (subroutine name: foo ...))
//! ```
//!
//! The client displays this verbatim in a VSCode `OutputChannel`.

use super::super::{INVALID_PARAMS, JsonRpcError, LspServer, Value, json};

impl LspServer {
    /// Handle the `perl/showAst` custom request.
    ///
    /// # Parameters (JSON object)
    /// - `uri` (**required**): The document URI string, e.g. `"file:///foo.pl"`.
    ///
    /// # Returns
    /// - `Ok(Some(Value::String(...)))` — the AST as an S-expression string.
    /// - `Ok(Some(Value::Null))` — document is open but has no AST yet.
    /// - `Err(INVALID_PARAMS)` — `uri` param is missing or document is not open.
    pub(super) fn handle_show_ast_dispatch(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Distinguish an absent request params object from an object missing its URI.
        let params = params.ok_or_else(|| JsonRpcError {
            code: INVALID_PARAMS,
            message: "perl/showAst: missing required parameter 'params'".to_string(),
            data: None,
        })?;
        let uri = params.get("uri").and_then(|u| u.as_str()).ok_or_else(|| JsonRpcError {
            code: INVALID_PARAMS,
            message: "perl/showAst: missing required parameter 'uri'".to_string(),
            data: None,
        })?;

        let docs = self.documents_guard();

        match docs.get(uri) {
            Some(state) => {
                match state.current_parsed().and_then(|p| p.ast().cloned()) {
                    Some(ast) => {
                        // Serialize the AST to S-expression format
                        let sexp = ast.to_sexp();
                        Ok(Some(json!(sexp)))
                    }
                    None => {
                        // Document is open but has no AST yet (parse in progress,
                        // or parse failed entirely). Return null so the client can
                        // show "No AST available" gracefully.
                        Ok(Some(Value::Null))
                    }
                }
            }
            None => Err(JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("Document not found: {uri}"),
                data: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    // Test assertions favor `expect_err()` with a descriptive message over
    // silent unwraps; the workspace-wide deny is a production-code rule.
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn show_ast_missing_params_names_method_and_field() {
        let err = LspServer::new()
            .handle_show_ast_dispatch(None)
            .expect_err("missing showAst params must be rejected");

        assert_eq!(err.code, INVALID_PARAMS);
        assert_eq!(err.message, "perl/showAst: missing required parameter 'params'");
    }

    #[test]
    fn show_ast_missing_uri_names_method_and_field() {
        let err = LspServer::new()
            .handle_show_ast_dispatch(Some(json!({})))
            .expect_err("missing showAst URI must be rejected");

        assert_eq!(err.code, INVALID_PARAMS);
        assert_eq!(err.message, "perl/showAst: missing required parameter 'uri'");
    }
}
