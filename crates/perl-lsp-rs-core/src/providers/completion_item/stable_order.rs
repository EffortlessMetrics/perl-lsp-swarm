//! Deterministic ordering for completion-candidate assembly.
//!
//! The candidate merge engine intentionally preserves existing provider rank
//! semantics. This module supplies the final, non-semantic tie-break needed when
//! equal-ranked candidates differ only in identity metadata, presentation, or
//! conflicting insertion plans. It is used before merge so equal compatible
//! candidates select the same winner regardless of provider iteration order,
//! and after merge so retained conflicts have a total output order.

use std::cmp::Ordering;

use super::{CompletionCandidate, CompletionItem, CompletionItemLabelDetails, InsertTextFormat};

/// Deterministic input order before identity merge.
pub(crate) fn candidate_premerge_order(
    left: &CompletionCandidate,
    right: &CompletionCandidate,
) -> Ordering {
    completion_item_order(&left.item, &right.item)
        .then_with(|| left.identity.cmp(&right.identity))
        .then_with(|| candidate_metadata_order(left, right))
}

/// Deterministic total order after identity merge.
pub(crate) fn candidate_output_order(
    left: &CompletionCandidate,
    right: &CompletionCandidate,
) -> Ordering {
    completion_item_order(&left.item, &right.item)
        .then_with(|| left.identity.cmp(&right.identity))
        .then_with(|| candidate_metadata_order(left, right))
}

fn completion_item_order(left: &CompletionItem, right: &CompletionItem) -> Ordering {
    let left_sort = left.sort_text.as_deref().unwrap_or(left.label.as_ref());
    let right_sort = right.sort_text.as_deref().unwrap_or(right.label.as_ref());
    left_sort
        .cmp(right_sort)
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.label.cmp(&right.label))
}

fn candidate_metadata_order(left: &CompletionCandidate, right: &CompletionCandidate) -> Ordering {
    left.source_anchor
        .cmp(&right.source_anchor)
        .then_with(|| left.receiver_package.cmp(&right.receiver_package))
        .then_with(|| left.defining_package.cmp(&right.defining_package))
        .then_with(|| left.insertion_plan_id.cmp(&right.insertion_plan_id))
        .then_with(|| left.evidence.freshness.cmp(&right.evidence.freshness))
        .then_with(|| left.evidence.proof.cmp(&right.evidence.proof))
        .then_with(|| left.evidence.confidence.cmp(&right.evidence.confidence))
        .then_with(|| left.evidence.generation.cmp(&right.evidence.generation))
        .then_with(|| left.evidence.producer.cmp(&right.evidence.producer))
        .then_with(|| left.item.insert_text.cmp(&right.item.insert_text))
        .then_with(|| {
            insert_text_format_order(&left.item.insert_text_format, &right.item.insert_text_format)
        })
        .then_with(|| {
            additional_edits_order(&left.item.additional_edits, &right.item.additional_edits)
        })
        .then_with(|| left.item.text_edit_range.cmp(&right.item.text_edit_range))
        .then_with(|| left.item.filter_text.cmp(&right.item.filter_text))
        .then_with(|| left.item.detail.cmp(&right.item.detail))
        .then_with(|| left.item.documentation.cmp(&right.item.documentation))
        .then_with(|| left.item.commit_characters.cmp(&right.item.commit_characters))
        .then_with(|| label_details_order(&left.item.label_details, &right.item.label_details))
        .then_with(|| left.limitations.cmp(&right.limitations))
        .then_with(|| left.conflicts.cmp(&right.conflicts))
}

fn insert_text_format_order(left: &InsertTextFormat, right: &InsertTextFormat) -> Ordering {
    match (left, right) {
        (InsertTextFormat::PlainText, InsertTextFormat::PlainText) => Ordering::Equal,
        (InsertTextFormat::PlainText, InsertTextFormat::Snippet { .. }) => Ordering::Less,
        (InsertTextFormat::Snippet { .. }, InsertTextFormat::PlainText) => Ordering::Greater,
        (
            InsertTextFormat::Snippet { plain_fallback: left },
            InsertTextFormat::Snippet { plain_fallback: right },
        ) => left.cmp(right),
    }
}

fn additional_edits_order(
    left: &[(perl_parser_core::SourceLocation, String)],
    right: &[(perl_parser_core::SourceLocation, String)],
) -> Ordering {
    for ((left_range, left_text), (right_range, right_text)) in left.iter().zip(right.iter()) {
        let order = left_range
            .start
            .cmp(&right_range.start)
            .then_with(|| left_range.end.cmp(&right_range.end))
            .then_with(|| left_text.cmp(right_text));
        if !matches!(order, Ordering::Equal) {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

fn label_details_order(
    left: &Option<CompletionItemLabelDetails>,
    right: &Option<CompletionItemLabelDetails>,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => {
            left.detail.cmp(&right.detail).then_with(|| left.description.cmp(&right.description))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use perl_semantic_facts::EntityId;

    use super::super::{
        CompletionCandidate, CompletionItem, CompletionItemKind, InsertTextFormat,
        merge_and_sort_completion_candidates,
    };

    fn item(detail: &str) -> CompletionItem {
        CompletionItem {
            label: Cow::Borrowed("run"),
            kind: CompletionItemKind::Function,
            detail: Some(Cow::Owned(detail.to_string())),
            documentation: Some(Cow::Owned(format!("documentation:{detail}"))),
            insert_text: Some(Cow::Borrowed("run()")),
            insert_text_format: InsertTextFormat::PlainText,
            sort_text: Some(Cow::Borrowed("100")),
            filter_text: Some(Cow::Borrowed("run")),
            additional_edits: Vec::new(),
            text_edit_range: Some((0, 3)),
            commit_characters: None,
            label_details: None,
        }
    }

    #[test]
    fn compatible_equal_candidates_choose_the_same_winner_after_input_reversal() {
        let first = CompletionCandidate::semantic(EntityId(1), "method", item("alpha"));
        let second = CompletionCandidate::semantic(EntityId(1), "method", item("beta"));

        let forward = merge_and_sort_completion_candidates(vec![first.clone(), second.clone()]);
        let reverse = merge_and_sort_completion_candidates(vec![second, first]);

        assert_eq!(forward.len(), 1);
        assert_eq!(reverse.len(), 1);
        assert_eq!(forward[0].item.detail, reverse[0].item.detail);
        assert_eq!(forward[0].item.documentation, reverse[0].item.documentation);
    }

    #[test]
    fn conflicting_insertions_have_the_same_order_after_input_reversal() {
        let first = CompletionCandidate::semantic(EntityId(2), "default_export", item("first"))
            .with_insertion_plan_id("import:Alpha");
        let mut second_item = item("second");
        second_item.additional_edits.push((
            perl_parser_core::SourceLocation { start: 0, end: 0 },
            "use Beta;\n".to_string(),
        ));
        let second = CompletionCandidate::semantic(EntityId(2), "default_export", second_item)
            .with_insertion_plan_id("import:Beta");

        let forward = merge_and_sort_completion_candidates(vec![first.clone(), second.clone()]);
        let reverse = merge_and_sort_completion_candidates(vec![second, first]);
        let forward_plans =
            forward.iter().map(|candidate| candidate.insertion_plan_id.clone()).collect::<Vec<_>>();
        let reverse_plans =
            reverse.iter().map(|candidate| candidate.insertion_plan_id.clone()).collect::<Vec<_>>();

        assert_eq!(forward.len(), 2);
        assert_eq!(reverse.len(), 2);
        assert_eq!(forward_plans, reverse_plans);
    }
}
