//! String-literal completion compatibility facade.
//!
//! htmx header names are completed by the canonical htmx provider before the
//! secure filesystem provider is consulted. Every other prefix delegates
//! unchanged, preserving the file-completion traversal and sanitization policy.

use crate::providers::completion_item::CompletionItem;
use crate::providers::file_completion::complete_file_paths as complete_filesystem_paths;
use crate::providers::htmx::complete_header_names;

pub use crate::providers::file_completion::FileCompletionContext;

/// Complete htmx header names or delegate to secure file-path completion.
#[must_use]
pub fn complete_file_paths(
    context: &FileCompletionContext,
    is_cancelled: &dyn Fn() -> bool,
) -> Vec<CompletionItem> {
    if is_cancelled() {
        return Vec::new();
    }

    if let Some(headers) = complete_header_names(context) {
        return headers;
    }

    complete_filesystem_paths(context, is_cancelled)
}

#[cfg(test)]
mod tests {
    use super::{FileCompletionContext, complete_file_paths};
    use crate::providers::htmx::complete_header_names;

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

        assert_eq!(labels, expected);
        assert!(
            complete_header_names(&FileCompletionContext::new("HX", 0, 2)).is_some_and(|items| {
                items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>() == expected
            })
        );
    }

    #[test]
    fn unknown_hx_header_prefix_does_not_fall_through_to_filesystem_completion() {
        let context = FileCompletionContext::new("HX-Not-A-Header", 0, 15);

        assert!(complete_header_names(&context).is_some_and(|items| items.is_empty()));
        assert!(complete_file_paths(&context, &|| false).is_empty());
    }

    #[test]
    fn ordinary_string_prefix_remains_owned_by_file_completion() {
        let context = FileCompletionContext::new("fixtures/", 0, 9);

        assert!(complete_header_names(&context).is_none());
    }

    #[test]
    fn cancellation_prevents_htmx_completion_work() {
        let context = FileCompletionContext::new("HX-", 0, 3);

        assert!(complete_file_paths(&context, &|| true).is_empty());
    }
}
