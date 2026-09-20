#![warn(missing_docs)]
//! Completion item domain types and sorting utilities.
//!
//! This microcrate isolates completion payload representation and deterministic
//! identity merge, ordering, and deduplication policy from provider logic.

mod candidate;
mod snippet;
mod stable_order;

pub use candidate::{
    CompletionCandidate, CompletionCandidateConflict, CompletionCandidateEvidence,
    CompletionCandidateIdentity, CompletionCandidateProof, CompletionFinalization,
    CompletionRankClass, CompletionRankKey,
};
pub use snippet::{InsertTextFormat, render_snippet_plaintext, snippet_body_defects};

use perl_parser_core::SourceLocation;
use std::borrow::Cow;

/// Type of completion item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompletionItemKind {
    /// Variable (scalar, array, hash).
    Variable,
    /// Function or method.
    Function,
    /// Perl keyword.
    Keyword,
    /// Package or module.
    Module,
    /// File path.
    File,
    /// Snippet with placeholders.
    Snippet,
    /// Constant value.
    Constant,
    /// Property or hash key.
    Property,
}

/// Secondary label information shown inline next to the main label (LSP 3.17+).
///
/// `detail` appears directly after the label (e.g. a function signature).
/// `description` appears further right (e.g. a fully-qualified source module).
/// Only populated when the client advertises `completionItem.labelDetailsSupport`.
#[derive(Debug, Clone, Default)]
pub struct CompletionItemLabelDetails {
    /// Short annotation shown right after the label, e.g. `(arg: Type) -> Return`.
    pub detail: Option<String>,
    /// Qualifier shown to the far right, e.g. `POSIX` for `floor` from POSIX.
    pub description: Option<String>,
}

/// A single completion suggestion.
///
/// String fields that carry static data (builtin names, keyword strings,
/// regex pattern labels) use `Cow::Borrowed(&'static str)` — zero heap
/// allocation. Runtime-derived strings (user symbol names, formatted sort
/// keys, workspace identifiers) use `Cow::Owned(String)` with the same
/// cost as before.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    /// The text to insert.
    pub label: Cow<'static, str>,
    /// Kind of completion.
    pub kind: CompletionItemKind,
    /// Optional detail text.
    pub detail: Option<Cow<'static, str>>,
    /// Optional documentation.
    pub documentation: Option<Cow<'static, str>>,
    /// Text to insert (if different from label).
    pub insert_text: Option<Cow<'static, str>>,
    /// How the client must interpret `insert_text`.
    ///
    /// Independent of `kind`: LSP's `insertTextFormat` describes insertion
    /// grammar, while `kind` drives icon, ranking, and commit characters. A
    /// builtin function that inserts a snippet stays `Function` and sets this
    /// to `Snippet`.
    pub insert_text_format: InsertTextFormat,
    /// Sort priority (lower is better).
    pub sort_text: Option<Cow<'static, str>>,
    /// Filter text for matching.
    pub filter_text: Option<Cow<'static, str>>,
    /// Additional text edits to apply.
    pub additional_edits: Vec<(SourceLocation, String)>,
    /// Range to replace in the document (for proper prefix handling).
    pub text_edit_range: Option<(usize, usize)>, // (start, end) offsets
    /// Commit characters that trigger auto-insertion (LSP 3.0+).
    /// Each entry must be exactly one character per LSP spec.
    pub commit_characters: Option<Vec<String>>,
    /// LSP 3.17+ label details shown inline in the completion list.
    /// Only serialized when the client advertises `labelDetailsSupport`.
    pub label_details: Option<CompletionItemLabelDetails>,
}

/// Merge identity-bearing candidates, rank the complete admitted set once, and
/// then apply the result cap.
///
/// Equal-ranked inputs are put in a deterministic total order before the
/// identity merge. The internal typed sort is stable, so candidates that remain
/// equal after semantic rank retain that deterministic order at the cap boundary.
#[must_use]
pub fn finalize_completion_candidates(
    mut candidates: Vec<CompletionCandidate>,
    cap: usize,
) -> CompletionFinalization {
    candidates.sort_by(stable_order::candidate_premerge_order);
    candidate::finalize_completion_candidates(candidates, cap)
}

/// Merge identity-bearing completion candidates and apply the final typed order
/// without truncation.
#[must_use]
pub fn merge_and_sort_completion_candidates(
    candidates: Vec<CompletionCandidate>,
) -> Vec<CompletionCandidate> {
    finalize_completion_candidates(candidates, usize::MAX).candidates
}

/// Remove duplicates and sort completions with stable, deterministic ordering.
///
/// Existing providers enter through [`CompletionCandidate::legacy`], preserving
/// their current label-based behavior. Providers that migrate to explicit
/// candidate identity use [`merge_and_sort_completion_candidates`] directly and
/// can therefore retain same-label distinct entities.
#[must_use]
pub fn deduplicate_and_sort(completions: Vec<CompletionItem>) -> Vec<CompletionItem> {
    merge_and_sort_completion_candidates(
        completions.into_iter().map(CompletionCandidate::legacy).collect(),
    )
    .into_iter()
    .map(|candidate| candidate.item)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{CompletionItem, CompletionItemKind, InsertTextFormat, deduplicate_and_sort};
    use proptest::prelude::*;
    use std::borrow::Cow;
    use std::collections::{BTreeMap, HashSet};

    fn item(label: &str, kind: CompletionItemKind, sort_text: Option<&str>) -> CompletionItem {
        CompletionItem {
            label: label.to_string().into(),
            kind,
            detail: None,
            documentation: None,
            insert_text: None,
            sort_text: sort_text.map(|s| s.to_string().into()),
            filter_text: None,
            additional_edits: Vec::new(),
            text_edit_range: None,
            commit_characters: None,
            insert_text_format: InsertTextFormat::PlainText,
            label_details: None,
        }
    }

    fn completion_kind_strategy() -> impl Strategy<Value = CompletionItemKind> {
        prop_oneof![
            Just(CompletionItemKind::Variable),
            Just(CompletionItemKind::Function),
            Just(CompletionItemKind::Keyword),
            Just(CompletionItemKind::Module),
            Just(CompletionItemKind::File),
            Just(CompletionItemKind::Snippet),
            Just(CompletionItemKind::Constant),
            Just(CompletionItemKind::Property),
        ]
    }

    fn completion_item_strategy() -> impl Strategy<Value = CompletionItem> {
        (
            "[A-Za-z_]{0,10}",
            completion_kind_strategy(),
            prop::option::of("[0-9]{0,3}[_A-Za-z]{0,8}"),
        )
            .prop_map(|(label, kind, sort_text)| CompletionItem {
                label: label.into(),
                kind,
                detail: None,
                documentation: None,
                insert_text: None,
                sort_text: sort_text.map(Into::into),
                filter_text: None,
                additional_edits: Vec::new(),
                text_edit_range: None,
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            })
    }

    fn sort_key(item: &CompletionItem) -> (String, CompletionItemKind, String) {
        (
            item.sort_text.as_deref().unwrap_or(item.label.as_ref()).to_string(),
            item.kind,
            item.label.to_string(),
        )
    }

    fn visible_shape(
        items: &[CompletionItem],
    ) -> Vec<(String, CompletionItemKind, Option<String>)> {
        items
            .iter()
            .map(|item| {
                (
                    item.label.to_string(),
                    item.kind,
                    item.sort_text.as_ref().map(|s: &Cow<'static, str>| s.to_string()),
                )
            })
            .collect()
    }

    proptest! {
        #[test]
        fn prop_deduplicate_and_sort_drops_empty_labels_and_keeps_unique_labels(
            items in prop::collection::vec(completion_item_strategy(), 0..96)
        ) {
            let result = deduplicate_and_sort(items);
            let mut labels = HashSet::new();

            for item in &result {
                prop_assert!(!item.label.is_empty());
                prop_assert!(labels.insert(item.label.to_string()));
            }
        }

        #[test]
        fn prop_deduplicate_and_sort_orders_by_sort_key_kind_then_label(
            items in prop::collection::vec(completion_item_strategy(), 0..96)
        ) {
            let result = deduplicate_and_sort(items);

            for adjacent in result.windows(2) {
                let left = &adjacent[0];
                let right = &adjacent[1];
                prop_assert!(
                    sort_key(left) <= sort_key(right),
                    "completion items must remain sorted: left={left:?}, right={right:?}"
                );
            }
        }

        #[test]
        fn prop_deduplicate_and_sort_keeps_best_rank_for_each_label(
            items in prop::collection::vec(completion_item_strategy(), 0..96)
        ) {
            let mut best_by_label = BTreeMap::<String, String>::new();

            for item in &items {
                if item.label.is_empty() {
                    continue;
                }

                let rank = item.sort_text.as_deref().unwrap_or(item.label.as_ref()).to_string();
                best_by_label
                    .entry(item.label.to_string())
                    .and_modify(|best| {
                        if rank < *best {
                            *best = rank.clone();
                        }
                    })
                    .or_insert(rank);
            }

            let result = deduplicate_and_sort(items);

            for item in &result {
                let actual_rank = item.sort_text.as_deref().unwrap_or(item.label.as_ref());
                let expected_rank = best_by_label.get(item.label.as_ref());
                prop_assert_eq!(expected_rank, Some(&actual_rank.to_string()));
            }
        }

        #[test]
        fn prop_deduplicate_and_sort_is_idempotent(
            items in prop::collection::vec(completion_item_strategy(), 0..96)
        ) {
            let once = deduplicate_and_sort(items);
            let twice = deduplicate_and_sort(once.clone());

            prop_assert_eq!(visible_shape(&once), visible_shape(&twice));
        }
    }

    #[test]
    fn deduplicates_on_label_using_best_sort_text() {
        let items = vec![
            item("foo", CompletionItemKind::Function, Some("200")),
            item("foo", CompletionItemKind::Variable, Some("050")),
            item("bar", CompletionItemKind::Function, Some("100")),
        ];

        let result = deduplicate_and_sort(items);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].label, "foo");
        assert_eq!(result[0].kind, CompletionItemKind::Variable);
        assert_eq!(result[1].label, "bar");
    }

    #[test]
    fn drops_empty_labels() {
        let items = vec![
            item("", CompletionItemKind::Function, Some("001")),
            item("ok", CompletionItemKind::Function, Some("002")),
        ];

        let result = deduplicate_and_sort(items);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "ok");
    }
}
