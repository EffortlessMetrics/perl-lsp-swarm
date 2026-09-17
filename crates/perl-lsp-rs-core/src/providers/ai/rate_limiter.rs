//! Simple token-bucket rate limiter for AI API calls.

use std::sync::Mutex;
use std::time::Instant;

/// A simple rate limiter using token bucket algorithm.
///
/// # Not a concurrency control
///
/// This bounds how many requests may *start* per second. A token is consumed
/// at dispatch and never returned, so the burst allowance says nothing about
/// how many requests are simultaneously active — several callers can each take
/// a token and then all remain in flight. The live-request ceiling
/// (`maxInflight`) is [`super::inflight::InflightGate`] (`#8300`).
pub struct RateLimiter {
    state: Mutex<RateLimiterState>,
}

struct RateLimiterState {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// `rps` is the maximum requests per second.
    ///
    /// `burst_capacity` is the token-bucket refill ceiling — how many requests
    /// may start back-to-back after an idle period. It is deliberately *not*
    /// named for concurrency: it cannot bound simultaneously active requests.
    pub fn new(rps: f64, burst_capacity: u32) -> Self {
        Self {
            state: Mutex::new(RateLimiterState {
                tokens: f64::from(burst_capacity),
                max_tokens: f64::from(burst_capacity),
                refill_rate: rps,
                last_refill: Instant::now(),
            }),
        }
    }

    /// Try to acquire a token. Returns true if allowed, false if rate-limited.
    pub fn try_acquire(&self) -> bool {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };

        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        state.tokens = (state.tokens + elapsed * state.refill_rate).min(state.max_tokens);
        state.last_refill = now;

        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_burst() {
        let limiter = RateLimiter::new(1.0, 3);
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
    }

    #[test]
    fn blocks_after_burst() {
        let limiter = RateLimiter::new(1.0, 1);
        assert!(limiter.try_acquire());
        assert!(!limiter.try_acquire());
    }
}
