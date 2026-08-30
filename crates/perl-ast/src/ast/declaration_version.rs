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
//! # Source fidelity is structural
//!
//! A value is built from the source text and a range, and the spelling is
//! *derived* by slicing — [`DeclarationVersionSyntax::from_source`] is the
//! only constructor, and it takes no caller-supplied string. A caller
//! therefore cannot pair a reconstructed, normalized, or simply wrong
//! spelling with a real source range: there is no API that accepts one.
//! Slicing a range the caller already computed is not a source rescan; the
//! producer does no searching, matching, or re-lexing.
//!
//! # Exactness is checked, not asserted
//!
//! An exact form only admits its own closed spelling grammar:
//! [`DeclarationVersionForm::Decimal`] cannot carry `v1.2.3`, and
//! [`DeclarationVersionForm::VString`] cannot carry arbitrary text. A spelling
//! the parser could not read as the form it expected belongs to
//! [`DeclarationVersionForm::RecoveredOrUnknown`], which is the only form that
//! admits anything. Without that check the exact/recovered distinction would
//! be a caller's assertion rather than a property of the value.
//!
//! This is spelling shape only. It decides nothing about what a version
//! *means*, orders nothing, and compares nothing.
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
#[non_exhaustive]
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

    /// Whether `spelling` matches this form's closed source grammar.
    ///
    /// An exact form only admits its own spelling: a decimal tag cannot carry
    /// a v-string, and neither can carry arbitrary text.
    /// [`Self::RecoveredOrUnknown`] admits anything and is the only escape —
    /// that is what makes "exact" mean something.
    #[must_use]
    pub fn accepts(self, spelling: &str) -> bool {
        match self {
            Self::Decimal => is_decimal_spelling(spelling),
            Self::VString => is_vstring_spelling(spelling),
            Self::RecoveredOrUnknown => true,
        }
    }
}

/// One run of version digits, with Perl's `_` numeric separator allowed after
/// the leading digit (`1`, `036`, `23_45`).
fn is_digit_run(component: &str) -> bool {
    component.starts_with(|c: char| c.is_ascii_digit())
        && component.bytes().all(|b| b.is_ascii_digit() || b == b'_')
}

/// A decimal declaration VERSION: one integer part and at most one fractional
/// part (`1`, `1.23`, `0.001`, `5.036`, `1.23_45`). A second dot makes it a
/// v-string, not a decimal.
fn is_decimal_spelling(spelling: &str) -> bool {
    let mut parts = spelling.splitn(2, '.');
    let Some(integer) = parts.next() else {
        return false;
    };
    if !is_digit_run(integer) {
        return false;
    }
    match parts.next() {
        None => true,
        // `splitn(2, ..)` leaves any further dots in the remainder, so a
        // three-part spelling fails the digit-run check here.
        Some(fraction) => is_digit_run(fraction),
    }
}

/// A v-string declaration VERSION: either `v`-prefixed with one or more
/// components (`v5`, `v1.2.3`), or the bare three-or-more-component form Perl
/// also reads as a v-string (`1.2.3`).
fn is_vstring_spelling(spelling: &str) -> bool {
    if let Some(rest) = spelling.strip_prefix('v') {
        return rest.split('.').all(is_digit_run);
    }
    spelling.split('.').count() >= 3 && spelling.split('.').all(is_digit_run)
}

/// Whether a recorded declaration VERSION is an exact reading of the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeclarationVersionDisposition {
    /// The spelling was read completely and matched a known source form.
    Exact,
    /// The spelling was present but could not be read exactly.
    Recovered,
}

/// The owner-neutral source syntax of one declaration VERSION.
///
/// Construct through [`DeclarationVersionSyntax::from_source`], the only
/// constructor. It derives the spelling from the source, so the retained
/// spelling and the retained range cannot disagree.
///
/// # Example
///
/// ```rust
/// use perl_ast::ast::{DeclarationVersionForm, DeclarationVersionSyntax};
/// use perl_ast::SourceLocation;
///
/// let source = "package Demo 1.23;";
/// //                         ^^^^  bytes 13..17
/// let version = DeclarationVersionSyntax::from_source(
///     DeclarationVersionForm::Decimal,
///     source,
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
    /// Records the declaration VERSION that `range` covers in `source`.
    ///
    /// The spelling is taken from `source[range]`, never from the caller, so
    /// the value is a source record by construction rather than by promise.
    ///
    /// # Errors
    ///
    /// - [`DeclarationVersionSyntaxError::InvertedRange`] when `start > end`.
    /// - [`DeclarationVersionSyntaxError::RangeOutOfBounds`] when the range
    ///   runs past the end of `source`.
    /// - [`DeclarationVersionSyntaxError::RangeNotOnCharBoundary`] when the
    ///   range splits a multi-byte character.
    /// - [`DeclarationVersionSyntaxError::EmptyExactSpelling`] when an exact
    ///   form covers no bytes. A zero-width reading is only representable as
    ///   [`DeclarationVersionForm::RecoveredOrUnknown`].
    pub fn from_source(
        form: DeclarationVersionForm,
        source: &str,
        range: SourceLocation,
    ) -> Result<Self, DeclarationVersionSyntaxError> {
        if range.start > range.end {
            return Err(DeclarationVersionSyntaxError::InvertedRange {
                start: range.start,
                end: range.end,
            });
        }

        if range.end > source.len() {
            return Err(DeclarationVersionSyntaxError::RangeOutOfBounds {
                start: range.start,
                end: range.end,
                source_len: source.len(),
            });
        }

        let Some(slice) = source.get(range.start..range.end) else {
            return Err(DeclarationVersionSyntaxError::RangeNotOnCharBoundary {
                start: range.start,
                end: range.end,
            });
        };

        if slice.is_empty() && form.disposition() == DeclarationVersionDisposition::Exact {
            return Err(DeclarationVersionSyntaxError::EmptyExactSpelling { form });
        }

        if !form.accepts(slice) {
            return Err(DeclarationVersionSyntaxError::SpellingDoesNotMatchForm {
                form,
                start: range.start,
                end: range.end,
            });
        }

        Ok(Self { form, raw: slice.to_string(), range })
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

/// Why a declaration VERSION reading could not be recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeclarationVersionSyntaxError {
    /// The supplied range ends before it starts.
    InvertedRange {
        /// Start byte offset as supplied.
        start: usize,
        /// End byte offset as supplied.
        end: usize,
    },
    /// The supplied range runs past the end of the source.
    RangeOutOfBounds {
        /// Start byte offset as supplied.
        start: usize,
        /// End byte offset as supplied.
        end: usize,
        /// Byte length of the source the range was read against.
        source_len: usize,
    },
    /// The supplied range splits a multi-byte character.
    RangeNotOnCharBoundary {
        /// Start byte offset as supplied.
        start: usize,
        /// End byte offset as supplied.
        end: usize,
    },
    /// An exact form covers no bytes of source.
    EmptyExactSpelling {
        /// The exact form that was supplied.
        form: DeclarationVersionForm,
    },
    /// The covered spelling does not match the closed grammar of the exact
    /// form it was recorded under.
    ///
    /// A spelling the parser cannot read as the form it expected belongs to
    /// [`DeclarationVersionForm::RecoveredOrUnknown`], which is the only form
    /// that admits arbitrary text.
    SpellingDoesNotMatchForm {
        /// The exact form that was supplied.
        form: DeclarationVersionForm,
        /// Start byte offset of the offending spelling.
        start: usize,
        /// End byte offset of the offending spelling.
        end: usize,
    },
}

impl fmt::Display for DeclarationVersionSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvertedRange { start, end } => {
                write!(f, "declaration VERSION range {start}..{end} ends before it starts")
            }
            Self::RangeOutOfBounds { start, end, source_len } => write!(
                f,
                "declaration VERSION range {start}..{end} runs past the {source_len}-byte source"
            ),
            Self::RangeNotOnCharBoundary { start, end } => {
                write!(f, "declaration VERSION range {start}..{end} splits a multi-byte character")
            }
            Self::EmptyExactSpelling { form } => write!(
                f,
                "declaration VERSION form `{}` requires a spelling; a zero-width reading is only representable as `recovered`",
                form.tag()
            ),
            Self::SpellingDoesNotMatchForm { form, start, end } => write!(
                f,
                "declaration VERSION at {start}..{end} is not a `{}` spelling; record an unreadable version as `recovered`",
                form.tag()
            ),
        }
    }
}

impl Error for DeclarationVersionSyntaxError {}
