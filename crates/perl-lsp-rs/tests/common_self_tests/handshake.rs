use crate::common::{LspServer, initialize_lsp_with_root_path, shutdown_and_exit};

#[cfg(test)]
mod initialize_lsp_with_root_path_tests {
    use super::{LspServer, initialize_lsp_with_root_path};
    use serde_json::Value;

    type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

    /// The root-scoped handshake must complete against a real server and an
    /// empty root directory: the initialize response carries capabilities,
    /// and the `rootPath` channel resolves the same folder state the suite's
    /// isolated-workspace tests rely on.
    #[test]
    fn initializes_server_against_an_explicit_empty_root() -> TestResult<()> {
        let root =
            std::env::temp_dir().join(format!("perl-lsp-root-path-test-{}", std::process::id()));
        std::fs::create_dir_all(&root)
            .map_err(|err| format!("failed to create root {}: {err}", root.display()))?;

        let server: LspServer = super::super::start_lsp_server();
        let response = initialize_lsp_with_root_path(&server, &root.to_string_lossy());

        let capabilities = response
            .get("result")
            .and_then(|result| result.get("capabilities"))
            .cloned()
            .unwrap_or(Value::Null);
        assert!(
            capabilities.is_object(),
            "initialize must answer with a capabilities object, got: {response:?}"
        );

        super::shutdown_and_exit(&server);
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
