//! Shared HTTP/SSE request shaping for web-based AI inline-completion connectors.
//!
//! The concrete provider owns transport, credentials, and rate limiting. This
//! module keeps provider-specific JSON payload and stream-event mapping in one
//! place so additional web connectors can reuse the inline-completion contract.

/// Web AI protocol family used by an HTTP connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebAiProtocol {
    /// OpenAI-compatible chat completions API.
    OpenAiChatCompletions,
    /// OpenAI Responses API.
    OpenAiResponses,
}

impl WebAiProtocol {
    /// Infer the protocol from a configured endpoint URL.
    pub fn infer_from_endpoint(endpoint: &str) -> Self {
        if endpoint.contains("/responses") {
            Self::OpenAiResponses
        } else {
            Self::OpenAiChatCompletions
        }
    }
}

/// Provider-neutral prompt and generation options for a web AI request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAiRequestParts {
    /// Model identifier to send to the connector.
    pub model: String,
    /// System/developer instructions for the completion model.
    pub system: String,
    /// User prompt containing the cursor marker and surrounding code.
    pub user: String,
    /// Maximum generated output tokens.
    pub max_output_tokens: u32,
    /// Whether the connector should stream SSE responses.
    pub stream: bool,
}

impl WebAiRequestParts {
    /// Build the JSON request body for a specific web AI protocol.
    pub fn to_json_body(&self, protocol: WebAiProtocol) -> serde_json::Value {
        match protocol {
            WebAiProtocol::OpenAiResponses => serde_json::json!({
                "model": self.model,
                "max_output_tokens": self.max_output_tokens,
                "stream": self.stream,
                "instructions": self.system,
                "input": self.user,
            }),
            WebAiProtocol::OpenAiChatCompletions => serde_json::json!({
                "model": self.model,
                "max_tokens": self.max_output_tokens,
                "stream": self.stream,
                "messages": [
                    { "role": "system", "content": self.system },
                    { "role": "user", "content": self.user }
                ]
            }),
        }
    }
}

/// Extract a text delta from a single SSE `data:` payload.
pub fn content_delta_from_sse_data(data: &str) -> Option<String> {
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

/// Extract a stream finish reason from a single SSE `data:` payload.
pub fn finish_reason_from_sse_data(data: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request_parts() -> WebAiRequestParts {
        WebAiRequestParts {
            model: "test-model".to_string(),
            system: "system prompt".to_string(),
            user: "user prompt".to_string(),
            max_output_tokens: 42,
            stream: true,
        }
    }

    #[test]
    fn builds_chat_completions_body() -> Result<(), Box<dyn std::error::Error>> {
        let body = request_parts().to_json_body(WebAiProtocol::OpenAiChatCompletions);

        assert_eq!(body.get("model").and_then(serde_json::Value::as_str), Some("test-model"));
        assert_eq!(body.get("max_tokens").and_then(serde_json::Value::as_u64), Some(42));
        assert_eq!(body.get("stream").and_then(serde_json::Value::as_bool), Some(true));
        let messages = body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .ok_or("chat body should contain messages")?;
        assert_eq!(messages.len(), 2);
        Ok(())
    }

    #[test]
    fn builds_responses_body() -> Result<(), Box<dyn std::error::Error>> {
        let body = request_parts().to_json_body(WebAiProtocol::OpenAiResponses);

        assert_eq!(body.get("model").and_then(serde_json::Value::as_str), Some("test-model"));
        assert_eq!(body.get("max_output_tokens").and_then(serde_json::Value::as_u64), Some(42));
        assert_eq!(
            body.get("instructions").and_then(serde_json::Value::as_str),
            Some("system prompt")
        );
        assert_eq!(body.get("input").and_then(serde_json::Value::as_str), Some("user prompt"));
        assert!(body.get("messages").is_none());
        Ok(())
    }

    #[test]
    fn infers_responses_protocol_from_endpoint() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            WebAiProtocol::infer_from_endpoint("https://api.openai.com/v1/responses"),
            WebAiProtocol::OpenAiResponses
        );
        assert_eq!(
            WebAiProtocol::infer_from_endpoint("https://api.openai.com/v1/chat/completions"),
            WebAiProtocol::OpenAiChatCompletions
        );
        Ok(())
    }

    #[test]
    fn extracts_chat_and_responses_deltas() -> Result<(), Box<dyn std::error::Error>> {
        let chat = r#"{"choices":[{"delta":{"content":"my $"},"finish_reason":null}]}"#;
        let responses = r#"{"type":"response.output_text.delta","delta":"value"}"#;

        assert_eq!(content_delta_from_sse_data(chat), Some("my $".to_string()));
        assert_eq!(content_delta_from_sse_data(responses), Some("value".to_string()));
        Ok(())
    }

    #[test]
    fn extracts_finish_reasons() -> Result<(), Box<dyn std::error::Error>> {
        let chat = r#"{"choices":[{"finish_reason":"stop"}]}"#;
        let responses = r#"{"type":"response.completed"}"#;

        assert_eq!(finish_reason_from_sse_data(chat), Some("stop".to_string()));
        assert_eq!(finish_reason_from_sse_data(responses), Some("stop".to_string()));
        Ok(())
    }
}
