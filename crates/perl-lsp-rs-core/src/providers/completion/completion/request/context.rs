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
    let mut items = crate::providers::htmx::complete_attribute_names(source, position)?;
    let context = provider.analyze_context(source, position);
    if context.in_string || context.in_regex || rejects_lexical_block(source, position) {
        return None;
    }
    for item in &mut items {
        if let Some((start, end)) = item.text_edit_range {
            if end < context.suffix_end {
                item.text_edit_range = Some((start, context.suffix_end));
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
        assert!(crate::providers::htmx::complete_attribute_names("my $hx = 'hx-get';", 3).is_none());
    }

    fn provider_for(source: &str) -> Result<CompletionProvider, Box<dyn std::error::Error>> {
        use crate::providers::completion::completion::CompletionProvider;
        use perl_parser_core::Parser;

        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        Ok(CompletionProvider::new(&ast))
    }

    #[test]
    fn perl_string_does_not_receive_markup_attribute_completions() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "my $s = '<div hx-ge";
        let position = source.len();
        let provider = provider_for(source)?;
        assert!(try_complete_htmx_attributes(&provider, source, position).is_none());
        Ok(())
    }

    #[test]
    fn pod_block_does_not_receive_markup_attribute_completions() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "=pod\n<div hx-ge";
        let position = source.len();
        let provider = provider_for(source)?;
        assert!(try_complete_htmx_attributes(&provider, source, position).is_none());
        Ok(())
    }

    #[test]
    fn fragment_href_does_not_block_markup_attribute_completions() -> Result<(), Box<dyn std::error::Error>>
    {
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
        let item = items
            .iter()
            .find(|item| item.label == "hx-get")
            .ok_or("missing hx-get candidate")?;
        assert_eq!(item.text_edit_range, Some((cursor - "hx-ge".len(), source.len())));
        Ok(())
    }

    #[test]
    fn regex_body_does_not_claim_markup_attribute_route() -> Result<(), Box<dyn std::error::Error>> {
        let source = "my $r = qr{<div hx-on:click";
        let position = source.len();
        let provider = provider_for(source)?;
        assert!(try_complete_htmx_attributes(&provider, source, position).is_none());
        Ok(())
    }

    #[test]
    fn markup_helper_surfaces_catalog_in_markup() {
        let source = "<button hx-ge";
        let items = crate::providers::htmx::complete_attribute_names(source, source.len())
            .expect("markup slot");
        assert!(items.iter().any(|item| item.label == "hx-get"));
    }

    #[test]
    fn completion_request_returns_htmx_attributes_before_perl_dispatch() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "<div hx-on";
        let provider = provider_for(source)?;
        let items = provider.get_completions(source, source.len());
        assert!(items.iter().any(|item| item.label == "hx-on:"));
        assert!(!items.iter().any(|item| item.label == "hx-on"));
        Ok(())
    }

    #[test]
    fn dash_trigger_does_not_suppress_htmx_attribute_completion() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "<div hx-";
        let provider = provider_for(source)?;
        let items = provider.get_completions(source, source.len());
        assert!(!items.is_empty());
        assert!(items.iter().all(|item| item.label.starts_with("hx-")));
        Ok(())
    }
}
