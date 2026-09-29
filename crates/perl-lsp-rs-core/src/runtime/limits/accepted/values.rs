//! Typed accepted magnitudes. Consumers receive these, not client integers.

use std::time::Duration;

/// Accepted result-count budget applied at one canonical composition boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ResultCap(usize);

impl ResultCap {
    pub(crate) const fn from_accepted(count: usize) -> Self {
        Self(count)
    }

    /// Accepted count. This is not a client-requested integer.
    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

/// Accepted wall-clock deadline for one operation family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Deadline(Duration);

impl Deadline {
    pub(crate) const fn from_accepted(duration: Duration) -> Self {
        Self(duration)
    }

    pub(crate) const fn get(self) -> Duration {
        self.0
    }

    pub(crate) fn as_millis(self) -> u128 {
        self.0.as_millis()
    }
}

/// Accepted retained-byte budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ByteBudget(usize);

impl ByteBudget {
    pub(crate) const fn from_accepted(bytes: usize) -> Self {
        Self(bytes)
    }

    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

/// Accepted count threshold (storm, per-file symbols, and similar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CountThreshold(usize);

impl CountThreshold {
    pub(crate) const fn from_accepted(count: usize) -> Self {
        Self(count)
    }

    pub(crate) const fn get(self) -> usize {
        self.0
    }
}
