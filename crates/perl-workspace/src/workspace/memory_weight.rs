//! Checked arithmetic and conservative collection-weight helpers for workspace
//! memory evidence.
#![expect(
    missing_docs,
    reason = "policy:17198-memory-evidence: weight helper identifiers mirror std sizing APIs"
)]

use std::collections::HashMap;
use std::mem::size_of;

/// Overflow or other non-pass weight aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryWeightError {
    Overflow,
}

/// Logical owned bytes vs observable collection capacity for one retained owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CollectionWeight {
    pub live_entries: usize,
    pub logical_bytes: usize,
    pub capacity_bytes: usize,
}

impl CollectionWeight {
    pub const fn empty() -> Self {
        Self { live_entries: 0, logical_bytes: 0, capacity_bytes: 0 }
    }

    pub fn merge(&self, other: Self) -> Result<Self, MemoryWeightError> {
        Ok(Self {
            live_entries: checked_add(self.live_entries, other.live_entries)?,
            logical_bytes: checked_add(self.logical_bytes, other.logical_bytes)?,
            capacity_bytes: checked_add(self.capacity_bytes, other.capacity_bytes)?,
        })
    }
}

pub fn checked_add(left: usize, right: usize) -> Result<usize, MemoryWeightError> {
    left.checked_add(right).ok_or(MemoryWeightError::Overflow)
}

pub fn checked_mul(left: usize, right: usize) -> Result<usize, MemoryWeightError> {
    left.checked_mul(right).ok_or(MemoryWeightError::Overflow)
}

pub fn string_logical(s: &str) -> usize {
    s.len()
}

pub fn vec_logical<T>(vec: &[T]) -> Result<usize, MemoryWeightError> {
    checked_mul(vec.len(), size_of::<T>())
}

pub fn vec_capacity<T>(vec: &Vec<T>) -> Result<usize, MemoryWeightError> {
    checked_mul(vec.capacity(), size_of::<T>())
}

/// Conservative hash-table slack: bucket slots × pair size (ignores owned key bytes).
pub fn hashmap_capacity_slack<K, V>(map: &HashMap<K, V>) -> Result<usize, MemoryWeightError> {
    checked_mul(map.capacity(), size_of::<(K, V)>())
}

pub fn accumulate<I>(mut iter: I) -> Result<usize, MemoryWeightError>
where
    I: Iterator<Item = Result<usize, MemoryWeightError>>,
{
    iter.try_fold(0usize, |acc, next| checked_add(acc, next?))
}
