//! Fact-class selection.
//!
//! Extracting every fact class for a large workspace is expensive. Callers that
//! only need, say, package and symbol facts should not pay for POD and dist
//! metadata extraction. [`FactClasses`] is a small dependency-free bitset that
//! lets a query request exactly the classes it needs; producers honour it and
//! skip the rest, recording nothing (not fabricating empties) for unrequested
//! classes.

use serde::{Deserialize, Serialize};

/// A bitset over the extractable project-fact classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactClasses(u32);

impl FactClasses {
    /// File facts (paths, roles, digests, line index, parse status).
    pub const FILES: FactClasses = FactClasses(1 << 0);
    /// Package declarations.
    pub const PACKAGES: FactClasses = FactClasses(1 << 1);
    /// Sub/method/constant/typeglob symbol facts.
    pub const SYMBOLS: FactClasses = FactClasses(1 << 2);
    /// `use`/`require`/`no`/`use lib` import facts.
    pub const IMPORTS: FactClasses = FactClasses(1 << 3);
    /// Exporter-style export facts.
    pub const EXPORTS: FactClasses = FactClasses(1 << 4);
    /// Module resolution / inheritance facts.
    pub const MODULES: FactClasses = FactClasses(1 << 5);
    /// POD facts.
    pub const POD: FactClasses = FactClasses(1 << 6);
    /// Test facts.
    pub const TESTS: FactClasses = FactClasses(1 << 7);
    /// Distribution metadata facts.
    pub const DIST: FactClasses = FactClasses(1 << 8);

    const ALL_BITS: u32 = (1 << 9) - 1;

    /// The empty set.
    #[must_use]
    pub const fn empty() -> Self {
        FactClasses(0)
    }

    /// Every known fact class.
    #[must_use]
    pub const fn all() -> Self {
        FactClasses(Self::ALL_BITS)
    }

    /// Whether `self` contains every class in `other`.
    #[must_use]
    pub const fn contains(self, other: FactClasses) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Whether `self` shares any class with `other`.
    #[must_use]
    pub const fn intersects(self, other: FactClasses) -> bool {
        (self.0 & other.0) != 0
    }

    /// The union of two sets.
    #[must_use]
    pub const fn union(self, other: FactClasses) -> Self {
        FactClasses(self.0 | other.0)
    }

    /// Add `other`'s classes in place.
    pub fn insert(&mut self, other: FactClasses) {
        self.0 |= other.0;
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::BitOr for FactClasses {
    type Output = FactClasses;
    fn bitor(self, rhs: FactClasses) -> FactClasses {
        self.union(rhs)
    }
}

impl core::ops::BitOrAssign for FactClasses {
    fn bitor_assign(&mut self, rhs: FactClasses) {
        self.insert(rhs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_all() {
        assert!(FactClasses::empty().is_empty());
        assert!(!FactClasses::all().is_empty());
        assert!(FactClasses::all().contains(FactClasses::FILES));
        assert!(FactClasses::all().contains(FactClasses::DIST));
    }

    #[test]
    fn contains_is_subset_semantics() {
        let set = FactClasses::FILES | FactClasses::SYMBOLS;
        assert!(set.contains(FactClasses::FILES));
        assert!(set.contains(FactClasses::SYMBOLS));
        assert!(set.contains(FactClasses::FILES | FactClasses::SYMBOLS));
        assert!(!set.contains(FactClasses::POD));
        assert!(!set.contains(FactClasses::FILES | FactClasses::POD));
    }

    #[test]
    fn intersects() {
        let set = FactClasses::FILES | FactClasses::SYMBOLS;
        assert!(set.intersects(FactClasses::FILES));
        assert!(set.intersects(FactClasses::SYMBOLS | FactClasses::POD));
        assert!(!set.intersects(FactClasses::POD | FactClasses::DIST));
    }

    #[test]
    fn insert_and_bitor_assign() {
        let mut set = FactClasses::empty();
        set.insert(FactClasses::FILES);
        set |= FactClasses::POD;
        assert!(set.contains(FactClasses::FILES));
        assert!(set.contains(FactClasses::POD));
        assert!(!set.contains(FactClasses::SYMBOLS));
    }

    #[test]
    fn all_contains_each_known_class() {
        for class in [
            FactClasses::FILES,
            FactClasses::PACKAGES,
            FactClasses::SYMBOLS,
            FactClasses::IMPORTS,
            FactClasses::EXPORTS,
            FactClasses::MODULES,
            FactClasses::POD,
            FactClasses::TESTS,
            FactClasses::DIST,
        ] {
            assert!(FactClasses::all().contains(class));
        }
    }
}
