use super::super::{
    CompletionContext, CompletionItem, CompletionProvider, lexical_context, sort, variables,
};
use perl_semantic_analyzer::symbol::SymbolKind;
use std::borrow::Cow;

/// Emit only scope-visible sigil-compatible bindings for an exact interpolation slot.
pub(super) fn complete_slot(
    provider: &CompletionProvider,
    context: &CompletionContext,
    source: &str,
    position: usize,
    is_cancelled: &dyn Fn() -> bool,
) -> Vec<CompletionItem> {
    let Some(geometry) = lexical_context::interpolation_slot_geometry(source, position) else {
        return Vec::new();
    };
    if geometry.escaped {
        return Vec::new();
    }

    // Perl interpolates `$name` / `@array` (and their `${}` / `@{}` forms)
    // in double-quoted strings, `qq`, backticks, and interpolating heredocs.
    // Bare `%hash` stays literal, so hash sigils are not interpolation slots.
    let (sigil, kind) = match geometry.prefix.chars().next() {
        Some('$') => ("$", SymbolKind::scalar()),
        Some('@') => ("@", SymbolKind::array()),
        _ => return Vec::new(),
    };

    let mut slot_context = context.clone();
    slot_context.prefix = geometry.prefix.clone();
    slot_context.prefix_start =
        if geometry.braced { geometry.name_start } else { geometry.prefix_start };

    let mut completions = Vec::new();
    variables::add_variable_completions(
        &mut completions,
        &slot_context,
        kind,
        &provider.symbol_table,
    );
    if is_cancelled() {
        return Vec::new();
    }
    variables::add_special_variables(&mut completions, &slot_context, sigil);
    if geometry.braced {
        completions.retain(braced_name_is_admitted);
        strip_sigil_from_braced_inserts(&mut completions, geometry.name_start, position);
    }
    sort::deduplicate_and_sort(completions)
}

fn braced_name_is_admitted(item: &CompletionItem) -> bool {
    let Some(insert) = item.insert_text.as_deref() else {
        return false;
    };
    let name = insert.trim_start_matches(['$', '@', '%']);
    !name.is_empty() && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn strip_sigil_from_braced_inserts(
    completions: &mut [CompletionItem],
    name_start: usize,
    position: usize,
) {
    for item in completions {
        if let Some(insert) = item.insert_text.as_deref() {
            let stripped = insert.trim_start_matches(['$', '@', '%']);
            item.insert_text = Some(Cow::Owned(stripped.to_string()));
        }
        if let Some(filter) = item.filter_text.as_deref() {
            let stripped = filter.trim_start_matches(['$', '@', '%']);
            item.filter_text = Some(Cow::Owned(stripped.to_string()));
        }
        item.text_edit_range = Some((name_start, position));
    }
}
