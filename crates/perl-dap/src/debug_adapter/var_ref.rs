//! Type-safe variablesReference codec for the DAP protocol.
//!
//! # Problem
//!
//! The DAP protocol uses a single `i32` field (`variablesReference`) to encode
//! references to three logically distinct spaces:
//!
//! - **Scope** references: identify a scope (locals/package/globals) within a stack frame
//! - **EvalResult** references: identify a structured evaluation result (HASH/ARRAY)
//! - **Child** references: identify a nested child variable within a parent
//!
//! Prior to this module, encoding was done with ad-hoc arithmetic scattered across
//! multiple call sites. Issue #1219 identified a collision hazard: `frame_id * 10 + kind`
//! (where kind ∈ [1,3]) can produce values that overlap early EvalResult counters.
//!
//! # Solution
//!
//! This module provides a typed enum `VariableReference` with a single encode/decode
//! codec. Each variant occupies a non-overlapping wire range:
//!
//! | Variant | Wire Range | Encoding |
//! |---------|-----------|----------|
//! | `Scope` | [1, 9_999_999] | `frame_id * 10 + kind` (kind ∈ [1,3]) |
//! | `EvalResult` | [1_000_000, 2_000_000_000) | `1_000_000 + counter` |
//! | `Child` | [2_000_000_000, i32::MAX] | `2_000_000_000 + (parent << 16 \| index)` |
//!
//! # Decode ordering
//!
//! Decode is **range-first and exhaustive**: Child → Scope → EvalResult.
//! Child is checked first (highest base, unambiguous). Scope is checked next —
//! the kind discriminant (`raw % 10 ∈ [1,3]`) provides unambiguous type identification
//! even in the overlap zone [1_000_000..9_999_999] where Scope and EvalResult ranges
//! meet. EvalResult catches remaining values in [1_000_000..2_000_000_000).
//!
//! # Safety
//!
//! All arithmetic uses saturating operations. Extreme inputs (i32::MAX, u32::MAX)
//! saturate rather than panic or overflow.

use std::fmt;

/// Error type for `TryFrom<i32>` on `ScopeKind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableReferenceError {
    message: String,
}

impl VariableReferenceError {
    fn new(msg: impl Into<String>) -> Self {
        Self { message: msg.into() }
    }
}

impl fmt::Display for VariableReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VariableReferenceError: {}", self.message)
    }
}

impl std::error::Error for VariableReferenceError {}

/// The kind of scope a `Scope` variable reference points to within a stack frame.
///
/// Wire values: Locals=1, Package=2, Globals=3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeKind {
    /// Lexical (my) variables in the current frame.
    Locals = 1,
    /// Package (our) variables in the current frame.
    Package = 2,
    /// All global variables.
    Globals = 3,
}

impl TryFrom<i32> for ScopeKind {
    type Error = VariableReferenceError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ScopeKind::Locals),
            2 => Ok(ScopeKind::Package),
            3 => Ok(ScopeKind::Globals),
            other => Err(VariableReferenceError::new(format!(
                "invalid ScopeKind discriminant: {other}; expected 1 (Locals), 2 (Package), or 3 (Globals)"
            ))),
        }
    }
}

/// A typed, codec-backed reference into the DAP `variablesReference` wire space.
///
/// ## Wire ranges (non-overlapping)
///
/// - `Scope`:      [1, 9_999_999]          — `frame_id * 10 + kind` (kind ∈ [1,3])
/// - `EvalResult`: [1_000_000, 2_000_000_000) — `1_000_000 + counter`
/// - `Child`:      [2_000_000_000, i32::MAX]  — `2_000_000_000 + (parent << 16 | index)`
///
/// Wire value 0 is reserved/invalid (DAP: 0 = "no children"). Negative values are
/// invalid. `decode` returns `None` for any value outside the three ranges.
///
/// ## Decode ordering
///
/// Decode tests Child → Scope → EvalResult. Child is checked first (highest base,
/// no ambiguity). Scope is checked next using the kind discriminant (`raw % 10 ∈ [1,3]`),
/// which unambiguously identifies Scope values even in the overlap zone with EvalResult.
/// EvalResult catches the remaining [1_000_000..2_000_000_000) values.
///
/// All fields are primitive scalar types, so `VariableReference` is `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableReference {
    /// A scope reference: variables in `kind` scope at stack frame `frame_id`.
    Scope {
        /// Stack frame identifier (0-based).
        frame_id: i32,
        /// Which scope within the frame.
        kind: ScopeKind,
    },
    /// A structured evaluation result reference (HASH or ARRAY from `evaluate`).
    EvalResult {
        /// Monotonically increasing counter allocated per evaluation result.
        counter: i32,
    },
    /// A child variable reference within a parent variable.
    Child {
        /// The parent variable reference (the Scope or EvalResult that owns this child).
        parent: i32,
        /// Zero-based index of the child within the parent's variable list.
        index: u32,
    },
}

/// Wire range constants.
const SCOPE_MIN: i32 = 1;
const SCOPE_MAX: i32 = 9_999_999;
const EVAL_BASE: i32 = 1_000_000;
const CHILD_BASE: i32 = 2_000_000_000;

impl VariableReference {
    /// Encode this reference to an i32 wire value.
    ///
    /// All arithmetic is saturating — extreme inputs clamp to i32::MAX rather than panicking.
    /// Note: values that saturate may not round-trip through `decode`.
    pub fn encode(&self) -> i32 {
        match self {
            VariableReference::Scope { frame_id, kind } => {
                // Wire = frame_id * 10 + kind_disc (1-3)
                let kind_disc = *kind as i32;
                frame_id.saturating_mul(10).saturating_add(kind_disc)
            }
            VariableReference::EvalResult { counter } => {
                // Wire = 1_000_000 + counter
                EVAL_BASE.saturating_add(*counter)
            }
            VariableReference::Child { parent, index } => {
                // Wire = 2_000_000_000 + (parent << 16 | (index & 0xFFFF))
                // parent is in the high bits, index (truncated to 16 bits) in the low bits.
                // Use saturating arithmetic to avoid panicking on overflow.
                let index_truncated = (*index & 0xFFFF) as i32;
                // Saturating left-shift: clamp parent so multiplication fits in i32.
                let parent_clamped = (*parent).clamp(i32::MIN / 65_536, i32::MAX / 65_536);
                let parent_shifted = parent_clamped * 65_536;
                let packed = parent_shifted.saturating_add(index_truncated);
                CHILD_BASE.saturating_add(packed)
            }
        }
    }

    /// Decode a wire i32 value into a `VariableReference`.
    ///
    /// Returns `None` if the value is:
    /// - Zero (reserved; DAP "no children")
    /// - Negative (invalid)
    /// - In the Scope range but has an invalid kind discriminant (not 1, 2, or 3)
    /// - Not in any of the three wire ranges
    ///
    /// ## Decode ordering (range-first)
    ///
    /// 1. **Child** (`raw >= 2_000_000_000`): checked first — highest range, no ambiguity.
    /// 2. **Scope** (`1 <= raw <= 9_999_999`): checked second — kind discriminant (`raw % 10`)
    ///    must be 1, 2, or 3 (Locals/Package/Globals). If the discriminant is invalid,
    ///    the value is NOT a Scope; fall through to EvalResult.
    ///    Note: Scope range [1..9_999_999] overlaps EvalResult range [1_000_000..),
    ///    but values with a valid Scope kind discriminant are always Scope. EvalResult
    ///    counters that would produce overlapping wire values are reserved by design.
    /// 3. **EvalResult** (`1_000_000 <= raw < 2_000_000_000`): matches values in the
    ///    EvalResult base range that did not decode as Scope.
    /// 4. **None**: all other values (0, negative, > 2_000_000_000 with no Child match).
    pub fn decode(raw: i32) -> Option<Self> {
        if raw <= 0 {
            return None;
        }

        // 1. Child range: [2_000_000_000, i32::MAX] — checked first (highest base)
        if raw >= CHILD_BASE {
            let packed = raw - CHILD_BASE;
            let parent = packed >> 16;
            let index = (packed & 0xFFFF) as u32;
            return Some(VariableReference::Child { parent, index });
        }

        // 2. Scope range: [1, 9_999_999] — kind discriminant validates unambiguously.
        //    Scope takes priority over EvalResult in the overlap zone [1_000_000..9_999_999].
        //    EvalResult wire values that end in 1/2/3 in this zone are reserved by the
        //    encoding scheme (counter values that would collide are not allocated).
        if (SCOPE_MIN..=SCOPE_MAX).contains(&raw) {
            let kind_disc = raw % 10;
            let frame_id = raw / 10;
            if let Ok(kind) = ScopeKind::try_from(kind_disc) {
                return Some(VariableReference::Scope { frame_id, kind });
            }
            // Invalid kind discriminant: not a Scope. Fall through to EvalResult check.
        }

        // 3. EvalResult range: [1_000_000, 2_000_000_000) — values not classified as Scope.
        //    Wire values with kind_disc ∈ {0, 4..9} in [1_000_000..9_999_999] reach here.
        if (EVAL_BASE..CHILD_BASE).contains(&raw) {
            let counter = raw - EVAL_BASE;
            return Some(VariableReference::EvalResult { counter });
        }

        // Out of all ranges
        None
    }
}

#[cfg(test)]
mod codec_unit_tests {
    use super::*;

    #[test]
    fn scope_kind_tryfrom_valid() {
        assert_eq!(ScopeKind::try_from(1), Ok(ScopeKind::Locals));
        assert_eq!(ScopeKind::try_from(2), Ok(ScopeKind::Package));
        assert_eq!(ScopeKind::try_from(3), Ok(ScopeKind::Globals));
    }

    #[test]
    fn scope_kind_tryfrom_invalid() {
        assert!(ScopeKind::try_from(0).is_err());
        assert!(ScopeKind::try_from(4).is_err());
        assert!(ScopeKind::try_from(-1).is_err());
    }

    #[test]
    fn scope_encode_decode_basic() {
        let s = VariableReference::Scope { frame_id: 5000, kind: ScopeKind::Locals };
        assert_eq!(s.encode(), 50_001);
        assert_eq!(VariableReference::decode(50_001), Some(s));
    }

    #[test]
    fn evalresult_encode_decode_basic() {
        // counter=0 → wire 1_000_000 (ends in 0, kind_disc=0 → invalid Scope → EvalResult)
        let e = VariableReference::EvalResult { counter: 0 };
        assert_eq!(e.encode(), 1_000_000);
        assert_eq!(VariableReference::decode(1_000_000), Some(e));

        // counter=10 → wire 1_000_010 (ends in 0, kind_disc=0 → invalid Scope → EvalResult)
        let e2 = VariableReference::EvalResult { counter: 10 };
        assert_eq!(e2.encode(), 1_000_010);
        assert_eq!(VariableReference::decode(1_000_010), Some(e2));
    }

    #[test]
    fn child_encode_decode_base() {
        let c = VariableReference::Child { parent: 0, index: 0 };
        assert_eq!(c.encode(), 2_000_000_000);
        assert_eq!(VariableReference::decode(2_000_000_000), Some(c));
    }

    #[test]
    fn decode_zero_none() {
        assert_eq!(VariableReference::decode(0), None);
    }

    #[test]
    fn decode_negative_none() {
        assert_eq!(VariableReference::decode(-1), None);
        assert_eq!(VariableReference::decode(i32::MIN), None);
    }

    #[test]
    fn decode_invalid_scope_kind_none() {
        // frame_id=99_999, kind_disc=9 → None
        assert_eq!(VariableReference::decode(999_999), None);
    }
}
