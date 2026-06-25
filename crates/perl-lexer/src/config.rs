//! Configuration for the Perl lexer.

use std::sync::Arc;

use crate::LocalSymbolTable;

/// Configuration options for the Perl lexer.
///
/// Controls interpolation handling, position tracking, lookahead limits, and
/// optional per-file symbol table for bareword/regex disambiguation.
/// Use [`Default::default`] for sensible defaults.
///
/// # Examples
///
/// ```rust
/// use perl_lexer::LexerConfig;
///
/// let config = LexerConfig {
///     parse_interpolation: true,
///     track_positions: true,
///     max_lookahead: 1024,
///     symbol_table: None,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct LexerConfig {
    /// Enable interpolation parsing in strings.
    pub parse_interpolation: bool,
    /// Track token positions for error reporting.
    pub track_positions: bool,
    /// Maximum lookahead for disambiguation.
    pub max_lookahead: usize,
    /// Optional pre-scanned symbol table for this source file.
    ///
    /// When set, the lexer treats bareword identifiers that appear in
    /// `known_subs` or `known_constants` as term-introducing, so `/` after
    /// them is lexed as a regex delimiter rather than the division operator.
    ///
    /// Build with [`LocalSymbolTable::scan_subs`] and wrap in an [`Arc`]
    /// before passing here.
    pub symbol_table: Option<Arc<LocalSymbolTable>>,
}

impl Default for LexerConfig {
    fn default() -> Self {
        Self {
            parse_interpolation: true,
            track_positions: true,
            max_lookahead: 1024,
            symbol_table: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LexerConfig;

    #[test]
    fn default_enables_interpolation_and_position_tracking() {
        let config = LexerConfig::default();

        assert!(config.parse_interpolation);
        assert!(config.track_positions);
    }

    #[test]
    fn default_uses_expected_lookahead_limit() {
        let config = LexerConfig::default();

        assert_eq!(config.max_lookahead, 1024);
    }

    #[test]
    fn clone_preserves_field_values() {
        let config = LexerConfig {
            parse_interpolation: false,
            track_positions: false,
            max_lookahead: 256,
            symbol_table: None,
        };

        let cloned = config.clone();

        assert!(!cloned.parse_interpolation);
        assert!(!cloned.track_positions);
        assert_eq!(cloned.max_lookahead, 256);
    }
}
