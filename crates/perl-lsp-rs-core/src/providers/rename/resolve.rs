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
    // Prefer the smallest containing span across definitions and
    // references alike: an enclosing declaration (a sub, a package) also
    // contains every position in its body, and HashMap iteration order must
    // not let it win over the specific symbol under the cursor. Checking
    // definitions first would credit the enclosing sub for every use of a
    // nested lexical in its body.
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

#[cfg(test)]
mod tests {
    use super::find_symbol_at_position;
    use perl_parser::Parser;
    use perl_semantic_analyzer::symbol::{SymbolExtractor, SymbolKind};

    #[test]
    fn find_symbol_at_position_prefers_the_nested_lexical_over_the_enclosing_sub() {
        // The sub symbol spans the whole node; a position on the inner
        // lexical must resolve to the lexical at every offset of its token,
        // regardless of symbol-table iteration order (#18117).
        let code = "sub run { my $value = 1; print $value; }";
        let ast = Parser::new(code).parse().expect("parse");
        let table = SymbolExtractor::new_with_source(code).extract(&ast);
        let decl = code.find("$value").expect("declaration") + 1;
        for position in decl..decl + "value".len() {
            let found = find_symbol_at_position(position, &table, code);
            assert_eq!(
                found.map(|(name, kind)| (name, kind)),
                Some(("value".to_string(), SymbolKind::scalar())),
                "position {position}"
            );
        }
        // The later use resolves to the same lexical, not the enclosing sub.
        let use_pos = code.rfind("$value").expect("use") + 1;
        let found = find_symbol_at_position(use_pos, &table, code);
        assert_eq!(
            found.map(|(name, kind)| (name, kind)),
            Some(("value".to_string(), SymbolKind::scalar()))
        );
    }
}
