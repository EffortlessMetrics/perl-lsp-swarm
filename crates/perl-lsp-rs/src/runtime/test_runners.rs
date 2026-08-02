//! Test discovery and execution helpers.
//!
//! `run_test` and `run_test_file` are used by the `perl/runTest` and
//! `perl/runTestFile` custom LSP commands.

use super::*;

impl LspServer {
    /// Run a specific test
    pub(crate) fn run_test(&self, test_id: &str) -> Result<Option<Value>, JsonRpcError> {
        tracing::debug!(test_id, "Running test");

        // Parse test ID to get URI and test name
        let parts: Vec<&str> = test_id.split("::").collect();
        if parts.len() < 2 {
            return Ok(Some(json!({"status": "error", "message": "Invalid test ID"})));
        }

        let uri = parts[0];
        let test_name = parts[1..].join("::");

        let documents = self.documents.lock();
        if let Some(doc) = documents.get(uri) {
            let runner = TestRunner::new(doc.text.clone(), uri.to_string());
            let results = runner.run_test(&test_name);

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

        let documents = self.documents.lock();
        if let Some(doc) = documents.get(uri) {
            let runner = TestRunner::new(doc.text.clone(), uri.to_string());
            let results = runner.run_test(uri);

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
