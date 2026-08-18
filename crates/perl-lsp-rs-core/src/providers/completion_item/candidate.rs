//! Transport-neutral completion candidate identity, merge, and finalization policy.
//!
//! Providers do not all have canonical semantic identity yet. The candidate
//! envelope therefore keeps an explicit legacy-label identity for compatibility
//! while allowing migrated providers to attach semantic or source-anchored
//! identity and typed rank dimensions before LSP presentation.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use perl_semantic_facts::{
    Confidence, EntityId, SemanticConfidence, SemanticFreshness, SemanticProducer, SourceAnchor,
    SourceGeneration,
};

use super::CompletionItem;

/// Stable identity used to decide whether two completion candidates describe
/// the same semantic projection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompletionCandidateIdentity {
    /// Canonical semantic entity plus the projection being offered.
    Semantic {
        /// Canonical semantic entity identifier.
        entity_id: EntityId,
        /// Projection identity, such as `method`, `default_export`, or
        /// `generated_accessor`.
        projection: String,
    },
    /// Source-backed candidate whose canonical entity is not available yet.
    SourceAnchored {
        /// Stable owner or producer identity.
        owner: String,
        /// Canonical source or generator anchor.
        anchor: SourceAnchor,
        /// Projection identity at the anchor.
        projection: String,
    },
    /// Compatibility identity for providers that have not migrated yet.
    ///
    /// This deliberately preserves the existing label-based deduplication
    /// behavior until those providers can supply stronger identity.
    LegacyLabel(String),
}

/// Strength of the proof behind a completion candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompletionCandidateProof {
    /// Compatibility or fallback provider evidence.
    LegacyFallback,
    /// Current bounded evidence with known limitations.
    Qualified,
    /// Current exact evidence for the represented candidate class.
    Exact,
}

/// Evidence envelope retained while completion candidates are merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCandidateEvidence {
    /// Subsystem that produced or adapted this candidate.
    pub producer: SemanticProducer,
    /// Provider-local proof class for the represented completion candidate.
    pub proof: CompletionCandidateProof,
    /// Shared semantic confidence carried by the candidate's source facts.
    pub confidence: SemanticConfidence,
    /// Shared semantic freshness for the consuming request.
    pub freshness: SemanticFreshness,
    /// Source or semantic generation when one is available.
    pub generation: SourceGeneration,
}

impl Default for CompletionCandidateEvidence {
    fn default() -> Self {
        Self {
            producer: SemanticProducer::Unknown,
            proof: CompletionCandidateProof::LegacyFallback,
            confidence: SemanticConfidence::Unknown,
            freshness: SemanticFreshness::Unknown,
            generation: SourceGeneration::Unknown,
        }
    }
}

impl CompletionCandidateEvidence {
    fn strength(&self) -> (u8, CompletionCandidateProof, u8) {
        (freshness_strength(self.freshness), self.proof, confidence_strength(self.confidence))
    }
}

fn freshness_strength(freshness: SemanticFreshness) -> u8 {
    match freshness {
        SemanticFreshness::Fresh => 3,
        SemanticFreshness::NotApplicable => 2,
        SemanticFreshness::Unknown => 1,
        SemanticFreshness::Stale => 0,
        _ => 0,
    }
}

fn confidence_strength(confidence: SemanticConfidence) -> u8 {
    match confidence {
        SemanticConfidence::Known(Confidence::High) => 3,
        SemanticConfidence::Known(Confidence::Medium) => 2,
        SemanticConfidence::Known(Confidence::Low) => 1,
        SemanticConfidence::Unknown => 0,
        _ => 0,
    }
}

/// Ordered rank class used for one completion relevance dimension.
///
/// Variants are declared from best to worst so normal `Ord` comparison yields
/// the final completion order directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompletionRankClass {
    /// Exact semantic fit for the request context.
    Exact,
    /// Strong but not exact fit.
    Strong,
    /// Neutral compatibility value when no typed rank is supplied yet.
    Neutral,
    /// Useful qualified result with explicit limitations.
    Qualified,
    /// Bounded fallback result.
    Fallback,
    /// Legacy compatibility result awaiting provider migration.
    Legacy,
}

/// Typed completion rank dimensions evaluated at the final assembly boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRankKey {
    /// Semantic fit to the expected value, name, call, or key context.
    pub semantic_fit: CompletionRankClass,
    /// Visibility or import status at the request position.
    pub visibility: CompletionRankClass,
    /// Fit between the candidate and the resolved receiver.
    pub receiver_fit: CompletionRankClass,
    /// Lexical or scope distance; lower values rank first.
    pub scope_distance: u32,
    /// Relative authority of the candidate's source tier.
    pub source_tier: CompletionRankClass,
    /// Exact, qualified, fallback, or legacy disposition.
    pub fallback: CompletionRankClass,
    /// Prefix or match quality for this request.
    pub match_quality: CompletionRankClass,
    /// Existing provider sort key retained during staged migration.
    pub compatibility_sort: String,
}

impl CompletionRankKey {
    /// Preserve the current provider-defined order while typed rank dimensions
    /// are introduced one candidate family at a time.
    #[must_use]
    pub fn compatibility(item: &CompletionItem) -> Self {
        Self {
            semantic_fit: CompletionRankClass::Neutral,
            visibility: CompletionRankClass::Neutral,
            receiver_fit: CompletionRankClass::Neutral,
            scope_distance: 0,
            source_tier: CompletionRankClass::Neutral,
            fallback: CompletionRankClass::Neutral,
            match_quality: CompletionRankClass::Neutral,
            compatibility_sort: item
                .sort_text
                .as_deref()
                .unwrap_or(item.label.as_ref())
                .to_string(),
        }
    }
}

/// Conflict that prevents two candidates with one identity from being merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompletionCandidateConflict {
    /// Candidates would apply different insert text or additional edits.
    InsertionPlan,
    /// Candidates identify different source or generator anchors.
    SourceAnchor,
    /// Candidates belong to different accepted semantic generations.
    Generation,
    /// Candidates disagree about the receiver package.
    ReceiverOwner,
    /// Candidates disagree about the package that defines the member.
    DefiningOwner,
    /// Candidates carry different explicit insertion-plan identities.
    InsertionPlanIdentity,
}

/// Completion candidate before LSP rendering and final ranking.
#[derive(Debug, Clone)]
pub struct CompletionCandidate {
    /// Stable candidate or projection identity.
    pub identity: CompletionCandidateIdentity,
    /// Existing transport-neutral completion presentation payload.
    pub item: CompletionItem,
    /// Proof, confidence, freshness, and generation evidence.
    pub evidence: CompletionCandidateEvidence,
    /// Typed rank dimensions used at the final assembly boundary.
    pub rank: CompletionRankKey,
    /// Exact source or generator anchor when available.
    pub source_anchor: Option<SourceAnchor>,
    /// Receiver package identity when the candidate is receiver-dependent.
    pub receiver_package: Option<String>,
    /// Package that defines the offered member when applicable.
    pub defining_package: Option<String>,
    /// Stable insertion-plan identity supplied by the insertion planner.
    pub insertion_plan_id: Option<String>,
    /// Known limitations retained through merging.
    pub limitations: Vec<String>,
    /// Conflicts that prevented an identity-bearing merge.
    pub conflicts: Vec<CompletionCandidateConflict>,
}

impl CompletionCandidate {
    /// Wrap an unmigrated provider item while preserving current label-based
    /// deduplication and ordering behavior.
    #[must_use]
    pub fn legacy(item: CompletionItem) -> Self {
        let identity = CompletionCandidateIdentity::LegacyLabel(item.label.to_string());
        let rank = CompletionRankKey::compatibility(&item);
        Self {
            identity,
            item,
            evidence: CompletionCandidateEvidence::default(),
            rank,
            source_anchor: None,
            receiver_package: None,
            defining_package: None,
            insertion_plan_id: None,
            limitations: Vec::new(),
            conflicts: Vec::new(),
        }
    }

    /// Build a candidate for one canonical semantic entity projection.
    #[must_use]
    pub fn semantic(
        entity_id: EntityId,
        projection: impl Into<String>,
        item: CompletionItem,
    ) -> Self {
        let rank = CompletionRankKey::compatibility(&item);
        Self {
            identity: CompletionCandidateIdentity::Semantic {
                entity_id,
                projection: projection.into(),
            },
            item,
            evidence: CompletionCandidateEvidence::default(),
            rank,
            source_anchor: None,
            receiver_package: None,
            defining_package: None,
            insertion_plan_id: None,
            limitations: Vec::new(),
            conflicts: Vec::new(),
        }
    }

    /// Build a source-anchored candidate without inventing a semantic entity ID.
    #[must_use]
    pub fn source_anchored(
        owner: impl Into<String>,
        anchor: SourceAnchor,
        projection: impl Into<String>,
        item: CompletionItem,
    ) -> Self {
        let rank = CompletionRankKey::compatibility(&item);
        Self {
            identity: CompletionCandidateIdentity::SourceAnchored {
                owner: owner.into(),
                anchor,
                projection: projection.into(),
            },
            item,
            evidence: CompletionCandidateEvidence::default(),
            rank,
            source_anchor: Some(anchor),
            receiver_package: None,
            defining_package: None,
            insertion_plan_id: None,
            limitations: Vec::new(),
            conflicts: Vec::new(),
        }
    }

    /// Attach the candidate's proof and currentness evidence.
    #[must_use]
    pub fn with_evidence(mut self, evidence: CompletionCandidateEvidence) -> Self {
        self.evidence = evidence;
        self
    }

    /// Attach typed rank dimensions for final assembly.
    #[must_use]
    pub fn with_rank(mut self, rank: CompletionRankKey) -> Self {
        self.rank = rank;
        self
    }

    /// Attach receiver and defining-package identity.
    #[must_use]
    pub fn with_owners(
        mut self,
        receiver_package: Option<String>,
        defining_package: Option<String>,
    ) -> Self {
        self.receiver_package = receiver_package;
        self.defining_package = defining_package;
        self
    }

    /// Attach a stable insertion-plan identity.
    #[must_use]
    pub fn with_insertion_plan_id(mut self, insertion_plan_id: impl Into<String>) -> Self {
        self.insertion_plan_id = Some(insertion_plan_id.into());
        self
    }
}

/// Result of the single completion merge, rank, and cap boundary.
#[derive(Debug, Clone)]
pub struct CompletionFinalization {
    /// Final identity-merged and ranked candidates after the result cap.
    pub candidates: Vec<CompletionCandidate>,
    /// Whether additional admitted candidates existed beyond the result cap.
    pub is_incomplete: bool,
}

/// Merge identity-bearing candidates, rank the complete admitted set once, and
/// then apply the result cap.
///
/// No provider may append candidates after this boundary. A later runtime PR
/// can move the LSP request cap onto this API without changing candidate merge
/// or ranking semantics.
#[must_use]
pub fn finalize_completion_candidates(
    candidates: Vec<CompletionCandidate>,
    cap: usize,
) -> CompletionFinalization {
    let mut candidates = merge_completion_candidates(candidates);
    candidates.sort_by(completion_candidate_order);
    let is_incomplete = candidates.len() > cap;
    candidates.truncate(cap);
    CompletionFinalization { candidates, is_incomplete }
}

/// Test-only shorthand for the uncapped finalization path.
#[cfg(test)]
fn merge_and_sort_completion_candidates(
    candidates: Vec<CompletionCandidate>,
) -> Vec<CompletionCandidate> {
    finalize_completion_candidates(candidates, usize::MAX).candidates
}

fn merge_completion_candidates(candidates: Vec<CompletionCandidate>) -> Vec<CompletionCandidate> {
    if candidates.is_empty() {
        return candidates;
    }

    let mut merged = Vec::<CompletionCandidate>::new();
    let mut indexes = HashMap::<CompletionCandidateIdentity, Vec<usize>>::new();

    for mut candidate in candidates {
        if candidate.item.label.is_empty() {
            continue;
        }

        let identity = candidate.identity.clone();
        let matching_indexes = indexes.get(&identity).cloned().unwrap_or_default();
        let mut compatible_index = None;
        let mut observed_conflicts = BTreeSet::new();

        for index in matching_indexes {
            let conflicts = candidate_conflicts(&merged[index], &candidate);
            if conflicts.is_empty() {
                compatible_index = Some(index);
                break;
            }
            for conflict in conflicts {
                observed_conflicts.insert(conflict);
                if !merged[index].conflicts.contains(&conflict) {
                    merged[index].conflicts.push(conflict);
                }
            }
        }

        if let Some(index) = compatible_index {
            let existing = merged.remove(index);
            merged.insert(index, merge_compatible_candidates(existing, candidate));
            // Backfill during the merge can install field values (for example a
            // receiver owner) that conflict with same-identity siblings the merged
            // candidate was never checked against. Re-check those siblings so the
            // recorded conflicts describe the retained set, not just push-time state.
            let sibling_indexes = indexes.get(&identity).cloned().unwrap_or_default();
            for sibling_index in sibling_indexes {
                if sibling_index == index {
                    continue;
                }
                let conflicts = candidate_conflicts(&merged[sibling_index], &merged[index]);
                for conflict in conflicts {
                    if !merged[sibling_index].conflicts.contains(&conflict) {
                        merged[sibling_index].conflicts.push(conflict);
                    }
                    if !merged[index].conflicts.contains(&conflict) {
                        merged[index].conflicts.push(conflict);
                    }
                }
            }
            continue;
        }

        candidate.conflicts.extend(observed_conflicts);
        let index = merged.len();
        merged.push(candidate);
        indexes.entry(identity).or_default().push(index);
    }

    merged
}

fn candidate_conflicts(
    left: &CompletionCandidate,
    right: &CompletionCandidate,
) -> Vec<CompletionCandidateConflict> {
    if matches!(&left.identity, CompletionCandidateIdentity::LegacyLabel(_)) {
        return Vec::new();
    }

    let mut conflicts = Vec::new();
    if !insertion_is_compatible(&left.item, &right.item) {
        conflicts.push(CompletionCandidateConflict::InsertionPlan);
    }
    if option_ref_conflicts(&left.source_anchor, &right.source_anchor) {
        conflicts.push(CompletionCandidateConflict::SourceAnchor);
    }
    if generation_conflicts(&left.evidence.generation, &right.evidence.generation) {
        conflicts.push(CompletionCandidateConflict::Generation);
    }
    if option_ref_conflicts(&left.receiver_package, &right.receiver_package) {
        conflicts.push(CompletionCandidateConflict::ReceiverOwner);
    }
    if option_ref_conflicts(&left.defining_package, &right.defining_package) {
        conflicts.push(CompletionCandidateConflict::DefiningOwner);
    }
    if option_ref_conflicts(&left.insertion_plan_id, &right.insertion_plan_id) {
        conflicts.push(CompletionCandidateConflict::InsertionPlanIdentity);
    }
    conflicts
}

fn insertion_is_compatible(left: &CompletionItem, right: &CompletionItem) -> bool {
    left.insert_text == right.insert_text
        && left.insert_text_format == right.insert_text_format
        && left.additional_edits == right.additional_edits
        && left.text_edit_range == right.text_edit_range
}

fn generation_conflicts(left: &SourceGeneration, right: &SourceGeneration) -> bool {
    matches!((left, right), (SourceGeneration::Known(left), SourceGeneration::Known(right)) if left != right)
}

fn option_ref_conflicts<T: PartialEq>(left: &Option<T>, right: &Option<T>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left != right)
}

fn merge_compatible_candidates(
    left: CompletionCandidate,
    right: CompletionCandidate,
) -> CompletionCandidate {
    let right_is_stronger = candidate_preference(&right, &left) == Ordering::Greater;
    let (mut winner, loser) = if right_is_stronger { (right, left) } else { (left, right) };

    if winner.item.detail.is_none() {
        winner.item.detail = loser.item.detail;
    }
    if winner.item.documentation.is_none() {
        winner.item.documentation = loser.item.documentation;
    }
    if winner.item.label_details.is_none() {
        winner.item.label_details = loser.item.label_details;
    }
    if winner.source_anchor.is_none() {
        winner.source_anchor = loser.source_anchor;
    }
    if winner.receiver_package.is_none() {
        winner.receiver_package = loser.receiver_package;
    }
    if winner.defining_package.is_none() {
        winner.defining_package = loser.defining_package;
    }
    if winner.insertion_plan_id.is_none() {
        winner.insertion_plan_id = loser.insertion_plan_id;
    }

    let mut limitations = BTreeSet::new();
    limitations.extend(winner.limitations);
    limitations.extend(loser.limitations);
    winner.limitations = limitations.into_iter().collect();

    let mut conflicts = BTreeSet::new();
    conflicts.extend(winner.conflicts);
    conflicts.extend(loser.conflicts);
    winner.conflicts = conflicts.into_iter().collect();
    winner
}

fn candidate_preference(left: &CompletionCandidate, right: &CompletionCandidate) -> Ordering {
    left.evidence
        .strength()
        .cmp(&right.evidence.strength())
        .then_with(|| completion_candidate_order(right, left))
}

fn completion_candidate_order(left: &CompletionCandidate, right: &CompletionCandidate) -> Ordering {
    evidence_order(left)
        .cmp(&evidence_order(right))
        .then_with(|| left.rank.semantic_fit.cmp(&right.rank.semantic_fit))
        .then_with(|| left.rank.visibility.cmp(&right.rank.visibility))
        .then_with(|| left.rank.receiver_fit.cmp(&right.rank.receiver_fit))
        .then_with(|| left.rank.scope_distance.cmp(&right.rank.scope_distance))
        .then_with(|| left.rank.source_tier.cmp(&right.rank.source_tier))
        .then_with(|| left.rank.fallback.cmp(&right.rank.fallback))
        .then_with(|| left.rank.match_quality.cmp(&right.rank.match_quality))
        .then_with(|| left.rank.compatibility_sort.cmp(&right.rank.compatibility_sort))
        .then_with(|| left.item.kind.cmp(&right.item.kind))
        .then_with(|| left.item.label.cmp(&right.item.label))
        .then_with(|| left.identity.cmp(&right.identity))
}

fn evidence_order(candidate: &CompletionCandidate) -> (u8, u8, u8) {
    (
        freshness_order(candidate.evidence.freshness),
        proof_order(candidate.evidence.proof),
        confidence_order(candidate.evidence.confidence),
    )
}

fn freshness_order(freshness: SemanticFreshness) -> u8 {
    match freshness {
        SemanticFreshness::Fresh => 0,
        SemanticFreshness::NotApplicable => 1,
        SemanticFreshness::Unknown => 2,
        SemanticFreshness::Stale => 3,
        _ => 4,
    }
}

fn proof_order(proof: CompletionCandidateProof) -> u8 {
    match proof {
        CompletionCandidateProof::Exact => 0,
        CompletionCandidateProof::Qualified => 1,
        CompletionCandidateProof::LegacyFallback => 2,
    }
}

fn confidence_order(confidence: SemanticConfidence) -> u8 {
    match confidence {
        SemanticConfidence::Known(Confidence::High) => 0,
        SemanticConfidence::Known(Confidence::Medium) => 1,
        SemanticConfidence::Known(Confidence::Low) => 2,
        SemanticConfidence::Unknown => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use perl_semantic_facts::{FileId, SemanticConfidence, SourceAnchor};

    use super::*;
    use crate::providers::completion_item::{CompletionItemKind, InsertTextFormat};

    fn item(label: &str, detail: Option<&str>, sort_text: &str) -> CompletionItem {
        CompletionItem {
            label: Cow::Owned(label.to_string()),
            kind: CompletionItemKind::Function,
            detail: detail.map(|value| Cow::Owned(value.to_string())),
            documentation: None,
            insert_text: Some(Cow::Owned(format!("{label}()"))),
            insert_text_format: InsertTextFormat::PlainText,
            sort_text: Some(Cow::Owned(sort_text.to_string())),
            filter_text: Some(Cow::Owned(label.to_string())),
            additional_edits: Vec::new(),
            text_edit_range: Some((0, label.len())),
            commit_characters: None,
            label_details: None,
        }
    }

    fn rank(
        candidate: &CompletionCandidate,
        semantic_fit: CompletionRankClass,
    ) -> CompletionRankKey {
        let mut rank = CompletionRankKey::compatibility(&candidate.item);
        rank.semantic_fit = semantic_fit;
        rank
    }

    #[test]
    fn legacy_candidates_preserve_label_deduplication() {
        let candidates = vec![
            CompletionCandidate::legacy(item("run", Some("weaker"), "200")),
            CompletionCandidate::legacy(item("run", Some("better"), "100")),
        ];

        let result = merge_and_sort_completion_candidates(candidates);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].item.detail.as_deref(), Some("better"));
    }

    #[test]
    fn legacy_duplicate_labels_backfill_missing_presentation_fields() {
        // Deliberate envelope behavior: the old label dedup dropped the losing
        // duplicate whole, while the merge retains compatible presentation
        // fields the winner lacks. Inclusion and ordering are unchanged.
        let sparse = item("run", None, "100");
        let mut documented = item("run", Some("Foo::run"), "100");
        documented.documentation = Some(Cow::Owned("docs".to_string()));

        let result = merge_and_sort_completion_candidates(vec![
            CompletionCandidate::legacy(sparse),
            CompletionCandidate::legacy(documented),
        ]);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].item.detail.as_deref(), Some("Foo::run"));
        assert_eq!(result[0].item.documentation.as_deref(), Some("docs"));
    }

    #[test]
    fn same_label_distinct_semantic_entities_remain_distinct() {
        let candidates = vec![
            CompletionCandidate::semantic(
                EntityId(1),
                "method",
                item("run", Some("Foo::run"), "100"),
            ),
            CompletionCandidate::semantic(
                EntityId(2),
                "method",
                item("run", Some("Bar::run"), "100"),
            ),
        ];

        let result = merge_and_sort_completion_candidates(candidates);
        assert_eq!(result.len(), 2);
        assert!(
            result.iter().any(|candidate| candidate.item.detail.as_deref() == Some("Foo::run"))
        );
        assert!(
            result.iter().any(|candidate| candidate.item.detail.as_deref() == Some("Bar::run"))
        );
    }

    #[test]
    fn same_identity_prefers_current_exact_evidence() {
        let stale = CompletionCandidate::semantic(
            EntityId(7),
            "method",
            item("run", Some("legacy"), "050"),
        )
        .with_evidence(CompletionCandidateEvidence {
            producer: SemanticProducer::WorkspaceIndex,
            proof: CompletionCandidateProof::LegacyFallback,
            confidence: SemanticConfidence::Known(Confidence::Low),
            freshness: SemanticFreshness::Stale,
            generation: SourceGeneration::known("generation-3"),
        });
        let current =
            CompletionCandidate::semantic(EntityId(7), "method", item("run", None, "200"))
                .with_evidence(CompletionCandidateEvidence {
                    producer: SemanticProducer::SemanticAnalyzer,
                    proof: CompletionCandidateProof::Exact,
                    confidence: SemanticConfidence::Known(Confidence::High),
                    freshness: SemanticFreshness::Fresh,
                    generation: SourceGeneration::known("generation-3"),
                });

        let result = merge_and_sort_completion_candidates(vec![stale, current]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].evidence.producer, SemanticProducer::SemanticAnalyzer);
        assert_eq!(result[0].item.detail.as_deref(), Some("legacy"));
    }

    #[test]
    fn conflicting_insertion_plans_remain_separate() {
        let first =
            CompletionCandidate::semantic(EntityId(9), "default_export", item("run", None, "100"))
                .with_insertion_plan_id("import:Foo");
        let mut second_item = item("run", None, "100");
        second_item.additional_edits.push((
            perl_parser_core::SourceLocation { start: 0, end: 0 },
            "use Bar;\n".to_string(),
        ));
        let second = CompletionCandidate::semantic(EntityId(9), "default_export", second_item)
            .with_insertion_plan_id("import:Bar");

        let result = merge_and_sort_completion_candidates(vec![first, second]);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|candidate| !candidate.conflicts.is_empty()));
        assert!(result.iter().any(|candidate| {
            candidate.conflicts.contains(&CompletionCandidateConflict::InsertionPlan)
        }));
        assert!(result.iter().any(|candidate| {
            candidate.conflicts.contains(&CompletionCandidateConflict::InsertionPlanIdentity)
        }));
    }

    #[test]
    fn different_source_anchors_for_one_identity_do_not_merge() {
        let first = CompletionCandidate::source_anchored(
            "adapter",
            SourceAnchor::new(None, FileId(1), 10, 14),
            "generated_accessor",
            item("name", None, "100"),
        );
        let second = CompletionCandidate::source_anchored(
            "adapter",
            SourceAnchor::new(None, FileId(1), 20, 24),
            "generated_accessor",
            item("name", None, "100"),
        );

        let result = merge_and_sort_completion_candidates(vec![first, second]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn merge_rechecks_sibling_conflicts_after_backfill() {
        // A and B stay separate over incompatible insertion plans. C is
        // insertion-compatible with B and merges in, backfilling its receiver
        // owner onto the merged entry. That backfilled owner conflicts with A's,
        // and both retained entries must record it.
        let mut first_item = item("run", None, "100");
        first_item.insert_text = Some(Cow::Owned("a()".to_string()));
        let first = CompletionCandidate::semantic(EntityId(11), "method", first_item)
            .with_owners(Some("R1".to_string()), None);
        let mut second_item = item("run", None, "100");
        second_item.insert_text = Some(Cow::Owned("b()".to_string()));
        let second = CompletionCandidate::semantic(EntityId(11), "method", second_item);
        let mut third_item = item("run", None, "100");
        third_item.insert_text = Some(Cow::Owned("b()".to_string()));
        let third = CompletionCandidate::semantic(EntityId(11), "method", third_item)
            .with_owners(Some("R2".to_string()), None);

        let result = merge_and_sort_completion_candidates(vec![first, second, third]);

        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|candidate| {
            candidate.conflicts.contains(&CompletionCandidateConflict::ReceiverOwner)
        }));
    }

    #[test]
    fn typed_rank_beats_compatibility_sort() {
        let exact =
            CompletionCandidate::semantic(EntityId(11), "method", item("exact", None, "900"));
        let exact_rank = rank(&exact, CompletionRankClass::Exact);
        let exact = exact.with_rank(exact_rank);

        let fallback =
            CompletionCandidate::semantic(EntityId(12), "method", item("fallback", None, "001"));
        let fallback_rank = rank(&fallback, CompletionRankClass::Fallback);
        let fallback = fallback.with_rank(fallback_rank);

        let result = merge_and_sort_completion_candidates(vec![fallback, exact]);
        assert_eq!(result[0].item.label, "exact");
        assert_eq!(result[1].item.label, "fallback");
    }

    #[test]
    fn current_evidence_precedes_stale_semantic_fit() {
        let stale = CompletionCandidate::semantic(
            EntityId(13),
            "method",
            item("stale_exact_fit", None, "001"),
        )
        .with_evidence(CompletionCandidateEvidence {
            producer: SemanticProducer::SemanticAnalyzer,
            proof: CompletionCandidateProof::Exact,
            confidence: SemanticConfidence::Known(Confidence::High),
            freshness: SemanticFreshness::Stale,
            generation: SourceGeneration::known("generation-1"),
        });
        let stale_rank = rank(&stale, CompletionRankClass::Exact);
        let stale = stale.with_rank(stale_rank);

        let current = CompletionCandidate::semantic(
            EntityId(14),
            "method",
            item("current_fallback", None, "999"),
        )
        .with_evidence(CompletionCandidateEvidence {
            producer: SemanticProducer::WorkspaceIndex,
            proof: CompletionCandidateProof::LegacyFallback,
            confidence: SemanticConfidence::Known(Confidence::Low),
            freshness: SemanticFreshness::Fresh,
            generation: SourceGeneration::known("generation-2"),
        });
        let current_rank = rank(&current, CompletionRankClass::Fallback);
        let current = current.with_rank(current_rank);

        let result = merge_and_sort_completion_candidates(vec![stale, current]);
        assert_eq!(result[0].item.label, "current_fallback");
        assert_eq!(result[1].item.label, "stale_exact_fit");
    }

    #[test]
    fn cap_is_applied_after_typed_rank() {
        let first =
            CompletionCandidate::semantic(EntityId(21), "method", item("fallback_a", None, "001"));
        let first_rank = rank(&first, CompletionRankClass::Fallback);
        let first = first.with_rank(first_rank);

        let second =
            CompletionCandidate::semantic(EntityId(22), "method", item("fallback_b", None, "002"));
        let second_rank = rank(&second, CompletionRankClass::Fallback);
        let second = second.with_rank(second_rank);

        let exact =
            CompletionCandidate::semantic(EntityId(23), "method", item("exact", None, "999"));
        let exact_rank = rank(&exact, CompletionRankClass::Exact);
        let exact = exact.with_rank(exact_rank);

        let result = finalize_completion_candidates(vec![first, second, exact], 1);
        assert!(result.is_incomplete);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].item.label, "exact");
    }

    #[test]
    fn finalization_reports_cap_boundaries() {
        let make = |id, label: &str| {
            CompletionCandidate::semantic(EntityId(id), "method", item(label, None, label))
        };

        let below = finalize_completion_candidates(vec![make(31, "a")], 2);
        assert!(!below.is_incomplete);
        assert_eq!(below.candidates.len(), 1);

        let equal = finalize_completion_candidates(vec![make(32, "a"), make(33, "b")], 2);
        assert!(!equal.is_incomplete);
        assert_eq!(equal.candidates.len(), 2);

        let above =
            finalize_completion_candidates(vec![make(34, "a"), make(35, "b"), make(36, "c")], 2);
        assert!(above.is_incomplete);
        assert_eq!(above.candidates.len(), 2);
    }

    #[test]
    fn finalization_is_stable_across_input_order() {
        let candidates = vec![
            CompletionCandidate::semantic(EntityId(41), "method", item("b", None, "200")),
            CompletionCandidate::semantic(EntityId(42), "method", item("a", None, "100")),
            CompletionCandidate::semantic(EntityId(43), "method", item("c", None, "300")),
        ];
        let mut reversed = candidates.clone();
        reversed.reverse();

        let first = finalize_completion_candidates(candidates, usize::MAX);
        let second = finalize_completion_candidates(reversed, usize::MAX);
        let first_ids: Vec<_> =
            first.candidates.iter().map(|candidate| candidate.identity.clone()).collect();
        let second_ids: Vec<_> =
            second.candidates.iter().map(|candidate| candidate.identity.clone()).collect();

        assert_eq!(first_ids, second_ids);
    }
}
