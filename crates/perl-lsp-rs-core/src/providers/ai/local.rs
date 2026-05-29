//! Fast local inline-completion model.
//!
//! The local provider is intentionally small and dependency-free. It gives the
//! LSP runtime a real offline AI backend shape (complete + streaming) while the
//! deterministic provider remains the safety net for Perl-specific precision.

use crate::providers::inline_completion::{
    BackendError, BackendRequest, InlineCompletionBackend, PreparedInlineCompletionContext,
    StreamChunk, StreamControl,
};
use std::collections::BTreeSet;

const DEFAULT_MAX_CANDIDATES: usize = 3;
const DEFAULT_CHUNK_CHARS: usize = 12;
const APPROX_CHARS_PER_TOKEN: u32 = 4;

/// Configuration for the lightweight local inline-completion model.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LocalAiConfig {
    /// Model identifier used for logs and future model selection.
    pub model: String,
    /// Maximum number of candidate strings generated for one request.
    pub max_candidates: usize,
    /// Maximum number of Unicode scalar values per streamed chunk.
    pub stream_chunk_chars: usize,
}

impl LocalAiConfig {
    /// Create default local model settings for a configured model identifier.
    pub fn for_model(model: String) -> Self {
        Self { model, ..Self::default() }
    }
}

impl Default for LocalAiConfig {
    fn default() -> Self {
        Self {
            model: "perl-local-small".to_string(),
            max_candidates: DEFAULT_MAX_CANDIDATES,
            stream_chunk_chars: DEFAULT_CHUNK_CHARS,
        }
    }
}

/// Offline, low-latency inline-completion backend.
///
/// This is a small local model scaffold: it derives features from the prepared
/// editor context, ranks Perl-shaped continuations, and streams cumulative
/// chunks through the same trait used by remote providers.
pub struct LocalAiProvider {
    config: LocalAiConfig,
}

impl LocalAiProvider {
    /// Create a local provider with explicit configuration.
    pub const fn new(config: LocalAiConfig) -> Self {
        Self { config }
    }

    /// Return the configured model identifier.
    pub fn model(&self) -> &str {
        self.config.model.as_str()
    }

    fn candidate_texts(&self, req: &BackendRequest) -> Vec<String> {
        let mut candidates = Vec::new();
        let mut seen = BTreeSet::new();
        let max_chars = max_generated_chars(req.max_output_tokens);

        for candidate in raw_local_candidates(&req.context) {
            let candidate = trim_candidate(candidate, max_chars);
            if candidate.is_empty() || !seen.insert(candidate.clone()) {
                continue;
            }
            candidates.push(candidate);
            if candidates.len() >= self.config.max_candidates {
                break;
            }
        }

        candidates
    }

    fn stream_text(
        &self,
        text: &str,
        sink: &mut dyn FnMut(StreamChunk) -> StreamControl,
    ) -> StreamControl {
        let chunk_chars = self.config.stream_chunk_chars.max(1);
        let total_chars = text.chars().count();
        let mut cumulative = String::new();
        let mut chunk_len = 0usize;
        let mut emitted_chars = 0usize;

        for ch in text.chars() {
            cumulative.push(ch);
            chunk_len += 1;
            emitted_chars += 1;

            if chunk_len >= chunk_chars || emitted_chars == total_chars {
                let is_final = emitted_chars == total_chars;
                let control = sink(StreamChunk { text: cumulative.clone(), is_final });
                if control == StreamControl::Stop {
                    return StreamControl::Stop;
                }
                chunk_len = 0;
            }
        }

        StreamControl::Continue
    }
}

impl Default for LocalAiProvider {
    fn default() -> Self {
        Self::new(LocalAiConfig::default())
    }
}

impl InlineCompletionBackend for LocalAiProvider {
    fn complete(&self, req: &BackendRequest) -> Result<Vec<String>, BackendError> {
        Ok(self.candidate_texts(req))
    }

    fn stream(
        &self,
        req: &BackendRequest,
        sink: &mut dyn FnMut(StreamChunk) -> StreamControl,
    ) -> Result<(), BackendError> {
        let mut candidates = self.candidate_texts(req);
        let Some(candidate) = candidates.drain(..).next() else {
            return Ok(());
        };

        self.stream_text(candidate.as_str(), sink);
        Ok(())
    }
}

fn raw_local_candidates(context: &PreparedInlineCompletionContext) -> Vec<String> {
    let mut candidates = Vec::new();
    let trimmed_prefix = context.prefix.trim_end();

    if is_return_prefix(trimmed_prefix) {
        candidates.extend(return_candidates(context));
    }

    if trimmed_prefix.ends_with("my $") {
        candidates.push("result = ".to_string());
        if context.current_function.as_deref() == Some("new") {
            candidates.push("self = shift;".to_string());
        }
    }

    if should_suggest_sub_body(context, trimmed_prefix) {
        candidates.push(sub_body_candidate(context));
    }

    if candidates.is_empty() && looks_like_statement_gap(context, trimmed_prefix) {
        candidates.extend(statement_gap_candidates(context));
    }

    candidates
}

fn is_return_prefix(trimmed_prefix: &str) -> bool {
    trimmed_prefix == "return" || trimmed_prefix.ends_with(" return")
}

fn return_candidates(context: &PreparedInlineCompletionContext) -> Vec<String> {
    let mut candidates = Vec::new();

    for variable in context.variables.iter().filter(|name| name.starts_with('$')) {
        if variable == "$self" {
            continue;
        }
        candidates.push(format!(" {variable};"));
    }

    if context.current_function.as_deref().is_some_and(is_boolean_function_name) {
        candidates.push(" 1;".to_string());
    }

    candidates.push(";".to_string());
    candidates
}

fn is_boolean_function_name(name: &str) -> bool {
    name.starts_with("is_") || name.starts_with("has_") || name.starts_with("can_")
}

fn should_suggest_sub_body(
    context: &PreparedInlineCompletionContext,
    trimmed_prefix: &str,
) -> bool {
    trimmed_prefix.starts_with("sub ")
        && !context.current_line.contains('{')
        && context.current_function.is_some()
}

fn sub_body_candidate(context: &PreparedInlineCompletionContext) -> String {
    if context.current_function.as_deref() == Some("new") {
        return " {\n    my $class = shift;\n    return bless {}, $class;\n}".to_string();
    }

    " {\n    return;\n}".to_string()
}

fn looks_like_statement_gap(
    context: &PreparedInlineCompletionContext,
    trimmed_prefix: &str,
) -> bool {
    trimmed_prefix.is_empty()
        && context.current_function.is_some()
        && context.current_line.trim().is_empty()
}

fn statement_gap_candidates(context: &PreparedInlineCompletionContext) -> Vec<String> {
    let mut candidates = Vec::new();

    for variable in context.variables.iter().filter(|name| name.starts_with('$')) {
        if variable == "$self" {
            continue;
        }
        candidates.push(format!("return {variable};"));
    }

    if context.imports.iter().any(|import| import == "Test::More") {
        candidates.push("ok($result);".to_string());
    }

    candidates
}

fn max_generated_chars(max_output_tokens: u32) -> usize {
    usize::try_from(max_output_tokens.saturating_mul(APPROX_CHARS_PER_TOKEN)).unwrap_or(usize::MAX)
}

fn trim_candidate(candidate: String, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    candidate.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::{LocalAiConfig, LocalAiProvider};
    use crate::providers::inline_completion::{
        BackendRequest, InlineCompletionBackend, PreparedInlineCompletionContext, StreamControl,
    };

    fn request(prefix: &str) -> BackendRequest {
        BackendRequest {
            context: PreparedInlineCompletionContext {
                prefix: prefix.to_string(),
                current_line: prefix.to_string(),
                previous_non_empty_line: Some("my $result = compute();".to_string()),
                current_function: Some("build".to_string()),
                current_package: Some("Local::Example".to_string()),
                variables: vec!["$result".to_string(), "$self".to_string()],
                imports: Vec::new(),
            },
            max_output_tokens: 64,
            timeout_ms: 25,
        }
    }

    #[test]
    fn local_model_prefers_visible_scalar_for_return_prefix()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = LocalAiProvider::default();
        let completions = provider.complete(&request("    return"))?;

        assert_eq!(completions.first().map(String::as_str), Some(" $result;"));
        Ok(())
    }

    #[test]
    fn local_model_streams_cumulative_chunks() -> Result<(), Box<dyn std::error::Error>> {
        let provider = LocalAiProvider::new(LocalAiConfig {
            model: "test-local".to_string(),
            max_candidates: 1,
            stream_chunk_chars: 4,
        });
        let mut chunks = Vec::new();

        provider.stream(&request("    return"), &mut |chunk| {
            chunks.push((chunk.text, chunk.is_final));
            StreamControl::Continue
        })?;

        assert_eq!(chunks.first().map(|(text, _)| text.as_str()), Some(" $re"));
        assert_eq!(
            chunks.last().map(|(text, final_chunk)| (text.as_str(), *final_chunk)),
            Some((" $result;", true))
        );
        Ok(())
    }
}
