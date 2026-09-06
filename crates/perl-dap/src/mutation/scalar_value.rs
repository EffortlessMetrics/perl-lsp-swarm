//! Scalar mutation value algebra (#10736).
//!
//! One typed scalar value model consumed by every variable-edit path. The
//! types here carry *data*, never Perl source and never raw DAP request text:
//! a [`MutationValue`] is what survives below the value-parser boundary
//! (#10745), so a string that happens to look like `$foo`, `system("x")`, or
//! `json:[1]` is ordinary inert string data with no interpolation, command, or
//! structured meaning.
//!
//! # Exactness
//!
//! Numbers never pass through `f64`. Both exact cohorts keep canonical decimal
//! *text*, which is what lets a 256-digit integer or a high-precision decimal
//! round-trip without binary floating-point rounding, and what keeps the
//! semantic value independent of the client's original spelling.
//!
//! # Single exact-decimal authority
//!
//! [`ExactDecimal`] is deliberately **not** redefined here. It is the crate's
//! one exact-decimal carrier, already checked in for the structured profile
//! (#11327), and the scalar profile reuses it rather than cloning a rival
//! number authority — which is what #11327's own start conditions asked for.
//! See [`crate::mutation`] for the shared-ownership note.

use std::fmt;

use serde::Serialize;

use super::structured_value::ExactDecimal;

/// Schema version of the scalar mutation value profile (`MutationValueText.v1`).
pub const MUTATION_SCALAR_VALUE_SCHEMA_VERSION: u32 = 1;

/// Exact integer kept as canonical decimal text.
///
/// Canonical form is `0`, or an optional `-` followed by a non-zero leading
/// digit and any further digits. Construction is checked, so a non-canonical
/// spelling cannot enter the exact model outside the value parser.
///
/// # Size is the parser's bound, not this type's
///
/// This type admits canonical text of any length and deliberately enforces no
/// digit budget. `MutationValueText.v1`'s limits — 256 significant digits,
/// 65_536 input bytes, 32_768 decoded string bytes — are *profile* policy
/// owned by the value parser (#10745), exactly as the structured profile's
/// budgets live in [`StructuredMutationLimits`](super::StructuredMutationLimits)
/// rather than in its value types. Duplicating them here would create a second,
/// unversioned copy of a limit that a reviewed profile bump is supposed to move
/// in one place. [`significant_digits`](Self::significant_digits) exists so the
/// parser charges its budget against this type's own reading of the text.
///
/// # Negative zero
///
/// `-0` is **not** canonical integer text and is refused here. Perl's integer
/// negative zero is numerically indistinguishable from `0`, so the value
/// parser normalizes the client spelling `-0` to `0` before admission and the
/// domain keeps exactly one representation of that value. Signed zero survives
/// only in the decimal cohort, where `-0.0` is a distinct spelling Perl can
/// observe.
#[derive(Clone, PartialEq, Eq)]
pub struct ExactInteger {
    /// Canonical form: `0`, or `-?[1-9][0-9]*`.
    canonical: String,
}

impl ExactInteger {
    /// Admit canonical decimal integer text (`0` or `-?[1-9][0-9]*`).
    ///
    /// Returns `None` for any non-canonical spelling, including `+5`, `007`,
    /// `-0`, `1_000`, an empty string, a bare sign, or any trailing text. The
    /// value parser owns normalization; this type owns admission.
    pub fn admitted(canonical: &str) -> Option<Self> {
        if !is_canonical_integer(canonical) {
            return None;
        }
        Some(Self { canonical: canonical.to_string() })
    }

    /// Canonical text form of this exact integer.
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// Count of significant decimal digits, ignoring the sign.
    ///
    /// This is the quantity the value parser's digit budget is charged
    /// against; it is exposed so the budget cannot be recomputed from a
    /// different reading of the same text.
    pub fn significant_digits(&self) -> usize {
        self.canonical.bytes().filter(u8::is_ascii_digit).count()
    }

    /// Whether this integer is strictly negative.
    pub fn is_negative(&self) -> bool {
        self.canonical.starts_with('-')
    }
}

impl fmt::Debug for ExactInteger {
    /// Redacted: the digits are an assigned debuggee value, so `{:?}` reports
    /// the shape and not the number. Deriving `Debug` here would have reopened
    /// through diagnostics exactly what withholding `Serialize` closed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ExactInteger(<{} digits redacted>)", self.significant_digits())
    }
}

/// Canonical integer grammar: `0`, or optional `-` then a non-zero leading
/// digit followed by any digits. No `+`, separators, leading zeros, or `-0`.
fn is_canonical_integer(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    if bytes.first() == Some(&b'-') {
        index += 1;
    }
    let digits_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index != bytes.len() || index == digits_start {
        return false;
    }
    // `0` is canonical; `-0`, `00`, and `0…` are not.
    if bytes[digits_start] == b'0' {
        return index - digits_start == 1 && digits_start == 0;
    }
    true
}

/// Stable discriminant of a scalar value cohort, safe to place in a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MutationValueKind {
    /// Perl `undef`.
    Undef,
    /// Exact integer.
    ExactInteger,
    /// Exact decimal/exponent number.
    ExactDecimal,
    /// Unicode string data.
    UnicodeString,
}

/// Which versioned value profile admitted a value.
///
/// The scalar and structured profiles are mechanically separate: no conversion
/// exists between [`MutationValue`] and
/// [`StructuredValue`](super::structured_value::StructuredValue), and this
/// discriminant is what a receipt or backend records so one profile can never
/// be presented as the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MutationValueProfile {
    /// `MutationValueText.v1` — the scalar core (#8364).
    ScalarV1,
    /// `MutationStructuredValue.v1` — the optional structured train (#11326).
    StructuredV1,
}

impl MutationValueProfile {
    /// Schema version pinned for this profile.
    pub fn schema_version(self) -> u32 {
        match self {
            Self::ScalarV1 => MUTATION_SCALAR_VALUE_SCHEMA_VERSION,
            Self::StructuredV1 => super::structured_value::MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION,
        }
    }
}

/// One admitted scalar mutation value.
///
/// This is typed data below the parser boundary. It retains no raw client
/// text, no spelling, and no Perl source; string content is inert regardless
/// of what it resembles.
///
/// # Which numeric cohort a value takes
///
/// The two exact cohorts overlap by construction: `ExactDecimal::admitted("5")`
/// succeeds, because canonical JSON-number text includes bare integers. That
/// overlap lives in the shared carrier and is not resolved here — it is
/// resolved by the value parser (#10745), whose grammar makes the choice
/// deterministic:
///
/// ```text
/// at least one `.` or exponent  -> ExactDecimal
/// otherwise                     -> ExactInteger
/// ```
///
/// So `5` is always [`MutationValue::ExactInteger`] and never
/// [`MutationValue::ExactDecimal`], and one client spelling never has two
/// representations. A producer ignoring this rule would create exactly the
/// ambiguity an exact model exists to avoid.
#[derive(Clone, PartialEq)]
pub enum MutationValue {
    /// Perl `undef`.
    Undef,
    /// Exact integer, canonical text, no `f64`.
    ExactInteger(ExactInteger),
    /// Exact decimal/exponent number, canonical text, no `f64`.
    ExactDecimal(ExactDecimal),
    /// Unicode string data.
    UnicodeString(String),
}

impl fmt::Debug for MutationValue {
    /// Redacted: the payload is debuggee data. Reports cohort and size only,
    /// so a `tracing` span or a test assertion cannot print an assigned value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undef => f.write_str("Undef"),
            Self::ExactInteger(integer) => {
                write!(f, "ExactInteger(<{} digits redacted>)", integer.significant_digits())
            }
            Self::ExactDecimal(decimal) => {
                write!(f, "ExactDecimal(<{} bytes redacted>)", decimal.canonical().len())
            }
            Self::UnicodeString(text) => {
                write!(f, "UnicodeString(<{} bytes redacted>)", text.len())
            }
        }
    }
}

impl MutationValue {
    /// Stable cohort discriminant.
    pub fn kind(&self) -> MutationValueKind {
        match self {
            Self::Undef => MutationValueKind::Undef,
            Self::ExactInteger(_) => MutationValueKind::ExactInteger,
            Self::ExactDecimal(_) => MutationValueKind::ExactDecimal,
            Self::UnicodeString(_) => MutationValueKind::UnicodeString,
        }
    }

    /// The profile that admits this value. Always the scalar core.
    pub fn profile(&self) -> MutationValueProfile {
        MutationValueProfile::ScalarV1
    }

    /// Receipt-safe projection: cohort and bounded size only.
    ///
    /// The private payload — the assigned number or string content — never
    /// appears, because a mutation value is debuggee data that must not leak
    /// into logs, receipts, or diagnostics.
    pub fn receipt_projection(&self) -> MutationValueReceipt {
        let payload_bytes = match self {
            Self::Undef => 0,
            Self::ExactInteger(integer) => integer.canonical().len(),
            Self::ExactDecimal(decimal) => decimal.canonical().len(),
            Self::UnicodeString(text) => text.len(),
        };
        MutationValueReceipt {
            kind: self.kind(),
            profile: self.profile(),
            schema_version: MUTATION_SCALAR_VALUE_SCHEMA_VERSION,
            payload_bytes,
        }
    }
}

/// Redacted projection of a scalar value for receipts and diagnostics.
///
/// Carries identity and size, never content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationValueReceipt {
    /// Value cohort.
    pub kind: MutationValueKind,
    /// Admitting profile.
    pub profile: MutationValueProfile,
    /// Pinned profile schema version.
    pub schema_version: u32,
    /// Byte length of the redacted payload.
    pub payload_bytes: usize,
}
