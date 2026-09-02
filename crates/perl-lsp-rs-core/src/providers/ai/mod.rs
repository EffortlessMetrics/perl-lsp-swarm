//! AI completion providers for perl-lsp.
//!
//! This crate provides pluggable AI backends for inline code completion.
//! The primary provider is OpenAI-compatible, supporting any endpoint that
//! implements the OpenAI chat completions API with SSE streaming.

pub mod destination;
pub mod inflight;
pub mod openai;
pub mod prompt;
pub mod rate_limiter;
pub mod sanitize;
pub mod sse;

pub use destination::{
    ApprovedDestination, DestinationError, credential_may_attach, validate_endpoint,
    validate_endpoint_with_resolver, validate_tcp_attach_host,
    validate_tcp_attach_host_with_resolver,
};
pub use inflight::{InflightCounters, InflightGate, InflightPermit};
pub use openai::{OpenAiConfig, OpenAiProvider};
pub use rate_limiter::RateLimiter;
