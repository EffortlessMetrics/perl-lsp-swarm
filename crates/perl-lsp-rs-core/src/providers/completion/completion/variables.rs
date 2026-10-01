//! Variable completion for Perl
//!
//! Provides completion for scalar, array, and hash variables with scope analysis.
//! Variables are ranked by scope distance: immediate scope > parent scope >
//! package level, giving users the most relevant completions first.
//!
//! Every local variable path admits candidates through
//! [`lexical_visibility`] BEFORE construction: a binding that is not visible
//! at the cursor never becomes a candidate, regardless of rank (#8941).

use super::lexical_visibility::{admit, select_exact_identities};
use super::scope_distance::compute_scope_sort_key;
use super::{context::CompletionContext, items::CompletionItem, items::InsertTextFormat};
use perl_semantic_analyzer::symbol::{ScopeId, Symbol, SymbolKind, SymbolTable};
use std::borrow::Cow;

fn admitted_symbols<'a>(
    symbol_table: &'a SymbolTable,
    cursor_scope_id: ScopeId,
    cursor_position: usize,
    symbols: impl IntoIterator<Item = &'a Symbol>,
) -> Vec<&'a Symbol> {
    let admitted: Vec<&Symbol> = symbols
        .into_iter()
        .filter(|symbol| admit(symbol_table, cursor_scope_id, cursor_position, symbol).is_visible())
        .collect();
    select_exact_identities(symbol_table, admitted)
}

fn variable_item(
    context: &CompletionContext,
    symbol_table: &SymbolTable,
    symbol: &Symbol,
    name: &str,
    sigil: &str,
    sort_tier: char,
    detail: String,
) -> CompletionItem {
    let insert_text = format!("{sigil}{name}");
    let scope_sort_key =
        compute_scope_sort_key(symbol_table, context.cursor_scope_id, symbol.scope_id);
    CompletionItem {
        label: Cow::Owned(insert_text.clone()),
        kind: super::items::CompletionItemKind::Variable,
        detail: Some(Cow::Owned(detail)),
        documentation: symbol.documentation.clone().map(Cow::Owned),
        insert_text: Some(Cow::Owned(insert_text.clone())),
        sort_text: Some(Cow::Owned(format!("{sort_tier}{scope_sort_key}_{name}"))),
        // Include the sigil in filter_text so strict-filtering clients match
        // when the user types the sigil prefix (e.g. `$c` matching `$count`).
        // (#5050 item 4)
        filter_text: Some(Cow::Owned(insert_text)),
        additional_edits: vec![],
        text_edit_range: Some((context.prefix_start, context.position)),
        commit_characters: None,
        insert_text_format: InsertTextFormat::PlainText,
        label_details: None,
    }
}

/// Add variable completions with scope-distance ranking.
///
/// Variables from the immediate scope rank highest, then parent scopes,
/// then package/global scope. This produces more relevant completion
/// lists when the same prefix matches variables at multiple scope depths.
pub fn add_variable_completions(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    kind: SymbolKind,
    symbol_table: &SymbolTable,
) {
    let sigil = kind.sigil().unwrap_or("");
    let prefix_without_sigil = context.prefix.trim_start_matches(sigil);

    for (name, symbols) in &symbol_table.symbols {
        if !name.starts_with(prefix_without_sigil) {
            continue;
        }

        // Admission before candidate construction (#8941): sibling/child/
        // ended scopes and future declarations contribute nothing; shadowed
        // outers are dropped so one label resolves to exactly one binding.
        let selected = admitted_symbols(
            symbol_table,
            context.cursor_scope_id,
            context.position,
            symbols.iter().filter(|symbol| symbol.kind == kind),
        );

        for symbol in selected {
            let detail =
                format!("{} {}{}", symbol.declaration.as_deref().unwrap_or(""), sigil, name)
                    .trim()
                    .to_string();
            completions.push(variable_item(
                context,
                symbol_table,
                symbol,
                name,
                sigil,
                '1',
                detail,
            ));
        }
    }
}

/// Add special Perl variables
///
/// Offers a curated set of Perl magic/special variables (from perlvar) as
/// completion items keyed by sigil. Each entry carries a one-line description
/// so editors can display a tooltip without requiring the user to consult
/// the documentation separately.
pub fn add_special_variables(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    sigil: &str,
) {
    let special_vars: &[(&str, &str)] = match sigil {
        "$" => &[
            // Topic / default variable
            ("$_", "Default input and pattern-search space (topic variable)"),
            // I/O and formatting
            ("$.", "Current line number of the last filehandle read"),
            ("$,", "Output field separator for print"),
            ("$/", "Input record separator (undef to slurp)"),
            ("$\\", "Output record separator appended by print"),
            ("$|", "Output auto-flush: set to 1 to disable buffering"),
            ("$\"", "List separator for interpolated arrays (default: space)"),
            ("$;", "Subscript separator for multi-dimensional hashes"),
            // Error and status
            ("$!", "Errno / last OS error message (POSIX::strerror)"),
            ("$@", "Error from the last eval block or do-file"),
            ("$?", "Child process status (wait status from system/backtick)"),
            // Process info
            ("$$", "Process ID of the current Perl process"),
            ("$0", "Name of the running program (can be assigned)"),
            // Regex capture groups
            ("$1", "First regex capture group from last successful match"),
            ("$2", "Second regex capture group from last successful match"),
            ("$3", "Third regex capture group from last successful match"),
            ("$4", "Fourth regex capture group from last successful match"),
            ("$5", "Fifth regex capture group from last successful match"),
            ("$6", "Sixth regex capture group from last successful match"),
            ("$7", "Seventh regex capture group from last successful match"),
            ("$8", "Eighth regex capture group from last successful match"),
            ("$9", "Ninth regex capture group from last successful match"),
            // Regex match strings
            ("$&", "Entire string matched by the last successful regex"),
            ("$`", "String preceding the last successful regex match"),
            ("$'", "String following the last successful regex match"),
            ("$+", "Last bracket matched by the last successful regex"),
            // Control variables
            ("$^O", "Operating system name (e.g. 'linux', 'MSWin32')"),
            ("$^V", "Perl interpreter version as a v-string"),
            ("$^T", "Script start time in seconds since the epoch"),
            ("$^W", "Global warning flag (prefer 'use warnings' instead)"),
            ("$^A", "Accumulator variable for write/format output"),
            ("$^I", "In-place edit extension (e.g. '.bak' with -i flag)"),
            ("$^F", "Maximum system file descriptor (default: 2)"),
            ("$^X", "Path to the current Perl interpreter executable"),
            ("$^D", "Debugging flags (numeric, set by -D flag)"),
            ("$^P", "Internal debugger flag; true when under debugger"),
            ("$^S", "Current interpreter state: true inside eval"),
            ("$^E", "OS-specific extended error information"),
            ("$^H", "Compile-time hints bitmask (internal)"),
            ("$^M", "Emergency memory pool for out-of-memory handler"),
            ("$^N", "Most recently matched capture group in the current regex"),
            ("$^R", "Result of the last successful (?{...}) assertion"),
        ],
        "@" => &[
            ("@_", "Subroutine arguments (passed by reference)"),
            ("@+", "Offsets where regex capture groups ended in the last successful match"),
            ("@-", "Offsets where regex capture groups started in the last successful match"),
            ("@ARGV", "Command-line arguments to the script"),
            ("@INC", "Module search paths (@INC for use/require)"),
            ("@ISA", "List of base classes for the current package"),
            ("@EXPORT", "Symbols exported by default from an Exporter module"),
            ("@EXPORT_OK", "Symbols exported on request from an Exporter module"),
        ],
        "%" => &[
            ("%ENV", "Environment variables (read/write)"),
            ("%INC", "Map of loaded module file paths keyed by module name"),
            ("%SIG", "Signal handlers keyed by signal name"),
            ("%+", "Named capture buffers from the last successful regex"),
            ("%-", "All named capture buffers (multi-valued) from last regex"),
        ],
        _ => &[],
    };

    for (var, description) in special_vars {
        if var.starts_with(&context.prefix) {
            completions.push(CompletionItem {
                label: Cow::Owned(var.to_string()),
                kind: super::items::CompletionItemKind::Variable,
                detail: Some(Cow::Borrowed("special variable")),
                documentation: Some(Cow::Owned(description.to_string())),
                insert_text: Some(Cow::Owned(var.to_string())),
                sort_text: Some(Cow::Owned(format!("0_{}", var))), // Special vars have highest priority
                filter_text: Some(Cow::Owned(var.to_string())),
                additional_edits: vec![],
                text_edit_range: Some((context.prefix_start, context.position)),
                commit_characters: None,
                insert_text_format: InsertTextFormat::PlainText,
                label_details: None,
            });
        }
    }
}

/// Add all variables without sigils (for interpolation contexts)
///
/// Uses the same cursor-visibility admission as the sigil path (#8941) and
/// scope-distance ranking so closer variables sort before distant ones,
/// while keeping the `5x_` prefix to rank below sigil-prefixed completions.
pub fn add_all_variables(
    completions: &mut Vec<CompletionItem>,
    context: &CompletionContext,
    symbol_table: &SymbolTable,
) {
    // Only add if the prefix doesn't already have a sigil
    if !context.prefix.starts_with(['$', '@', '%', '&']) {
        let selected = admitted_symbols(
            symbol_table,
            context.cursor_scope_id,
            context.position,
            symbol_table.symbols.iter().flat_map(|(name, symbols)| {
                symbols.iter().filter(move |symbol| {
                    symbol.kind.is_variable() && name.starts_with(&context.prefix)
                })
            }),
        );

        for symbol in selected {
            let name = symbol.name.as_str();
            let sigil = symbol.kind.sigil().unwrap_or("");
            let detail = format!("{} variable", symbol.declaration.as_deref().unwrap_or(""));
            completions.push(variable_item(
                context,
                symbol_table,
                symbol,
                name,
                sigil,
                '5',
                detail,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::SourceLocation;
    use perl_semantic_analyzer::symbol::{Scope, ScopeKind};

    fn table() -> SymbolTable {
        let mut table = SymbolTable::new();
        for (id, parent, kind, start, end) in [
            (0usize, None, ScopeKind::Global, 0usize, 100usize),
            (1, Some(0), ScopeKind::Subroutine, 10, 90),
            (2, Some(1), ScopeKind::Block, 20, 50),
            (3, Some(1), ScopeKind::Block, 55, 80),
        ] {
            table.scopes.insert(
                id,
                Scope {
                    id,
                    parent,
                    kind,
                    location: SourceLocation { start, end },
                    symbols: std::collections::HashSet::new(),
                },
            );
        }
        table
    }

    fn symbol(
        name: &str,
        kind: SymbolKind,
        declaration: &str,
        scope_id: ScopeId,
        start: usize,
    ) -> Symbol {
        Symbol {
            name: name.to_string(),
            qualified_name: name.to_string(),
            kind,
            location: SourceLocation { start, end: start + 4 },
            scope_id,
            declaration: Some(declaration.to_string()),
            documentation: None,
            attributes: vec![],
        }
    }

    #[test]
    fn admitted_symbols_empty_iterator_is_empty() {
        let table = table();
        let selected = admitted_symbols(&table, 2, 30, std::iter::empty());
        assert!(selected.is_empty());
    }

    #[test]
    fn admitted_symbols_filter_drops_after_cursor_and_sibling_scope() {
        let table = table();
        let visible = symbol("ok", SymbolKind::scalar(), "my", 2, 21);
        let future = symbol("later", SymbolKind::scalar(), "my", 2, 40);
        // Pre-cursor so DeclaredAfterCursor cannot mask a sibling-scope miss.
        let sibling = symbol("only_a", SymbolKind::scalar(), "my", 3, 25);
        let selected = admitted_symbols(&table, 2, 30, [&visible, &future, &sibling]);
        assert_eq!(selected.len(), 1);
        assert!(std::ptr::eq(selected[0], &visible));
    }

    #[test]
    fn admitted_symbols_identity_keeps_innermost_and_distinct_kinds() {
        let table = table();
        let outer = symbol("value", SymbolKind::scalar(), "my", 1, 11);
        let inner = symbol("value", SymbolKind::scalar(), "my", 2, 21);
        let array = symbol("value", SymbolKind::array(), "my", 2, 22);
        let selected = admitted_symbols(&table, 2, 30, [&outer, &inner, &array]);
        assert_eq!(selected.len(), 2);
        assert!(std::ptr::eq(selected[0], &inner), "innermost scalar must survive");
        assert!(std::ptr::eq(selected[1], &array), "distinct kind must not shadow");
    }
}
