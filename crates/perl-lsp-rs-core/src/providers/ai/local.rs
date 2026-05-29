//! Fast local inline-completion provider.
//!
//! The local provider is intentionally small and dependency-free: it builds a
//! request-local statistical model from the prepared editor context, ranks a
//! short list of Perl-shaped continuations, and streams the highest-ranked
//! candidate. It is a foundation for shipping local completions without a
//! network API key while preserving the same `InlineCompletionBackend` contract
//! used by remote providers.

use super::rate_limiter::RateLimiter;
use crate::providers::inline_completion::{
    BackendError, BackendRequest, InlineCompletionBackend, PreparedInlineCompletionContext,
    StreamChunk, StreamControl,
};
use std::collections::HashMap;
use std::sync::Arc;

const DEFAULT_MAX_CHARS: usize = 96;
const DEFAULT_NGRAM_ORDER: usize = 3;

/// Configuration for the small local inline-completion model.
#[derive(Debug, Clone)]
pub struct LocalAiConfig {
    /// Maximum characters to emit for one completion.
    pub max_chars: usize,
    /// Character n-gram order used by the request-local model.
    pub ngram_order: usize,
}

impl Default for LocalAiConfig {
    fn default() -> Self {
        Self { max_chars: DEFAULT_MAX_CHARS, ngram_order: DEFAULT_NGRAM_ORDER }
    }
}

/// A fast local inline-completion backend.
pub struct LocalAiProvider {
    config: LocalAiConfig,
    limiter: Arc<RateLimiter>,
}

impl LocalAiProvider {
    /// Create a new local provider.
    pub fn new(config: LocalAiConfig, limiter: Arc<RateLimiter>) -> Self {
        Self { config, limiter }
    }

    fn complete_context(&self, req: &BackendRequest) -> Vec<String> {
        let model = LocalInlineModel::from_context(&req.context, self.config.ngram_order);
        let max_chars = usize::try_from(req.max_output_tokens)
            .ok()
            .and_then(|tokens| tokens.checked_mul(4))
            .map(|token_chars| token_chars.min(self.config.max_chars))
            .unwrap_or(self.config.max_chars);
        model.completions(max_chars)
    }
}

impl InlineCompletionBackend for LocalAiProvider {
    fn complete(&self, req: &BackendRequest) -> Result<Vec<String>, BackendError> {
        if !self.limiter.try_acquire() {
            return Err(BackendError::RateLimited);
        }

        Ok(self.complete_context(req))
    }

    fn stream(
        &self,
        req: &BackendRequest,
        sink: &mut dyn FnMut(StreamChunk) -> StreamControl,
    ) -> Result<(), BackendError> {
        if !self.limiter.try_acquire() {
            return Err(BackendError::RateLimited);
        }

        for completion in self.complete_context(req) {
            if sink(StreamChunk { text: completion, is_final: true }) == StreamControl::Stop {
                break;
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
struct LocalInlineModel<'a> {
    context: &'a PreparedInlineCompletionContext,
    ngrams: CharNGramModel,
}

impl<'a> LocalInlineModel<'a> {
    fn from_context(context: &'a PreparedInlineCompletionContext, ngram_order: usize) -> Self {
        let corpus = local_training_corpus(context);
        Self { context, ngrams: CharNGramModel::train(corpus.as_str(), ngram_order) }
    }

    fn completions(&self, max_chars: usize) -> Vec<String> {
        let mut candidates = Vec::<RankedLocalCandidate>::new();
        self.push_semantic_candidates(&mut candidates);
        self.push_memory_candidates(&mut candidates, max_chars);
        self.push_ngram_candidate(&mut candidates, max_chars);

        candidates.sort_by(|left, right| {
            right.score.cmp(&left.score).then_with(|| left.text.len().cmp(&right.text.len()))
        });
        candidates.dedup_by(|left, right| left.text == right.text);

        candidates
            .into_iter()
            .filter_map(|candidate| sanitize_local_completion(candidate.text.as_str(), max_chars))
            .take(1)
            .collect()
    }

    fn push_semantic_candidates(&self, candidates: &mut Vec<RankedLocalCandidate>) {
        let prefix = self.context.prefix.trim_end();
        if prefix.ends_with("return") {
            if let Some(variable) = preferred_scalar_variable(&self.context.variables) {
                candidates.push(RankedLocalCandidate::new(format!("{variable};"), 120));
            }
        }

        if prefix.ends_with("return if") || prefix.ends_with("return unless") {
            if let Some(variable) = preferred_condition_variable(&self.context.variables) {
                candidates.push(RankedLocalCandidate::new(format!("{variable};"), 110));
            }
        }

        if prefix.ends_with("use") {
            if !self.context.imports.iter().any(|import| import == "strict") {
                candidates.push(RankedLocalCandidate::new(" strict;".to_string(), 90));
            }
            if !self.context.imports.iter().any(|import| import == "warnings") {
                candidates.push(RankedLocalCandidate::new(" warnings;".to_string(), 85));
            }
        }
    }

    fn push_memory_candidates(&self, candidates: &mut Vec<RankedLocalCandidate>, max_chars: usize) {
        let prefix = self.context.prefix.trim_start();
        if prefix.is_empty() {
            return;
        }

        for line in local_context_lines(self.context) {
            let candidate = line
                .trim_start()
                .strip_prefix(prefix)
                .and_then(|suffix| sanitize_local_completion(suffix, max_chars));
            if let Some(candidate) = candidate {
                candidates.push(RankedLocalCandidate::new(candidate, 70));
            }
        }
    }

    fn push_ngram_candidate(&self, candidates: &mut Vec<RankedLocalCandidate>, max_chars: usize) {
        if let Some(candidate) = self.ngrams.generate(self.context.prefix.as_str(), max_chars) {
            candidates.push(RankedLocalCandidate::new(candidate, 40));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RankedLocalCandidate {
    text: String,
    score: i16,
}

impl RankedLocalCandidate {
    fn new(text: String, score: i16) -> Self {
        Self { text, score }
    }
}

#[derive(Debug)]
struct CharNGramModel {
    order: usize,
    transitions: HashMap<String, HashMap<char, usize>>,
}

impl CharNGramModel {
    fn train(corpus: &str, requested_order: usize) -> Self {
        let order = requested_order.max(1);
        let mut transitions = HashMap::<String, HashMap<char, usize>>::new();
        let chars: Vec<char> = corpus.chars().collect();
        if chars.len() <= order {
            return Self { order, transitions };
        }

        for window in chars.windows(order + 1) {
            let key: String = window[..order].iter().collect();
            let next = window[order];
            *transitions.entry(key).or_default().entry(next).or_default() += 1;
        }

        Self { order, transitions }
    }

    fn generate(&self, seed: &str, max_chars: usize) -> Option<String> {
        if self.transitions.is_empty() || max_chars == 0 {
            return None;
        }

        let mut history: Vec<char> = seed.chars().collect();
        let mut generated = String::new();
        for _ in 0..max_chars {
            let key = suffix_key(&history, self.order)?;
            let next = self.most_likely_next(key.as_str())?;
            if generated.is_empty() && next.is_whitespace() && !next.eq(&' ') {
                return None;
            }
            generated.push(next);
            history.push(next);
            if matches!(next, ';' | '\n' | '}') {
                break;
            }
        }

        sanitize_local_completion(generated.as_str(), max_chars)
    }

    fn most_likely_next(&self, key: &str) -> Option<char> {
        self.transitions.get(key).and_then(|counts| {
            counts
                .iter()
                .max_by(|(left_char, left_count), (right_char, right_count)| {
                    left_count.cmp(right_count).then_with(|| right_char.cmp(left_char))
                })
                .map(|(ch, _count)| *ch)
        })
    }
}

fn local_training_corpus(context: &PreparedInlineCompletionContext) -> String {
    let mut corpus = String::new();
    if let Some(package) = &context.current_package {
        corpus.push_str("package ");
        corpus.push_str(package);
        corpus.push_str(";\n");
    }
    for import in &context.imports {
        corpus.push_str("use ");
        corpus.push_str(import);
        corpus.push_str(";\n");
    }
    if let Some(function) = &context.current_function {
        corpus.push_str("sub ");
        corpus.push_str(function);
        corpus.push_str(" {\n");
    }
    if let Some(previous) = &context.previous_non_empty_line {
        corpus.push_str(previous);
        corpus.push('\n');
    }
    corpus.push_str(context.current_line.as_str());
    corpus.push('\n');
    corpus.push_str(context.prefix.as_str());
    corpus
}

fn local_context_lines(context: &PreparedInlineCompletionContext) -> Vec<&str> {
    let mut lines = Vec::new();
    if let Some(previous) = context.previous_non_empty_line.as_deref() {
        lines.push(previous);
    }
    lines.push(context.current_line.as_str());
    lines
}

fn preferred_scalar_variable(variables: &[String]) -> Option<&str> {
    variables
        .iter()
        .find(|variable| variable.starts_with('$') && variable.as_str() != "$self")
        .or_else(|| variables.iter().find(|variable| variable.starts_with('$')))
        .map(String::as_str)
}

fn preferred_condition_variable(variables: &[String]) -> Option<&str> {
    variables
        .iter()
        .find(|variable| {
            variable.starts_with('$')
                && (variable.contains("ok")
                    || variable.contains("valid")
                    || variable.contains("ready")
                    || variable.contains("success"))
        })
        .or_else(|| variables.iter().find(|variable| variable.starts_with('$')))
        .map(String::as_str)
}

fn suffix_key(history: &[char], order: usize) -> Option<String> {
    if history.len() < order {
        return None;
    }
    Some(history[history.len() - order..].iter().collect())
}

fn sanitize_local_completion(candidate: &str, max_chars: usize) -> Option<String> {
    let trimmed = candidate.trim_end();
    if trimmed.is_empty() || trimmed.contains("<CURSOR>") {
        return None;
    }

    let mut output = String::new();
    for ch in trimmed.chars().take(max_chars) {
        if ch == '\r' {
            continue;
        }
        output.push(ch);
        if matches!(ch, ';' | '\n' | '}') {
            break;
        }
    }

    (!output.trim().is_empty()).then_some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_with_return_prefix() -> PreparedInlineCompletionContext {
        PreparedInlineCompletionContext {
            prefix: "    return ".to_string(),
            current_line: "    return ".to_string(),
            previous_non_empty_line: Some("    my $answer = compute_answer();".to_string()),
            current_function: Some("answer".to_string()),
            current_package: Some("Local::Model".to_string()),
            variables: vec!["$answer".to_string(), "$self".to_string()],
            imports: vec!["strict".to_string(), "warnings".to_string()],
        }
    }

    #[test]
    fn local_model_prefers_visible_return_variable() -> Result<(), Box<dyn std::error::Error>> {
        let provider =
            LocalAiProvider::new(LocalAiConfig::default(), Arc::new(RateLimiter::new(10.0, 1)));
        let req = BackendRequest {
            context: context_with_return_prefix(),
            max_output_tokens: 16,
            timeout_ms: 10,
        };

        let completions = provider.complete(&req)?;

        assert_eq!(completions, vec!["$answer;".to_string()]);
        Ok(())
    }

    #[test]
    fn local_model_streams_final_chunk() -> Result<(), Box<dyn std::error::Error>> {
        let provider =
            LocalAiProvider::new(LocalAiConfig::default(), Arc::new(RateLimiter::new(10.0, 1)));
        let req = BackendRequest {
            context: context_with_return_prefix(),
            max_output_tokens: 16,
            timeout_ms: 10,
        };
        let mut observed = Vec::new();

        provider.stream(&req, &mut |chunk| {
            observed.push((chunk.text, chunk.is_final));
            StreamControl::Continue
        })?;

        assert_eq!(observed, vec![("$answer;".to_string(), true)]);
        Ok(())
    }

    #[test]
    fn ngram_model_can_replay_local_line_suffix() -> Result<(), Box<dyn std::error::Error>> {
        let context = PreparedInlineCompletionContext {
            prefix: "my $value = ".to_string(),
            current_line: "my $value = ".to_string(),
            previous_non_empty_line: Some("my $value = build_value();".to_string()),
            current_function: None,
            current_package: None,
            variables: vec![],
            imports: vec!["strict".to_string()],
        };
        let model = LocalInlineModel::from_context(&context, 3);

        let completions = model.completions(32);

        assert_eq!(completions.first().map(String::as_str), Some("build_value();"));
        Ok(())
    }
}
