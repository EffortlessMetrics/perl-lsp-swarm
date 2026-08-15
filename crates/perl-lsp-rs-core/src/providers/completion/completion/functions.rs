//! Function completion for Perl
//!
//! Provides completion for user-defined subroutines with scope-distance ranking.
//! Functions defined in the same package rank higher than those from outer scopes.

use super::scope_distance::compute_scope_sort_key;
use super::{context::CompletionContext, items::CompletionItem, items::InsertTextFormat};
use perl_semantic_analyzer::symbol::{SymbolKind, SymbolTable};
use std::borrow::Cow;

/// Add function completions with scope-distance ranking.
///
/// User-defined subroutines are ranked by proximity to the cursor's scope,
/// so locally-defined helpers appear above package-level or outer-scope functions.
pub fn add_function_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    symbol_table: &SymbolTable,
) {
    let prefix_without_amp = context.prefix.trim_start_matches('&');

    for (name, symbols) in &symbol_table.symbols {
        for symbol in symbols {
            if (symbol.kind == SymbolKind::Subroutine || symbol.kind == SymbolKind::Constant)
                && name.starts_with(prefix_without_amp)
            {
                let scope_sort_key =
                    compute_scope_sort_key(symbol_table, context.cursor_scope_id, symbol.scope_id);
                let (kind, detail, insert_text, sort_tier) = if symbol.kind == SymbolKind::Constant
                {
                    (
                        super::items::CompletionItemKind::Constant,
                        Some(Cow::Borrowed("constant")),
                        Some(Cow::Owned(name.clone())),
                        "3",
                    )
                } else {
                    (
                        super::items::CompletionItemKind::Function,
                        Some(Cow::Borrowed("sub")),
                        Some(Cow::Owned(format!("{}()", name))),
                        "2",
                    )
                };
                completions.push(CompletionItem {
                    label: Cow::Owned(name.clone()),
                    kind,
                    detail,
                    documentation: symbol.documentation.clone().map(Cow::Owned),
                    insert_text,
                    sort_text: Some(Cow::Owned(format!("{sort_tier}{scope_sort_key}_{name}"))),
                    filter_text: Some(Cow::Owned(name.clone())),
                    additional_edits: vec![],
                    text_edit_range: Some((context.prefix_start, context.position)),
                    commit_characters: None,
                    insert_text_format: InsertTextFormat::PlainText,
                    label_details: None,
                });
            }
        }
    }
}
