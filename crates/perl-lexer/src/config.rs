//! Configuration for the Perl lexer.

use crate::symbol_table::LocalSymbolTable;

/// Configuration options for the Perl lexer.
///
/// The fields are retained as a public struct for source compatibility. Their
/// exact runtime effects are deliberately narrower than their historical names
/// suggested; see each field and the query methods below before selecting a
/// non-default value.
///
/// # Examples
///
/// ```rust
/// use perl_lexer::LexerConfig;
///
/// let config = LexerConfig {
///     parse_interpolation: true,
///     track_positions: true,
///     max_lookahead: LexerConfig::DEFAULT_MAX_LOOKAHEAD,
///     symbol_table: None,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct LexerConfig {
    /// Split supported interpolating strings into structured string parts.
    ///
    /// When `false`, interpolation-looking text remains part of the enclosing
    /// literal token. This switch does not change source consumption or token
    /// byte geometry.
    pub parse_interpolation: bool,
    /// Compatibility field retained for existing struct literals.
    ///
    /// Token byte spans are always tracked because the parser and editor
    /// consumers require them. Setting this field to `false` does **not** remove
    /// or replace `Token::start` and `Token::end`; use
    /// [`LexerConfig::POSITIONS_ARE_ALWAYS_TRACKED`] as the executable contract.
    /// Removal or retyping is tracked by issue #6715.
    pub track_positions: bool,
    /// Control package-qualified identifier continuation.
    ///
    /// `0` disables folding `::segment` continuations into the current
    /// identifier. Any non-zero value enables the current one-boundary
    /// lookahead path; values greater than one are presently equivalent. This
    /// is not a general byte or token scan budget.
    pub max_lookahead: usize,
    /// Optional file-local subroutine symbol table for bareword/regex disambiguation.
    ///
    /// When `Some`, the lexer treats identifiers that appear in this table as
    /// known subroutine names and sets `LexerMode::ExpectTerm` after them, so
    /// that a subsequent `/` is lexed as a regex rather than division.
    ///
    /// Build this with [`LocalSymbolTable::scan_subs`] before constructing the
    /// lexer. When `None` (the default), the existing heuristic is used:
    /// only built-in functions trigger `ExpectTerm`; all other bare identifiers
    /// trigger `ExpectOperator`.
    pub symbol_table: Option<LocalSymbolTable>,
}

impl LexerConfig {
    /// Default package-qualified identifier lookahead setting.
    pub const DEFAULT_MAX_LOOKAHEAD: usize = 1024;

    /// Token byte positions are part of the lexer contract in every configuration.
    pub const POSITIONS_ARE_ALWAYS_TRACKED: bool = true;

    /// Return whether structured interpolation parsing is enabled.
    pub fn interpolation_enabled(&self) -> bool {
        self.parse_interpolation
    }

    /// Return whether package-qualified identifier continuation is enabled.
    pub fn qualified_identifier_lookahead_enabled(&self) -> bool {
        self.max_lookahead > 0
    }

    /// Return whether a file-local subroutine table was supplied.
    pub fn has_symbol_table(&self) -> bool {
        self.symbol_table.is_some()
    }
}

impl Default for LexerConfig {
    fn default() -> Self {
        Self {
            parse_interpolation: true,
            track_positions: true,
            max_lookahead: Self::DEFAULT_MAX_LOOKAHEAD,
            symbol_table: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LexerConfig;

    #[test]
    fn default_enables_interpolation_and_preserves_position_contract() {
        let config = LexerConfig::default();

        assert!(config.interpolation_enabled());
        assert!(config.track_positions);
        assert!(LexerConfig::POSITIONS_ARE_ALWAYS_TRACKED);
    }

    #[test]
    fn default_uses_expected_lookahead_contract() {
        let config = LexerConfig::default();

        assert_eq!(config.max_lookahead, LexerConfig::DEFAULT_MAX_LOOKAHEAD);
        assert!(config.qualified_identifier_lookahead_enabled());
    }

    #[test]
    fn zero_lookahead_disables_qualified_identifier_continuation() {
        let config = LexerConfig { max_lookahead: 0, ..LexerConfig::default() };

        assert!(!config.qualified_identifier_lookahead_enabled());
    }

    #[test]
    fn default_symbol_table_is_none() {
        let config = LexerConfig::default();

        assert!(!config.has_symbol_table());
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

        assert!(!cloned.interpolation_enabled());
        assert!(!cloned.track_positions);
        assert_eq!(cloned.max_lookahead, 256);
        assert!(!cloned.has_symbol_table());
        assert!(LexerConfig::POSITIONS_ARE_ALWAYS_TRACKED);
    }
}
