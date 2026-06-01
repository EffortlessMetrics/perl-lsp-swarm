use super::PreparedInlineCompletionContext;

// ── AI backend interface ─────────────────────────────────────────────────────

/// Error type for backend operations.
#[derive(Debug)]
pub enum BackendError {
    /// Network or IO error.
    Transport(String),
    /// Authentication failure (bad key, expired token).
    Auth(String),
    /// Provider returned an error response.
    Provider(String),
    /// Request timed out.
    Timeout,
    /// Rate limit exceeded.
    RateLimited,
    /// Request was cancelled.
    Cancelled,
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(msg) => write!(f, "transport error: {}", msg),
            Self::Auth(msg) => write!(f, "auth error: {}", msg),
            Self::Provider(msg) => write!(f, "provider error: {}", msg),
            Self::Timeout => write!(f, "request timed out"),
            Self::RateLimited => write!(f, "rate limit exceeded"),
            Self::Cancelled => write!(f, "request cancelled"),
        }
    }
}

impl std::error::Error for BackendError {}

/// Request payload sent to an AI completion backend.
#[derive(Debug, Clone)]
pub struct BackendRequest {
    /// Prepared context from the current buffer.
    pub context: PreparedInlineCompletionContext,
    /// Maximum tokens to generate.
    pub max_output_tokens: u32,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
}

/// A chunk emitted by a streaming backend.
#[derive(Debug, Clone)]
pub struct StreamChunk {
    /// Cumulative candidate text so far (NOT a delta).
    pub text: String,
    /// Whether this is the final chunk.
    pub is_final: bool,
}

/// Control signal returned by the stream sink callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamControl {
    /// Continue receiving chunks.
    Continue,
    /// Stop the stream early.
    Stop,
}

/// Trait for AI inline completion backends.
///
/// Implementations provide streaming token generation. The default `complete()`
/// method buffers the stream into a one-shot result, so backends only need to
/// implement `stream()`.
///
/// The trait is sync and callback-based to keep this crate dependency-light
/// and runtime-agnostic. Network I/O happens in the provider crate.
pub trait InlineCompletionBackend: Send + Sync {
    /// One-shot completion: returns the final candidate texts.
    ///
    /// Default implementation buffers the stream.
    fn complete(&self, req: &BackendRequest) -> Result<Vec<String>, BackendError> {
        let mut final_text = String::new();
        self.stream(req, &mut |chunk| {
            final_text = chunk.text.clone();
            if chunk.is_final { StreamControl::Stop } else { StreamControl::Continue }
        })?;
        Ok(if final_text.is_empty() { vec![] } else { vec![final_text] })
    }

    /// Stream completion chunks to a callback sink.
    ///
    /// Each `StreamChunk.text` is **cumulative** — the full candidate so far,
    /// not a delta. The sink returns `StreamControl::Stop` to cancel early.
    fn stream(
        &self,
        req: &BackendRequest,
        sink: &mut dyn FnMut(StreamChunk) -> StreamControl,
    ) -> Result<(), BackendError>;
}
