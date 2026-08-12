//! Canonical normalization and semantic merge for critic findings.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use perl_parser_core::position::Range;
use serde::Serialize;

use super::{
    CriticFindingOrigin, CriticFindingShape, CriticIdentityCategory, CriticIdentityEntry,
    CriticIdentityRegistry, Severity,
};

/// Producer/code/shape identity observed before canonical normalization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CriticObservedIdentity {
    origin: CriticFindingOrigin,
    code: String,
    shape: CriticFindingShape,
}

impl CriticObservedIdentity {
    /// Construct an observed finding identity.
    #[must_use]
    pub fn new(
        origin: CriticFindingOrigin,
        code: impl Into<String>,
        shape: CriticFindingShape,
    ) -> Self {
        Self { origin, code: code.into(), shape }
    }

    /// Finding producer.
    #[must_use]
    pub const fn origin(&self) -> CriticFindingOrigin {
        self.origin
    }

    /// Observed diagnostic, native rule, or policy code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Reviewed syntax distinction used for identity resolution.
    #[must_use]
    pub const fn shape(&self) -> CriticFindingShape {
        self.shape
    }
}

/// One pre-normalization critic finding candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CriticFindingCandidate {
    identity: CriticObservedIdentity,
    source_generation: u64,
    severity: Severity,
    range: Range,
    message: String,
    explanation: Option<String>,
}

impl CriticFindingCandidate {
    /// Construct a finding candidate from one producer.
    #[must_use]
    pub fn new(
        identity: CriticObservedIdentity,
        source_generation: u64,
        severity: Severity,
        range: Range,
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self {
            identity,
            source_generation,
            severity,
            range,
            message: message.into(),
            explanation,
        }
    }

    /// Observed producer/code/shape identity.
    #[must_use]
    pub const fn identity(&self) -> &CriticObservedIdentity {
        &self.identity
    }

    /// Exact source generation that produced this candidate.
    #[must_use]
    pub const fn source_generation(&self) -> u64 {
        self.source_generation
    }

    /// Candidate severity.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Candidate source range.
    #[must_use]
    pub const fn range(&self) -> Range {
        self.range
    }

    /// Candidate user-facing message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Optional detailed explanation.
    #[must_use]
    pub fn explanation(&self) -> Option<&str> {
        self.explanation.as_deref()
    }
}

/// One producer contribution retained after semantic merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CriticFindingContributor {
    identity: CriticObservedIdentity,
    severity: Severity,
    message: String,
    explanation: Option<String>,
}

impl CriticFindingContributor {
    fn from_candidate(candidate: &CriticFindingCandidate) -> Self {
        Self {
            identity: candidate.identity.clone(),
            severity: candidate.severity,
            message: candidate.message.clone(),
            explanation: candidate.explanation.clone(),
        }
    }

    /// Observed producer/code/shape identity.
    #[must_use]
    pub const fn identity(&self) -> &CriticObservedIdentity {
        &self.identity
    }

    /// Severity reported by this producer.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Message reported by this producer.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Detailed explanation reported by this producer.
    #[must_use]
    pub fn explanation(&self) -> Option<&str> {
        self.explanation.as_deref()
    }
}

/// One normalized logical critic finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizedCriticFinding {
    canonical_id: Option<String>,
    public_code: String,
    approved_aliases: Vec<CriticObservedIdentity>,
    category: Option<CriticIdentityCategory>,
    severity: Severity,
    severity_conflict: bool,
    range: Range,
    source_generation: u64,
    message: String,
    explanation: Option<String>,
    contributors: Vec<CriticFindingContributor>,
    #[serde(skip)]
    presentation_rank: u8,
    #[serde(skip)]
    explanation_rank: u8,
}

impl NormalizedCriticFinding {
    fn from_candidate(
        candidate: CriticFindingCandidate,
        identity_entry: Option<&'static CriticIdentityEntry>,
    ) -> Self {
        let (canonical_id, approved_aliases, category) = identity_entry.map_or_else(
            || (None, vec![candidate.identity.clone()], None),
            |entry| {
                let aliases = entry
                    .aliases()
                    .iter()
                    .map(|alias| {
                        CriticObservedIdentity::new(alias.origin(), alias.code(), alias.shape())
                    })
                    .collect();
                (Some(entry.canonical_id().to_string()), aliases, Some(entry.category()))
            },
        );
        let presentation_rank = presentation_rank(candidate.identity.origin);
        let explanation_rank = candidate
            .explanation
            .as_ref()
            .map_or(u8::MAX, |_| explanation_rank(candidate.identity.origin));
        let contributor = CriticFindingContributor::from_candidate(&candidate);

        Self {
            canonical_id,
            public_code: candidate.identity.code.clone(),
            approved_aliases,
            category,
            severity: candidate.severity,
            severity_conflict: false,
            range: candidate.range,
            source_generation: candidate.source_generation,
            message: candidate.message,
            explanation: candidate.explanation,
            contributors: vec![contributor],
            presentation_rank,
            explanation_rank,
        }
    }

    fn merge_candidate(&mut self, candidate: CriticFindingCandidate) {
        if self.severity != candidate.severity {
            self.severity_conflict = true;
            if severity_score(candidate.severity) > severity_score(self.severity) {
                self.severity = candidate.severity;
            }
        }

        let candidate_presentation_rank = presentation_rank(candidate.identity.origin);
        if candidate_presentation_rank < self.presentation_rank {
            self.presentation_rank = candidate_presentation_rank;
            self.public_code = candidate.identity.code.clone();
            self.message = candidate.message.clone();
        }

        if candidate.explanation.is_some() {
            let candidate_explanation_rank = explanation_rank(candidate.identity.origin);
            if candidate_explanation_rank < self.explanation_rank {
                self.explanation_rank = candidate_explanation_rank;
                self.explanation = candidate.explanation.clone();
            }
        }

        self.contributors.push(CriticFindingContributor::from_candidate(&candidate));
    }

    fn finalize(mut self) -> Self {
        self.approved_aliases.sort();
        self.approved_aliases.dedup();
        self.contributors.sort_by(compare_contributors);
        self.contributors.dedup();
        self
    }

    /// Canonical logical finding ID, or `None` for an unregistered policy.
    #[must_use]
    pub fn canonical_id(&self) -> Option<&str> {
        self.canonical_id.as_deref()
    }

    /// Compatibility code selected for user-facing presentation.
    #[must_use]
    pub fn public_code(&self) -> &str {
        &self.public_code
    }

    /// Approved canonical compatibility aliases.
    #[must_use]
    pub fn approved_aliases(&self) -> &[CriticObservedIdentity] {
        &self.approved_aliases
    }

    /// Canonical category when the finding is registered.
    #[must_use]
    pub const fn category(&self) -> Option<CriticIdentityCategory> {
        self.category
    }

    /// Most prominent severity reported by contributing producers.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Whether contributing producers disagreed about severity.
    #[must_use]
    pub const fn has_severity_conflict(&self) -> bool {
        self.severity_conflict
    }

    /// Exact merged source range.
    #[must_use]
    pub const fn range(&self) -> Range {
        self.range
    }

    /// Exact source generation shared by all contributors.
    #[must_use]
    pub const fn source_generation(&self) -> u64 {
        self.source_generation
    }

    /// User-facing message selected by deterministic presentation precedence.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Richest available detailed explanation.
    #[must_use]
    pub fn explanation(&self) -> Option<&str> {
        self.explanation.as_deref()
    }

    /// Every producer contribution retained after merge.
    #[must_use]
    pub fn contributors(&self) -> &[CriticFindingContributor] {
        &self.contributors
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum MergeIdentity {
    Registered(&'static str),
    Unregistered(CriticObservedIdentity),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MergeKey {
    identity: MergeIdentity,
    start_byte: usize,
    end_byte: usize,
    source_generation: u64,
}

/// Normalize candidates and semantically merge declared aliases.
///
/// Candidates merge only when the identity registry resolves them to the same
/// canonical ID and they share the exact byte range and source generation.
/// Message, severity, and range coincidence never establish identity.
#[must_use]
pub fn normalize_critic_findings(
    candidates: impl IntoIterator<Item = CriticFindingCandidate>,
) -> Vec<NormalizedCriticFinding> {
    let mut groups: BTreeMap<MergeKey, NormalizedCriticFinding> = BTreeMap::new();

    for candidate in candidates {
        let identity_entry = CriticIdentityRegistry::resolve(
            candidate.identity.origin,
            &candidate.identity.code,
            candidate.identity.shape,
        );
        let merge_identity = identity_entry.map_or_else(
            || MergeIdentity::Unregistered(candidate.identity.clone()),
            |entry| MergeIdentity::Registered(entry.canonical_id()),
        );
        let key = MergeKey {
            identity: merge_identity,
            start_byte: candidate.range.start.byte,
            end_byte: candidate.range.end.byte,
            source_generation: candidate.source_generation,
        };

        match groups.get_mut(&key) {
            Some(existing) => existing.merge_candidate(candidate),
            None => {
                groups.insert(key, NormalizedCriticFinding::from_candidate(candidate, identity_entry));
            }
        }
    }

    groups.into_values().map(NormalizedCriticFinding::finalize).collect()
}

fn presentation_rank(origin: CriticFindingOrigin) -> u8 {
    match origin {
        CriticFindingOrigin::BuiltInDiagnostic => 0,
        CriticFindingOrigin::NativeCritic => 1,
        CriticFindingOrigin::LegacyPolicy => 2,
        CriticFindingOrigin::ExternalPerlCritic => 3,
    }
}

fn explanation_rank(origin: CriticFindingOrigin) -> u8 {
    match origin {
        CriticFindingOrigin::NativeCritic => 0,
        CriticFindingOrigin::LegacyPolicy => 1,
        CriticFindingOrigin::ExternalPerlCritic => 2,
        CriticFindingOrigin::BuiltInDiagnostic => 3,
    }
}

fn severity_score(severity: Severity) -> u8 {
    severity as u8
}

fn compare_contributors(
    left: &CriticFindingContributor,
    right: &CriticFindingContributor,
) -> Ordering {
    (
        &left.identity,
        severity_score(left.severity),
        &left.message,
        &left.explanation,
    )
        .cmp(&(
            &right.identity,
            severity_score(right.severity),
            &right.message,
            &right.explanation,
        ))
}

#[cfg(test)]
mod tests {
    use perl_parser_core::position::{Position, Range};

    use super::{
        CriticFindingCandidate, CriticObservedIdentity, normalize_critic_findings,
    };
    use crate::tooling::perl_critic::{
        CriticFindingOrigin, CriticFindingShape, Severity,
    };

    fn range(start: usize, end: usize) -> Range {
        Range {
            start: Position { byte: start, line: 0, column: start },
            end: Position { byte: end, line: 0, column: end },
        }
    }

    fn identity(
        origin: CriticFindingOrigin,
        code: &str,
        shape: CriticFindingShape,
    ) -> CriticObservedIdentity {
        CriticObservedIdentity::new(origin, code, shape)
    }

    fn candidate(
        observed_identity: CriticObservedIdentity,
        generation: u64,
        source_range: Range,
        severity: Severity,
        message: &str,
        explanation: Option<&str>,
    ) -> CriticFindingCandidate {
        CriticFindingCandidate::new(
            observed_identity,
            generation,
            severity,
            source_range,
            message,
            explanation.map(str::to_string),
        )
    }

    fn strict_alias_candidates() -> Vec<CriticFindingCandidate> {
        vec![
            candidate(
                identity(
                    CriticFindingOrigin::BuiltInDiagnostic,
                    "PL100",
                    CriticFindingShape::General,
                ),
                7,
                range(0, 0),
                Severity::Harsh,
                "Missing use strict",
                None,
            ),
            candidate(
                identity(
                    CriticFindingOrigin::NativeCritic,
                    "native.testing.require_use_strict",
                    CriticFindingShape::General,
                ),
                7,
                range(0, 0),
                Severity::Harsh,
                "Code does not use strict",
                Some("Always use strict to catch common mistakes"),
            ),
        ]
    }

    #[test]
    fn declared_aliases_merge_once_and_retain_both_producers() {
        let normalized = normalize_critic_findings(strict_alias_candidates());
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].canonical_id(), Some("critic.testing.require_use_strict"));
        assert_eq!(normalized[0].public_code(), "PL100");
        assert_eq!(normalized[0].message(), "Missing use strict");
        assert_eq!(
            normalized[0].explanation(),
            Some("Always use strict to catch common mistakes")
        );
        assert_eq!(normalized[0].contributors().len(), 2);
        assert!(normalized[0]
            .contributors()
            .iter()
            .any(|contributor| contributor.identity().origin() == CriticFindingOrigin::NativeCritic));
    }

    #[test]
    fn unrelated_same_range_same_severity_findings_survive() {
        let candidates = vec![
            candidate(
                identity(
                    CriticFindingOrigin::BuiltInDiagnostic,
                    "PL100",
                    CriticFindingShape::General,
                ),
                7,
                range(0, 0),
                Severity::Harsh,
                "Missing strict",
                None,
            ),
            candidate(
                identity(
                    CriticFindingOrigin::BuiltInDiagnostic,
                    "PL101",
                    CriticFindingShape::General,
                ),
                7,
                range(0, 0),
                Severity::Harsh,
                "Missing warnings",
                None,
            ),
        ];
        let normalized = normalize_critic_findings(candidates);
        assert_eq!(normalized.len(), 2);
    }

    #[test]
    fn aliases_from_different_generations_never_merge() {
        let mut candidates = strict_alias_candidates();
        candidates[1] = candidate(
            identity(
                CriticFindingOrigin::NativeCritic,
                "native.testing.require_use_strict",
                CriticFindingShape::General,
            ),
            8,
            range(0, 0),
            Severity::Harsh,
            "Code does not use strict",
            Some("Always use strict"),
        );
        let normalized = normalize_critic_findings(candidates);
        assert_eq!(normalized.len(), 2);
    }

    #[test]
    fn aliases_at_different_ranges_never_merge() {
        let mut candidates = strict_alias_candidates();
        candidates[1] = candidate(
            identity(
                CriticFindingOrigin::NativeCritic,
                "native.testing.require_use_strict",
                CriticFindingShape::General,
            ),
            7,
            range(1, 1),
            Severity::Harsh,
            "Code does not use strict",
            Some("Always use strict"),
        );
        let normalized = normalize_critic_findings(candidates);
        assert_eq!(normalized.len(), 2);
    }

    #[test]
    fn unknown_external_policy_is_not_guessed_into_a_native_finding() {
        let candidates = vec![
            candidate(
                identity(
                    CriticFindingOrigin::ExternalPerlCritic,
                    "Unknown::Policy",
                    CriticFindingShape::General,
                ),
                7,
                range(3, 5),
                Severity::Stern,
                "same message",
                None,
            ),
            candidate(
                identity(
                    CriticFindingOrigin::NativeCritic,
                    "native.common.assignment_in_condition",
                    CriticFindingShape::General,
                ),
                7,
                range(3, 5),
                Severity::Stern,
                "same message",
                None,
            ),
        ];
        let normalized = normalize_critic_findings(candidates);
        assert_eq!(normalized.len(), 2);
        assert!(normalized.iter().any(|finding| finding.canonical_id().is_none()));
    }

    #[test]
    fn severity_disagreement_is_retained_and_uses_the_more_prominent_value() {
        let candidates = vec![
            candidate(
                identity(
                    CriticFindingOrigin::BuiltInDiagnostic,
                    "PL100",
                    CriticFindingShape::General,
                ),
                7,
                range(0, 0),
                Severity::Harsh,
                "Missing strict",
                None,
            ),
            candidate(
                identity(
                    CriticFindingOrigin::NativeCritic,
                    "native.testing.require_use_strict",
                    CriticFindingShape::General,
                ),
                7,
                range(0, 0),
                Severity::Stern,
                "Code does not use strict",
                Some("Always use strict"),
            ),
        ];
        let normalized = normalize_critic_findings(candidates);
        assert_eq!(normalized.len(), 1);
        assert!(normalized[0].has_severity_conflict());
        assert_eq!(normalized[0].severity(), Severity::Stern);
    }

    #[test]
    fn output_is_deterministic_across_input_order() {
        let forward = strict_alias_candidates();
        let mut reverse = forward.clone();
        reverse.reverse();
        assert_eq!(normalize_critic_findings(forward), normalize_critic_findings(reverse));
    }
}
