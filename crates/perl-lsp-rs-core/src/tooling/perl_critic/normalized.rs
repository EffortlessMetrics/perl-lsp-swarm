//! Canonical normalization and semantic merge for critic findings.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use perl_parser_core::position::Range;
use serde::Serialize;

use super::{
    CriticFindingOrigin, CriticFindingShape, CriticIdentityCategory, CriticIdentityEntry,
    CriticIdentityRegistry, CriticObservedIdentity, Severity,
};

/// Owned record of a checked producer identity.
///
/// Construction is only available from [`CriticObservedIdentity`], whose
/// fields and shape selection are controlled by the identity authority.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct OwnedCriticObservedIdentity {
    origin: CriticFindingOrigin,
    code: String,
    shape: CriticFindingShape,
}

impl From<CriticObservedIdentity<'_>> for OwnedCriticObservedIdentity {
    fn from(identity: CriticObservedIdentity<'_>) -> Self {
        Self {
            origin: identity.origin(),
            code: identity.code().to_string(),
            shape: identity.shape(),
        }
    }
}

impl OwnedCriticObservedIdentity {
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

    /// Producer-owned reviewed finding shape.
    #[must_use]
    pub const fn shape(&self) -> CriticFindingShape {
        self.shape
    }

    fn resolve(&self) -> Option<&'static CriticIdentityEntry> {
        CriticIdentityRegistry::resolve_parts(self.origin, &self.code, self.shape)
    }
}

/// Logical source and exact generation that produced a candidate.
///
/// The 128-bit source key is an opaque, path-free document identity supplied by
/// the owning document/workspace authority. Equal generations from different
/// documents therefore cannot merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CriticSourceIdentity {
    source_key: [u8; 16],
    generation: u64,
}

impl CriticSourceIdentity {
    /// Construct a logical source identity for one exact document generation.
    #[must_use]
    pub const fn new(source_key: [u8; 16], generation: u64) -> Self {
        Self { source_key, generation }
    }

    /// Opaque path-free logical source key.
    #[must_use]
    pub const fn source_key(self) -> [u8; 16] {
        self.source_key
    }

    /// Exact source generation.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }
}

/// One pre-normalization critic finding candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CriticFindingCandidate {
    identity: OwnedCriticObservedIdentity,
    source_identity: CriticSourceIdentity,
    severity: Severity,
    range: Range,
    message: String,
    explanation: Option<String>,
}

impl CriticFindingCandidate {
    /// Construct a finding candidate from one checked producer identity.
    #[must_use]
    pub fn new(
        identity: CriticObservedIdentity<'_>,
        source_identity: CriticSourceIdentity,
        severity: Severity,
        range: Range,
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self {
            identity: identity.into(),
            source_identity,
            severity,
            range,
            message: message.into(),
            explanation,
        }
    }

    /// Checked observed producer identity.
    #[must_use]
    pub const fn identity(&self) -> &OwnedCriticObservedIdentity {
        &self.identity
    }

    /// Exact logical source and generation that produced this candidate.
    #[must_use]
    pub const fn source_identity(&self) -> CriticSourceIdentity {
        self.source_identity
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
    identity: OwnedCriticObservedIdentity,
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

    /// Checked observed producer identity.
    #[must_use]
    pub const fn identity(&self) -> &OwnedCriticObservedIdentity {
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
    approved_aliases: Vec<OwnedCriticObservedIdentity>,
    category: Option<CriticIdentityCategory>,
    severity: Severity,
    severity_conflict: bool,
    range: Range,
    source_identity: CriticSourceIdentity,
    message: String,
    explanation: Option<String>,
    contributors: Vec<CriticFindingContributor>,
    #[serde(skip)]
    presentation_rank: u8,
    #[serde(skip)]
    explanation_rank: u8,
    #[serde(skip)]
    explanation_code: Option<String>,
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
                    .copied()
                    .map(|alias| OwnedCriticObservedIdentity::from(alias.observed()))
                    .collect();
                (Some(entry.canonical_id().to_string()), aliases, Some(entry.category()))
            },
        );
        let presentation_rank = presentation_rank(candidate.identity.origin());
        let explanation_rank = candidate
            .explanation
            .as_ref()
            .map_or(u8::MAX, |_| explanation_rank(candidate.identity.origin()));
        let explanation_code = candidate
            .explanation
            .as_ref()
            .map(|_| candidate.identity.code.clone());
        let contributor = CriticFindingContributor::from_candidate(&candidate);

        Self {
            canonical_id,
            public_code: candidate.identity.code.clone(),
            approved_aliases,
            category,
            severity: candidate.severity,
            severity_conflict: false,
            range: candidate.range,
            source_identity: candidate.source_identity,
            message: candidate.message,
            explanation: candidate.explanation,
            contributors: vec![contributor],
            presentation_rank,
            explanation_rank,
            explanation_code,
        }
    }

    fn merge_candidate(&mut self, candidate: CriticFindingCandidate) {
        if self.severity != candidate.severity {
            self.severity_conflict = true;
            if severity_score(candidate.severity) > severity_score(self.severity) {
                self.severity = candidate.severity;
            }
        }

        let candidate_presentation_rank = presentation_rank(candidate.identity.origin());
        if (
            candidate_presentation_rank,
            candidate.identity.code.as_str(),
            candidate.message.as_str(),
        ) < (
            self.presentation_rank,
            self.public_code.as_str(),
            self.message.as_str(),
        ) {
            self.presentation_rank = candidate_presentation_rank;
            self.public_code = candidate.identity.code.clone();
            self.message = candidate.message.clone();
        }

        if let Some(candidate_explanation) = candidate.explanation.as_deref() {
            let candidate_explanation_rank = explanation_rank(candidate.identity.origin());
            let candidate_precedes = match (
                self.explanation.as_deref(),
                self.explanation_code.as_deref(),
            ) {
                (Some(current_explanation), Some(current_code)) => {
                    (
                        candidate_explanation_rank,
                        candidate_explanation,
                        candidate.identity.code.as_str(),
                    ) < (
                        self.explanation_rank,
                        current_explanation,
                        current_code,
                    )
                }
                _ => true,
            };
            if candidate_precedes {
                self.explanation_rank = candidate_explanation_rank;
                self.explanation = candidate.explanation.clone();
                self.explanation_code = Some(candidate.identity.code.clone());
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
    pub fn approved_aliases(&self) -> &[OwnedCriticObservedIdentity] {
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

    /// Exact logical source and generation shared by all contributors.
    #[must_use]
    pub const fn source_identity(&self) -> CriticSourceIdentity {
        self.source_identity
    }

    /// Exact source generation shared by all contributors.
    #[must_use]
    pub const fn source_generation(&self) -> u64 {
        self.source_identity.generation()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RangeIdentity {
    start_byte: usize,
    start_line: u32,
    start_column: u32,
    end_byte: usize,
    end_line: u32,
    end_column: u32,
}

impl From<Range> for RangeIdentity {
    fn from(range: Range) -> Self {
        Self {
            start_byte: range.start.byte,
            start_line: range.start.line,
            start_column: range.start.column,
            end_byte: range.end.byte,
            end_line: range.end.line,
            end_column: range.end.column,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MergeKey {
    canonical_id: &'static str,
    source_identity: CriticSourceIdentity,
    range: RangeIdentity,
}

/// Normalize candidates and semantically merge declared aliases.
///
/// Registered candidates merge only when the identity registry resolves them
/// to the same canonical ID and they share the same logical source, exact
/// generation, and complete range. Unregistered candidates are always emitted
/// as independent findings; coincidence never invents an identity for them.
#[must_use]
pub fn normalize_critic_findings(
    candidates: impl IntoIterator<Item = CriticFindingCandidate>,
) -> Vec<NormalizedCriticFinding> {
    let mut groups: BTreeMap<MergeKey, NormalizedCriticFinding> = BTreeMap::new();
    let mut normalized = Vec::new();

    for candidate in candidates {
        let identity_entry = candidate.identity.resolve();
        let Some(identity_entry) = identity_entry else {
            normalized.push(NormalizedCriticFinding::from_candidate(candidate, None).finalize());
            continue;
        };
        let key = MergeKey {
            canonical_id: identity_entry.canonical_id(),
            source_identity: candidate.source_identity,
            range: candidate.range.into(),
        };

        match groups.get_mut(&key) {
            Some(existing) => existing.merge_candidate(candidate),
            None => {
                groups.insert(
                    key,
                    NormalizedCriticFinding::from_candidate(candidate, Some(identity_entry)),
                );
            }
        }
    }

    normalized.extend(groups.into_values().map(NormalizedCriticFinding::finalize));
    normalized.sort_by(compare_normalized);
    normalized
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

fn compare_normalized(
    left: &NormalizedCriticFinding,
    right: &NormalizedCriticFinding,
) -> Ordering {
    left.source_identity
        .cmp(&right.source_identity)
        .then_with(|| RangeIdentity::from(left.range).cmp(&RangeIdentity::from(right.range)))
        .then_with(|| left.canonical_id.cmp(&right.canonical_id))
        .then_with(|| left.approved_aliases.cmp(&right.approved_aliases))
        .then_with(|| left.public_code.cmp(&right.public_code))
        .then_with(|| left.message.cmp(&right.message))
        .then_with(|| left.explanation.cmp(&right.explanation))
        .then_with(|| severity_score(left.severity).cmp(&severity_score(right.severity)))
}

#[cfg(test)]
mod tests {
    use perl_parser_core::position::{Position, Range};

    use super::{
        CriticFindingCandidate, CriticSourceIdentity, OwnedCriticObservedIdentity,
        normalize_critic_findings,
    };
    use crate::tooling::perl_critic::{
        CriticFindingOrigin, CriticIdentityRegistry, CriticObservedIdentity, Severity,
    };

    fn source(document: u8, generation: u64) -> CriticSourceIdentity {
        let mut source_key = [0; 16];
        source_key[15] = document;
        CriticSourceIdentity::new(source_key, generation)
    }

    fn range(start: usize, end: usize) -> Range {
        range_with_positions(
            start,
            0,
            u32::try_from(start).unwrap_or(u32::MAX),
            end,
            0,
            u32::try_from(end).unwrap_or(u32::MAX),
        )
    }

    fn range_with_positions(
        start_byte: usize,
        start_line: u32,
        start_column: u32,
        end_byte: usize,
        end_line: u32,
        end_column: u32,
    ) -> Range {
        Range {
            start: Position { byte: start_byte, line: start_line, column: start_column },
            end: Position { byte: end_byte, line: end_line, column: end_column },
        }
    }

    fn candidate(
        identity: CriticObservedIdentity<'_>,
        source_identity: CriticSourceIdentity,
        source_range: Range,
        severity: Severity,
        message: &str,
        explanation: Option<&str>,
    ) -> CriticFindingCandidate {
        CriticFindingCandidate::new(
            identity,
            source_identity,
            severity,
            source_range,
            message,
            explanation.map(str::to_string),
        )
    }

    fn general_candidate(
        origin: CriticFindingOrigin,
        code: &str,
        source_identity: CriticSourceIdentity,
        source_range: Range,
        severity: Severity,
        message: &str,
        explanation: Option<&str>,
    ) -> Option<CriticFindingCandidate> {
        CriticObservedIdentity::general(origin, code).ok().map(|identity| {
            candidate(
                identity,
                source_identity,
                source_range,
                severity,
                message,
                explanation,
            )
        })
    }

    fn strict_alias_candidates() -> Vec<CriticFindingCandidate> {
        let candidates = vec![
            general_candidate(
                CriticFindingOrigin::BuiltInDiagnostic,
                "PL100",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Missing use strict",
                None,
            ),
            general_candidate(
                CriticFindingOrigin::NativeCritic,
                "native.testing.require_use_strict",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Code does not use strict",
                Some("Always use strict to catch common mistakes"),
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        assert_eq!(candidates.len(), 2);
        candidates
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
            general_candidate(
                CriticFindingOrigin::BuiltInDiagnostic,
                "PL100",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Missing strict",
                None,
            ),
            general_candidate(
                CriticFindingOrigin::BuiltInDiagnostic,
                "PL101",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Missing warnings",
                None,
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        assert_eq!(normalize_critic_findings(candidates).len(), 2);
    }

    #[test]
    fn aliases_from_different_generations_never_merge() {
        let candidates = vec![
            general_candidate(
                CriticFindingOrigin::BuiltInDiagnostic,
                "PL100",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Missing strict",
                None,
            ),
            general_candidate(
                CriticFindingOrigin::NativeCritic,
                "native.testing.require_use_strict",
                source(1, 8),
                range(0, 0),
                Severity::Harsh,
                "Code does not use strict",
                Some("Always use strict"),
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        assert_eq!(normalize_critic_findings(candidates).len(), 2);
    }

    #[test]
    fn equal_generation_and_range_from_different_documents_never_merge() {
        let candidates = vec![
            general_candidate(
                CriticFindingOrigin::BuiltInDiagnostic,
                "PL100",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Missing strict",
                None,
            ),
            general_candidate(
                CriticFindingOrigin::NativeCritic,
                "native.testing.require_use_strict",
                source(2, 7),
                range(0, 0),
                Severity::Harsh,
                "Code does not use strict",
                Some("Always use strict"),
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        assert_eq!(normalize_critic_findings(candidates).len(), 2);
    }

    #[test]
    fn same_bytes_with_different_line_or_column_coordinates_never_merge() {
        let candidates = vec![
            general_candidate(
                CriticFindingOrigin::BuiltInDiagnostic,
                "PL100",
                source(1, 7),
                range_with_positions(3, 0, 3, 5, 0, 5),
                Severity::Harsh,
                "Missing strict",
                None,
            ),
            general_candidate(
                CriticFindingOrigin::NativeCritic,
                "native.testing.require_use_strict",
                source(1, 7),
                range_with_positions(3, 1, 0, 5, 1, 2),
                Severity::Harsh,
                "Code does not use strict",
                Some("Always use strict"),
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        assert_eq!(normalize_critic_findings(candidates).len(), 2);
    }

    #[test]
    fn identical_unregistered_candidates_remain_independent() {
        let candidates = CriticObservedIdentity::general(
            CriticFindingOrigin::ExternalPerlCritic,
            "Unknown::Policy",
        )
        .ok()
        .map_or_else(Vec::new, |identity| {
            vec![
                candidate(
                    identity,
                    source(1, 7),
                    range(3, 5),
                    Severity::Stern,
                    "same message",
                    None,
                ),
                candidate(
                    identity,
                    source(1, 7),
                    range(3, 5),
                    Severity::Stern,
                    "same message",
                    None,
                ),
            ]
        });
        assert_eq!(candidates.len(), 2);
        let normalized = normalize_critic_findings(candidates);
        assert_eq!(normalized.len(), 2);
        assert!(normalized.iter().all(|finding| finding.canonical_id().is_none()));
    }

    #[test]
    fn equal_rank_presentation_and_explanation_ties_are_order_independent() {
        let candidates = vec![
            general_candidate(
                CriticFindingOrigin::BuiltInDiagnostic,
                "PL100",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Zulu built-in message",
                None,
            ),
            general_candidate(
                CriticFindingOrigin::BuiltInDiagnostic,
                "PL100",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Alpha built-in message",
                None,
            ),
            general_candidate(
                CriticFindingOrigin::NativeCritic,
                "native.testing.require_use_strict",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Native message",
                Some("Zulu native explanation"),
            ),
            general_candidate(
                CriticFindingOrigin::NativeCritic,
                "native.testing.require_use_strict",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Native message",
                Some("Alpha native explanation"),
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let mut reverse = candidates.clone();
        reverse.reverse();

        let forward = normalize_critic_findings(candidates);
        let backward = normalize_critic_findings(reverse);
        assert_eq!(forward, backward);
        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].message(), "Alpha built-in message");
        assert_eq!(forward[0].explanation(), Some("Alpha native explanation"));
    }

    #[test]
    fn severity_disagreement_is_retained_and_uses_the_more_prominent_value() {
        let candidates = vec![
            general_candidate(
                CriticFindingOrigin::BuiltInDiagnostic,
                "PL100",
                source(1, 7),
                range(0, 0),
                Severity::Harsh,
                "Missing strict",
                None,
            ),
            general_candidate(
                CriticFindingOrigin::NativeCritic,
                "native.testing.require_use_strict",
                source(1, 7),
                range(0, 0),
                Severity::Stern,
                "Code does not use strict",
                Some("Always use strict"),
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
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

    #[test]
    fn every_registered_alias_resolves_through_owned_identity() {
        for entry in CriticIdentityRegistry::entries() {
            for alias in entry.aliases() {
                let owned = OwnedCriticObservedIdentity::from(alias.observed());
                assert_eq!(
                    owned.resolve().map(|resolved| resolved.canonical_id()),
                    Some(entry.canonical_id()),
                    "registered alias must remain resolvable through normalization: {:?}",
                    alias,
                );
            }
        }
    }
}
