//! HTTP connector primitives for web-based AI providers.
//!
//! The inline-completion providers keep provider-specific prompt and response
//! parsing in their own modules, while this module owns the minimal HTTP
//! boundary needed by hosted AI services. Tests and future connectors can swap
//! the transport without reimplementing provider logic.

use crate::providers::inline_completion::BackendError;
use std::io::Read;

/// JSON POST request sent to a web AI endpoint.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WebAiRequest {
    /// Fully-qualified endpoint URL.
    pub endpoint: String,
    /// Bearer token value for the `Authorization` header.
    pub bearer_token: String,
    /// JSON body to send.
    pub body: serde_json::Value,
    /// Global request timeout in milliseconds.
    pub timeout_ms: u64,
}

/// Response returned by a web AI endpoint.
#[non_exhaustive]
pub struct WebAiResponse {
    /// Response content type, when the server provided one.
    pub content_type: Option<String>,
    /// Response body stream.
    pub body: Box<dyn Read + Send>,
}

/// Transport boundary for hosted AI connector implementations.
pub trait WebAiConnector: Send + Sync {
    /// Send a JSON POST request and return the response body stream.
    fn post_json(&self, request: WebAiRequest) -> Result<WebAiResponse, BackendError>;
}

/// Default HTTP connector backed by `ureq`.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct UreqWebAiConnector;

impl WebAiConnector for UreqWebAiConnector {
    fn post_json(&self, request: WebAiRequest) -> Result<WebAiResponse, BackendError> {
        let timeout = std::time::Duration::from_millis(request.timeout_ms);
        let config = ureq::Agent::config_builder().timeout_global(Some(timeout)).build();
        let agent = ureq::Agent::new_with_config(config);

        let response = agent
            .post(&request.endpoint)
            .header("Authorization", &format!("Bearer {}", request.bearer_token))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream, application/json")
            .send_json(&request.body)
            .map_err(map_ureq_error)?;

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        Ok(WebAiResponse { content_type, body: Box::new(response.into_body().into_reader()) })
    }
}

fn map_ureq_error(error: ureq::Error) -> BackendError {
    let msg = error.to_string();
    if msg.contains("timed out") || msg.contains("timeout") {
        BackendError::Timeout
    } else if msg.contains("401") || msg.contains("403") {
        BackendError::Auth(msg)
    } else {
        BackendError::Transport(msg)
    }
}
