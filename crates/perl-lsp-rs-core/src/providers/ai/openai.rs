//! OpenAI-compatible completion provider.

use super::budget::{AiResponseBudget, BudgetKind, BudgetViolation};
use super::destination::{ApprovedDestination, credential_may_attach, validate_endpoint};
use super::prompt::build_fim_prompt;
use super::rate_limiter::RateLimiter;
use super::sanitize::{sanitize_completion_text, sanitize_streaming_text};
use super::sse::{SseParser, budget_violation};
use crate::config::{
    DEFAULT_AI_API_KEY_HEADER, DEFAULT_AI_API_KEY_PREFIX, is_safe_http_header_value_part,
    normalize_ai_api_key_header, normalize_ai_api_key_prefix,
};
use crate::providers::inline_completion::{
    BackendError, BackendRequest, InlineCompletionBackend, StreamChunk, StreamControl,
};
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};
use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

/// Configuration for the OpenAI-compatible provider.
#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    /// The API endpoint URL (e.g. `https://api.openai.com/v1/chat/completions`).
    pub endpoint: String,
    /// The model name to use (e.g. `gpt-4o`).
    pub model: String,
    /// API key for authentication.
    pub api_key: String,
    /// HTTP header that carries the API key.
    pub api_key_header: String,
    /// Optional authentication scheme prepended before the API key.
    pub api_key_prefix: Option<String>,
    /// Global timeout in milliseconds.
    pub timeout_ms: u64,
    /// Allow plain HTTP when the endpoint resolves to loopback only.
    pub local_model_mode: bool,
}

/// An OpenAI-compatible completion provider using ureq for HTTP.
pub struct OpenAiProvider {
    config: OpenAiConfig,
    limiter: Arc<RateLimiter>,
    /// Destination validated once on first use; subsequent requests only
    /// re-check credential binding against the URL about to be dispatched.
    approved: OnceLock<ApprovedDestination>,
    /// Resource ceiling applied to every response this provider reads.
    /// Compiled policy for now; a user-owned backend profile selects values
    /// within the same compiled maxima once #10252 lands.
    budget: AiResponseBudget,
}

impl OpenAiConfig {
    /// Build a default bearer-token configuration for OpenAI-compatible web APIs.
    pub fn new(endpoint: String, model: String, api_key: String, timeout_ms: u64) -> Self {
        Self {
            endpoint,
            model,
            api_key,
            api_key_header: DEFAULT_AI_API_KEY_HEADER.to_string(),
            api_key_prefix: Some(DEFAULT_AI_API_KEY_PREFIX.to_string()),
            timeout_ms,
            local_model_mode: false,
        }
    }
}

/// ureq resolver that returns only the IPs approved at validation time.
///
/// Pins connect-time DNS to the validated address set so a rebinding host
/// cannot swap a public IP (policy pass) for a private IP at HTTP connect.
#[derive(Debug)]
struct PinnedIpResolver {
    ips: Vec<IpAddr>,
}

impl Resolver for PinnedIpResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        _config: &ureq::config::Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let port = uri
            .authority()
            .and_then(|a| a.port_u16())
            .or_else(|| match uri.scheme_str() {
                Some("https") => Some(443),
                Some("http") => Some(80),
                _ => None,
            })
            .ok_or(ureq::Error::HostNotFound)?;

        let mut result = self.empty();
        for ip in self.ips.iter().take(16) {
            result.push(SocketAddr::new(*ip, port));
        }
        if result.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(result)
    }
}

/// Incremental accounting for the cumulative streamed completion.
///
/// Size is tracked as each delta arrives rather than recomputed from the text
/// already held, so a run of individually legal deltas is bounded as cheaply
/// as a single oversized one and no bound check rescans or clones the
/// candidate.
struct CumulativeOutput {
    text: String,
    chars: u64,
    newlines: u64,
}

impl CumulativeOutput {
    const fn new() -> Self {
        Self { text: String::new(), chars: 0, newlines: 0 }
    }

    /// The candidate accumulated so far.
    fn text(&self) -> &str {
        &self.text
    }

    /// Admit `delta`, or refuse it before any part of it is retained.
    fn push(&mut self, delta: &str, budget: AiResponseBudget) -> Result<(), BudgetViolation> {
        let delta_bytes = delta.len() as u64;
        budget.check(BudgetKind::DeltaBytes, delta_bytes)?;

        let bytes = (self.text.len() as u64).saturating_add(delta_bytes);
        budget.check(BudgetKind::CompletionBytes, bytes)?;

        let chars = self.chars.saturating_add(delta.chars().count() as u64);
        budget.check(BudgetKind::CompletionChars, chars)?;

        let newlines = self
            .newlines
            .saturating_add(delta.bytes().filter(|byte| *byte == b'\n').count() as u64);
        // A non-empty candidate spans one more line than it holds newlines.
        budget.check(BudgetKind::CompletionLines, newlines.saturating_add(1))?;

        self.text.push_str(delta);
        self.chars = chars;
        self.newlines = newlines;
        Ok(())
    }
}

impl OpenAiProvider {
    /// Create a new provider with the given config and rate limiter.
    pub fn new(config: OpenAiConfig, limiter: Arc<RateLimiter>) -> Self {
        Self {
            config,
            limiter,
            approved: OnceLock::new(),
            budget: AiResponseBudget::compiled_default(),
        }
    }

    fn approved_destination(&self) -> Result<&ApprovedDestination, BackendError> {
        if let Some(approved) = self.approved.get() {
            return Ok(approved);
        }
        let validated = validate_endpoint(&self.config.endpoint, self.config.local_model_mode)
            .map_err(|e| BackendError::Transport(e.to_string()))?;
        let _ = self.approved.set(validated);
        self.approved
            .get()
            .ok_or_else(|| BackendError::Transport("AI destination approval missing".to_string()))
    }

    fn auth_header_name(&self) -> &str {
        if normalize_ai_api_key_header(&self.config.api_key_header).is_some() {
            self.config.api_key_header.as_str()
        } else {
            DEFAULT_AI_API_KEY_HEADER
        }
    }

    fn auth_header_value(&self) -> Result<String, BackendError> {
        if !is_safe_http_header_value_part(&self.config.api_key) {
            return Err(BackendError::Auth(
                "AI API key contains unsupported HTTP header characters".to_string(),
            ));
        }

        let prefix =
            self.config.api_key_prefix.as_deref().and_then(normalize_ai_api_key_prefix).flatten();

        Ok(match prefix.as_deref() {
            Some(prefix) => format!("{prefix} {}", self.config.api_key),
            None => self.config.api_key.clone(),
        })
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

    fn sanitize_transport_message(message: &str, api_key: &str) -> String {
        let mut sanitized = message.to_string();
        if !api_key.is_empty() {
            sanitized = sanitized.replace(api_key, "<redacted>");
        }
        sanitized
    }

    fn map_transport_error(message: String, api_key: &str) -> BackendError {
        let sanitized = Self::sanitize_transport_message(&message, api_key);
        if sanitized.contains("timed out") || sanitized.contains("timeout") {
            BackendError::Timeout
        } else if sanitized.contains("401") || sanitized.contains("403") {
            BackendError::Auth(sanitized)
        } else {
            BackendError::Transport(sanitized)
        }
    }

    fn build_http_agent(
        timeout: std::time::Duration,
        approved: &ApprovedDestination,
    ) -> ureq::Agent {
        let config =
            ureq::Agent::config_builder().timeout_global(Some(timeout)).max_redirects(0).build();
        // Pin connect-time resolution to the IPs approved by validate_endpoint
        // so ureq cannot re-resolve DNS and bypass the SSRF allowlist (TOCTOU).
        ureq::Agent::with_parts(
            config,
            DefaultConnector::default(),
            PinnedIpResolver { ips: approved.resolved_ips.clone() },
        )
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
            // A content-filter rejection repudiates the candidate: normalize
            // it to "error" so the stream terminal treats it as a provider
            // failure instead of finalizing the rejected text.
            return Some(if reason == "content_filter" {
                "error".to_string()
            } else {
                reason.to_string()
            });
        }

        match parsed.get("type").and_then(serde_json::Value::as_str) {
            Some("response.completed") => Some("stop".to_string()),
            // `max_output_tokens` exhaustion is the Responses API equivalent
            // of the chat `length` finish reason: the accumulated text is
            // usable and must finalize instead of surfacing as a provider
            // error. Any other incomplete reason stays a failure.
            Some("response.incomplete") => {
                let token_limited = parsed
                    .get("incomplete_details")
                    .and_then(|details| details.get("reason"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|reason| reason == "max_output_tokens");
                Some(if token_limited { "length".to_string() } else { "error".to_string() })
            }
            Some("response.failed") | Some("error") => Some("error".to_string()),
            _ => None,
        }
    }

    /// Text for one `StreamChunk`: held-back live text for in-flight chunks,
    /// full boundary sanitization once the completion is final. Buffered
    /// `complete()` consumes this same final chunk, so both routes observe
    /// the identical sanitized candidate from this single choke point.
    fn stream_chunk_text(cumulative: &str, is_final: bool) -> String {
        if is_final {
            sanitize_completion_text(cumulative)
        } else {
            sanitize_streaming_text(cumulative)
        }
    }

    /// Drive the SSE event loop, forwarding cumulative candidate chunks to
    /// `sink`. Split out from [`Self::stream`] so the delta-to-sink wiring
    /// is testable without an HTTP transport.
    ///
    /// `budget` bounds the candidate itself. The parser has already bounded
    /// the bytes, lines, and events beneath it; this level stops a stream of
    /// individually legal deltas from accumulating without limit.
    fn drive_sse_stream<R: BufRead>(
        parser: &mut SseParser<R>,
        budget: AiResponseBudget,
        api_key: &str,
        sink: &mut dyn FnMut(StreamChunk) -> StreamControl,
    ) -> Result<(), BackendError> {
        let mut cumulative = CumulativeOutput::new();

        loop {
            match parser.next_event() {
                Ok(Some(event)) => {
                    // Provider failure events are typed errors, not candidate
                    // text: `response.failed`, `response.incomplete`, or an
                    // explicit error finish reason must surface as
                    // [`BackendError::Provider`] instead of silently
                    // finalizing whatever text accumulated by EOF.
                    if Self::extract_finish_reason(&event.data).as_deref() == Some("error") {
                        return Err(BackendError::Provider(
                            "stream ended with a provider failure event (response.failed, \
                             response.incomplete, content_filter, or error event)"
                                .to_string(),
                        ));
                    }
                    if let Some(delta) = Self::extract_content_delta(&event.data) {
                        // Refused before the delta is retained, so a chunk
                        // that crosses a limit is never partially emitted.
                        if let Err(violation) = cumulative.push(&delta, budget) {
                            return Err(BackendError::BudgetExceeded(violation));
                        }

                        let is_final = Self::extract_finish_reason(&event.data)
                            .is_some_and(|r| r == "stop" || r == "length");

                        // In-flight chunks show the live candidate with
                        // ambiguous fence markers held back; only the
                        // completion boundary runs the full strip (#5049).
                        // Stateless per-chunk stripping deleted already-shown
                        // code mid-stream whenever a content fence arrived
                        // and leaked partially delivered markers for a tick.
                        let control = sink(StreamChunk {
                            text: Self::stream_chunk_text(cumulative.text(), is_final),
                            is_final,
                        });

                        if control == StreamControl::Stop || is_final {
                            break;
                        }
                    }
                }
                Ok(None) => {
                    // Stream ended -- emit final chunk if we have content
                    if !cumulative.text().is_empty() {
                        sink(StreamChunk {
                            text: Self::stream_chunk_text(cumulative.text(), true),
                            is_final: true,
                        });
                    }
                    break;
                }
                Err(error) => {
                    // A refused response carries its typed violation through
                    // the IO channel; only a genuine transport failure is
                    // classified by message.
                    return Err(budget_violation(&error).map_or_else(
                        || Self::map_transport_error(error.to_string(), api_key),
                        BackendError::BudgetExceeded,
                    ));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::items_after_test_module,
    reason = "policy:#2064: OpenAI unit tests stay beside config helpers before backend implementation"
)]
mod tests {
    use super::{AiResponseBudget, OpenAiConfig, OpenAiProvider};
    use crate::providers::ai::rate_limiter::RateLimiter;
    use std::sync::Arc;

    /// The budget every existing stream test runs under: the same compiled
    /// policy production uses, so those tests keep proving production
    /// behavior rather than a relaxed test-only ceiling.
    fn test_budget() -> AiResponseBudget {
        AiResponseBudget::compiled_default()
    }

    fn provider_with_endpoint(endpoint: &str) -> OpenAiProvider {
        OpenAiProvider::new(
            OpenAiConfig::new(
                endpoint.to_string(),
                "gpt-4o-mini".to_string(),
                "test-key".to_string(),
                1000,
            ),
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

    /// Frame a delta sequence as chat-completions SSE events; the final
    /// event carries `finish_reason: "stop"`.
    fn sse_framed_deltas(deltas: &[String]) -> String {
        let last = deltas.len() - 1;
        let mut body = String::new();
        for (i, delta) in deltas.iter().enumerate() {
            let finish_reason: serde_json::Value =
                if i == last { "stop".into() } else { serde_json::Value::Null };
            let event = serde_json::json!({
                "choices": [{
                    "delta": { "content": delta },
                    "finish_reason": finish_reason,
                }]
            });
            body.push_str(&format!("data: {event}\n\n"));
        }
        body
    }

    /// Drive the real SSE loop over synthetic frames and collect every
    /// `(text, is_final)` chunk handed to the sink.
    fn collect_stream_chunks(deltas: &[String]) -> Vec<(String, bool)> {
        let body = sse_framed_deltas(deltas);
        let mut parser = crate::providers::ai::sse::SseParser::new(std::io::Cursor::new(body));
        let mut chunks: Vec<(String, bool)> = Vec::new();
        OpenAiProvider::drive_sse_stream(&mut parser, test_budget(), "test-key", &mut |chunk| {
            chunks.push((chunk.text, chunk.is_final));
            if chunk.is_final {
                crate::providers::inline_completion::StreamControl::Stop
            } else {
                crate::providers::inline_completion::StreamControl::Continue
            }
        })
        .expect("synthetic SSE frames must drive the stream loop");
        chunks
    }

    #[test]
    fn stream_holds_partial_fence_markers_and_sanitizes_only_at_the_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        // Character-by-character delivery of a fenced candidate: both the
        // opening and the closing marker arrive split across deltas.
        let deltas: Vec<String> = ["`", "`", "`", "perl", "\n", "my $x = 1;", "\n", "`", "`", "`"]
            .iter()
            .map(|delta| (*delta).to_string())
            .collect();
        let chunks = collect_stream_chunks(&deltas);

        // The cumulative partial opening marker never reaches the sink.
        assert_eq!(
            chunks[1],
            (String::new(), false),
            "partial opening fence must be held back, got: {:?}",
            chunks[1]
        );
        // No in-flight chunk surfaces any fence marker for this candidate,
        // and only the boundary event is final.
        for (text, is_final) in chunks.iter().take(chunks.len() - 1) {
            assert!(!is_final, "only the boundary event may be final");
            assert!(!text.contains('`'), "in-flight chunk surfaced a fence marker: {text:?}");
        }
        // The candidate grows monotonically once visible; shown code is
        // never deleted mid-stream.
        for window in chunks.windows(2) {
            assert!(
                window[1].0.starts_with(&window[0].0),
                "streamed candidate deleted shown text: {:?} -> {:?}",
                window[0].0,
                window[1].0
            );
        }
        // Boundary parity: the final streamed chunk equals the buffered
        // sanitize of the whole candidate, and `complete()` consumes exactly
        // this chunk, so both routes agree.
        let (final_text, is_final) = chunks.last().ok_or("expected at least one chunk")?;
        assert!(is_final);
        assert_eq!(final_text, "my $x = 1;");
        assert_eq!(
            final_text,
            &crate::providers::ai::sanitize::sanitize_completion_text(&deltas.concat())
        );
        Ok(())
    }

    #[test]
    fn stream_heredoc_candidate_never_collapses_or_truncates_mid_stream()
    -> Result<(), Box<dyn std::error::Error>> {
        // The sanitize.rs here-doc falsifier, streamed: a candidate whose
        // CONTENT contains line-initial fences must never collapse to an
        // empty candidate, never delete already-shown text, and land
        // unchanged at the boundary.
        let raw = "my $doc = <<'EOF';\n# Usage\n```perl\nmy $x = 1;\n```\nEOF";
        let deltas: Vec<String> = raw.chars().map(String::from).collect();
        let chunks = collect_stream_chunks(&deltas);

        assert!(
            chunks.iter().all(|(text, _)| !text.is_empty()),
            "content-anchored candidate must never collapse mid-stream"
        );
        for window in chunks.windows(2) {
            assert!(
                window[1].0.starts_with(&window[0].0),
                "streamed candidate deleted shown text: {:?} -> {:?}",
                window[0].0,
                window[1].0
            );
        }
        let (final_text, is_final) = chunks.last().ok_or("expected at least one chunk")?;
        assert!(is_final);
        assert_eq!(final_text, raw, "content fences must survive the boundary");
        Ok(())
    }

    #[test]
    fn stream_provider_failure_event_is_a_typed_error() {
        // `response.failed` / `response.incomplete` carry no candidate text:
        // they must surface as a typed provider error instead of being
        // ignored until EOF finalizes the accumulated text.
        let body = "data: {\"type\": \"response.failed\"}\n\n";
        let mut parser = crate::providers::ai::sse::SseParser::new(std::io::Cursor::new(body));
        let result =
            OpenAiProvider::drive_sse_stream(&mut parser, test_budget(), "test-key", &mut |_| {
                crate::providers::inline_completion::StreamControl::Continue
            });
        assert!(
            matches!(result, Err(crate::providers::inline_completion::BackendError::Provider(_))),
            "provider failure events must be typed errors"
        );
    }

    #[test]
    fn stream_failure_after_partial_text_never_finalizes_the_candidate() {
        // Text accumulated before the failure event must not be finalized as
        // a completion: no `is_final` chunk may reach the sink.
        let mut body = String::new();
        body.push_str(
            "data: {\"choices\":[{\"delta\":{\"content\":\"my $x = \"},\"finish_reason\":null}]}\n\n",
        );
        body.push_str("data: {\"type\": \"response.incomplete\"}\n\n");
        let mut parser = crate::providers::ai::sse::SseParser::new(std::io::Cursor::new(body));
        let mut chunks: Vec<(String, bool)> = Vec::new();
        let result = OpenAiProvider::drive_sse_stream(
            &mut parser,
            test_budget(),
            "test-key",
            &mut |chunk| {
                chunks.push((chunk.text, chunk.is_final));
                crate::providers::inline_completion::StreamControl::Continue
            },
        );
        assert!(
            matches!(result, Err(crate::providers::inline_completion::BackendError::Provider(_))),
            "provider failure events must be typed errors"
        );
        assert!(
            chunks.iter().all(|(_, is_final)| !is_final),
            "failure events must not finalize the candidate, got: {chunks:?}"
        );
    }

    #[test]
    fn stream_max_output_tokens_incomplete_finalizes_accumulated_text() {
        // `response.incomplete` with `max_output_tokens` is the Responses API
        // equivalent of the chat `length` finish reason: the accumulated text
        // is usable and must finalize, not surface as a provider error.
        let mut body = String::new();
        body.push_str(
            "data: {\"choices\":[{\"delta\":{\"content\":\"my $x = \"},\"finish_reason\":null}]}\n\n",
        );
        body.push_str(
            "data: {\"type\":\"response.incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}\n\n",
        );
        let mut parser = crate::providers::ai::sse::SseParser::new(std::io::Cursor::new(body));
        let mut chunks: Vec<(String, bool)> = Vec::new();
        let result = OpenAiProvider::drive_sse_stream(
            &mut parser,
            test_budget(),
            "test-key",
            &mut |chunk| {
                chunks.push((chunk.text, chunk.is_final));
                crate::providers::inline_completion::StreamControl::Continue
            },
        );
        result.expect("token-limited incomplete must not be a provider error");
        let (text, is_final) = chunks.last().expect("boundary chunk");
        assert!(is_final, "token-limited output must finalize");
        assert_eq!(text, "my $x = ");
    }

    #[test]
    fn stream_incomplete_without_token_limit_stays_a_provider_error() {
        let body = "data: {\"type\":\"response.incomplete\",\"incomplete_details\":{\"reason\":\"content_filter\"}}\n\n";
        let mut parser = crate::providers::ai::sse::SseParser::new(std::io::Cursor::new(body));
        let result =
            OpenAiProvider::drive_sse_stream(&mut parser, test_budget(), "test-key", &mut |_| {
                crate::providers::inline_completion::StreamControl::Continue
            });
        assert!(
            matches!(result, Err(crate::providers::inline_completion::BackendError::Provider(_))),
            "non-token-limited incomplete reasons stay failures"
        );
    }

    #[test]
    fn stream_content_filter_finish_reason_is_a_typed_error() {
        // A chat-completions `content_filter` rejection after partial text
        // must surface as a provider failure: the rejected text must never
        // be finalized into the candidate.
        let mut body = String::new();
        body.push_str(
            "data: {\"choices\":[{\"delta\":{\"content\":\"my $x = \"},\"finish_reason\":null}]}\n\n",
        );
        body.push_str(
            "data: {\"choices\":[{\"delta\":{\"content\":\"dropped\"},\"finish_reason\":\"content_filter\"}]}\n\n",
        );
        let mut parser = crate::providers::ai::sse::SseParser::new(std::io::Cursor::new(body));
        let mut chunks: Vec<(String, bool)> = Vec::new();
        let result = OpenAiProvider::drive_sse_stream(
            &mut parser,
            test_budget(),
            "test-key",
            &mut |chunk| {
                chunks.push((chunk.text, chunk.is_final));
                crate::providers::inline_completion::StreamControl::Continue
            },
        );
        assert!(
            matches!(result, Err(crate::providers::inline_completion::BackendError::Provider(_))),
            "content_filter rejections must be provider errors"
        );
        assert!(
            chunks.iter().all(|(_, is_final)| !is_final),
            "rejected text must not reach the sink as final"
        );
    }

    #[test]
    fn stream_responses_error_event_is_a_typed_error() {
        // A Responses API `type: "error"` event is a provider failure even
        // though it carries no choices/finish_reason shape.
        let body = "data: {\"type\": \"error\"}\n\n";
        let mut parser = crate::providers::ai::sse::SseParser::new(std::io::Cursor::new(body));
        let result =
            OpenAiProvider::drive_sse_stream(&mut parser, test_budget(), "test-key", &mut |_| {
                crate::providers::inline_completion::StreamControl::Continue
            });
        assert!(
            matches!(result, Err(crate::providers::inline_completion::BackendError::Provider(_))),
            "Responses API error events must be provider errors"
        );
    }

    #[test]
    fn default_config_uses_bearer_authorization_header() -> Result<(), Box<dyn std::error::Error>> {
        let provider = provider_with_endpoint("https://api.openai.com/v1/chat/completions");
        assert_eq!(provider.config.api_key_header, "Authorization");
        assert_eq!(provider.auth_header_name(), "Authorization");
        assert_eq!(provider.auth_header_value()?, "Bearer test-key");
        Ok(())
    }

    #[test]
    fn custom_web_connector_auth_header_can_send_raw_key() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut config = OpenAiConfig::new(
            "https://example.test/v1/chat/completions".to_string(),
            "custom-code-model".to_string(),
            "connector-key".to_string(),
            1000,
        );
        config.api_key_header = "x-api-key".to_string();
        config.api_key_prefix = None;
        let provider = OpenAiProvider::new(config, Arc::new(RateLimiter::new(1.0, 1)));

        assert_eq!(provider.config.api_key_header, "x-api-key");
        assert_eq!(provider.auth_header_name(), "x-api-key");
        assert_eq!(provider.auth_header_value()?, "connector-key");
        Ok(())
    }

    #[test]
    fn malformed_auth_header_name_falls_back_without_exposing_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut config = OpenAiConfig::new(
            "https://example.test/v1/chat/completions".to_string(),
            "custom-code-model".to_string(),
            "connector-key".to_string(),
            1000,
        );
        config.api_key_header = "x-api-key\r\nX-Injected".to_string();
        let provider = OpenAiProvider::new(config, Arc::new(RateLimiter::new(1.0, 1)));

        assert_eq!(provider.auth_header_name(), "Authorization");
        assert_eq!(provider.auth_header_value()?, "Bearer connector-key");
        Ok(())
    }

    #[test]
    fn malformed_auth_prefix_and_key_do_not_enter_header_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut config = OpenAiConfig::new(
            "https://example.test/v1/chat/completions".to_string(),
            "custom-code-model".to_string(),
            "connector-key".to_string(),
            1000,
        );
        config.api_key_prefix = Some("Token\r\nX-Injected".to_string());
        let provider = OpenAiProvider::new(config, Arc::new(RateLimiter::new(1.0, 1)));
        assert_eq!(provider.auth_header_value()?, "connector-key");

        let mut bad_key_config = OpenAiConfig::new(
            "https://example.test/v1/chat/completions".to_string(),
            "custom-code-model".to_string(),
            "connector-key\r\nX-Injected".to_string(),
            1000,
        );
        bad_key_config.api_key_prefix = None;
        let provider = OpenAiProvider::new(bad_key_config, Arc::new(RateLimiter::new(1.0, 1)));
        let Err(err) = provider.auth_header_value() else {
            return Err("invalid key must be rejected".into());
        };
        let message = err.to_string();
        assert!(message.contains("unsupported HTTP header characters"));
        assert!(!message.contains("connector-key"));
        Ok(())
    }

    // ── cumulative output bounds ─────────────────────────────────────────

    use crate::providers::ai::budget::{BudgetKind, BudgetViolation};
    use crate::providers::inline_completion::{BackendError, StreamControl};

    fn budget_with(kind: BudgetKind, value: u64) -> AiResponseBudget {
        let Ok(budget) = AiResponseBudget::compiled_default().with_limit(kind, value) else {
            unreachable!("{kind} narrowing to {value} must be within the compiled maximum");
        };
        budget
    }

    /// Frame `deltas` as chat-completions SSE events with no terminal
    /// `finish_reason`, drive the real loop, and return every chunk the sink
    /// saw alongside the outcome.
    fn drive_open_ended(
        deltas: &[String],
        budget: AiResponseBudget,
    ) -> (Vec<String>, Result<(), BackendError>) {
        let mut body = String::new();
        for delta in deltas {
            let event = serde_json::json!({
                "choices": [{ "delta": { "content": delta }, "finish_reason": null }]
            });
            body.push_str(&format!("data: {event}\n\n"));
        }
        let mut parser = crate::providers::ai::sse::SseParser::new(std::io::Cursor::new(body));
        let mut seen: Vec<String> = Vec::new();
        let outcome =
            OpenAiProvider::drive_sse_stream(&mut parser, budget, "test-key", &mut |chunk| {
                seen.push(chunk.text);
                StreamControl::Continue
            });
        (seen, outcome)
    }

    fn violation_of(outcome: &Result<(), BackendError>) -> Option<BudgetViolation> {
        match outcome {
            Err(BackendError::BudgetExceeded(violation)) => Some(*violation),
            _ => None,
        }
    }

    #[test]
    fn one_oversized_delta_is_refused_before_it_is_retained() {
        let deltas = vec!["x".repeat(64)];
        let (seen, outcome) = drive_open_ended(&deltas, budget_with(BudgetKind::DeltaBytes, 32));
        assert_eq!(violation_of(&outcome).map(|v| v.kind), Some(BudgetKind::DeltaBytes));
        assert!(seen.is_empty(), "an over-limit delta must not reach the sink: {seen:?}");
    }

    #[test]
    fn a_delta_exactly_at_its_limit_is_admitted() {
        // Negative control for the refusal above.
        let deltas = vec!["x".repeat(32)];
        let (seen, outcome) = drive_open_ended(&deltas, budget_with(BudgetKind::DeltaBytes, 32));
        assert!(outcome.is_ok(), "a delta at the limit must be admitted: {outcome:?}");
        assert_eq!(seen.last().map(String::as_str), Some("x".repeat(32).as_str()));
    }

    #[test]
    fn many_individually_legal_deltas_are_refused_once_they_cross_the_byte_limit() {
        // Each delta is far under the per-delta limit; only the cumulative
        // accounting can catch this.
        let deltas: Vec<String> = (0..64).map(|_| "abcd".to_string()).collect();
        // The delta limit is brought down alongside the completion limit so
        // the budget stays coherent; at 100 bytes it still dwarfs every
        // 4-byte delta, so only cumulative accounting can trip here.
        let Ok(budget) =
            budget_with(BudgetKind::DeltaBytes, 100).with_limit(BudgetKind::CompletionBytes, 100)
        else {
            unreachable!("narrowing both output limits to 100 must be coherent");
        };
        let (seen, outcome) = drive_open_ended(&deltas, budget);
        let violation = violation_of(&outcome);
        assert_eq!(violation.map(|v| v.kind), Some(BudgetKind::CompletionBytes));
        assert_eq!(violation.map(|v| v.limit), Some(100));
        // Nothing past the limit was ever handed out.
        for text in &seen {
            assert!(text.len() <= 100, "sink saw {} bytes past a 100-byte limit", text.len());
        }
    }

    #[test]
    fn cumulative_character_overflow_counts_scalars_not_bytes() {
        // Twenty four-byte emoji: 80 bytes, but only 20 characters. A limit of
        // 16 characters must trip while the byte limit stays untouched.
        let deltas: Vec<String> = (0..20).map(|_| "\u{1F980}".to_string()).collect();
        let (_, outcome) = drive_open_ended(&deltas, budget_with(BudgetKind::CompletionChars, 16));
        let violation = violation_of(&outcome);
        assert_eq!(violation.map(|v| v.kind), Some(BudgetKind::CompletionChars));
        assert_eq!(violation.map(|v| v.observed_at_least), Some(17));
    }

    #[test]
    fn cumulative_line_overflow_is_refused() {
        let deltas: Vec<String> = (0..10).map(|_| "line\n".to_string()).collect();
        let (_, outcome) = drive_open_ended(&deltas, budget_with(BudgetKind::CompletionLines, 4));
        assert_eq!(violation_of(&outcome).map(|v| v.kind), Some(BudgetKind::CompletionLines));
    }

    #[test]
    fn a_single_line_candidate_is_not_refused_by_a_one_line_limit() {
        // Boundary control for the newline accounting: a candidate with no
        // newline spans exactly one line and must survive a limit of one.
        let deltas = vec!["my $x = 1;".to_string()];
        let (_, outcome) = drive_open_ended(&deltas, budget_with(BudgetKind::CompletionLines, 1));
        assert!(outcome.is_ok(), "a single-line candidate must fit one line: {outcome:?}");
    }

    #[test]
    fn malformed_event_json_under_the_limits_is_not_a_budget_error() {
        // Negative control: budget enforcement must not swallow ordinary
        // parse outcomes. An undecodable event yields no delta and the stream
        // simply ends without content.
        let body = "data: {not json at all}\n\n";
        let mut parser = crate::providers::ai::sse::SseParser::new(std::io::Cursor::new(body));
        let mut seen = 0_usize;
        let outcome =
            OpenAiProvider::drive_sse_stream(&mut parser, test_budget(), "test-key", &mut |_| {
                seen = seen.saturating_add(1);
                StreamControl::Continue
            });
        assert!(outcome.is_ok(), "malformed JSON is not a budget breach: {outcome:?}");
        assert_eq!(seen, 0);
    }

    #[test]
    fn a_transport_level_budget_breach_surfaces_as_a_typed_backend_error() {
        // The violation is raised inside the parser and must reach the caller
        // as BudgetExceeded rather than being flattened into Transport.
        let body = "data: hello\n\ndata: world\n\n";
        let budget = budget_with(BudgetKind::EventCount, 1);
        let mut parser =
            crate::providers::ai::sse::SseParser::with_budget(std::io::Cursor::new(body), budget);
        let outcome =
            OpenAiProvider::drive_sse_stream(&mut parser, budget, "test-key", &mut |_| {
                StreamControl::Continue
            });
        assert_eq!(violation_of(&outcome).map(|v| v.kind), Some(BudgetKind::EventCount));
    }

    #[test]
    fn a_budget_error_message_leaks_no_response_content() {
        let secret = "SUPER-SECRET-COMPLETION";
        let deltas = vec![secret.to_string()];
        let (_, outcome) = drive_open_ended(&deltas, budget_with(BudgetKind::DeltaBytes, 4));
        let Err(error) = outcome else {
            unreachable!("an over-limit delta must fail");
        };
        let message = error.to_string();
        assert!(!message.contains(secret), "budget error leaked response content: {message}");
        assert!(message.contains("delta_bytes"), "budget error lost its limit identity: {message}");
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

        // Validate once (cached); bind credentials to the URL about to be POSTed.
        let approved = self.approved_destination()?;
        let request_url = self.config.endpoint.as_str();
        if !credential_may_attach(approved, request_url) {
            return Err(BackendError::Transport(
                "AI endpoint failed credential binding check".to_string(),
            ));
        }

        let body = self.build_request_body(req);
        let timeout = std::time::Duration::from_millis(req.timeout_ms);
        let agent = Self::build_http_agent(timeout, approved);
        let auth_value = self.auth_header_value()?;

        let response = agent
            .post(request_url)
            .header(self.auth_header_name(), auth_value.as_str())
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| Self::map_transport_error(e.to_string(), &self.config.api_key))?;

        // max_redirects(0) returns 3xx bodies instead of following them; reject
        // non-2xx so credentials never ride a silent redirect "success".
        let status = response.status();
        if !(200..300).contains(&status.as_u16()) {
            return Err(Self::map_transport_error(
                format!("unexpected HTTP status {status}"),
                &self.config.api_key,
            ));
        }

        // Every byte read from here on is counted against one request-scoped
        // budget: the non-2xx path above returns before any body is read, so
        // this is the only route that consumes a response.
        let reader = BufReader::new(response.into_body().into_reader());
        let mut parser = SseParser::with_budget(reader, self.budget);
        Self::drive_sse_stream(&mut parser, self.budget, &self.config.api_key, sink)
    }
}
