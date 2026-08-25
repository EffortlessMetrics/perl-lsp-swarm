//! Configuration for the Perl lexer.

use crate::symbol_table::LocalSymbolTable;

/// Configuration options for the Perl lexer.
///
/// The fields are retained as a public struct for source compatibility. Their
/// runtime effects are not interchangeable: interpolation changes string-part
/// segmentation, lookahead bounds shared cursor probes, and the optional symbol
/// table changes one declared bareword/regex ambiguity surface. Token byte spans
/// remain authoritative in every configuration.
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
    /// Split supported ordinary double-quoted strings into string parts.
    ///
    /// When `false`, the ordinary double-quoted scanner emits one literal part
    /// for a non-empty body instead of recognizing variable/expression islands.
    /// The enclosing token text and byte span do not change. Quote-like `qq`
    /// bodies are currently opaque whole tokens and therefore do not consume
    /// this switch.
    pub parse_interpolation: bool,
    /// Compatibility field retained for existing struct literals.
    ///
    /// Token byte spans are always tracked because parser and editor consumers
    /// require them. Setting this field to `false` does **not** remove or replace
    /// `Token::start` and `Token::end`; use
    /// [`LexerConfig::POSITIONS_ARE_ALWAYS_TRACKED`] as the executable contract.
    /// Removal or retyping is tracked by issue #6715.
    pub track_positions: bool,
    /// Maximum zero-based offset admitted by shared cursor lookahead helpers.
    ///
    /// The limit applies to `peek_char`, `peek_byte`, and fixed-pattern probes,
    /// so it can affect identifiers, multi-byte operators, delimiters, numeric
    /// disambiguation, and file-start normalization. `0` still permits the
    /// current character or a one-byte pattern; it rejects offset `1` and any
    /// longer pattern. Values are not generally equivalent, and character and
    /// byte probes retain their respective units.
    pub max_lookahead: usize,
    /// Optional file-local subroutine symbol table for bareword/regex disambiguation.
    ///
    /// When `Some`, the lexer treats identifiers that appear in this table as
    /// known subroutine names and sets `LexerMode::ExpectTerm` after them, so a
    /// subsequent `/` is lexed as a regex rather than division.
    ///
    /// Build this with [`LocalSymbolTable::scan_subs`] before constructing the
    /// lexer. When `None` (the default), the existing heuristic is used: only
    /// built-in functions trigger `ExpectTerm`; all other bare identifiers
    /// trigger `ExpectOperator`.
    pub symbol_table: Option<LocalSymbolTable>,
}

impl LexerConfig {
    /// Default shared cursor lookahead limit.
    pub const DEFAULT_MAX_LOOKAHEAD: usize = 1024;

    /// Token byte positions are part of the lexer contract in every configuration.
    pub const POSITIONS_ARE_ALWAYS_TRACKED: bool = true;

    /// Return whether structured interpolation recognition is enabled.
    pub fn interpolation_enabled(&self) -> bool {
        self.parse_interpolation
    }

    /// Return the maximum zero-based cursor offset admitted by lookahead helpers.
    pub fn lookahead_limit(&self) -> usize {
        self.max_lookahead
    }

    /// Return whether the configured shared cursor limit admits `offset`.
    pub fn permits_lookahead_offset(&self, offset: usize) -> bool {
        offset <= self.max_lookahead
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
    }

    #[test]
    fn default_uses_expected_shared_lookahead_limit() {
        let config = LexerConfig::default();

        assert_eq!(config.lookahead_limit(), LexerConfig::DEFAULT_MAX_LOOKAHEAD);
        assert!(config.permits_lookahead_offset(0));
        assert!(config.permits_lookahead_offset(1));
        assert!(!config.permits_lookahead_offset(LexerConfig::DEFAULT_MAX_LOOKAHEAD + 1));
    }

    #[test]
    fn zero_lookahead_admits_only_the_current_offset() {
        let config = LexerConfig { max_lookahead: 0, ..LexerConfig::default() };

        assert!(config.permits_lookahead_offset(0));
        assert!(!config.permits_lookahead_offset(1));
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
        assert_eq!(cloned.lookahead_limit(), 256);
        assert!(!cloned.has_symbol_table());
    }
}
