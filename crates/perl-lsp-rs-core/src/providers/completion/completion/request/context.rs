use super::super::{CompletionContext, CompletionItem, CompletionProvider, regex_patterns, sort};

pub(super) fn prepare_context(
    provider: &CompletionProvider,
    source: &str,
    position: usize,
) -> Option<CompletionContext> {
    if position > source.len() {
        return None;
    }

    let context = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        provider.analyze_context(source, position)
    }))
    .ok()
    .map(|mut context| {
        context.in_use_statement = CompletionProvider::is_use_statement_context(source, position)
            && !CompletionProvider::current_line_starts_in_string(source, position);
        context
    })?;

    (!context.in_comment).then_some(context)
}

pub(super) fn rejects_dash_trigger(context: &CompletionContext) -> bool {
    context.trigger_character == Some('-')
        && !(context.prefix.ends_with("->") && context.prefix.len() > 2)
}

pub(super) fn rejects_lexical_block(source: &str, position: usize) -> bool {
    CompletionProvider::is_in_heredoc(source, position)
        || CompletionProvider::is_in_pod(source, position)
}

/// Canonical htmx attribute names in admitted raw-markup slots.
///
/// Returns `Some` when the markup recognizer owns the cursor — including
/// `Some([])` for a proven slot with no catalog match — and `None` when
/// normal Perl completion should continue. Perl strings, regex bodies, POD,
/// and heredocs are excluded even when the bytes look like HTML.
pub(super) fn try_complete_htmx_attributes(
    provider: &CompletionProvider,
    source: &str,
    position: usize,
) -> Option<Vec<CompletionItem>> {
    if !crate::providers::completion::htmx_attribute_name_completion_admitted(source, position) {
        return None;
    }
    let htmx_context = crate::providers::htmx::htmx_attribute_name_context(source, position)?;
    let suffix_end =
        crate::providers::htmx::markup_attribute_name_suffix_end(source, htmx_context.prefix_start);
    let mut items = crate::providers::htmx::complete_attribute_names(source, position)?;
    let context = provider.analyze_context(source, position);
    for item in &mut items {
        if let Some((start, end)) = item.text_edit_range {
            let replace_end = suffix_end.max(context.suffix_end);
            if end < replace_end {
                item.text_edit_range = Some((start, replace_end));
            }
        }
    }
    Some(items)
}

pub(super) fn complete_regex_context(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    source: &str,
) -> Option<Vec<CompletionItem>> {
    if CompletionProvider::is_in_regex_flags(source, context.position) {
        regex_patterns::add_regex_flag_completions(completions, context, source);
        return Some(sort::deduplicate_and_sort(std::mem::take(completions)));
    }

    if context.in_regex && has_regex_sigil_prefix(&context.prefix) {
        return Some(Vec::new());
    }

    if context.in_regex {
        regex_patterns::add_regex_completions(completions, context, source);
        return Some(sort::deduplicate_and_sort(std::mem::take(completions)));
    }

    None
}

fn has_regex_sigil_prefix(prefix: &str) -> bool {
    matches!(prefix.chars().next(), Some('$' | '@' | '%'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex_context(prefix: &str) -> CompletionContext {
        CompletionContext {
            position: 25,
            trigger_character: None,
            in_string: false,
            in_regex: true,
            in_comment: false,
            in_use_statement: false,
            current_package: "main".to_string(),
            prefix: prefix.to_string(),
            prefix_start: 23,
            suffix_end: 25,
            cursor_scope_id: 0,
        }
    }

    #[test]
    fn complete_regex_context_suppresses_scalar_sigil_prefix() {
        let context = regex_context("$m");
        let mut completions = Vec::new();

        let result =
            complete_regex_context(&mut completions, &context, "if ($str =~ /match $m/) {");

        assert!(result.as_ref().is_some_and(Vec::is_empty));
    }

    #[test]
    fn complete_regex_context_suppresses_array_sigil_prefix() {
        let context = regex_context("@a");
        let mut completions = Vec::new();

        let result =
            complete_regex_context(&mut completions, &context, "if ($str =~ /pattern @a/) {");

        assert!(result.as_ref().is_some_and(Vec::is_empty));
    }

    #[test]
    fn complete_regex_context_suppresses_hash_sigil_prefix() {
        let context = regex_context("%h");
        let mut completions = Vec::new();

        let result =
            complete_regex_context(&mut completions, &context, "if ($str =~ /pattern %h/) {");

        assert!(result.as_ref().is_some_and(Vec::is_empty));
    }

    #[test]
    fn complete_htmx_attribute_context_returns_none_outside_markup() {
        assert!(
            crate::providers::htmx::complete_attribute_names("my $hx = 'hx-get';", 3).is_none()
        );
    }

    fn provider_for(source: &str) -> Result<CompletionProvider, Box<dyn std::error::Error>> {
        use crate::providers::completion::completion::CompletionProvider;
        use perl_parser_core::Parser;

        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        Ok(CompletionProvider::new(&ast))
    }

    #[test]
    fn perl_string_does_not_receive_markup_attribute_completions()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "my $s = '<div hx-ge";
        let position = source.len();
        let provider = provider_for(source)?;
        assert!(try_complete_htmx_attributes(&provider, source, position).is_none());
        Ok(())
    }

    #[test]
    fn pod_block_does_not_receive_markup_attribute_completions()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "=pod\n<div hx-ge";
        let position = source.len();
        let provider = provider_for(source)?;
        assert!(try_complete_htmx_attributes(&provider, source, position).is_none());
        Ok(())
    }

    #[test]
    fn fragment_href_does_not_block_markup_attribute_completions()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "<a href=\"#section\" hx-ge";
        let position = source.len();
        let provider = provider_for(source)?;
        let items = try_complete_htmx_attributes(&provider, source, position)
            .ok_or("expected markup attribute slot")?;
        assert!(items.iter().any(|item| item.label == "hx-get"));
        Ok(())
    }

    #[test]
    fn mid_token_edit_range_replaces_attribute_suffix() -> Result<(), Box<dyn std::error::Error>> {
        let prefix = "<button hx-ge";
        let source = format!("{prefix}t");
        let cursor = prefix.len();
        let provider = provider_for(&source)?;
        let items = try_complete_htmx_attributes(&provider, &source, cursor)
            .ok_or("expected markup attribute slot")?;
        let item =
            items.iter().find(|item| item.label == "hx-get").ok_or("missing hx-get candidate")?;
        assert_eq!(item.text_edit_range, Some((cursor - "hx-ge".len(), source.len())));
        Ok(())
    }

    #[test]
    fn regex_body_does_not_claim_markup_attribute_route() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "my $r = qr{<div hx-on:click";
        let position = source.len();
        let provider = provider_for(source)?;
        assert!(try_complete_htmx_attributes(&provider, source, position).is_none());
        Ok(())
    }

    #[test]
    fn markup_helper_surfaces_catalog_in_markup() -> Result<(), Box<dyn std::error::Error>> {
        let source = "<button hx-ge";
        let items = crate::providers::htmx::complete_attribute_names(source, source.len())
            .ok_or("expected markup attribute slot")?;
        assert!(items.iter().any(|item| item.label == "hx-get"));
        Ok(())
    }

    #[test]
    fn perl_comment_does_not_receive_markup_attribute_completions()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "# <div hx-ge";
        let position = source.len();
        let provider = provider_for(source)?;
        assert!(try_complete_htmx_attributes(&provider, source, position).is_none());
        Ok(())
    }

    #[test]
    fn hx_on_event_suffix_included_in_text_edit_range() -> Result<(), Box<dyn std::error::Error>> {
        let source = "<div hx-on|:click>";
        let cursor = source.find("hx-on").map(|i| i + "hx-on".len()).ok_or("missing hx-on")?;
        let provider = provider_for(source)?;
        let items = try_complete_htmx_attributes(&provider, source, cursor)
            .ok_or("expected markup attribute slot")?;
        let item =
            items.iter().find(|item| item.label == "hx-on:").ok_or("missing hx-on: candidate")?;
        let suffix_end = source.find('>').ok_or("missing tag end")?;
        assert_eq!(item.text_edit_range, Some((cursor - "hx-on".len(), suffix_end)));
        Ok(())
    }

    #[test]
    fn completion_request_returns_htmx_attributes_before_perl_dispatch()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "<div hx-on";
        let provider = provider_for(source)?;
        let items = provider.get_completions(source, source.len());
        assert!(items.iter().any(|item| item.label == "hx-on:"));
        assert!(!items.iter().any(|item| item.label == "hx-on"));
        Ok(())
    }

    #[test]
    fn dash_trigger_does_not_suppress_htmx_attribute_completion()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "<div hx-";
        let provider = provider_for(source)?;
        let items = provider.get_completions(source, source.len());
        assert!(!items.is_empty());
        assert!(items.iter().all(|item| item.label.starts_with("hx-")));
        Ok(())
    }

    fn regex_flag_labels(items: &[CompletionItem]) -> Vec<String> {
        items
            .iter()
            .filter(|item| item.detail.as_deref() == Some("regex flag"))
            .map(|item| item.label.to_string())
            .collect()
    }

    #[test]
    fn method_arrow_after_angle_regex_does_not_offer_regex_flags()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "if ($name =~ m<sku>) {\n    print $inv->";
        let provider = provider_for(source)?;
        let items = provider.get_completions(source, source.len());
        let flags = regex_flag_labels(&items);
        assert!(
            flags.is_empty(),
            "method completion after `->` must not offer regex flags; got {flags:?} from {:?}",
            items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>()
        );
        for method in ["new", "isa", "can"] {
            assert!(
                items.iter().any(|item| item.label == *method),
                "expected generic method `{method}` at `$inv->`; got {:?}",
                items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>()
            );
        }
        Ok(())
    }

    #[test]
    fn mid_word_method_after_angle_regex_does_not_offer_regex_flags()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "$x =~ m<foo>;\nprint $inv->re";
        let provider = provider_for(source)?;
        let items = provider.get_completions(source, source.len());
        let flags = regex_flag_labels(&items);
        assert!(
            flags.is_empty(),
            "mid-word `$inv->re` must not offer regex flags; got {flags:?} from {:?}",
            items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>()
        );
        Ok(())
    }

    #[test]
    fn earlier_slash_substitution_does_not_offer_regex_flags_at_arrow()
    -> Result<(), Box<dyn std::error::Error>> {
        // A find-replace edit earlier in the buffer must not poison a clean
        // method-call site: the flag region ends at the substitution's
        // closing delimiter (#18760).
        let source = "package Inventory;\nsub add_item { 1 }\npackage main;\nmy $inv = Inventory->new;\n$sku =~ s/A1/B2/g;\nprint $inv->";
        let provider = provider_for(source)?;
        let items = provider.get_completions(source, source.len());
        let flags = regex_flag_labels(&items);
        assert!(
            flags.is_empty(),
            "clean call site after an earlier `s///` must not offer regex flags; got {flags:?} from {:?}",
            items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>()
        );
        for method in ["new", "isa", "can"] {
            assert!(
                items.iter().any(|item| item.label == *method),
                "expected generic method `{method}` at `$inv->` after an earlier `s///`; got {:?}",
                items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>()
            );
        }
        Ok(())
    }

    #[test]
    fn earlier_slash_substitution_does_not_offer_regex_flags_mid_word()
    -> Result<(), Box<dyn std::error::Error>> {
        // Non-arrow cursors are poisoned by the same unanchored count: a
        // mid-word completion below a closed `s///` used to answer the nine
        // regex-flag keywords instead of word candidates (#18760).
        let source = "$sku =~ s/A1/B2/g;\nmy $count";
        let provider = provider_for(source)?;
        let items = provider.get_completions(source, source.len());
        let flags = regex_flag_labels(&items);
        assert!(
            flags.is_empty(),
            "mid-word completion after a closed `s///` must not offer regex flags; got {flags:?} from {:?}",
            items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>()
        );
        Ok(())
    }

    #[test]
    fn closed_angle_regex_still_offers_flag_letters() -> Result<(), Box<dyn std::error::Error>> {
        let source = "$x =~ m<foo>";
        let provider = provider_for(source)?;
        let items = provider.get_completions(source, source.len());
        let flags = regex_flag_labels(&items);
        for flag in ["g", "i", "m", "s"] {
            assert!(
                flags.iter().any(|label| label == flag),
                "expected regex flag `{flag}` after `m<foo>`; got {flags:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn cross_file_mid_word_method_offers_class_method() -> Result<(), Box<dyn std::error::Error>> {
        use perl_parser_core::Parser;
        use perl_workspace::workspace_index::WorkspaceIndex;
        use std::sync::Arc;
        use url::Url;

        let index = Arc::new(WorkspaceIndex::new());
        index.index_initial_file(
            Url::parse("file:///lib/Inventory.pm")?,
            "package Inventory;\nsub remove_item { 1 }\nsub add_item { 1 }\n1;\n".to_string(),
        )?;
        let source = "use Inventory;\nmy $inv = Inventory->new;\nmy $low = $inv->re";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        let provider = CompletionProvider::new_with_index(&ast, Some(index));
        let items = provider.get_completions(source, source.len());
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_ref()).collect();
        assert!(
            labels.contains(&"remove_item"),
            "mid-word `$inv->re` must offer Inventory::remove_item; got {labels:?}"
        );
        Ok(())
    }
}
