//! Rename symbol resolution logic
//!
//! This module provides symbol resolution for rename operations.

use perl_parser_core::SourceLocation;
use perl_semantic_analyzer::symbol::{SymbolKind, SymbolTable};
use perl_symbol::cursor;

/// Find the symbol at a given position
pub fn find_symbol_at_position(
    position: usize,
    symbol_table: &SymbolTable,
    source: &str,
) -> Option<(String, SymbolKind)> {
    // First check if we're on a definition. Prefer the smallest containing
    // span: an enclosing declaration (a sub, a package) also contains the
    // position, and HashMap iteration order must not let it win over the
    // specific symbol under the cursor.
    let mut best: Option<(&String, SymbolKind, usize)> = None;
    for (name, symbols) in &symbol_table.symbols {
        for symbol in symbols {
            if symbol.location.start <= position && position <= symbol.location.end {
                let span = symbol.location.end - symbol.location.start;
                if best.is_none_or(|(_, _, best_span)| span < best_span) {
                    best = Some((name, symbol.kind, span));
                }
            }
        }
    }
    if let Some((name, kind, _)) = best {
        return Some((name.clone(), kind));
    }

    // Then check references, with the same smallest-span preference.
    let mut best: Option<(&String, SymbolKind, usize)> = None;
    for (name, references) in &symbol_table.references {
        for reference in references {
            if reference.location.start <= position && position <= reference.location.end {
                let span = reference.location.end - reference.location.start;
                if best.is_none_or(|(_, _, best_span)| span < best_span) {
                    best = Some((name, reference.kind, span));
                }
            }
        }
    }
    if let Some((name, kind, _)) = best {
        return Some((name.clone(), kind));
    }

    // Try to extract from source text
    extract_symbol_from_source(position, source)
}

/// Extract symbol from source text at position
pub fn extract_symbol_from_source(position: usize, source: &str) -> Option<(String, SymbolKind)> {
    let (name, kind) = cursor::extract_symbol_from_source(position, source)?;
    Some((name, map_cursor_kind(kind)))
}

/// Get the range of a symbol at position
pub fn get_symbol_range_at_position(position: usize, source: &str) -> Option<SourceLocation> {
    let (start, end) = cursor::get_symbol_range_at_position(position, source)?;
    Some(SourceLocation { start, end })
}

fn map_cursor_kind(kind: cursor::CursorSymbolKind) -> SymbolKind {
    match kind {
        cursor::CursorSymbolKind::Scalar => SymbolKind::scalar(),
        cursor::CursorSymbolKind::Array => SymbolKind::array(),
        cursor::CursorSymbolKind::Hash => SymbolKind::hash(),
        cursor::CursorSymbolKind::Subroutine => SymbolKind::Subroutine,
    }
}
