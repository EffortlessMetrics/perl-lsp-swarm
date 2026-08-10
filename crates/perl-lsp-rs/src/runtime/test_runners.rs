//! Test discovery and execution helpers.
//!
//! `run_test` and `run_test_file` are used by the `perl/runTest` and
//! `perl/runTestFile` custom LSP commands.

use super::{JsonRpcError, LspServer, TestRunner, Value, document_not_found_error, json};

impl LspServer {
    /// Run an operation with a cloned document snapshot.
    ///
    /// The documents map is locked only while cloning the text. Test
    /// execution can spawn a subprocess, so keeping the map guard alive
    /// across the operation would block unrelated LSP requests.
    fn with_document_snapshot<T>(
        &self,
        uri: &str,
        operation: impl FnOnce(String) -> T,
    ) -> Option<T> {
        let document_text = {
            let documents = self.documents_guard();
            documents.get(uri).map(|doc| doc.text_arc.to_string())
        };
        document_text.map(operation)
    }

    /// Run a specific test
    pub(crate) fn run_test(&self, test_id: &str) -> Result<Option<Value>, JsonRpcError> {
        tracing::debug!(test_id, "Running test");

        // Parse test ID to get URI and test name. The legacy runner only
        // supports file-level execution, so preserve the URI as its input;
        // passing the bare test name would make it resolve a cwd-relative
        // file unrelated to the requested document.
        let Some((uri, _test_name)) = test_id.split_once("::") else {
            return Ok(Some(json!({"status": "error", "message": "Invalid test ID"})));
        };

        if let Some(results) = self.with_document_snapshot(uri, |document_text| {
            let runner = TestRunner::new(document_text, uri.to_string());
            runner.run_test(uri)
        }) {
            // Convert results to JSON
            let json_results: Vec<Value> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "testId": result.test_id,
                        "status": result.status.as_str(),
                        "message": result.message,
                        "duration": result.duration
                    })
                })
                .collect();

            return Ok(Some(json!({
                "status": "success",
                "results": json_results
            })));
        }

        Ok(Some(document_not_found_error()))
    }

    /// Run a named subtest — not yet implemented server-side.
    ///
    /// Returns an error rather than fabricating success, so the client does
    /// not silently believe the subtest ran (#4972).
    pub(crate) fn run_subtest(&self, subtest_name: &str) -> Result<Option<Value>, JsonRpcError> {
        Err(JsonRpcError::new(
            crate::protocol::METHOD_NOT_FOUND,
            format!("runSubtest '{subtest_name}' is not implemented server-side"),
        ))
    }

    /// Debug a specific test — not yet implemented server-side.
    ///
    /// Returns an error rather than fabricating success, so the client does
    /// not silently believe the debug session started (#4972).
    pub(crate) fn debug_test(&self, test_id: &str) -> Result<Option<Value>, JsonRpcError> {
        Err(JsonRpcError::new(
            crate::protocol::METHOD_NOT_FOUND,
            format!("debugTest '{test_id}' is not implemented server-side"),
        ))
    }

    /// Run all tests in a file
    pub(crate) fn run_test_file(&self, uri: &str) -> Result<Option<Value>, JsonRpcError> {
        tracing::debug!(uri, "Running test file");

        if let Some(results) = self.with_document_snapshot(uri, |document_text| {
            let runner = TestRunner::new(document_text, uri.to_string());
            runner.run_test(uri)
        }) {
            // Convert results to JSON
            let json_results: Vec<Value> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "testId": result.test_id,
                        "status": result.status.as_str(),
                        "message": result.message,
                        "duration": result.duration
                    })
                })
                .collect();

            return Ok(Some(json!({
                "status": "success",
                "results": json_results
            })));
        }

        Ok(Some(document_not_found_error()))
    }
}

#[cfg(test)]
mod tests {
    use super::LspServer;
    use std::sync::Arc;

    #[test]
    fn document_snapshot_releases_lock_before_operation() -> Result<(), Box<dyn std::error::Error>>
    {
        let server = Arc::new(LspServer::new());
        let uri = "file:///document-snapshot.pl";
        server.test_apply_did_open(uri, "sub test { 1 }\n", 1)?;

        let operation_server = Arc::clone(&server);
        let observed = server
            .with_document_snapshot(uri, move |text| {
                assert_eq!(text, "sub test { 1 }\n");
                operation_server.documents.try_lock().is_some()
            })
            .ok_or("expected an open document snapshot")?;

        assert!(observed, "documents lock must be available to the execution operation");
        Ok(())
    }
}
