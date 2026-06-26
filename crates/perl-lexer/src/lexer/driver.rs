use std::sync::Arc;

use crate::symbol_table::LocalSymbolTable;
use crate::{LexerConfig, LexerMode, PerlLexer, Position};

impl<'a> PerlLexer<'a> {
    /// Create a new lexer with default configuration.
    pub fn new(input: &'a str) -> Self {
        Self::with_config(input, LexerConfig::default())
    }

    /// Create a new lexer with a pre-pass symbol table built from the same
    /// source text.
    ///
    /// The symbol table is populated by scanning `input` for `sub NAME`
    /// declarations before lexing begins. This lets the lexer correctly
    /// disambiguate bareword function calls followed by `/` (regex vs.
    /// division) for user-defined subroutines.
    ///
    /// Use this constructor instead of [`Self::new`] when you want the full
    /// disambiguation fix for user-defined subs (issue #1353).
    pub fn with_source_symbols(input: &'a str) -> Self {
        let table = LocalSymbolTable::scan_subs(input);
        let config = LexerConfig { symbol_table: Some(Arc::new(table)), ..LexerConfig::default() };
        Self::with_config(input, config)
    }

    /// Create a new lexer with explicit configuration.
    pub fn with_config(input: &'a str, config: LexerConfig) -> Self {
        Self {
            input,
            input_bytes: input.as_bytes(),
            position: 0,
            mode: LexerMode::ExpectTerm,
            config,
            delimiter_stack: Vec::new(),
            in_prototype: false,
            prototype_depth: 0,
            after_sub: false,
            after_arrow: false,
            hash_brace_depth: 0,
            after_var_subscript: false,
            paren_depth: 0,
            current_pos: Position::start(),
            after_newline: true,
            pending_heredocs: Vec::new(),
            line_start_offset: 0,
            emit_heredoc_body_tokens: false,
            current_quote_op: None,
            eof_emitted: false,
            start_time: std::time::Instant::now(),
        }
    }
}
