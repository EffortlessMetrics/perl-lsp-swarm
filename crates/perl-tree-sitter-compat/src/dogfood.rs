//! Adoption receipts for the facade-first compatibility path.

use std::sync::atomic::{AtomicU64, Ordering};

static FACADE_TREES: AtomicU64 = AtomicU64::new(0);
static RECOVERED_TREES: AtomicU64 = AtomicU64::new(0);
static NATIVE_FALLBACKS: AtomicU64 = AtomicU64::new(0);

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
}

/// Return process-local adoption counters.
#[must_use]
pub fn adoption_stats() -> AdoptionStats {
    AdoptionStats {
        facade_trees: FACADE_TREES.load(Ordering::Relaxed),
        recovered_trees: RECOVERED_TREES.load(Ordering::Relaxed),
        native_fallbacks: NATIVE_FALLBACKS.load(Ordering::Relaxed),
    }
}

pub(crate) fn record_facade_tree(recovered: bool) {
    FACADE_TREES.fetch_add(1, Ordering::Relaxed);
    if recovered {
        RECOVERED_TREES.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn record_native_fallback() {
    NATIVE_FALLBACKS.fetch_add(1, Ordering::Relaxed);
}
