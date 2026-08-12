//! Source-generation-bound regex analysis retained by the parser.
//!
//! Whole-operator geometry remains parser-owned. `perl-regex` owns bounded
//! regex-body analysis. These types bind both planes to one immutable source
//! snapshot so HIR, PIR, and providers can consume facts without rescanning.

use perl_ast::SourceLocation;
use perl_regex::{
    RegexAnalyzer, RegexValidator,
    analyzer::{
        CaptureLanguageProfile, ModifierAnalysis, ModifierSequence, PatternControlAnalysis,
        PatternControlEffect, PatternControlKind, RegexOperator,
    },
    validator::{RegexAnalysis, RegexRange},
};

use super::quote_geometry::{RegexFamilyGeometry, RegexFamilyOperator};

/// Current retained-regex-analysis model version.
pub const REGEX_ANALYSIS_MODEL_VERSION: u32 = 1;

/// Stable identity for one retained regex-family record within a parse output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegexAnalysisId(usize);

impl RegexAnalysisId {
    /// Return the zero-based source-order index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Deterministic content identity for the immutable source snapshot.
///
/// This is a bounded parser freshness key, not a cryptographic authenticity
/// proof. Security and release evidence should use the repository's evidence
/// envelope and digest authority instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegexSourceDigest([u8; 16]);

impl RegexSourceDigest {
    /// Derive a deterministic content identity from one complete source snapshot.
    #[must_use]
    pub fn for_source(source: &str) -> Self {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        const SECOND_OFFSET: u64 = 0x8422_2325_cbf2_9ce4;
        const SECOND_PRIME: u64 = 0x0000_0100_0000_01c5;

        let mut forward = FNV_OFFSET;
        for byte in source.as_bytes() {
            forward ^= u64::from(*byte);
            forward = forward.wrapping_mul(FNV_PRIME);
        }

        let mut reverse = SECOND_OFFSET;
        for byte in source.as_bytes().iter().rev() {
            reverse ^= u64::from(*byte);
            reverse = reverse.wrapping_mul(SECOND_PRIME);
        }

        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&forward.to_be_bytes());
        bytes[8..].copy_from_slice(&reverse.to_be_bytes());
        Self(bytes)
    }

    /// Return the normalized digest bytes.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }

    /// Render the deterministic lowercase hexadecimal identity.
    #[must_use]
    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(self.0.len() * 2);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}

/// Why a parser-owned operator has no canonical regex-body analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum RegexAnalysisAvailability {
    /// Canonical modifier, structural, capture, and control analysis is retained.
    Analyzed,
    /// Transliteration is deliberately represented without regex-body analysis.
    TransliterationNotRegex,
    /// Exact operator/body geometry was unavailable for this recovered form.
    GeometryUnavailable,
    /// Modifier source coordinates could not be represented without overflow.
    ModifierRangeOverflow,
}

impl RegexAnalysisAvailability {
    /// Stable machine token for parser/compiler adapters and receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Analyzed => "analyzed",
            Self::TransliterationNotRegex => "transliteration_not_regex",
            Self::GeometryUnavailable => "geometry_unavailable",
            Self::ModifierRangeOverflow => "modifier_range_overflow",
        }
    }
}

/// Canonical static results retained for one regex pattern body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RetainedRegexPatternAnalysis {
    /// Typed safety, risk, and bounded-complexity analysis.
    pub structural: RegexAnalysis,
    /// Captures, references, pattern controls, and local completeness.
    pub controls: PatternControlAnalysis,
}

/// One parser-owned regex-family occurrence and its canonical static evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegexAnalysisRecord {
    /// Stable identity within the containing [`RegexAnalysisTable`].
    pub id: RegexAnalysisId,
    /// Operator family when exact geometry was recovered.
    pub operator: Option<RegexFamilyOperator>,
    /// Exact or best-known operator range in original-source bytes.
    pub full_range: SourceLocation,
    /// Source-backed operator/body/replacement/modifier geometry.
    pub geometry: Option<RegexFamilyGeometry>,
    /// Language and source-encoding profile used by the analysis.
    pub profile: CaptureLanguageProfile,
    /// Lossless modifier tokens, requirements, effective modes, and diagnostics.
    pub modifiers: Option<ModifierAnalysis>,
    /// Canonical regex-body analysis. Transliteration deliberately stores `None`.
    pub pattern: Option<RetainedRegexPatternAnalysis>,
    /// Why the pattern result is present, absent, or not applicable.
    pub availability: RegexAnalysisAvailability,
}

impl RegexAnalysisRecord {
    /// Exact pattern/search-list range when geometry is available.
    #[must_use]
    pub fn pattern_range(&self) -> Option<SourceLocation> {
        self.geometry.as_ref().map(|geometry| geometry.pattern.range)
    }

    /// Exact substitution/transliteration replacement range when present.
    #[must_use]
    pub fn replacement_range(&self) -> Option<SourceLocation> {
        self.geometry
            .as_ref()
            .and_then(|geometry| geometry.replacement.as_ref())
            .map(|replacement| replacement.range)
    }

    /// Exact modifier range when geometry is available.
    #[must_use]
    pub fn modifier_range(&self) -> Option<SourceLocation> {
        self.geometry.as_ref().map(|geometry| geometry.modifiers.range)
    }

    /// Map a body-relative `perl-regex` range into original-source bytes.
    #[must_use]
    pub fn map_pattern_range(&self, range: RegexRange) -> Option<SourceLocation> {
        let pattern = self.pattern_range()?;
        let start = pattern.start.checked_add(range.start)?;
        let end = pattern.start.checked_add(range.end)?;
        (start <= end && end <= pattern.end).then_some(SourceLocation { start, end })
    }

    /// Whether the operator can execute Perl or supply pattern text at runtime.
    #[must_use]
    pub fn has_embedded_code(&self) -> bool {
        let replacement_evaluates = self
            .modifiers
            .as_ref()
            .is_some_and(|analysis| analysis.effective.substitution_evaluation_depth > 0);
        let Some(pattern) = &self.pattern else {
            return replacement_evaluates;
        };
        replacement_evaluates
            || !pattern.structural.facts.embedded_code.is_empty()
            || pattern.controls.facts.iter().any(|fact| {
                matches!(
                    &fact.kind,
                    PatternControlKind::ImmediateEmbeddedCode
                        | PatternControlKind::OptimisticEmbeddedCode
                        | PatternControlKind::DeferredRuntimePattern
                ) || matches!(
                    fact.effect,
                    PatternControlEffect::DynamicExecution | PatternControlEffect::DynamicPattern
                ) && !matches!(&fact.kind, PatternControlKind::SourceInterpolation)
            })
    }

    /// Whether every retained result is exact for the supplied source/profile.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        if self.availability != RegexAnalysisAvailability::Analyzed {
            return false;
        }
        let Some(modifiers) = &self.modifiers else {
            return false;
        };
        let Some(pattern) = &self.pattern else {
            return false;
        };
        modifiers.diagnostics.is_empty()
            && pattern.structural.completeness.is_complete()
            && !pattern.structural.malformed
            && !pattern.structural.is_exhausted()
            && pattern.controls.status.is_complete()
            && pattern
                .controls
                .facts
                .iter()
                .all(|fact| fact.resolution.is_exact())
            && pattern.controls.captures.declarations.iter().all(|declaration| {
                matches!(
                    declaration.confidence.profile,
                    perl_regex::analyzer::CaptureProfileConfidence::Exact
                )
            })
    }
}

/// Generation-bound retained regex analysis for one immutable parser input.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegexAnalysisTable {
    /// Retained model version.
    pub model_version: u32,
    /// Source content identity, or `None` for compatibility-constructed output.
    pub source_digest: Option<RegexSourceDigest>,
    /// Complete source length when the digest is known.
    pub source_len: Option<usize>,
    /// Records in deterministic source order.
    pub records: Vec<RegexAnalysisRecord>,
    analysis_invocations: usize,
}

impl Default for RegexAnalysisTable {
    fn default() -> Self {
        Self::unknown()
    }
}

impl RegexAnalysisTable {
    /// Create an empty table bound to one immutable source snapshot.
    #[must_use]
    pub fn for_source(source: &str) -> Self {
        Self {
            model_version: REGEX_ANALYSIS_MODEL_VERSION,
            source_digest: Some(RegexSourceDigest::for_source(source)),
            source_len: Some(source.len()),
            records: Vec::new(),
            analysis_invocations: 0,
        }
    }

    /// Create an empty compatibility table with no source-generation claim.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            model_version: REGEX_ANALYSIS_MODEL_VERSION,
            source_digest: None,
            source_len: None,
            records: Vec::new(),
            analysis_invocations: 0,
        }
    }

    /// Whether this table is bound to the exact supplied source bytes.
    #[must_use]
    pub fn source_matches(&self, source: &str) -> bool {
        self.source_len == Some(source.len())
            && self.source_digest == Some(RegexSourceDigest::for_source(source))
    }

    /// Find a retained record by stable identity.
    #[must_use]
    pub fn record(&self, id: RegexAnalysisId) -> Option<&RegexAnalysisRecord> {
        self.records.get(id.index())
    }

    /// Find the record whose complete operator range exactly matches `range`.
    #[must_use]
    pub fn find_by_full_range(&self, range: SourceLocation) -> Option<&RegexAnalysisRecord> {
        self.records.iter().find(|record| record.full_range == range)
    }

    /// Find the record whose pattern range exactly matches `range`.
    #[must_use]
    pub fn find_by_pattern_range(&self, range: SourceLocation) -> Option<&RegexAnalysisRecord> {
        self.records
            .iter()
            .find(|record| record.pattern_range() == Some(range))
    }

    /// Find the narrowest retained occurrence containing an original-source byte.
    #[must_use]
    pub fn find_at_offset(&self, offset: usize) -> Option<&RegexAnalysisRecord> {
        self.records
            .iter()
            .filter(|record| record.full_range.start <= offset && offset < record.full_range.end)
            .min_by_key(|record| record.full_range.end.saturating_sub(record.full_range.start))
    }

    /// Number of new canonical body analyses executed while constructing this table.
    #[must_use]
    pub const fn analysis_invocations(&self) -> usize {
        self.analysis_invocations
    }

    /// Whether no regex-family occurrence was retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Number of retained regex-family occurrences.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn retain_geometry(
        &mut self,
        geometry: RegexFamilyGeometry,
        profile: CaptureLanguageProfile,
    ) -> RegexAnalysisRecord {
        if let Some(record) = self.records.iter().find(|record| {
            record.full_range == geometry.full_range && record.operator == Some(geometry.operator)
        }) {
            return record.clone();
        }

        let id = RegexAnalysisId(self.records.len());
        let operator = map_operator(geometry.operator);
        let Some(sequence) = ModifierSequence::new(
            geometry.modifiers.text.clone(),
            geometry.modifiers.range.start,
        ) else {
            let record = RegexAnalysisRecord {
                id,
                operator: Some(geometry.operator),
                full_range: geometry.full_range,
                geometry: Some(geometry),
                profile,
                modifiers: None,
                pattern: None,
                availability: RegexAnalysisAvailability::ModifierRangeOverflow,
            };
            self.records.push(record.clone());
            return record;
        };
        let modifier_analysis =
            RegexAnalyzer::analyze_modifiers(operator, sequence, profile.regex);

        let (pattern, availability) = if matches!(
            geometry.operator,
            RegexFamilyOperator::Transliteration
                | RegexFamilyOperator::TransliterationAlias
        ) {
            (None, RegexAnalysisAvailability::TransliterationNotRegex)
        } else {
            self.analysis_invocations = self.analysis_invocations.saturating_add(1);
            let structural = RegexValidator::new().analyze_with_modifiers(
                &geometry.pattern.text,
                modifier_analysis.effective,
            );
            let controls = RegexAnalyzer::analyze_pattern_controls(
                &geometry.pattern.text,
                geometry.pattern.range.start,
                modifier_analysis.effective,
                profile,
            );
            (
                Some(RetainedRegexPatternAnalysis { structural, controls }),
                RegexAnalysisAvailability::Analyzed,
            )
        };

        let record = RegexAnalysisRecord {
            id,
            operator: Some(geometry.operator),
            full_range: geometry.full_range,
            geometry: Some(geometry),
            profile,
            modifiers: Some(modifier_analysis),
            pattern,
            availability,
        };
        self.records.push(record.clone());
        record
    }

    pub(crate) fn retain_unavailable(
        &mut self,
        full_range: SourceLocation,
        reason: RegexAnalysisAvailability,
        profile: CaptureLanguageProfile,
    ) -> RegexAnalysisRecord {
        if let Some(record) = self.records.iter().find(|record| {
            record.full_range == full_range
                && record.availability == reason
                && record.geometry.is_none()
        }) {
            return record.clone();
        }
        let record = RegexAnalysisRecord {
            id: RegexAnalysisId(self.records.len()),
            operator: None,
            full_range,
            geometry: None,
            profile,
            modifiers: None,
            pattern: None,
            availability: reason,
        };
        self.records.push(record.clone());
        record
    }
}

fn map_operator(operator: RegexFamilyOperator) -> RegexOperator {
    match operator {
        RegexFamilyOperator::BareMatch => RegexOperator::BareMatch,
        RegexFamilyOperator::Match => RegexOperator::Match,
        RegexFamilyOperator::QuoteRegex => RegexOperator::QuoteRegex,
        RegexFamilyOperator::Substitution => RegexOperator::Substitution,
        RegexFamilyOperator::Transliteration => RegexOperator::Transliteration,
        RegexFamilyOperator::TransliterationAlias => RegexOperator::TransliterationAlias,
    }
}
