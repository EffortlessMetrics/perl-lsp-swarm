use crate::PragmaState;

/// Parsed Perl version from a lexical `use v...;` or `use 5.xxx;` pragma.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PerlVersion {
    /// Major Perl version component.
    pub major: u32,
    /// Minor Perl version component.
    pub minor: u32,
}

impl PerlVersion {
    /// Create a new Perl version value.
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

/// Parse a Perl version string into a major/minor pair.
///
/// Handles lexical version pragmas such as:
/// - `v5.36`
/// - `v5.36.0`
/// - `5.036`
/// - `5.043011`
/// - `5.10`
/// - developer releases like `5.012_001`
///
/// Underscores in numeric VERSION literals are Perl visual digit separators,
/// so repeated separators such as `5.043_0_11` name the same number as
/// `5.043011`.
pub fn parse_perl_version(module: &str) -> Option<PerlVersion> {
    let is_v_string = module.starts_with('v');
    let s = module.strip_prefix('v').unwrap_or(module);
    let is_decimal = !is_v_string && s.matches('.').count() == 1;
    let mut parts = s.splitn(3, '.');

    let major = parse_version_component(parts.next()?)?;
    let minor = match parts.next() {
        Some(part) => parse_minor_version_component(part, is_decimal)?,
        None => 0,
    };

    Some(PerlVersion::new(major, minor))
}

fn parse_version_component(component: &str) -> Option<u32> {
    let component = validated_version_digits(component)?;
    component.parse().ok()
}

fn parse_minor_version_component(component: &str, is_decimal: bool) -> Option<u32> {
    let component = validated_version_digits(component)?;
    // Perl decimal versions group fractional digits in threes. The current
    // public model retains only major/minor, so discard later patch groups
    // instead of interpreting `5.043011` as the future minor version 43011.
    let component =
        if is_decimal && component.len() > 3 { component.get(..3)? } else { &component };
    component.parse().ok()
}

/// Perl allows underscore characters between digits in numeric literals as
/// visual separators (`5.012_001`, `5.043_0_11`). Accept any number of
/// separators placed strictly between digits, reject misplaced ones
/// (leading, trailing, or repeated), and return the digit sequence with
/// separators removed.
fn validated_version_digits(component: &str) -> Option<String> {
    let mut digits = String::with_capacity(component.len());
    // Start as if after a separator so a leading `_` is rejected.
    let mut previous_was_separator = true;
    for character in component.chars() {
        match character {
            '_' if previous_was_separator => return None,
            '_' => previous_was_separator = true,
            digit if digit.is_ascii_digit() => {
                previous_was_separator = false;
                digits.push(digit);
            }
            _ => return None,
        }
    }
    // A trailing separator never saw its following digit.
    if previous_was_separator {
        return None;
    }
    Some(digits)
}

/// Whether `use VERSION` implies `strict` for this version.
#[must_use]
pub fn version_implies_strict(version: PerlVersion) -> bool {
    version >= PerlVersion::new(5, 12)
}

/// Whether `use VERSION` implies `warnings` for this version.
#[must_use]
pub fn version_implies_warnings(version: PerlVersion) -> bool {
    version >= PerlVersion::new(5, 35)
}

/// Returns the language features implicitly enabled by declaring `use VERSION`.
///
/// Mirrors the Perl `feature` pragma bundle semantics: each `use vX.Y`
/// declaration implicitly enables the same features as `use feature ':X.Y'`.
/// Features that were removed from a bundle (for example `switch` removed in
/// v5.36 and `smartmatch` removed in v5.42) are not included for that
/// version and above. Versions older than v5.10 load the `:default` bundle.
///
/// Reference: <https://perldoc.perl.org/feature#FEATURE-BUNDLES>
#[must_use]
pub fn features_enabled_by_version(version: PerlVersion) -> Vec<&'static str> {
    let bundle = if version < PerlVersion::new(5, 10) {
        DEFAULT_FEATURES
    } else if version >= PerlVersion::new(5, 42) {
        BUNDLE_5_42_FEATURES
    } else if version >= PerlVersion::new(5, 40) {
        BUNDLE_5_40_FEATURES
    } else if version >= PerlVersion::new(5, 38) {
        BUNDLE_5_38_FEATURES
    } else if version >= PerlVersion::new(5, 36) {
        BUNDLE_5_36_FEATURES
    } else if version >= PerlVersion::new(5, 34) {
        BUNDLE_5_34_FEATURES
    } else if version >= PerlVersion::new(5, 28) {
        BUNDLE_5_28_FEATURES
    } else if version >= PerlVersion::new(5, 24) {
        BUNDLE_5_24_FEATURES
    } else if version >= PerlVersion::new(5, 16) {
        BUNDLE_5_16_FEATURES
    } else if version >= PerlVersion::new(5, 12) {
        BUNDLE_5_12_FEATURES
    } else {
        BUNDLE_5_10_FEATURES
    };

    bundle.to_vec()
}

pub(crate) const DEFAULT_FEATURES: &[&str] = &[
    "indirect",
    "multidimensional",
    "bareword_filehandles",
    "apostrophe_as_package_separator",
    "smartmatch",
];

const BUNDLE_5_10_FEATURES: &[&str] = &[
    "apostrophe_as_package_separator",
    "bareword_filehandles",
    "indirect",
    "multidimensional",
    "say",
    "smartmatch",
    "state",
    "switch",
];

const BUNDLE_5_12_FEATURES: &[&str] = &[
    "apostrophe_as_package_separator",
    "bareword_filehandles",
    "indirect",
    "multidimensional",
    "say",
    "smartmatch",
    "state",
    "switch",
    "unicode_strings",
];

const BUNDLE_5_16_FEATURES: &[&str] = &[
    "apostrophe_as_package_separator",
    "bareword_filehandles",
    "current_sub",
    "evalbytes",
    "fc",
    "indirect",
    "multidimensional",
    "say",
    "smartmatch",
    "state",
    "switch",
    "unicode_eval",
    "unicode_strings",
];

const BUNDLE_5_24_FEATURES: &[&str] = &[
    "apostrophe_as_package_separator",
    "bareword_filehandles",
    "current_sub",
    "evalbytes",
    "fc",
    "indirect",
    "multidimensional",
    "postderef_qq",
    "say",
    "smartmatch",
    "state",
    "switch",
    "unicode_eval",
    "unicode_strings",
];

const BUNDLE_5_28_FEATURES: &[&str] = &[
    "apostrophe_as_package_separator",
    "bareword_filehandles",
    "bitwise",
    "current_sub",
    "evalbytes",
    "fc",
    "indirect",
    "multidimensional",
    "postderef_qq",
    "say",
    "smartmatch",
    "state",
    "switch",
    "unicode_eval",
    "unicode_strings",
];

const BUNDLE_5_34_FEATURES: &[&str] = BUNDLE_5_28_FEATURES;

const BUNDLE_5_36_FEATURES: &[&str] = &[
    "apostrophe_as_package_separator",
    "bareword_filehandles",
    "bitwise",
    "current_sub",
    "evalbytes",
    "fc",
    "isa",
    "postderef_qq",
    "say",
    "signatures",
    "smartmatch",
    "state",
    "unicode_eval",
    "unicode_strings",
];

const BUNDLE_5_38_FEATURES: &[&str] = &[
    "apostrophe_as_package_separator",
    "bitwise",
    "current_sub",
    "evalbytes",
    "fc",
    "isa",
    "module_true",
    "postderef_qq",
    "say",
    "signatures",
    "smartmatch",
    "state",
    "unicode_eval",
    "unicode_strings",
];

const BUNDLE_5_40_FEATURES: &[&str] = &[
    "apostrophe_as_package_separator",
    "bitwise",
    "current_sub",
    "evalbytes",
    "fc",
    "isa",
    "module_true",
    "postderef_qq",
    "say",
    "signatures",
    "smartmatch",
    "state",
    "try",
    "unicode_eval",
    "unicode_strings",
];

const BUNDLE_5_42_FEATURES: &[&str] = &[
    "bitwise",
    "current_sub",
    "evalbytes",
    "fc",
    "isa",
    "module_true",
    "postderef_qq",
    "say",
    "signatures",
    "state",
    "try",
    "unicode_eval",
    "unicode_strings",
];

pub(crate) fn enable_effective_version_semantics(state: &mut PragmaState, version: PerlVersion) {
    state.perl_version = Some(version);
    if version_implies_strict(version) {
        state.strict_vars = true;
        state.strict_subs = true;
        state.strict_refs = true;
    }
    if version_implies_warnings(version) {
        state.warnings = true;
    }
    // Populate the version-implied feature set.
    // Replace (not merge) so the highest `use vX.Y` wins if multiple appear.
    state.features = features_enabled_by_version(version);
    state.unicode_strings = state.has_feature("unicode_strings");
    state.signatures_strict = false;
}
