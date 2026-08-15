//! Typed results for bounded static regex analysis.
//!
//! These types keep machine identity, source ranges, reusable facts, and
//! completeness separate from presentation text. Ranges are byte offsets
//! relative to the regex body supplied to [`super::RegexValidator::analyze`].

/// A half-open byte range relative to an analyzed regex body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegexRange {
    /// Inclusive byte offset where the range starts.
    pub start: usize,
    /// Exclusive byte offset where the range ends.
    pub end: usize,
}

impl RegexRange {
    /// Construct a range when `start <= end`.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Option<Self> {
        if start <= end { Some(Self { start, end }) } else { None }
    }

    /// Construct an input-contained range without clamping impossible evidence.
    pub(crate) fn anchored(start: usize, width: usize, input_len: usize) -> Option<Self> {
        let end = start.checked_add(width)?;
        (start <= input_len && end <= input_len).then_some(Self { start, end })
    }

    /// Return the range length in bytes.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Return whether the range is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Return whether `offset` lies inside this half-open range.
    #[must_use]
    pub const fn contains(self, offset: usize) -> bool {
        self.start <= offset && offset < self.end
    }

    /// Return whether this range overlaps another half-open range.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// Operational class of a regex diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RegexDiagnosticClass {
    /// The selected Perl profile establishes invalid syntax or conformance.
    Syntax,
    /// Valid Perl syntax introduces executable or runtime-supplied behavior.
    DynamicBoundary,
    /// Static structure warrants a non-fatal risk warning.
    RiskAdvisory,
    /// A configured static-analysis policy limit was exceeded.
    PolicyLimit,
}

impl RegexDiagnosticClass {
    /// Stable machine token for receipts and protocol adapters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::DynamicBoundary => "dynamic_boundary",
            Self::RiskAdvisory => "risk_advisory",
            Self::PolicyLimit => "policy_limit",
        }
    }
}

/// Stable identity of a regex diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum RegexDiagnosticCode {
    /// Immediate embedded Perl code: `(?{ ... })`.
    EmbeddedCodeImmediate,
    /// Deferred runtime regex construction: `(??{ ... })`.
    EmbeddedCodeDeferred,
    /// A repeated group already contains a backtracking quantifier.
    NestedQuantifierRisk,
    /// Configured Unicode-property count was exceeded.
    UnicodePropertyLimit,
    /// Configured nested-lookbehind depth was exceeded.
    LookbehindNestingLimit,
    /// Configured branch-reset nesting depth was exceeded.
    BranchResetNestingLimit,
    /// Configured branch count inside a branch-reset group was exceeded.
    BranchResetBranchLimit,
    /// A modifier character is not recognized by the static model.
    UnknownModifier,
    /// A known modifier is not valid for the selected operator family.
    ModifierNotAllowedForOperator,
    /// More than one effective `/a`, `/d`, `/l`, or `/u` mode was requested.
    ConflictingCharacterSetModifiers,
    /// A character-set modifier was repeated more often than Perl allows.
    RepeatedCharacterSetModifier,
    /// The modifier is accepted by Perl here but has no effect.
    ModifierHasNoEffect,
    /// The modifier form requires a newer Perl language version.
    ModifierRequiresPerlVersion,
    /// The requested feature-qualified behavior is unavailable for the profile.
    ModifierRequiresFeature,
}

impl RegexDiagnosticCode {
    /// Stable machine token for catalogs, suppression, and receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmbeddedCodeImmediate => "embedded_code_immediate",
            Self::EmbeddedCodeDeferred => "embedded_code_deferred",
            Self::NestedQuantifierRisk => "nested_quantifier_risk",
            Self::UnicodePropertyLimit => "unicode_property_limit",
            Self::LookbehindNestingLimit => "lookbehind_nesting_limit",
            Self::BranchResetNestingLimit => "branch_reset_nesting_limit",
            Self::BranchResetBranchLimit => "branch_reset_branch_limit",
            Self::UnknownModifier => "unknown_modifier",
            Self::ModifierNotAllowedForOperator => "modifier_not_allowed_for_operator",
            Self::ConflictingCharacterSetModifiers => "conflicting_character_set_modifiers",
            Self::RepeatedCharacterSetModifier => "repeated_character_set_modifier",
            Self::ModifierHasNoEffect => "modifier_has_no_effect",
            Self::ModifierRequiresPerlVersion => "modifier_requires_perl_version",
            Self::ModifierRequiresFeature => "modifier_requires_feature",
        }
    }

    pub(crate) const fn class(self) -> RegexDiagnosticClass {
        match self {
            Self::EmbeddedCodeImmediate | Self::EmbeddedCodeDeferred => {
                RegexDiagnosticClass::DynamicBoundary
            }
            Self::NestedQuantifierRisk => RegexDiagnosticClass::RiskAdvisory,
            Self::UnicodePropertyLimit
            | Self::LookbehindNestingLimit
            | Self::BranchResetNestingLimit
            | Self::BranchResetBranchLimit => RegexDiagnosticClass::PolicyLimit,
            Self::UnknownModifier
            | Self::ModifierNotAllowedForOperator
            | Self::ConflictingCharacterSetModifiers
            | Self::RepeatedCharacterSetModifier
            | Self::ModifierRequiresPerlVersion
            | Self::ModifierRequiresFeature => RegexDiagnosticClass::Syntax,
            // Perl compiles these forms and only warns, so they stay advisory.
            Self::ModifierHasNoEffect => RegexDiagnosticClass::RiskAdvisory,
        }
    }
}

/// One typed diagnostic emitted by static regex analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegexDiagnostic {
    /// Stable machine identity.
    pub code: RegexDiagnosticCode,
    /// Operational class derived from the diagnostic identity.
    pub class: RegexDiagnosticClass,
    /// Body-relative byte range.
    pub range: RegexRange,
    /// Configured limit relevant to a policy diagnostic, when applicable.
    pub limit: Option<usize>,
}

impl RegexDiagnostic {
    pub(crate) const fn new(
        code: RegexDiagnosticCode,
        range: RegexRange,
        limit: Option<usize>,
    ) -> Self {
        Self { code, class: code.class(), range, limit }
    }

    /// Render the current human-readable message.
    ///
    /// Consumers should use [`RegexDiagnostic::code`] and
    /// [`RegexDiagnostic::class`] for machine decisions.
    #[must_use]
    pub fn message(&self) -> String {
        match self.code {
            RegexDiagnosticCode::EmbeddedCodeImmediate => {
                "Embedded code execution is not allowed in regex patterns".to_string()
            }
            RegexDiagnosticCode::EmbeddedCodeDeferred => {
                "Deferred embedded code execution is not allowed in regex patterns".to_string()
            }
            RegexDiagnosticCode::NestedQuantifierRisk => {
                "Nested quantifiers may cause catastrophic backtracking".to_string()
            }
            RegexDiagnosticCode::UnicodePropertyLimit => format!(
                "Too many Unicode properties in regex (max {})",
                self.limit.unwrap_or_default()
            ),
            RegexDiagnosticCode::LookbehindNestingLimit => {
                "Regex lookbehind nesting too deep".to_string()
            }
            RegexDiagnosticCode::BranchResetNestingLimit => {
                "Regex branch reset nesting too deep".to_string()
            }
            RegexDiagnosticCode::BranchResetBranchLimit => format!(
                "Too many branches in branch reset group (max {})",
                self.limit.unwrap_or_default()
            ),
            RegexDiagnosticCode::UnknownModifier => "Unknown regex modifier".to_string(),
            RegexDiagnosticCode::ModifierNotAllowedForOperator => {
                "Regex modifier is not valid for this operator".to_string()
            }
            RegexDiagnosticCode::ConflictingCharacterSetModifiers => {
                "Character-set regex modifiers are mutually exclusive".to_string()
            }
            RegexDiagnosticCode::RepeatedCharacterSetModifier => {
                "Character-set regex modifier is repeated too many times".to_string()
            }
            RegexDiagnosticCode::ModifierHasNoEffect => {
                "Regex modifier has no effect here".to_string()
            }
            RegexDiagnosticCode::ModifierRequiresPerlVersion => {
                "Regex modifier requires a newer Perl version".to_string()
            }
            RegexDiagnosticCode::ModifierRequiresFeature => {
                "Regex modifier requires an unavailable Perl feature".to_string()
            }
        }
    }
}

/// Kind of executable code embedded in the pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EmbeddedCodeKind {
    /// Immediate embedded Perl code, `(?{ ... })`.
    Immediate,
    /// Deferred runtime regex construction, `(??{ ... })`.
    Deferred,
}

/// Source-backed embedded-code fact.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbeddedCodeFact {
    /// Embedded-code form.
    pub kind: EmbeddedCodeKind,
    /// Full body-relative range of the executable/dynamic construct.
    pub range: RegexRange,
}

/// Kind of source region that prevents a fully static interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RegexDynamicRegionKind {
    /// Immediate embedded code.
    EmbeddedCodeImmediate,
    /// Deferred runtime-supplied regex text.
    EmbeddedCodeDeferred,
    /// Source interpolation such as `$name`, `${expr}`, or `@values`.
    ///
    /// Reserved for a follow-up scanner slice; this PR only populates embedded-code
    /// dynamic regions so hosted ripr stays within the new-gap budget.
    Interpolation,
}

/// One source-backed dynamic region.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegexDynamicRegionFact {
    /// Dynamic-region category.
    pub kind: RegexDynamicRegionKind,
    /// Full body-relative range of the dynamic region.
    pub range: RegexRange,
}

/// Reusable structural facts emitted by regex analysis.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RegexFacts {
    /// Embedded executable or runtime-supplied regions in source order.
    pub embedded_code: Vec<EmbeddedCodeFact>,
    /// Dynamic regions in source order (embedded code in this slice).
    pub dynamic_regions: Vec<RegexDynamicRegionFact>,
    /// Nested-quantifier advisory ranges in source order.
    pub nested_quantifiers: Vec<RegexRange>,
}

/// Completeness of the bounded static result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RegexAnalysisCompleteness {
    /// The current bounded analyzers completed without dynamic regions or policy exhaustion.
    Complete,
    /// Executable or runtime-supplied pattern text prevents a fully static interpretation.
    Dynamic,
    /// One or more configured policy limits were exceeded.
    PolicyLimited,
    /// Dynamic regions and policy-limit findings both apply.
    DynamicAndPolicyLimited,
}

impl RegexAnalysisCompleteness {
    pub(crate) const fn from_flags(dynamic: bool, policy_limited: bool) -> Self {
        match (dynamic, policy_limited) {
            (false, false) => Self::Complete,
            (true, false) => Self::Dynamic,
            (false, true) => Self::PolicyLimited,
            (true, true) => Self::DynamicAndPolicyLimited,
        }
    }

    /// Whether the analysis has no known dynamic or policy-limited boundary.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }

    /// Whether executable or runtime-supplied pattern text is present.
    #[must_use]
    pub const fn has_dynamic_boundary(self) -> bool {
        matches!(self, Self::Dynamic | Self::DynamicAndPolicyLimited)
    }

    /// Whether a configured policy limit was exceeded.
    #[must_use]
    pub const fn is_policy_limited(self) -> bool {
        matches!(self, Self::PolicyLimited | Self::DynamicAndPolicyLimited)
    }
}

/// Complete typed result of one bounded static regex analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegexAnalysis {
    /// All diagnostics in deterministic source order.
    pub diagnostics: Vec<RegexDiagnostic>,
    /// Reusable structural facts.
    pub facts: RegexFacts,
    /// Local completeness classification.
    pub completeness: RegexAnalysisCompleteness,
}

impl RegexAnalysis {
    /// Whether no diagnostic was emitted.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }
}
