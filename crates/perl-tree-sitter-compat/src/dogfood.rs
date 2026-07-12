//! Adoption receipts for the facade-first compatibility path.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static FACADE_TREES: AtomicU64 = AtomicU64::new(0);
static RECOVERED_TREES: AtomicU64 = AtomicU64::new(0);
static NATIVE_FALLBACKS: AtomicU64 = AtomicU64::new(0);
static FACADE_DURATION_US: AtomicU64 = AtomicU64::new(0);

/// Process-local counters for the facade-first adapter path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AdoptionStats {
    /// Number of trees produced by the Rust-native facade.
    pub facade_trees: u64,
    /// Number of facade trees that contained recovery diagnostics or nodes.
    pub recovered_trees: u64,
    /// Number of trees produced by the native fallback path.
    pub native_fallbacks: u64,
    /// Aggregate facade parse-and-projection time in microseconds.
    pub facade_duration_us: u64,
}

/// Return process-local adoption counters.
#[must_use]
pub fn adoption_stats() -> AdoptionStats {
    AdoptionStats {
        facade_trees: FACADE_TREES.load(Ordering::Relaxed),
        recovered_trees: RECOVERED_TREES.load(Ordering::Relaxed),
        native_fallbacks: NATIVE_FALLBACKS.load(Ordering::Relaxed),
        facade_duration_us: FACADE_DURATION_US.load(Ordering::Relaxed),
    }
}

pub(crate) fn record_facade_tree(recovered: bool, duration_us: u64) {
    FACADE_TREES.fetch_add(1, Ordering::Relaxed);
    FACADE_DURATION_US.fetch_add(duration_us, Ordering::Relaxed);
    if recovered {
        RECOVERED_TREES.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn record_native_fallback() {
    NATIVE_FALLBACKS.fetch_add(1, Ordering::Relaxed);
}

/// Convert a monotonic duration into a non-zero receipt value.
pub(crate) fn elapsed_us(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).map_or(u64::MAX, |value| value.max(1))
}
