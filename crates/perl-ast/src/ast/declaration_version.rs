//! Shared declaration VERSION source syntax (#10716).
//!
//! `package NAME VERSION` and native `class NAME VERSION` both admit an
//! optional version in their declaration header. This module owns the single
//! owner-neutral *source* identity for that spelling so the package rail
//! (#10753) and the canonical class rail (#10762) consume one type, and the
//! parser primitive (#11089) produces it once.
//!
//! # What this contract owns
//!
//! The spelling family, the exact raw source text, the exact byte range, and
//! whether that reading is exact or recovered. Nothing else.
//!
//! # What this contract does not own
//!
//! No normalized value, ordering, comparison, equivalence, feature
//! activation, module-import, or directive semantics. A decimal spelling and
//! a v-string spelling stay *different source forms* here even where later
//! semantics would treat them as equal, and no accessor on this type derives
//! a number from the spelling. Version meaning belongs to the semantic layer,
//! not to `perl-ast`.
//!
//! Absence is expressed by the owner, as `Option<DeclarationVersionSyntax>`.
//! A version that was present but unreadable is *not* absence: it is
//! [`DeclarationVersionForm::RecoveredOrUnknown`], which keeps whatever text
//! and geometry the parser did observe.

use perl_position_tracking::SourceLocation;
use std::error::Error;
use std::fmt;

/// The source spelling family of a declaration VERSION.
///
/// These are source forms, not values. `1.23` and `v1.2.3` are different
/// forms and remain distinguishable even if a later semantic layer decides
/// they denote the same version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationVersionForm {
    /// Decimal spelling, such as `1.23` or `0.001`.
    Decimal,
    /// V-string spelling, such as `v1.2.3` or `v5`.
    VString,
    /// A version was present in the header but is not an exact reading of
    /// either spelling above — malformed, truncated, or otherwise recovered.
    ///
    /// This is not absence. An absent version is `None` at the owner.
    RecoveredOrUnknown,
}

impl DeclarationVersionForm {
    /// Stable lowercase tag used by the deterministic [`Display`] rendering.
    ///
    /// [`Display`]: std::fmt::Display
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Decimal => "decimal",
            Self::VString => "vstring",
            Self::RecoveredOrUnknown => "recovered",
        }
    }

    /// The completeness disposition implied by this form.
    ///
    /// The disposition is *derived*, never stored alongside the form, so a
    /// value claiming both `RecoveredOrUnknown` and
    /// [`DeclarationVersionDisposition::Exact`] is unrepresentable rather than
    /// merely rejected.
    #[must_use]
    pub const fn disposition(self) -> DeclarationVersionDisposition {
        match self {
            Self::Decimal | Self::VString => DeclarationVersionDisposition::Exact,
            Self::RecoveredOrUnknown => DeclarationVersionDisposition::Recovered,
        }
    }
}

/// Whether a recorded declaration VERSION is an exact reading of the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationVersionDisposition {
    /// The spelling was read completely and matched a known source form.
    Exact,
    /// The spelling was present but could not be read exactly.
    Recovered,
}

/// The owner-neutral source syntax of one declaration VERSION.
///
/// Construct through [`DeclarationVersionSyntax::new`], which rejects states
/// where the retained spelling and the retained range contradict each other.
///
/// # Example
///
/// ```rust
/// use perl_ast::ast::{DeclarationVersionForm, DeclarationVersionSyntax};
/// use perl_ast::SourceLocation;
///
/// // package Demo 1.23;
/// //              ^^^^  bytes 13..17
/// let version = DeclarationVersionSyntax::new(
///     DeclarationVersionForm::Decimal,
///     "1.23",
///     SourceLocation { start: 13, end: 17 },
/// )?;
///
/// assert_eq!(version.raw(), "1.23");
/// assert_eq!(version.range().start, 13);
/// assert!(version.is_exact());
/// # Ok::<(), perl_ast::ast::DeclarationVersionSyntaxError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclarationVersionSyntax {
    form: DeclarationVersionForm,
    raw: String,
    range: SourceLocation,
}

impl DeclarationVersionSyntax {
    /// Records one declaration VERSION spelling at its exact source range.
    ///
    /// `raw` must be the source text the range covers: the check is
    /// `raw.len() == range.end - range.start`. That is what keeps this type a
    /// *source* record — a caller cannot store a reconstructed or normalized
    /// string against a real source range.
    ///
    /// # Errors
    ///
    /// - [`DeclarationVersionSyntaxError::InvertedRange`] when `start > end`.
    /// - [`DeclarationVersionSyntaxError::RangeLengthMismatch`] when the
    ///   spelling is not the exact byte extent of the range.
    /// - [`DeclarationVersionSyntaxError::EmptyExactSpelling`] when an exact
    ///   form carries no spelling at all. A zero-width reading is only
    ///   representable as [`DeclarationVersionForm::RecoveredOrUnknown`].
    pub fn new(
        form: DeclarationVersionForm,
        raw: impl Into<String>,
        range: SourceLocation,
    ) -> Result<Self, DeclarationVersionSyntaxError> {
        if range.start > range.end {
            return Err(DeclarationVersionSyntaxError::InvertedRange {
                start: range.start,
                end: range.end,
            });
        }

        let raw = raw.into();
        let range_len = range.end - range.start;
        if raw.len() != range_len {
            return Err(DeclarationVersionSyntaxError::RangeLengthMismatch {
                raw_len: raw.len(),
                range_len,
            });
        }

        if raw.is_empty() && form.disposition() == DeclarationVersionDisposition::Exact {
            return Err(DeclarationVersionSyntaxError::EmptyExactSpelling { form });
        }

        Ok(Self { form, raw, range })
    }

    /// The source spelling family of this version.
    #[must_use]
    pub const fn form(&self) -> DeclarationVersionForm {
        self.form
    }

    /// The exact source text of this version, byte for byte.
    ///
    /// Trailing zeros, leading zeros, and separator spelling are preserved:
    /// `1.230` and `1.23` are different spellings and different values here.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The exact byte range this spelling occupies in the source.
    #[must_use]
    pub const fn range(&self) -> SourceLocation {
        self.range
    }

    /// Whether this reading is exact or recovered.
    #[must_use]
    pub const fn disposition(&self) -> DeclarationVersionDisposition {
        self.form.disposition()
    }

    /// Whether this reading is exact.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self.disposition(), DeclarationVersionDisposition::Exact)
    }
}

/// Deterministic one-line projection: `<form-tag>:<raw>@<start>..<end>`.
///
/// This is a stable diagnostic and receipt rendering. It never renders a
/// normalized or numeric interpretation of the spelling, and two spellings
/// that differ in form or in source text render differently.
impl fmt::Display for DeclarationVersionSyntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}@{}..{}", self.form.tag(), self.raw, self.range.start, self.range.end)
    }
}

/// Why a declaration VERSION spelling could not be recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationVersionSyntaxError {
    /// The supplied range ends before it starts.
    InvertedRange {
        /// Start byte offset as supplied.
        start: usize,
        /// End byte offset as supplied.
        end: usize,
    },
    /// The spelling is not the exact byte extent of the supplied range.
    RangeLengthMismatch {
        /// Byte length of the supplied spelling.
        raw_len: usize,
        /// Byte length of the supplied range.
        range_len: usize,
    },
    /// An exact form was supplied with no spelling.
    EmptyExactSpelling {
        /// The exact form that was supplied.
        form: DeclarationVersionForm,
    },
}

impl fmt::Display for DeclarationVersionSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvertedRange { start, end } => {
                write!(f, "declaration VERSION range {start}..{end} ends before it starts")
            }
            Self::RangeLengthMismatch { raw_len, range_len } => write!(
                f,
                "declaration VERSION spelling is {raw_len} bytes but its range covers {range_len}"
            ),
            Self::EmptyExactSpelling { form } => write!(
                f,
                "declaration VERSION form `{}` requires a spelling; a zero-width reading is only representable as `recovered`",
                form.tag()
            ),
        }
    }
}

impl Error for DeclarationVersionSyntaxError {}
