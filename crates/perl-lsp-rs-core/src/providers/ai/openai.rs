//! OpenAI-compatible completion provider.

use super::prompt::build_fim_prompt;
use super::rate_limiter::RateLimiter;
use super::sse::SseParser;
use super::web::{UreqWebAiConnector, WebAiConnector, WebAiRequest, WebAiResponse};
use crate::providers::inline_completion::{
    BackendError, BackendRequest, InlineCompletionBackend, StreamChunk, StreamControl,
};
use std::io::{BufReader, Read};
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
    connector: Arc<dyn WebAiConnector>,
}

impl OpenAiProvider {
    /// Create a new provider with the given config and rate limiter.
    pub fn new(config: OpenAiConfig, limiter: Arc<RateLimiter>) -> Self {
        Self { config, limiter, connector: Arc::new(UreqWebAiConnector) }
    }

    /// Create a provider with an injected web connector.
    ///
    /// This is used by tests and by future hosted AI connectors that need to
    /// customize transport behavior while preserving OpenAI-compatible prompt
    /// and response handling.
    pub fn new_with_connector(
        config: OpenAiConfig,
        limiter: Arc<RateLimiter>,
        connector: Arc<dyn WebAiConnector>,
    ) -> Self {
        Self { config, limiter, connector }
    }

    fn build_request_body(&self, req: &BackendRequest) -> serde_json::Value {
        let (system, user) = build_fim_prompt(&req.context);

        if self.uses_responses_api() {
            return serde_json::json!({
                "model": self.config.model,
                "max_output_tokens": req.max_output_tokens,
                "stream": true,
                "instructions": system,
                "input": user,
            });
        }

        serde_json::json!({
            "model": self.config.model,
            "max_tokens": req.max_output_tokens,
            "stream": true,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        })
    }

    fn uses_responses_api(&self) -> bool {
        self.config.endpoint.contains("/responses")
    }

    fn extract_content_delta(data: &str) -> Option<String> {
        let parsed: serde_json::Value = serde_json::from_str(data).ok()?;

        if let Some(delta) = parsed
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(serde_json::Value::as_str)
        {
            return Some(delta.to_string());
        }

        let event_type = parsed.get("type").and_then(serde_json::Value::as_str)?;
        if event_type != "response.output_text.delta" {
            return None;
        }

        parsed.get("delta").and_then(serde_json::Value::as_str).map(str::to_string)
    }

    fn extract_finish_reason(data: &str) -> Option<String> {
        let parsed: serde_json::Value = serde_json::from_str(data).ok()?;

        if let Some(reason) = parsed
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(serde_json::Value::as_str)
        {
            return Some(reason.to_string());
        }

        match parsed.get("type").and_then(serde_json::Value::as_str) {
            Some("response.completed") => Some("stop".to_string()),
            Some("response.failed") | Some("response.incomplete") => Some("error".to_string()),
            _ => None,
        }
    }

    fn extract_complete_text(data: &str) -> Option<String> {
        let parsed: serde_json::Value = serde_json::from_str(data).ok()?;

        if let Some(content) = parsed
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str)
        {
            return Some(content.to_string());
        }

        if let Some(text) = parsed.get("output_text").and_then(serde_json::Value::as_str) {
            return Some(text.to_string());
        }

        parsed
            .get("output")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("content").and_then(serde_json::Value::as_array))
            .flatten()
            .filter_map(|content| content.get("text").and_then(serde_json::Value::as_str))
            .next()
            .map(str::to_string)
    }

    fn response_is_json(response: &WebAiResponse) -> bool {
        response.content_type.as_deref().is_some_and(|content_type| {
            content_type.to_ascii_lowercase().contains("application/json")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenAiConfig, OpenAiProvider};
    use crate::providers::ai::rate_limiter::RateLimiter;
    use crate::providers::ai::web::{WebAiConnector, WebAiRequest, WebAiResponse};
    use crate::providers::inline_completion::{
        BackendError, BackendRequest, InlineCompletionBackend, PreparedInlineCompletionContext,
        StreamControl,
    };
    use std::io::Cursor;
    use std::sync::Arc;

    struct StaticJsonConnector {
        body: String,
    }

    impl WebAiConnector for StaticJsonConnector {
        fn post_json(&self, _request: WebAiRequest) -> Result<WebAiResponse, BackendError> {
            Ok(WebAiResponse {
                content_type: Some("application/json".to_string()),
                body: Box::new(Cursor::new(self.body.clone().into_bytes())),
            })
        }
    }

    fn backend_request() -> BackendRequest {
        BackendRequest {
            context: PreparedInlineCompletionContext {
                prefix: "my $x = ".to_string(),
                current_line: "my $x = ".to_string(),
                previous_non_empty_line: Some("use strict;".to_string()),
                current_function: Some("example".to_string()),
                current_package: Some("My::Package".to_string()),
                variables: vec!["$self".to_string()],
                imports: vec!["strict".to_string()],
            },
            max_output_tokens: 32,
            timeout_ms: 1000,
        }
    }

    fn provider_with_json_response(body: &str) -> OpenAiProvider {
        OpenAiProvider::new_with_connector(
            OpenAiConfig {
                endpoint: "https://example.test/v1/chat/completions".to_string(),
                model: "test-model".to_string(),
                api_key: "test-key".to_string(),
                timeout_ms: 1000,
            },
            Arc::new(RateLimiter::new(1.0, 1)),
            Arc::new(StaticJsonConnector { body: body.to_string() }),
        )
    }

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
        assert!(provider.uses_responses_api());

        let provider = provider_with_endpoint("https://api.openai.com/v1/chat/completions");
        assert!(!provider.uses_responses_api());
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

    #[test]
    fn streams_non_sse_chat_completion_json() -> Result<(), Box<dyn std::error::Error>> {
        let provider = provider_with_json_response(
            r#"{"choices":[{"message":{"content":"$self->render"},"finish_reason":"stop"}]}"#,
        );
        let mut chunks = Vec::new();
        provider.stream(&backend_request(), &mut |chunk| {
            chunks.push((chunk.text, chunk.is_final));
            StreamControl::Continue
        })?;

        assert_eq!(chunks, vec![("$self->render".to_string(), true)]);
        Ok(())
    }

    #[test]
    fn extracts_responses_complete_json_text() {
        let data = r#"{"output":[{"content":[{"type":"output_text","text":"return $value;"}]}]}"#;
        assert_eq!(OpenAiProvider::extract_complete_text(data), Some("return $value;".to_string()));
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
        let response = self.connector.post_json(WebAiRequest {
            endpoint: self.config.endpoint.clone(),
            bearer_token: self.config.api_key.clone(),
            body,
            timeout_ms: req.timeout_ms,
        })?;

        if Self::response_is_json(&response) {
            let mut body = String::new();
            BufReader::new(response.body)
                .read_to_string(&mut body)
                .map_err(|e| BackendError::Transport(e.to_string()))?;
            if let Some(text) = Self::extract_complete_text(&body) {
                sink(StreamChunk { text, is_final: true });
            }
            return Ok(());
        }

        let reader = BufReader::new(response.body);
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
