//! OpenAI-compatible completion provider.

use super::prompt::build_fim_prompt;
use super::rate_limiter::RateLimiter;
use super::sse::SseParser;
use super::web_connector::{
    WebAiProtocol, WebAiRequestParts, content_delta_from_sse_data, finish_reason_from_sse_data,
};
use crate::providers::inline_completion::{
    BackendError, BackendRequest, InlineCompletionBackend, StreamChunk, StreamControl,
};
use std::io::BufReader;
use std::sync::Arc;

/// Configuration for the OpenAI-compatible provider.
#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    /// The API endpoint URL (e.g. `https://api.openai.com/v1/chat/completions`).
    pub endpoint: String,
    /// The model name to use (e.g. `gpt-4o`).
    pub model: String,
    /// API key for authentication.
    pub api_key: String,
    /// Global timeout in milliseconds.
    pub timeout_ms: u64,
}

/// An OpenAI-compatible completion provider using ureq for HTTP.
pub struct OpenAiProvider {
    config: OpenAiConfig,
    limiter: Arc<RateLimiter>,
}

impl OpenAiProvider {
    /// Create a new provider with the given config and rate limiter.
    pub fn new(config: OpenAiConfig, limiter: Arc<RateLimiter>) -> Self {
        Self { config, limiter }
    }

    fn build_request_body(&self, req: &BackendRequest) -> serde_json::Value {
        let (system, user) = build_fim_prompt(&req.context);
        let parts = WebAiRequestParts {
            model: self.config.model.clone(),
            system,
            user,
            max_output_tokens: req.max_output_tokens,
            stream: true,
        };

        parts.to_json_body(self.protocol())
    }

    fn protocol(&self) -> WebAiProtocol {
        WebAiProtocol::infer_from_endpoint(&self.config.endpoint)
    }

    fn extract_content_delta(data: &str) -> Option<String> {
        content_delta_from_sse_data(data)
    }

    fn extract_finish_reason(data: &str) -> Option<String> {
        finish_reason_from_sse_data(data)
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenAiConfig, OpenAiProvider};
    use crate::providers::ai::rate_limiter::RateLimiter;
    use crate::providers::ai::web_connector::WebAiProtocol;
    use std::sync::Arc;

    fn provider_with_endpoint(endpoint: &str) -> OpenAiProvider {
        OpenAiProvider::new(
            OpenAiConfig {
                endpoint: endpoint.to_string(),
                model: "gpt-4o-mini".to_string(),
                api_key: "test-key".to_string(),
                timeout_ms: 1000,
            },
            Arc::new(RateLimiter::new(1.0, 1)),
        )
    }

    #[test]
    fn detects_responses_api_endpoint() {
        let provider = provider_with_endpoint("https://api.openai.com/v1/responses");
        assert_eq!(provider.protocol(), WebAiProtocol::OpenAiResponses);

        let provider = provider_with_endpoint("https://api.openai.com/v1/chat/completions");
        assert_eq!(provider.protocol(), WebAiProtocol::OpenAiChatCompletions);
    }

    #[test]
    fn extracts_chat_completions_delta() {
        let data = r#"{"choices":[{"delta":{"content":"my $"},"finish_reason":null}]}"#;
        assert_eq!(OpenAiProvider::extract_content_delta(data), Some("my $".to_string()));
    }

    #[test]
    fn extracts_responses_delta() {
        let data = r#"{"type":"response.output_text.delta","delta":"my $"}"#;
        assert_eq!(OpenAiProvider::extract_content_delta(data), Some("my $".to_string()));
    }

    #[test]
    fn detects_responses_completion_event() {
        let data = r#"{"type":"response.completed"}"#;
        assert_eq!(OpenAiProvider::extract_finish_reason(data), Some("stop".to_string()));
    }
}

impl InlineCompletionBackend for OpenAiProvider {
    fn stream(
        &self,
        req: &BackendRequest,
        sink: &mut dyn FnMut(StreamChunk) -> StreamControl,
    ) -> Result<(), BackendError> {
        if !self.limiter.try_acquire() {
            return Err(BackendError::RateLimited);
        }

        let body = self.build_request_body(req);
        let timeout = std::time::Duration::from_millis(req.timeout_ms);

        let config = ureq::Agent::config_builder().timeout_global(Some(timeout)).build();
        let agent = ureq::Agent::new_with_config(config);

        let response = agent
            .post(&self.config.endpoint)
            .header("Authorization", &format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("timed out") || msg.contains("timeout") {
                    BackendError::Timeout
                } else if msg.contains("401") || msg.contains("403") {
                    BackendError::Auth(msg)
                } else {
                    BackendError::Transport(msg)
                }
            })?;

        let reader = BufReader::new(response.into_body().into_reader());
        let mut parser = SseParser::new(reader);
        let mut cumulative = String::new();

        loop {
            match parser.next_event() {
                Ok(Some(event)) => {
                    if let Some(delta) = Self::extract_content_delta(&event.data) {
                        cumulative.push_str(&delta);

                        let is_final = Self::extract_finish_reason(&event.data)
                            .is_some_and(|r| r == "stop" || r == "length");

                        let control = sink(StreamChunk { text: cumulative.clone(), is_final });

                        if control == StreamControl::Stop || is_final {
                            break;
                        }
                    }
                }
                Ok(None) => {
                    // Stream ended -- emit final chunk if we have content
                    if !cumulative.is_empty() {
                        sink(StreamChunk { text: cumulative, is_final: true });
                    }
                    break;
                }
                Err(e) => {
                    return Err(BackendError::Transport(e.to_string()));
                }
            }
        }

        Ok(())
    }
}
