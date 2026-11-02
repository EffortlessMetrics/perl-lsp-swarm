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
/// normal Perl completion should continue.
pub(super) fn complete_htmx_attribute_context(
    source: &str,
    position: usize,
) -> Option<Vec<CompletionItem>> {
    let items = crate::providers::htmx::complete_attribute_names(source, position)?;
    Some(sort::deduplicate_and_sort(items))
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
        assert!(complete_htmx_attribute_context("my $hx = 'hx-get';", 3).is_none());
    }

    #[test]
    fn complete_htmx_attribute_context_surfaces_catalog_in_markup() {
        let source = "<button hx-ge";
        let items = complete_htmx_attribute_context(source, source.len()).expect("markup slot");
        assert!(items.iter().any(|item| item.label == "hx-get"));
    }

    #[test]
    fn completion_request_returns_htmx_attributes_before_perl_dispatch() {
        use crate::providers::completion::completion::CompletionProvider;
        use perl_parser_core::Parser;

        let source = "<div hx-on";
        let mut parser = Parser::new(source);
        let ast = parser.parse().expect("parse template markup");
        let provider = CompletionProvider::new(&ast);
        let items = provider.get_completions(source, source.len());

        assert!(items.iter().any(|item| item.label == "hx-on:"));
        assert!(!items.iter().any(|item| item.label == "hx-on"));
    }

    #[test]
    fn dash_trigger_does_not_suppress_htmx_attribute_completion() {
        use crate::providers::completion::completion::CompletionProvider;
        use perl_parser_core::Parser;

        let source = "<div hx-";
        let mut parser = Parser::new(source);
        let ast = parser.parse().expect("parse template markup");
        let provider = CompletionProvider::new(&ast);
        let context = provider.analyze_context(source, source.len());
        assert!(rejects_dash_trigger(&CompletionContext {
            trigger_character: Some('-'),
            ..context
        }));

        let items = provider.get_completions(source, source.len());
        assert!(!items.is_empty());
        assert!(items.iter().all(|item| item.label.starts_with("hx-")));
    }
}
