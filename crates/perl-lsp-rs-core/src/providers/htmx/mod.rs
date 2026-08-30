//! Canonical htmx protocol metadata and parser-independent completion.

mod catalog;
mod markup;

pub use catalog::{
    HTMX_ATTRIBUTES, HTMX_HEADERS, HtmxAttributeFamily, HtmxAttributeSpec,
    HtmxHeaderDirection, HtmxHeaderSpec,
};
pub use markup::{
    MAX_MARKUP_LOOKBACK_BYTES, HtmxAttributeNameContext, htmx_attribute_name_context,
};

use crate::providers::completion_item::{CompletionItem, CompletionItemKind, InsertTextFormat};
use crate::providers::file_completion::FileCompletionContext;
use std::borrow::Cow;

/// Complete canonical htmx request and response header names.
///
/// Returns `None` when the prefix is not htmx-shaped, allowing the caller to
/// delegate to another string-literal completion provider. An htmx-shaped but
/// unknown prefix returns `Some(Vec::new())` and therefore fails closed.
#[must_use]
pub fn complete_header_names(context: &FileCompletionContext) -> Option<Vec<CompletionItem>> {
    if !is_htmx_header_prefix(&context.prefix) {
        return None;
    }

    Some(
        HTMX_HEADERS
            .iter()
            .filter(|header| starts_with_ignore_ascii_case(header.name, &context.prefix))
            .map(|header| {
                completion_item(
                    Cow::Borrowed(header.name),
                    header.direction.detail(),
                    header.documentation,
                    context.prefix_start,
                    context.position,
                )
            })
            .collect(),
    )
}

/// Complete canonical htmx attribute names in a proven raw-markup slot.
///
/// Returns `None` when the cursor is not in an admitted htmx attribute-name
/// context. Both canonical `hx-*` and standard `data-hx-*` spellings are
/// supported. Dynamic event handlers are represented by the `hx-on:` family
/// prefix; event-name completion is a separate grammar.
#[must_use]
pub fn complete_attribute_names(source: &str, position: usize) -> Option<Vec<CompletionItem>> {
    let context = htmx_attribute_name_context(source, position)?;
    let data_prefixed = starts_with_ignore_ascii_case(context.prefix, "data-hx");

    Some(
        HTMX_ATTRIBUTES
            .iter()
            .filter_map(|attribute| {
                let label = if data_prefixed {
                    Cow::Owned(format!("data-{}", attribute.name))
                } else {
                    Cow::Borrowed(attribute.name)
                };
                starts_with_ignore_ascii_case(label.as_ref(), context.prefix).then(|| {
                    completion_item(
                        label,
                        attribute.detail(),
                        attribute.documentation,
                        context.prefix_start,
                        context.position,
                    )
                })
            })
            .collect(),
    )
}

fn completion_item(
    label: Cow<'static, str>,
    detail: &'static str,
    documentation: &'static str,
    prefix_start: usize,
    position: usize,
) -> CompletionItem {
    CompletionItem {
        insert_text: Some(label.clone()),
        sort_text: Some(label.clone()),
        filter_text: Some(label.clone()),
        label,
        kind: CompletionItemKind::Property,
        detail: Some(Cow::Borrowed(detail)),
        documentation: Some(Cow::Borrowed(documentation)),
        insert_text_format: InsertTextFormat::PlainText,
        additional_edits: Vec::new(),
        text_edit_range: Some((prefix_start, position)),
        commit_characters: None,
        label_details: None,
    }
}

fn is_htmx_header_prefix(prefix: &str) -> bool {
    prefix.eq_ignore_ascii_case("HX")
        || prefix.get(..3).is_some_and(|head| head.eq_ignore_ascii_case("HX-"))
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::{
        HTMX_ATTRIBUTES, HTMX_HEADERS, HtmxAttributeFamily, HtmxHeaderDirection,
        complete_attribute_names, complete_header_names,
    };
    use crate::providers::file_completion::FileCompletionContext;

    #[test]
    fn canonical_header_catalog_is_exact_and_directional() {
        let names: Vec<&str> = HTMX_HEADERS.iter().map(|header| header.name).collect();

        assert_eq!(
            names,
            vec![
                "HX-Boosted",
                "HX-Current-URL",
                "HX-History-Restore-Request",
                "HX-Location",
                "HX-Prompt",
                "HX-Push-Url",
                "HX-Redirect",
                "HX-Refresh",
                "HX-Replace-Url",
                "HX-Request",
                "HX-Reselect",
                "HX-Reswap",
                "HX-Retarget",
                "HX-Target",
                "HX-Trigger",
                "HX-Trigger-After-Settle",
                "HX-Trigger-After-Swap",
                "HX-Trigger-Name",
            ]
        );
        assert!(HTMX_HEADERS.iter().any(|header| {
            header.name == "HX-Trigger"
                && header.direction == HtmxHeaderDirection::RequestAndResponse
        }));
    }

    #[test]
    fn canonical_attribute_catalog_is_exact_and_marks_the_dynamic_family() {
        let names: Vec<&str> = HTMX_ATTRIBUTES.iter().map(|attribute| attribute.name).collect();

        assert_eq!(
            names,
            vec![
                "hx-boost",
                "hx-confirm",
                "hx-delete",
                "hx-disable",
                "hx-disabled-elt",
                "hx-disinherit",
                "hx-encoding",
                "hx-ext",
                "hx-get",
                "hx-headers",
                "hx-history",
                "hx-history-elt",
                "hx-include",
                "hx-indicator",
                "hx-inherit",
                "hx-on:",
                "hx-params",
                "hx-patch",
                "hx-post",
                "hx-preserve",
                "hx-prompt",
                "hx-push-url",
                "hx-put",
                "hx-replace-url",
                "hx-request",
                "hx-select",
                "hx-select-oob",
                "hx-swap",
                "hx-swap-oob",
                "hx-sync",
                "hx-target",
                "hx-trigger",
                "hx-validate",
                "hx-vals",
                "hx-vars",
            ]
        );
        assert!(HTMX_ATTRIBUTES.iter().any(|attribute| {
            attribute.name == "hx-on:"
                && attribute.family == HtmxAttributeFamily::EventHandler
                && !attribute.deprecated
        }));
        assert!(!HTMX_ATTRIBUTES.iter().any(|attribute| attribute.name == "hx-on"));
    }

    #[test]
    fn header_completion_preserves_the_existing_public_shape() {
        let context = FileCompletionContext::new("hx-red", 11, 17);
        let completions = complete_header_names(&context);

        assert!(completions.is_some_and(|items| {
            items.len() == 1
                && items.first().is_some_and(|item| {
                    item.label == "HX-Redirect"
                        && item.insert_text.as_deref() == Some("HX-Redirect")
                        && item.text_edit_range == Some((11, 17))
                })
        }));

        let trigger = FileCompletionContext::new("HX-Trigger", 0, 10);
        assert!(complete_header_names(&trigger).is_some_and(|items| {
            items.iter().any(|item| {
                item.label == "HX-Trigger"
                    && item.documentation.as_deref()
                        == Some(
                            "Request: contains the `id` of the triggering element. Response: \
                             triggers client-side events when the response is received.",
                        )
            })
        }));
    }

    #[test]
    fn completes_canonical_and_data_prefixed_attributes_in_stable_order() {
        let canonical = "<div hx-";
        let data = "<div data-hx-";
        let canonical_labels: Vec<String> = complete_attribute_names(canonical, canonical.len())
            .into_iter()
            .flatten()
            .map(|item| item.label.into_owned())
            .collect();
        let data_labels: Vec<String> = complete_attribute_names(data, data.len())
            .into_iter()
            .flatten()
            .map(|item| item.label.into_owned())
            .collect();

        assert_eq!(canonical_labels.len(), HTMX_ATTRIBUTES.len());
        assert_eq!(
            canonical_labels,
            HTMX_ATTRIBUTES
                .iter()
                .map(|attribute| attribute.name.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            data_labels,
            HTMX_ATTRIBUTES
                .iter()
                .map(|attribute| format!("data-{}", attribute.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn matching_is_case_insensitive_with_canonical_insertion_and_exact_range() {
        let source = "<button\n  HX-Re";
        let completions = complete_attribute_names(source, source.len());

        assert!(completions.is_some_and(|items| {
            let labels: Vec<&str> = items.iter().map(|item| item.label.as_ref()).collect();
            labels == ["hx-replace-url", "hx-request"]
                && items.iter().all(|item| {
                    item.text_edit_range == Some((source.len() - "HX-Re".len(), source.len()))
                        && item
                            .insert_text
                            .as_deref()
                            .is_some_and(|text| text.starts_with("hx-"))
                })
        }));
    }

    #[test]
    fn dynamic_hx_on_family_is_not_the_deprecated_plain_attribute() {
        let source = "<div hx-o";
        let completions = complete_attribute_names(source, source.len());

        assert!(completions.is_some_and(|items| {
            items.len() == 1
                && items.first().is_some_and(|item| {
                    item.label == "hx-on:"
                        && item.detail.as_deref()
                            == Some("htmx event-handler attribute family")
                })
        }));
    }
}
