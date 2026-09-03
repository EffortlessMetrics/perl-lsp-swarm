//! Validated canonical Perl module names.
//!
//! A [`ModuleName`] is a module-style request operand (`use Foo::Bar;`) that has
//! passed the crate's single module-identifier grammar. Construction is the only
//! way to obtain one, so an arbitrary `&str` cannot masquerade as a validated
//! lookup subject.
//!
//! The grammar is owned by `token_core::is_module_identifier_segment`; this
//! module classifies *why* an input is rejected but never widens or narrows what
//! is accepted. `path::is_lookup_safe_module_name` is expressed in terms of this
//! type so the two cannot drift.

use std::borrow::Cow;
use std::fmt;

use serde::{Serialize, Serializer};

use crate::token_core::is_module_identifier_segment;

/// How a validated module name was spelled in source.
///
/// Perl accepts both the canonical `::` package separator and the legacy `'`
/// separator. Normalizing to `::` loses that distinction, so it is recorded here
/// instead of being silently discarded.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PackageSeparatorForm {
    /// Written with `::` separators only, or a single segment with no separator.
    Canonical,
    /// Written with `'` separators only.
    Legacy,
    /// Written with both `::` and `'` separators.
    Mixed,
}

/// Target profile deciding whether the legacy `'` package separator is a
/// separator at all.
///
/// The legacy separator is a target-profile question, not a normalization
/// detail: some consumers must keep accepting it, and some must refuse it. It is
/// therefore an explicit input rather than an unconditional rewrite.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LegacySeparatorProfile {
    /// Treat `'` as a package separator and record the spelling.
    ///
    /// This is the historical behaviour of `normalize_package_separator` and the
    /// default so existing callers keep their accept-set.
    #[default]
    Accept,
    /// Refuse `'` as a package separator.
    Reject,
}

/// Why a candidate string is not a validated module name.
///
/// Each variant is a distinct classification: an invalid request must never be
/// reported as a valid request that was merely not found.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleNameError {
    /// The input was empty.
    Empty,
    /// The input contained an interior NUL byte.
    InteriorNul,
    /// The input contained a control character.
    ControlCharacter {
        /// The offending character.
        character: char,
    },
    /// The input contained a filesystem path separator (`/` or `\`).
    PathSeparator {
        /// The offending separator.
        separator: char,
    },
    /// The input used absolute-path or drive-relative syntax.
    AbsoluteOrDriveSyntax,
    /// The input contained a `.` or `..` traversal segment.
    TraversalSegment,
    /// A package segment was empty (for example `Foo::`, `::Foo`, `Foo::::Bar`).
    EmptySegment,
    /// A package segment is not a Perl module identifier.
    InvalidSegment {
        /// The offending segment, as written after separator normalization.
        segment: String,
    },
    /// The input used the legacy `'` separator under a profile that refuses it.
    LegacySeparatorRejected,
}

impl fmt::Display for ModuleNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("module name is empty"),
            Self::InteriorNul => f.write_str("module name contains an interior NUL"),
            Self::ControlCharacter { character } => {
                write!(f, "module name contains control character U+{:04X}", *character as u32)
            }
            Self::PathSeparator { separator } => {
                write!(f, "module name contains path separator `{separator}`")
            }
            Self::AbsoluteOrDriveSyntax => {
                f.write_str("module name uses absolute-path or drive-relative syntax")
            }
            Self::TraversalSegment => f.write_str("module name contains a `.` or `..` segment"),
            Self::EmptySegment => f.write_str("module name contains an empty package segment"),
            Self::InvalidSegment { segment } => {
                write!(f, "`{segment}` is not a Perl module identifier segment")
            }
            Self::LegacySeparatorRejected => {
                f.write_str("legacy `'` package separator is not accepted by this target profile")
            }
        }
    }
}

impl std::error::Error for ModuleNameError {}

impl ModuleNameError {
    /// Stable identifier for evidence rows and diagnostics.
    ///
    /// The returned value is part of the boundary vocabulary and is intended to
    /// stay stable across refactors of the human-readable [`fmt::Display`] text.
    #[must_use]
    pub const fn boundary_id(&self) -> &'static str {
        match self {
            Self::Empty => "module_name.empty",
            Self::InteriorNul => "module_name.interior_nul",
            Self::ControlCharacter { .. } => "module_name.control_character",
            Self::PathSeparator { .. } => "module_name.path_separator",
            Self::AbsoluteOrDriveSyntax => "module_name.absolute_or_drive_syntax",
            Self::TraversalSegment => "module_name.traversal_segment",
            Self::EmptySegment => "module_name.empty_segment",
            Self::InvalidSegment { .. } => "module_name.invalid_segment",
            Self::LegacySeparatorRejected => "module_name.legacy_separator_rejected",
        }
    }
}

/// A validated canonical Perl module name.
///
/// The stored spelling is always canonical (`::`); the source spelling is
/// retained separately as a [`PackageSeparatorForm`] so no caller has to
/// re-derive it from a normalized string.
///
/// A `ModuleName` never carries a filesystem path. Converting one to a relative
/// `.pm` path is a separate, explicit step owned by the `path` module.
///
/// # Identity
///
/// Equality, ordering, and hashing are defined over the canonical spelling
/// alone. `Foo'Bar` and `Foo::Bar` name the same module and resolve to the same
/// file, so they compare equal and hash identically; deriving these traits would
/// instead include [`PackageSeparatorForm`] and make the two distinct map keys
/// for one module. The source spelling stays reachable through
/// [`ModuleName::separator_form`] and [`ModuleName::legacy_spelling`] — it is
/// retained provenance, not part of the name's identity.
#[derive(Debug, Clone)]
pub struct ModuleName {
    canonical: String,
    separator_form: PackageSeparatorForm,
}

impl PartialEq for ModuleName {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for ModuleName {}

impl Serialize for ModuleName {
    /// Module names are logical identities, so their canonical spelling is
    /// safe to serialize. Source forms remain represented by the separate
    /// separator-form accessor.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.canonical)
    }
}

impl std::hash::Hash for ModuleName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
    }
}

impl PartialOrd for ModuleName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ModuleName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.canonical.cmp(&other.canonical)
    }
}

impl ModuleName {
    /// Validate `text` as a module name under the default target profile.
    ///
    /// # Errors
    ///
    /// Returns the classified [`ModuleNameError`] for the first rule `text`
    /// violates.
    pub fn parse(text: &str) -> Result<Self, ModuleNameError> {
        Self::parse_with_profile(text, LegacySeparatorProfile::Accept)
    }

    /// Validate `text` as a module name under an explicit target profile.
    ///
    /// # Errors
    ///
    /// Returns the classified [`ModuleNameError`] for the first rule `text`
    /// violates.
    pub fn parse_with_profile(
        text: &str,
        profile: LegacySeparatorProfile,
    ) -> Result<Self, ModuleNameError> {
        let (canonical, separator_form) = validate(text, profile)?;
        Ok(Self { canonical: canonical.into_owned(), separator_form })
    }

    /// Report whether `text` would validate, without constructing a name.
    ///
    /// This is the allocation-free predicate form: a canonically spelled name is
    /// validated entirely against borrowed data. Use it when only the answer is
    /// needed; use [`Self::parse`] when the classified rejection reason or the
    /// validated name itself is needed.
    #[must_use]
    pub fn is_valid(text: &str) -> bool {
        Self::is_valid_with_profile(text, LegacySeparatorProfile::Accept)
    }

    /// Report whether `text` would validate under an explicit target profile.
    #[must_use]
    pub fn is_valid_with_profile(text: &str, profile: LegacySeparatorProfile) -> bool {
        validate(text, profile).is_ok()
    }

    /// The canonical `::`-separated spelling.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// The legacy `'`-separated spelling of the same name.
    #[must_use]
    pub fn legacy_spelling(&self) -> String {
        crate::name::legacy_package_separator(&self.canonical).into_owned()
    }

    /// How the name was spelled in source.
    #[must_use]
    pub const fn separator_form(&self) -> PackageSeparatorForm {
        self.separator_form
    }

    /// The canonical package segments, in order.
    pub fn segments(&self) -> impl Iterator<Item = &str> + '_ {
        self.canonical.split("::")
    }
}

impl fmt::Display for ModuleName {
    /// Renders the canonical module name.
    ///
    /// This is a logical module identity, never a filesystem path.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical)
    }
}

impl TryFrom<&str> for ModuleName {
    type Error = ModuleNameError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

/// Validate `text` and return its canonical spelling plus separator form.
///
/// The canonical spelling borrows `text` whenever no legacy `'` separator is
/// present, so the predicate forms ([`ModuleName::is_valid`], and through it
/// `path::is_lookup_safe_module_name`) allocate nothing on the common path.
/// Only [`ModuleName::parse_with_profile`] takes ownership, and only on success.
fn validate(
    text: &str,
    profile: LegacySeparatorProfile,
) -> Result<(Cow<'_, str>, PackageSeparatorForm), ModuleNameError> {
    if text.is_empty() {
        return Err(ModuleNameError::Empty);
    }

    for character in text.chars() {
        if character == '\0' {
            return Err(ModuleNameError::InteriorNul);
        }
        if character.is_control() {
            return Err(ModuleNameError::ControlCharacter { character });
        }
        if character == '/' || character == '\\' {
            return Err(ModuleNameError::PathSeparator { separator: character });
        }
    }

    if uses_absolute_or_drive_syntax(text) {
        return Err(ModuleNameError::AbsoluteOrDriveSyntax);
    }

    let has_legacy = text.contains('\'');
    if has_legacy && profile == LegacySeparatorProfile::Reject {
        return Err(ModuleNameError::LegacySeparatorRejected);
    }

    let has_canonical = text.contains("::");
    // `crate::name` owns the `'` -> `::` projection; do not reimplement it here.
    let canonical = crate::name::normalize_package_separator(text);

    for segment in canonical.split("::") {
        if segment.is_empty() {
            return Err(ModuleNameError::EmptySegment);
        }
        if segment == "." || segment == ".." {
            return Err(ModuleNameError::TraversalSegment);
        }
        if !is_module_identifier_segment(segment) {
            return Err(ModuleNameError::InvalidSegment { segment: segment.to_string() });
        }
    }

    let separator_form = match (has_canonical, has_legacy) {
        (true, true) => PackageSeparatorForm::Mixed,
        (false, true) => PackageSeparatorForm::Legacy,
        _ => PackageSeparatorForm::Canonical,
    };

    Ok((canonical, separator_form))
}

/// Detect absolute-path or drive-relative syntax.
///
/// A leading `X:` is drive syntax only when it is not the start of a `X::`
/// package separator, so single-letter packages such as `C::Foo` stay valid.
fn uses_absolute_or_drive_syntax(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if !first.is_ascii_alphabetic() {
        return false;
    }
    if chars.next() != Some(':') {
        return false;
    }

    chars.next() != Some(':')
}

#[cfg(test)]
mod tests {
    use super::{
        Cow, LegacySeparatorProfile, ModuleName, ModuleNameError, PackageSeparatorForm, validate,
    };

    #[test]
    fn canonical_nested_name_is_validated() -> Result<(), ModuleNameError> {
        let name = ModuleName::parse("Foo::Bar::Baz")?;
        assert_eq!(name.canonical(), "Foo::Bar::Baz");
        assert_eq!(name.separator_form(), PackageSeparatorForm::Canonical);
        assert_eq!(name.segments().collect::<Vec<_>>(), vec!["Foo", "Bar", "Baz"]);
        Ok(())
    }

    #[test]
    fn single_segment_pragma_is_validated() -> Result<(), ModuleNameError> {
        for pragma in ["strict", "warnings", "utf8", "_Private"] {
            let name = ModuleName::parse(pragma)?;
            assert_eq!(name.canonical(), pragma);
            assert_eq!(name.separator_form(), PackageSeparatorForm::Canonical);
        }
        Ok(())
    }

    #[test]
    fn legacy_separator_is_recorded_not_erased() -> Result<(), ModuleNameError> {
        let legacy = ModuleName::parse("Foo'Bar")?;
        assert_eq!(legacy.canonical(), "Foo::Bar");
        assert_eq!(legacy.separator_form(), PackageSeparatorForm::Legacy);
        assert_eq!(legacy.legacy_spelling(), "Foo'Bar");

        let mixed = ModuleName::parse("Foo'Bar::Baz")?;
        assert_eq!(mixed.canonical(), "Foo::Bar::Baz");
        assert_eq!(mixed.separator_form(), PackageSeparatorForm::Mixed);
        Ok(())
    }

    #[test]
    fn legacy_separator_profile_is_explicit() {
        assert_eq!(
            ModuleName::parse_with_profile("Foo'Bar", LegacySeparatorProfile::Reject),
            Err(ModuleNameError::LegacySeparatorRejected)
        );
        assert!(
            ModuleName::parse_with_profile("Foo::Bar", LegacySeparatorProfile::Reject).is_ok(),
            "canonical names stay valid under the rejecting profile"
        );
    }

    #[test]
    fn single_letter_package_is_not_drive_syntax() -> Result<(), ModuleNameError> {
        let name = ModuleName::parse("C::Foo")?;
        assert_eq!(name.canonical(), "C::Foo");
        Ok(())
    }

    #[test]
    fn rejections_are_classified_not_collapsed() {
        let cases = [
            ("", ModuleNameError::Empty),
            ("Foo\0Bar", ModuleNameError::InteriorNul),
            ("Foo\nBar", ModuleNameError::ControlCharacter { character: '\n' }),
            ("Foo/Bar", ModuleNameError::PathSeparator { separator: '/' }),
            ("Foo\\Bar", ModuleNameError::PathSeparator { separator: '\\' }),
            ("C:foo", ModuleNameError::AbsoluteOrDriveSyntax),
            ("Foo::..::Bar", ModuleNameError::TraversalSegment),
            ("Foo::", ModuleNameError::EmptySegment),
            ("::Foo", ModuleNameError::EmptySegment),
            ("Foo::::Bar", ModuleNameError::EmptySegment),
            ("$Foo", ModuleNameError::InvalidSegment { segment: "$Foo".to_string() }),
        ];

        for (input, expected) in cases {
            assert_eq!(
                ModuleName::parse(input),
                Err(expected),
                "`{input}` must carry its own classification"
            );
        }
    }

    #[test]
    fn every_rejection_has_a_stable_boundary_id() {
        for input in ["", "Foo\0", "Foo\n", "Foo/Bar", "C:foo", "Foo::..", "Foo::", "$Foo"] {
            let boundary_id = ModuleName::parse(input).err().map(|error| error.boundary_id());
            assert!(
                boundary_id.is_some_and(|id| id.starts_with("module_name.")),
                "`{input}` must be rejected with a namespaced boundary id, got {boundary_id:?}"
            );
        }
    }

    #[test]
    fn canonical_names_validate_without_allocating() -> Result<(), ModuleNameError> {
        // The predicate form gates production reference extraction per candidate
        // token, so validating a canonically spelled name must stay borrow-only.
        // `Cow::Owned` here would be the allocation regression this pins.
        let (canonical, _) = validate("Foo::Bar", LegacySeparatorProfile::Accept)?;
        assert!(
            matches!(canonical, Cow::Borrowed(_)),
            "a canonical spelling must validate against borrowed data"
        );

        let (single, _) = validate("strict", LegacySeparatorProfile::Accept)?;
        assert!(matches!(single, Cow::Borrowed(_)));

        // A legacy spelling genuinely needs a rewrite, so one allocation is correct.
        let (legacy, _) = validate("Foo'Bar", LegacySeparatorProfile::Accept)?;
        assert!(
            matches!(legacy, Cow::Owned(_)),
            "a legacy spelling must be normalized into an owned canonical form"
        );
        Ok(())
    }

    #[test]
    fn traversal_never_validates() {
        for input in ["..", ".", "../../etc/passwd", "Foo::..", "..::Foo"] {
            assert!(ModuleName::parse(input).is_err(), "`{input}` must never validate");
        }
    }

    /// Two spellings of one module are one name.
    ///
    /// Deriving `PartialEq`/`Hash` would include `separator_form` and make these
    /// distinct keys for a single module — a caller keying a map by module would
    /// silently hold two entries pointing at the same file.
    #[test]
    fn spelling_is_provenance_not_identity() -> Result<(), ModuleNameError> {
        use std::collections::HashSet;

        let canonical = ModuleName::parse("Foo::Bar")?;
        let legacy = ModuleName::parse("Foo'Bar")?;

        assert_ne!(
            legacy.separator_form(),
            canonical.separator_form(),
            "the provenance really does differ, so this is not a vacuous comparison"
        );
        assert_eq!(canonical, legacy, "same module, same name");
        assert_eq!(
            canonical.cmp(&legacy),
            std::cmp::Ordering::Equal,
            "`Ord` must agree with `Eq` or sorted collections corrupt"
        );

        let mut set = HashSet::new();
        set.insert(canonical);
        set.insert(legacy);
        assert_eq!(set.len(), 1, "one module must occupy one map slot");
        Ok(())
    }
}
