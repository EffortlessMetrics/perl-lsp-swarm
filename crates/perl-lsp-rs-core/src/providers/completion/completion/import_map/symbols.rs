use perl_module::import::resolve_known_export_tag;
use std::collections::HashSet;

pub(super) fn collect_import_symbols(
    module: &str,
    arg: &str,
    symbols: &mut HashSet<String>,
) -> (bool, bool) {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return (false, false);
    }
    if matches!(trimmed, "=>" | "," | "(" | ")" | "[" | "]" | "{" | "}") {
        return (false, false);
    }

    let mut content = trimmed;
    if let Some(inner) = content.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        content = inner.trim();
    }

    if content.starts_with("qw") {
        content = content
            .trim_start_matches("qw")
            .trim_start_matches(|c: char| "([{/<|!".contains(c))
            .trim_end_matches(|c: char| ")]}/|!>".contains(c))
            .trim();
        return collect_words(module, content, symbols, !content.is_empty());
    }

    let cleaned = content.trim_matches(|c: char| c == '\'' || c == '"');
    if cleaned.is_empty() {
        return (false, false);
    }

    collect_words(module, cleaned, symbols, true)
}

fn collect_words(
    module: &str,
    words: &str,
    symbols: &mut HashSet<String>,
    has_symbols_when_nonempty: bool,
) -> (bool, bool) {
    let mut unresolved_tag = false;
    for word in words.split_whitespace().filter(|word| !word.is_empty()) {
        if word.starts_with(':') {
            if let Some(expanded) = resolve_known_export_tag(module, word) {
                symbols.extend(expanded.iter().map(|name| (*name).to_string()));
            } else {
                unresolved_tag = true;
            }
        } else {
            symbols.insert(word.to_string());
        }
    }
    (has_symbols_when_nonempty, unresolved_tag)
}
