//! String-literal completion compatibility facade.
//!
//! htmx header names are completed before the secure filesystem provider is
//! consulted. Every other prefix delegates unchanged to the file-completion
//! microcrate, preserving its traversal and sanitization policy.

use crate::providers::completion_item::{CompletionItem, CompletionItemKind, InsertTextFormat};
use crate::providers::file_completion::complete_file_paths as complete_filesystem_paths;
use std::borrow::Cow;

pub use crate::providers::file_completion::FileCompletionContext;

struct HtmxHeader {
    name: &'static str,
    detail: &'static str,
    documentation: &'static str,
}

const HTMX_HEADERS: &[HtmxHeader] = &[
    HtmxHeader {
        name: "HX-Boosted",
        detail: "htmx request header",
        documentation: "Indicates that the request came from an element using `hx-boost`.",
    },
    HtmxHeader {
        name: "HX-Current-URL",
        detail: "htmx request header",
        documentation: "Contains the browser URL when the htmx request was issued.",
    },
    HtmxHeader {
        name: "HX-History-Restore-Request",
        detail: "htmx request header",
        documentation: concat!(
            "Set to `true` for a history restoration request after a local ",
            "history-cache miss."
        ),
    },
    HtmxHeader {
        name: "HX-Location",
        detail: "htmx response header",
        documentation: "Performs a client-side redirect without a full page reload.",
    },
    HtmxHeader {
        name: "HX-Prompt",
        detail: "htmx request header",
        documentation: "Contains the user's response to an `hx-prompt` dialog.",
    },
    HtmxHeader {
        name: "HX-Push-Url",
        detail: "htmx response header",
        documentation: "Pushes a URL into the browser history stack.",
    },
    HtmxHeader {
        name: "HX-Redirect",
        detail: "htmx response header",
        documentation: "Redirects the browser to a new location with a full page reload.",
    },
    HtmxHeader {
        name: "HX-Refresh",
        detail: "htmx response header",
        documentation: "When set to `true`, causes a full page refresh.",
    },
    HtmxHeader {
        name: "HX-Replace-Url",
        detail: "htmx response header",
        documentation: "Replaces the current URL in the browser location bar.",
    },
    HtmxHeader {
        name: "HX-Request",
        detail: "htmx request header",
        documentation: "Set to `true` on requests issued by htmx.",
    },
    HtmxHeader {
        name: "HX-Reselect",
        detail: "htmx response header",
        documentation: concat!(
            "Selects which part of the response will be swapped, overriding ",
            "`hx-select`."
        ),
    },
    HtmxHeader {
        name: "HX-Reswap",
        detail: "htmx response header",
        documentation: "Overrides the response swap strategy using an `hx-swap` value.",
    },
    HtmxHeader {
        name: "HX-Retarget",
        detail: "htmx response header",
        documentation: concat!(
            "Uses a CSS selector to override the element that receives the ",
            "swapped content."
        ),
    },
    HtmxHeader {
        name: "HX-Target",
        detail: "htmx request header",
        documentation: "Contains the `id` of the target element when one exists.",
    },
    HtmxHeader {
        name: "HX-Trigger",
        detail: "htmx request and response header",
        documentation: concat!(
            "Request: contains the `id` of the triggering element. Response: ",
            "triggers client-side events when the response is received."
        ),
    },
    HtmxHeader {
        name: "HX-Trigger-After-Settle",
        detail: "htmx response header",
        documentation: "Triggers client-side events after the settle step.",
    },
    HtmxHeader {
        name: "HX-Trigger-After-Swap",
        detail: "htmx response header",
        documentation: "Triggers client-side events after the swap step.",
    },
    HtmxHeader {
        name: "HX-Trigger-Name",
        detail: "htmx request header",
        documentation: "Contains the `name` of the triggering element when one exists.",
    },
];

/// Complete htmx header names or delegate to secure file-path completion.
#[must_use]
pub fn complete_file_paths(
    context: &FileCompletionContext,
    is_cancelled: &dyn Fn() -> bool,
) -> Vec<CompletionItem> {
    if is_cancelled() {
        return Vec::new();
    }

    if let Some(headers) = complete_htmx_headers(context) {
        return headers;
    }

    complete_filesystem_paths(context, is_cancelled)
}

fn complete_htmx_headers(context: &FileCompletionContext) -> Option<Vec<CompletionItem>> {
    if !is_htmx_header_prefix(&context.prefix) {
        return None;
    }

    Some(
        HTMX_HEADERS
            .iter()
            .filter(|header| starts_with_ignore_ascii_case(header.name, &context.prefix))
            .map(|header| CompletionItem {
                label: Cow::Borrowed(header.name),
                kind: CompletionItemKind::Property,
                detail: Some(Cow::Borrowed(header.detail)),
                documentation: Some(Cow::Borrowed(header.documentation)),
                insert_text: Some(Cow::Borrowed(header.name)),
                insert_text_format: InsertTextFormat::PlainText,
                sort_text: Some(Cow::Borrowed(header.name)),
                filter_text: Some(Cow::Borrowed(header.name)),
                additional_edits: Vec::new(),
                text_edit_range: Some((context.prefix_start, context.position)),
                commit_characters: None,
                label_details: None,
            })
            .collect(),
    )
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
    use super::{FileCompletionContext, complete_file_paths, complete_htmx_headers};

    #[test]
    fn htmx_prefix_exposes_the_current_request_and_response_header_catalog() {
        let context = FileCompletionContext::new("HX-", 7, 10);
        let completions = complete_file_paths(&context, &|| false);
        let labels: Vec<&str> = completions.iter().map(|item| item.label.as_ref()).collect();

        assert_eq!(
            labels,
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
    }

    #[test]
    fn htmx_matching_is_case_insensitive_and_replaces_the_typed_prefix() {
        let context = FileCompletionContext::new("hx-red", 11, 17);
        let completions = complete_file_paths(&context, &|| false);

        assert_eq!(completions.len(), 1);
        assert!(completions.first().is_some_and(|item| {
            item.label == "HX-Redirect"
                && item.insert_text.as_deref() == Some("HX-Redirect")
                && item.text_edit_range == Some((11, 17))
        }));
    }

    #[test]
    fn hx_trigger_documents_its_request_and_response_roles() {
        let context = FileCompletionContext::new("HX-Trigger", 0, 10);
        let completions = complete_file_paths(&context, &|| false);

        assert!(completions.iter().any(|item| {
            item.label == "HX-Trigger"
                && item.detail.as_deref() == Some("htmx request and response header")
        }));
    }

    #[test]
    fn bare_hx_prefix_exposes_the_full_catalog_without_filesystem_fallthrough() {
        let expected = [
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
        ];
        let context = FileCompletionContext::new("hx", 4, 6);
        let completions = complete_file_paths(&context, &|| false);
        let labels: Vec<&str> = completions.iter().map(|item| item.label.as_ref()).collect();

        // The exact catalog, in order, proves a result that merely appended
        // filesystem entries to the header list cannot pass.
        assert_eq!(labels, expected);
        assert!(
            complete_htmx_headers(&FileCompletionContext::new("HX", 0, 2)).is_some_and(
                |items| items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>()
                    == expected
            )
        );
    }

    #[test]
    fn unknown_hx_header_prefix_does_not_fall_through_to_filesystem_completion() {
        let context = FileCompletionContext::new("HX-Not-A-Header", 0, 15);

        assert!(complete_htmx_headers(&context).is_some_and(|items| items.is_empty()));
        assert!(complete_file_paths(&context, &|| false).is_empty());
    }

    #[test]
    fn ordinary_string_prefix_remains_owned_by_file_completion() {
        let context = FileCompletionContext::new("fixtures/", 0, 9);

        assert!(complete_htmx_headers(&context).is_none());
    }

    #[test]
    fn cancellation_prevents_htmx_completion_work() {
        let context = FileCompletionContext::new("HX-", 0, 3);

        assert!(complete_file_paths(&context, &|| true).is_empty());
    }
}
