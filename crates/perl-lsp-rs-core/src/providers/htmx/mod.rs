//! Canonical htmx protocol metadata and parser-independent completion.

mod catalog;
mod markup;

pub use catalog::{
    HTMX_ATTRIBUTES, HTMX_CATALOG_PROVENANCE, HTMX_HEADERS, HtmxAttributeFamily, HtmxAttributeSpec,
    HtmxCatalogProvenance, HtmxHeaderDirection, HtmxHeaderSpec,
};
pub use markup::{HtmxAttributeNameContext, MAX_MARKUP_SCAN_BYTES, htmx_attribute_name_context};

use crate::providers::completion_item::{CompletionItem, CompletionItemKind, InsertTextFormat};
use crate::providers::file_completion::FileCompletionContext;
use catalog::starts_with_ignore_ascii_case;
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
/// `position` is a UTF-8 byte offset into `source`, not an LSP `Position`:
/// callers holding UTF-16 code-unit positions must convert before calling.
///
/// Returns `None` when the cursor is not in an admitted htmx attribute-name
/// context, so the caller can delegate to another completion grammar.
/// `Some(Vec::new())` means the slot is proven but the catalog has no
/// candidate for the typed prefix — for example an `hx-on:` family prefix
/// with an event name already typed (`hx-on:click`). Both canonical `hx-*`
/// and standard `data-hx-*` spellings are supported. Dynamic event handlers
/// are represented by the `hx-on:` family prefix; event-name completion is a
/// separate grammar.
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

#[cfg(test)]
mod tests {
    use super::{
        HTMX_ATTRIBUTES, HTMX_CATALOG_PROVENANCE, HTMX_HEADERS, HtmxAttributeFamily,
        HtmxHeaderDirection, complete_attribute_names, complete_header_names,
    };
    use crate::providers::file_completion::FileCompletionContext;

    /// Does this provenance pin the reviewed document to an immutable revision?
    ///
    /// A recorded review is only reproducible if the revision cannot change
    /// under it. A branch moves by design, and a Git tag can be moved or
    /// deleted — retagging would swap the reviewed document while leaving the
    /// provenance untouched. Only a commit is immutable, so the revision must
    /// be a full 40-hex object id and the URL must be addressed by it.
    ///
    /// Expressed as a predicate rather than inline assertions so the negative
    /// controls below can falsify it. Asserting only against the real value
    /// would leave "immutable" true by accident rather than by construction.
    fn pins_an_immutable_revision(provenance: &super::HtmxCatalogProvenance) -> bool {
        provenance.reference_commit.len() == 40
            && provenance.reference_commit.chars().all(|c| c.is_ascii_hexdigit())
            && provenance.reference_url.contains(&format!("/blob/{}/", provenance.reference_commit))
    }

    /// Does this URL address the htmx reference document itself?
    ///
    /// Immutability alone does not identify a document. A commit-addressed URL
    /// into a fork, into a different file of the same repository, or into a
    /// rendered mirror satisfies every revision rule above while describing
    /// something the catalog is not a transcription of. The catalog encodes one
    /// named upstream document, so this names it exactly rather than accepting
    /// any URL that merely looks commit-shaped.
    ///
    /// Takes the two fields rather than the struct so the controls below can
    /// supply candidate URLs without leaking them to obtain `&'static str`.
    fn addresses_the_htmx_reference_document(url: &str, commit: &str) -> bool {
        url == format!(
            "https://github.com/bigskysoftware/htmx/blob/{commit}/www/content/reference.md"
        )
    }

    /// Is this a real `YYYY-MM-DD` calendar date?
    ///
    /// Shape and numeric ranges are not enough: `2026-02-30` satisfies both and
    /// is not a date. Month lengths and the Gregorian leap rule are short enough
    /// to state exactly, so this states them rather than approximating — and the
    /// controls below exercise the boundaries that make the difference.
    fn is_calendar_date(text: &str) -> bool {
        let parts: Vec<&str> = text.split('-').collect();
        let [year, month, day] = parts[..] else { return false };

        if (year.len(), month.len(), day.len()) != (4, 2, 2) {
            return false;
        }
        if !parts.iter().all(|part| part.chars().all(|c| c.is_ascii_digit())) {
            return false;
        }

        let (Ok(year), Ok(month), Ok(day)) =
            (year.parse::<u32>(), month.parse::<u32>(), day.parse::<u32>())
        else {
            return false;
        };

        let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_month = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap_year => 29,
            2 => 28,
            _ => return false,
        };

        (1..=days_in_month).contains(&day)
    }

    #[test]
    fn an_impossible_calendar_date_is_rejected() {
        // Every one of these passes a shape-and-range check, which is what this
        // control exists to falsify.
        for invalid in [
            "2026-02-30",
            "2026-04-31",
            "2026-02-29",
            "2100-02-29",
            "2026-99-99",
            "2026-00-10",
            "2026-01-00",
            "2026-13-01",
            "----------",
            "0000000000",
            "2026-1-05",
            "26-01-05",
            "2026-+1-05",
        ] {
            assert!(!is_calendar_date(invalid), "{invalid} is not a calendar date");
        }

        // Genuine leap days must still be accepted, or the rule is just stricter
        // rather than correct.
        assert!(is_calendar_date("2024-02-29"));
        assert!(is_calendar_date("2000-02-29"));
        assert!(is_calendar_date("2026-12-31"));
    }

    #[test]
    fn provenance_pins_an_immutable_reviewed_revision() {
        assert!(pins_an_immutable_revision(&HTMX_CATALOG_PROVENANCE));
    }

    #[test]
    fn a_revision_that_is_not_a_commit_is_rejected() {
        let committed = HTMX_CATALOG_PROVENANCE;

        // Retargeting the tag cannot preserve this provenance: addressing the
        // URL by the tag, or by a branch, fails even when every other field is
        // left exactly as reviewed.
        for moving in [
            "https://github.com/bigskysoftware/htmx/blob/v2.0.10/www/content/reference.md",
            "https://github.com/bigskysoftware/htmx/blob/main/www/content/reference.md",
            "https://github.com/bigskysoftware/htmx/blob/master/www/content/reference.md",
        ] {
            let tag_addressed = super::HtmxCatalogProvenance { reference_url: moving, ..committed };
            assert!(
                !pins_an_immutable_revision(&tag_addressed),
                "{moving} is not an immutable revision"
            );
        }

        // An abbreviated or non-hex revision is not a full object id, even when
        // the URL agrees with it.
        for short in ["bdc7d7d", "v2.0.10", "not-a-commit-at-all"] {
            let abbreviated = super::HtmxCatalogProvenance {
                reference_commit: short,
                reference_url: "https://github.com/bigskysoftware/htmx/blob/bdc7d7d/x.md",
                ..committed
            };
            assert!(!pins_an_immutable_revision(&abbreviated), "{short} is not a full commit");
        }

        // A well-formed commit whose URL points somewhere else is also not
        // pinned: the two fields must agree, not merely both look plausible.
        let mismatched = super::HtmxCatalogProvenance {
            reference_url: "https://github.com/bigskysoftware/htmx/blob/\
                            0000000000000000000000000000000000000000/www/content/reference.md",
            ..committed
        };
        assert!(!pins_an_immutable_revision(&mismatched));
    }

    #[test]
    fn provenance_addresses_the_htmx_reference_document_itself() {
        let provenance = HTMX_CATALOG_PROVENANCE;
        assert!(addresses_the_htmx_reference_document(
            provenance.reference_url,
            provenance.reference_commit
        ));
    }

    #[test]
    fn a_commit_addressed_url_to_some_other_document_is_rejected() {
        let commit = HTMX_CATALOG_PROVENANCE.reference_commit;

        // Every one of these is immutably addressed and none of them is the
        // document this catalog transcribes, which is what this control exists
        // to falsify.
        for elsewhere in [
            // A fork, pinned just as immutably as the real thing.
            format!("https://github.com/someone/htmx/blob/{commit}/www/content/reference.md"),
            // The right repository, a different document.
            format!("https://github.com/bigskysoftware/htmx/blob/{commit}/README.md"),
            // The rendered site, which is a mirror rather than the source.
            "https://htmx.org/reference/".to_string(),
            // The right document with a fragment appended, which no longer names
            // the whole reviewed file.
            format!(
                "https://github.com/bigskysoftware/htmx/blob/{commit}/www/content/reference.md#attributes"
            ),
        ] {
            assert!(
                !addresses_the_htmx_reference_document(&elsewhere, commit),
                "{elsewhere} is not the reviewed htmx reference document"
            );
        }
    }

    #[test]
    fn provenance_contract_agrees_with_the_reviewed_version() {
        let provenance = HTMX_CATALOG_PROVENANCE;
        let expected = format!("{}.{}.", provenance.contract_major, provenance.contract_minor);

        assert!(
            provenance.htmx_version.starts_with(&expected),
            "recorded version {} does not describe contract {}.{}",
            provenance.htmx_version,
            provenance.contract_major,
            provenance.contract_minor
        );
        assert!(
            is_calendar_date(provenance.reviewed_on),
            "review date {} is not a real YYYY-MM-DD calendar date",
            provenance.reviewed_on
        );
    }

    #[test]
    fn provenance_names_the_dynamic_family_on_both_sides_of_the_transcription() {
        let provenance = HTMX_CATALOG_PROVENANCE;

        // The upstream spelling must not itself be a catalog entry, and the
        // catalog spelling must be the one entry carrying the dynamic family.
        // Getting this backwards would make a drift report reconcile the wrong
        // pair and hide a real rename.
        assert!(
            !HTMX_ATTRIBUTES
                .iter()
                .any(|attribute| attribute.name == provenance.upstream_event_handler_name)
        );
        assert!(HTMX_ATTRIBUTES.iter().any(|attribute| {
            attribute.name == provenance.catalog_event_handler_name
                && attribute.family == HtmxAttributeFamily::EventHandler
        }));
    }

    #[test]
    fn extension_owned_vocabularies_are_absent_from_the_core_catalog() {
        // htmx 1.x shipped WebSocket and SSE support as core attributes; htmx 2
        // moved both to extensions, along with their non-`hx-` companions.
        // Claiming any of them here would advertise support this server does not
        // have, so their absence is part of the catalog contract.
        for excluded in [
            "hx-ws",
            "hx-sse",
            "ws-connect",
            "ws-send",
            "sse-connect",
            "sse-swap",
            "sse-close",
            "hx-on",
        ] {
            assert!(
                !HTMX_ATTRIBUTES.iter().any(|attribute| attribute.name == excluded),
                "{excluded} is extension-owned or deprecated and must not be a core candidate"
            );
        }
    }

    #[test]
    fn canonical_header_catalog_is_exact_and_directional() {
        let expected: Vec<(&str, HtmxHeaderDirection)> = vec![
            ("HX-Boosted", HtmxHeaderDirection::Request),
            ("HX-Current-URL", HtmxHeaderDirection::Request),
            ("HX-History-Restore-Request", HtmxHeaderDirection::Request),
            ("HX-Location", HtmxHeaderDirection::Response),
            ("HX-Prompt", HtmxHeaderDirection::Request),
            ("HX-Push-Url", HtmxHeaderDirection::Response),
            ("HX-Redirect", HtmxHeaderDirection::Response),
            ("HX-Refresh", HtmxHeaderDirection::Response),
            ("HX-Replace-Url", HtmxHeaderDirection::Response),
            ("HX-Request", HtmxHeaderDirection::Request),
            ("HX-Reselect", HtmxHeaderDirection::Response),
            ("HX-Reswap", HtmxHeaderDirection::Response),
            ("HX-Retarget", HtmxHeaderDirection::Response),
            ("HX-Target", HtmxHeaderDirection::Request),
            ("HX-Trigger", HtmxHeaderDirection::RequestAndResponse),
            ("HX-Trigger-After-Settle", HtmxHeaderDirection::Response),
            ("HX-Trigger-After-Swap", HtmxHeaderDirection::Response),
            ("HX-Trigger-Name", HtmxHeaderDirection::Request),
        ];
        let actual: Vec<(&str, HtmxHeaderDirection)> =
            HTMX_HEADERS.iter().map(|header| (header.name, header.direction)).collect();

        assert_eq!(actual, expected);
        assert!(HTMX_HEADERS.iter().all(|header| !header.documentation.is_empty()));
    }

    #[test]
    fn canonical_attribute_catalog_is_exact_and_marks_the_dynamic_family() {
        use HtmxAttributeFamily::{EventHandler, Fixed};

        let expected: Vec<(&str, HtmxAttributeFamily, bool)> = vec![
            ("hx-boost", Fixed, false),
            ("hx-confirm", Fixed, false),
            ("hx-delete", Fixed, false),
            ("hx-disable", Fixed, false),
            ("hx-disabled-elt", Fixed, false),
            ("hx-disinherit", Fixed, false),
            ("hx-encoding", Fixed, false),
            ("hx-ext", Fixed, false),
            ("hx-get", Fixed, false),
            ("hx-headers", Fixed, false),
            ("hx-history", Fixed, false),
            ("hx-history-elt", Fixed, false),
            ("hx-include", Fixed, false),
            ("hx-indicator", Fixed, false),
            ("hx-inherit", Fixed, false),
            ("hx-on:", EventHandler, false),
            ("hx-params", Fixed, false),
            ("hx-patch", Fixed, false),
            ("hx-post", Fixed, false),
            ("hx-preserve", Fixed, false),
            ("hx-prompt", Fixed, false),
            ("hx-push-url", Fixed, false),
            ("hx-put", Fixed, false),
            ("hx-replace-url", Fixed, false),
            ("hx-request", Fixed, false),
            ("hx-select", Fixed, false),
            ("hx-select-oob", Fixed, false),
            ("hx-swap", Fixed, false),
            ("hx-swap-oob", Fixed, false),
            ("hx-sync", Fixed, false),
            ("hx-target", Fixed, false),
            ("hx-trigger", Fixed, false),
            ("hx-validate", Fixed, false),
            ("hx-vals", Fixed, false),
            ("hx-vars", Fixed, true),
        ];
        let actual: Vec<(&str, HtmxAttributeFamily, bool)> = HTMX_ATTRIBUTES
            .iter()
            .map(|attribute| (attribute.name, attribute.family, attribute.deprecated))
            .collect();

        assert_eq!(actual, expected);
        assert!(HTMX_ATTRIBUTES.iter().all(|attribute| !attribute.documentation.is_empty()));
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
            HTMX_ATTRIBUTES.iter().map(|attribute| attribute.name.to_string()).collect::<Vec<_>>()
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
                        && item.insert_text.as_deref().is_some_and(|text| text.starts_with("hx-"))
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
                        && item.detail.as_deref() == Some("htmx event-handler attribute family")
                })
        }));
    }

    #[test]
    fn typed_hx_on_event_name_keeps_the_proven_slot_with_no_catalog_candidate() {
        let family = "<div hx-on";
        assert!(
            complete_attribute_names(family, family.len())
                .is_some_and(|items| items.iter().any(|item| item.label == "hx-on:"))
        );

        // Once the event name is typed the family prefix no longer matches,
        // but the slot must stay proven (`Some`, empty) so the future
        // event-name grammar can fall through instead of seeing `None`.
        let event = "<div hx-on:click";
        assert!(complete_attribute_names(event, event.len()).is_some_and(|items| items.is_empty()));
    }
}
