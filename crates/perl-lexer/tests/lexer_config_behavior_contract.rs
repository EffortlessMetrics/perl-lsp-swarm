//! Public behavior contract for `LexerConfig` and feature-independent tokenization.
//!
//! These tests deliberately exercise the public `PerlLexer::with_config` path.
//! They separate observable configuration effects from compatibility fields and
//! from the currently empty `simd` Cargo feature.

use perl_lexer::{LexerConfig, LocalSymbolTable, PerlLexer, Token, TokenType};

type R = Result<(), Box<dyn std::error::Error>>;

fn first_token(input: &str, config: LexerConfig) -> Result<Token, Box<dyn std::error::Error>> {
    PerlLexer::with_config(input, config).next_token().ok_or("expected one lexer token".into())
}

#[test]
fn interpolation_switch_changes_segmentation_without_changing_source_geometry() -> R {
    let input = r#""hello $name""#;

    let enabled = first_token(input, LexerConfig::default())?;
    assert!(matches!(&enabled.token_type, TokenType::InterpolatedString(_)));
    assert_eq!(enabled.text.as_ref(), input);
    assert_eq!(enabled.start, 0);
    assert_eq!(enabled.end, input.len());

    let disabled_config = LexerConfig { parse_interpolation: false, ..LexerConfig::default() };
    let disabled = first_token(input, disabled_config)?;
    assert!(!matches!(&disabled.token_type, TokenType::InterpolatedString(_)));
    assert_eq!(disabled.text.as_ref(), input);
    assert_eq!(disabled.start, enabled.start);
    assert_eq!(disabled.end, enabled.end);
    Ok(())
}

#[test]
fn position_compatibility_field_does_not_remove_authoritative_byte_spans() -> R {
    let input = "my $café = 1;\r\nprint $café;";
    let config = LexerConfig { track_positions: false, ..LexerConfig::default() };
    let tokens = PerlLexer::with_config(input, config).collect_tokens();

    assert!(LexerConfig::POSITIONS_ARE_ALWAYS_TRACKED);
    for token in tokens.iter().filter(|token| !matches!(token.token_type, TokenType::EOF)) {
        assert!(token.start <= token.end, "token span is reversed: {token:?}");
        assert_eq!(
            input.get(token.start..token.end),
            Some(token.text.as_ref()),
            "token text must be the exact source slice for {token:?}"
        );
    }
    Ok(())
}

#[test]
fn max_lookahead_is_the_qualified_identifier_enable_boundary() -> R {
    let input = "Foo::bar";

    let disabled = first_token(
        input,
        LexerConfig { max_lookahead: 0, ..LexerConfig::default() },
    )?;
    assert!(matches!(&disabled.token_type, TokenType::Identifier(name) if name.as_ref() == "Foo"));
    assert_eq!(disabled.text.as_ref(), "Foo");

    for max_lookahead in [1, 2, LexerConfig::DEFAULT_MAX_LOOKAHEAD] {
        let enabled = first_token(
            input,
            LexerConfig { max_lookahead, ..LexerConfig::default() },
        )?;
        assert!(
            matches!(&enabled.token_type, TokenType::Identifier(name) if name.as_ref() == input),
            "non-zero max_lookahead={max_lookahead} must retain the qualified name, got {:?}",
            enabled.token_type
        );
        assert_eq!(enabled.text.as_ref(), input);
    }
    Ok(())
}

#[test]
fn symbol_table_changes_only_the_declared_bareword_slash_ambiguity() -> R {
    let input = "builder /pattern/; sub builder { 1 }";

    let heuristic_tokens = PerlLexer::new(input).collect_tokens();
    assert!(
        heuristic_tokens.iter().any(|token| matches!(token.token_type, TokenType::Division)),
        "without a symbol table the unknown bareword path should leave slash as division"
    );

    let config = LexerConfig {
        symbol_table: Some(LocalSymbolTable::scan_subs(input)),
        ..LexerConfig::default()
    };
    assert!(config.has_symbol_table());
    let symbol_tokens = PerlLexer::with_config(input, config).collect_tokens();
    assert!(
        symbol_tokens.iter().any(|token| matches!(token.token_type, TokenType::RegexMatch)),
        "a known local sub must make the following slash term-introducing"
    );
    Ok(())
}

#[test]
fn canonical_token_contract_is_identical_under_every_compiled_feature_set() -> R {
    // This same test is executed in default and all-features builds. The
    // compatibility `simd` feature is not allowed to alter the token contract
    // unless it gains an independently proven implementation.
    let input = "my $x = q{value}; $x =~ /value/;";
    let tokens = PerlLexer::new(input).collect_tokens();

    assert!(tokens.iter().any(|token| matches!(token.token_type, TokenType::QuoteSingle)));
    assert!(tokens.iter().any(|token| matches!(token.token_type, TokenType::RegexMatch)));
    assert!(matches!(tokens.last().map(|token| &token.token_type), Some(TokenType::EOF)));
    Ok(())
}
