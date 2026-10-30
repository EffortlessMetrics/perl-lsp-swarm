//! AI completion providers for perl-lsp.
//!
//! This crate provides pluggable AI backends for inline code completion.
//! The primary provider is OpenAI-compatible, supporting any endpoint that
//! implements the OpenAI chat completions API with SSE streaming.
//!
//! Response-budget types remain available without the network backend because
//! the shared inline-completion error contract carries their refusal metadata.

pub mod budget;
#[cfg(feature = "ai-provider")]
pub mod destination;
#[cfg(feature = "ai-provider")]
pub mod inflight;
#[cfg(feature = "ai-provider")]
pub mod openai;
#[cfg(feature = "ai-provider")]
pub mod prompt;
#[cfg(feature = "ai-provider")]
pub mod rate_limiter;
#[cfg(feature = "ai-provider")]
pub mod sanitize;
#[cfg(feature = "ai-provider")]
pub mod sse;

#[cfg(feature = "ai-provider")]
pub use destination::{
    ApprovedDestination, DestinationError, credential_may_attach, validate_endpoint,
    validate_endpoint_with_resolver, validate_tcp_attach_host,
    validate_tcp_attach_host_with_resolver,
};
#[cfg(feature = "ai-provider")]
pub use inflight::{InflightCounters, InflightGate, InflightPermit};
#[cfg(feature = "ai-provider")]
pub use openai::{OpenAiConfig, OpenAiProvider};
#[cfg(feature = "ai-provider")]
pub use rate_limiter::RateLimiter;
