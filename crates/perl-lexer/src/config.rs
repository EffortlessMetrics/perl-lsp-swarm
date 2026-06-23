//! Configuration for the Perl lexer.

use crate::symbol_table::LocalSymbolTable;

/// Configuration options for the Perl lexer.
///
/// Controls interpolation handling, position tracking, lookahead limits, and
/// optional symbol-table-assisted bareword/regex disambiguation.
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
///
/// With pre-pass symbol table to improve `/`-after-bareword disambiguation:
///
/// ```rust
/// use perl_lexer::{LexerConfig, LocalSymbolTable};
///
/// let input = "sub my_fn;\nmy_fn /regex/;";
/// let config = LexerConfig {
///     symbol_table: Some(LocalSymbolTable::scan_subs(input)),
///     ..LexerConfig::default()
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
    /// Optional pre-pass symbol table of declared `sub` names.
    ///
    /// When set, the lexer uses it to switch mode to `ExpectTerm` for known
    /// function names so that a following `/` is tokenized as a regex delimiter
    /// rather than division.  Build it with [`LocalSymbolTable::scan_subs`].
    pub symbol_table: Option<LocalSymbolTable>,
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
