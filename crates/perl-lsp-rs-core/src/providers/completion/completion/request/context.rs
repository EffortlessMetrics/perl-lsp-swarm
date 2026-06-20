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
        context.in_use_statement = CompletionProvider::is_use_statement_context(source, position);
        context
    })?;

    (!context.in_comment).then_some(context)
}

pub(super) fn rejects_dash_trigger(context: &CompletionContext) -> bool {
    context.trigger_character == Some('-')
        && !(context.prefix.ends_with("->") && context.prefix.len() > 2)
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

    if context.in_regex && !matches!(context.prefix.chars().next(), Some('$' | '@' | '%')) {
        regex_patterns::add_regex_completions(completions, context, source);
        return Some(sort::deduplicate_and_sort(std::mem::take(completions)));
    }

    // Inside a regex with a sigil prefix: suppress variable completions.
    // Variables like $1, $2 can be used in regex, but general variable completion
    // (offering all variables in scope) is noise and should be suppressed.
    if context.in_regex && matches!(context.prefix.chars().next(), Some('$' | '@' | '%')) {
        return Some(vec![]);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_variable_completion_inside_regex_with_dollar_sigil() {
        let context = CompletionContext {
            position: 25,
            trigger_character: None,
            in_string: false,
            in_regex: true,
            in_comment: false,
            in_use_statement: false,
            current_package: "main".to_string(),
            prefix: "$m".to_string(),
            prefix_start: 23,
            cursor_scope_id: 0,
        };
        let mut completions = vec![];
        let source = "if ($str =~ /match $m/) {";

        let result = complete_regex_context(&mut completions, &context, source);

        // Should return Some(empty vec) to suppress variable completions inside regex
        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_no_variable_completion_inside_regex_with_at_sigil() {
        let context = CompletionContext {
            position: 25,
            trigger_character: None,
            in_string: false,
            in_regex: true,
            in_comment: false,
            in_use_statement: false,
            current_package: "main".to_string(),
            prefix: "@a".to_string(),
            prefix_start: 23,
            cursor_scope_id: 0,
        };
        let mut completions = vec![];
        let source = "if ($str =~ /pattern @a/) {";

        let result = complete_regex_context(&mut completions, &context, source);

        // Should return Some(empty vec) to suppress array completions inside regex
        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_no_variable_completion_inside_regex_with_percent_sigil() {
        let context = CompletionContext {
            position: 25,
            trigger_character: None,
            in_string: false,
            in_regex: true,
            in_comment: false,
            in_use_statement: false,
            current_package: "main".to_string(),
            prefix: "%h".to_string(),
            prefix_start: 23,
            cursor_scope_id: 0,
        };
        let mut completions = vec![];
        let source = "if ($str =~ /pattern %h/) {";

        let result = complete_regex_context(&mut completions, &context, source);

        // Should return Some(empty vec) to suppress hash completions inside regex
        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }
}
